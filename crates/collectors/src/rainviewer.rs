//! RainViewer's radar index.
//!
//! This collector fetches an index, never imagery. It records where the most
//! recent radar composite lives; whoever wants to look at it — the map, the
//! panel — asks for the pixels themselves.
//!
//! That split is deliberate and load-bearing. Collector items are persisted as
//! JSON in `SourceState`, so a few hundred kilobytes of PNG entering this path
//! would be written to disk on every poll and read back on every snapshot. The
//! item carries a host and a path, which is all anyone downstream needs.
//!
//! Only observed frames are published. RainViewer's index also carries a
//! `nowcast` list — extrapolated future radar — and presenting a model's guess
//! as an observation is exactly the kind of borrowed confidence this app is
//! built to avoid.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::{
    CollectContext, Collector, CollectorBatch, CollectorError, CollectorItem, HttpFetcher,
    ItemKind, SafeHttpFetcher, SourceLink, collection::collect_http,
};

const RAINVIEWER_INDEX_URL: &str = "https://api.rainviewer.com/public/weather-maps.json";
/// RainViewer publishes a new composite roughly every ten minutes; a frame
/// older than half an hour means the feed has stalled rather than that the
/// weather is quiet.
const MAX_FRAME_AGE_SECONDS: i64 = 30 * 60;

pub struct RainViewerCollector {
    endpoint: Url,
    fetcher: Arc<dyn HttpFetcher>,
}

impl Default for RainViewerCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl RainViewerCollector {
    pub fn new() -> Self {
        Self::with_fetcher(Arc::new(SafeHttpFetcher::default()))
    }

    pub fn with_fetcher(fetcher: Arc<dyn HttpFetcher>) -> Self {
        Self {
            endpoint: Url::parse(RAINVIEWER_INDEX_URL).expect("constant URL is valid"),
            fetcher,
        }
    }
}

#[async_trait]
impl Collector for RainViewerCollector {
    fn name(&self) -> &'static str {
        "rainviewer-radar"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        collect_http(
            self.name(),
            self.fetcher.as_ref(),
            &self.endpoint,
            context,
            &[("accept", "application/json")],
            |response| parse_rainviewer_index(&response.body, Utc::now().timestamp()),
        )
        .await
    }
}

#[derive(Debug, Deserialize)]
struct Index {
    host: Option<String>,
    radar: Option<Radar>,
}

#[derive(Debug, Deserialize)]
struct Radar {
    past: Option<Vec<Frame>>,
}

#[derive(Debug, Deserialize)]
struct Frame {
    time: Option<i64>,
    path: Option<String>,
}

/// Publishes at most one item: the newest observed frame.
///
/// An empty result is a legitimate outcome — a stalled feed or an index with no
/// past frames — and is reported as no radar rather than as an error, so a
/// cosmetic overlay going quiet never takes a source's health down with it.
pub fn parse_rainviewer_index(
    body: &[u8],
    now_seconds: i64,
) -> Result<Vec<CollectorItem>, CollectorError> {
    let index: Index =
        serde_json::from_slice(body).map_err(|error| CollectorError::SchemaChanged {
            collector: "rainviewer-radar",
            detail: format!("index is not valid JSON: {error}"),
        })?;

    let host = index
        .host
        .as_deref()
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "rainviewer-radar",
            detail: "index host is missing".into(),
        })?;
    // The host arrives in the payload, so it is treated as untrusted input: a
    // redirected host would otherwise silently become a URL the app fetches.
    let host = Url::parse(host).map_err(|error| CollectorError::SchemaChanged {
        collector: "rainviewer-radar",
        detail: format!("index host is not a URL: {error}"),
    })?;
    if host.scheme() != "https" || host.host_str() != Some("tilecache.rainviewer.com") {
        return Err(CollectorError::SchemaChanged {
            collector: "rainviewer-radar",
            detail: format!("unexpected tile host {host}"),
        });
    }

    // Latest by timestamp rather than by position: ordering is a convention of
    // the feed, and the newest frame is the one that matters.
    let Some(frame) = index
        .radar
        .and_then(|radar| radar.past)
        .unwrap_or_default()
        .into_iter()
        .filter(|frame| frame.time.is_some() && frame.path.is_some())
        .max_by_key(|frame| frame.time.unwrap_or_default())
    else {
        return Ok(Vec::new());
    };
    let observed_seconds = frame.time.unwrap_or_default();
    if now_seconds.saturating_sub(observed_seconds) > MAX_FRAME_AGE_SECONDS {
        return Ok(Vec::new());
    }
    let observed_at = DateTime::<Utc>::from_timestamp(observed_seconds, 0).ok_or_else(|| {
        CollectorError::SchemaChanged {
            collector: "rainviewer-radar",
            detail: format!("frame timestamp {observed_seconds} is out of range"),
        }
    })?;

    // A path is a path. Anything that could escape the tile namespace or carry
    // its own query is rejected rather than normalized, because the value is
    // about to be concatenated onto a host and fetched.
    let path = frame.path.unwrap_or_default();
    let path = path.trim();
    if !path.starts_with("/v2/radar/")
        || path.contains("..")
        || path.contains(['?', '#', '\\', ' '])
        || path.len() > 128
    {
        return Err(CollectorError::SchemaChanged {
            collector: "rainviewer-radar",
            detail: "frame path is not a radar tile path".into(),
        });
    }

    Ok(vec![CollectorItem {
        id: format!("rainviewer:radar:{observed_seconds}"),
        kind: ItemKind::RadarFrame,
        title: "Radar composite".into(),
        summary: None,
        observed_at: Some(observed_at),
        starts_at: None,
        ends_at: None,
        location: None,
        source: SourceLink {
            name: "RainViewer".into(),
            url: Some(Url::parse("https://www.rainviewer.com/").expect("constant URL is valid")),
        },
        attributes: BTreeMap::from([
            ("host".into(), json!(host.as_str().trim_end_matches('/'))),
            ("path".into(), json!(path)),
        ]),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_786_844_400;

    fn index_json(host: &str, path: &str, time: i64) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "version": "2.0",
            "generated": time,
            "host": host,
            "radar": {
                "past": [
                    {"time": time - 600, "path": "/v2/radar/older"},
                    {"time": time, "path": path},
                ],
                // Extrapolated frames are present in the real feed and must
                // never be published as observations.
                "nowcast": [{"time": time + 600, "path": "/v2/radar/nowcast"}],
            },
        }))
        .unwrap()
    }

    #[test]
    fn publishes_only_the_newest_observed_frame() {
        let items = parse_rainviewer_index(
            &index_json(
                "https://tilecache.rainviewer.com",
                "/v2/radar/5ae56b545fdc",
                NOW,
            ),
            NOW,
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::RadarFrame);
        assert_eq!(items[0].attributes["path"], json!("/v2/radar/5ae56b545fdc"));
        assert_eq!(
            items[0].attributes["host"],
            json!("https://tilecache.rainviewer.com")
        );
        assert_eq!(items[0].observed_at.unwrap().timestamp(), NOW);
    }

    /// The item is a pointer. Imagery in this path would be persisted as JSON
    /// on every poll.
    #[test]
    fn an_item_never_carries_tile_bytes() {
        let items = parse_rainviewer_index(
            &index_json("https://tilecache.rainviewer.com", "/v2/radar/abc", NOW),
            NOW,
        )
        .unwrap();
        let serialized = serde_json::to_vec(&items[0]).unwrap();
        assert!(serialized.len() < 1_024, "{} bytes", serialized.len());
    }

    #[test]
    fn a_stalled_feed_reports_no_radar_rather_than_stale_radar() {
        let stale = NOW - 45 * 60;
        assert!(
            parse_rainviewer_index(
                &index_json("https://tilecache.rainviewer.com", "/v2/radar/abc", stale),
                NOW
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn an_index_without_past_frames_is_empty_rather_than_an_error() {
        let body = serde_json::to_vec(&json!({
            "host": "https://tilecache.rainviewer.com",
            "radar": {"past": [], "nowcast": []},
        }))
        .unwrap();
        assert!(parse_rainviewer_index(&body, NOW).unwrap().is_empty());
    }

    /// Host and path both arrive in the payload and are both about to be
    /// concatenated into a URL the app fetches.
    #[test]
    fn a_redirected_host_or_an_escaping_path_is_rejected() {
        assert!(
            parse_rainviewer_index(
                &index_json("https://tiles.example.com", "/v2/radar/abc", NOW),
                NOW
            )
            .is_err()
        );
        assert!(
            parse_rainviewer_index(
                &index_json("http://tilecache.rainviewer.com", "/v2/radar/abc", NOW),
                NOW
            )
            .is_err()
        );
        for path in [
            "/v2/radar/../../etc/passwd",
            "/other/path",
            "/v2/radar/abc?redirect=https://example.com",
            "https://example.com/v2/radar/abc",
        ] {
            assert!(
                parse_rainviewer_index(
                    &index_json("https://tilecache.rainviewer.com", path, NOW),
                    NOW
                )
                .is_err(),
                "accepted {path}"
            );
        }
    }
}
