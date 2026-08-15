use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use url::Url;

use crate::{
    CollectContext, Collector, CollectorBatch, CollectorCursor, CollectorError, CollectorHealth,
    CollectorItem, HealthState, HttpFetcher, ItemKind, Location, SafeHttpFetcher, SourceLink,
    geo::haversine_meters,
};

const DEFAULT_LAYER_URL: &str = "https://fl511.com/map/mapIcons/Bridge";
const DEFAULT_TOOLTIP_TEMPLATE: &str = "https://fl511.com/tooltip/Bridge/{id}?lang=en";
const MAX_TOOLTIP_CANDIDATES_PER_SELECTOR: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeState {
    Up,
    Down,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeRelation {
    Target,
    Upstream,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BridgeSelector {
    pub key: String,
    pub name_contains: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default = "default_search_radius")]
    pub search_radius_meters: f64,
    pub relation: BridgeRelation,
}

const fn default_search_radius() -> f64 {
    350.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fl511Config {
    pub layer_url: Url,
    pub tooltip_url_template: String,
    pub bridges: Vec<BridgeSelector>,
}

impl Fl511Config {
    /// Brickell plus the closest Miami River bridges whose openings are useful
    /// upstream evidence. Names are still verified from FL511 tooltips.
    pub fn brickell_and_upstream() -> Self {
        Self {
            layer_url: Url::parse(DEFAULT_LAYER_URL).expect("constant URL is valid"),
            tooltip_url_template: DEFAULT_TOOLTIP_TEMPLATE.into(),
            bridges: vec![
                BridgeSelector {
                    key: "brickell".into(),
                    name_contains: "Brickell Avenue Bridge".into(),
                    latitude: 25.7699,
                    longitude: -80.19005,
                    search_radius_meters: 250.0,
                    relation: BridgeRelation::Target,
                },
                BridgeSelector {
                    key: "sw_2_ave".into(),
                    name_contains: "SW 2 Ave".into(),
                    latitude: 25.768907,
                    longitude: -80.197552,
                    search_radius_meters: 250.0,
                    relation: BridgeRelation::Upstream,
                },
                BridgeSelector {
                    key: "sw_1_st".into(),
                    name_contains: "SW 1 ST".into(),
                    latitude: 25.773038,
                    longitude: -80.200591,
                    search_radius_meters: 250.0,
                    relation: BridgeRelation::Upstream,
                },
                BridgeSelector {
                    key: "w_flagler".into(),
                    name_contains: "W FLAGLER".into(),
                    latitude: 25.774205,
                    longitude: -80.201287,
                    search_radius_meters: 250.0,
                    relation: BridgeRelation::Upstream,
                },
            ],
        }
    }
}

impl Default for Fl511Config {
    fn default() -> Self {
        Self::brickell_and_upstream()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fl511LayerEntry {
    pub item_id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub state_hint: BridgeState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BridgeTooltip {
    pub name: String,
    pub state: BridgeState,
    pub roadway: Option<String>,
    pub location: Option<String>,
    pub county: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fl511Discovery {
    pub selector_key: String,
    pub relation: BridgeRelation,
    pub item_id: String,
    pub name: String,
    pub state: BridgeState,
    pub state_hint: BridgeState,
    pub state_conflict: bool,
    pub roadway: Option<String>,
    pub location: Option<String>,
    pub county: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub distance_meters: f64,
    pub tooltip_url: Url,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum Fl511ParseError {
    #[error("FL511 bridge layer schema changed: {0}")]
    SchemaChanged(String),
    #[error("FL511 bridge tooltip could not be parsed: {0}")]
    InvalidTooltip(String),
}

pub fn parse_bridge_layer(body: &[u8]) -> Result<Vec<Fl511LayerEntry>, Fl511ParseError> {
    let root: Value = serde_json::from_slice(body)
        .map_err(|error| Fl511ParseError::SchemaChanged(format!("invalid JSON: {error}")))?;
    let entries = root
        .as_object()
        .and_then(|object| object.get("item2"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Fl511ParseError::SchemaChanged(
                "expected top-level item2 array from /map/mapIcons/Bridge".into(),
            )
        })?;

    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let object = entry.as_object().ok_or_else(|| {
                Fl511ParseError::SchemaChanged(format!("item2[{index}] is not an object"))
            })?;
            let item_id = object
                .get("itemId")
                .and_then(|value| match value {
                    Value::String(value) => Some(value.clone()),
                    Value::Number(value) => Some(value.to_string()),
                    _ => None,
                })
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    Fl511ParseError::SchemaChanged(format!(
                        "item2[{index}].itemId is missing or invalid"
                    ))
                })?;
            let location = object
                .get("location")
                .and_then(Value::as_array)
                .filter(|location| location.len() >= 2)
                .ok_or_else(|| {
                    Fl511ParseError::SchemaChanged(format!(
                        "item2[{index}].location is not [latitude, longitude]"
                    ))
                })?;
            let latitude = location[0].as_f64().ok_or_else(|| {
                Fl511ParseError::SchemaChanged(format!("item2[{index}].location[0] is not numeric"))
            })?;
            let longitude = location[1].as_f64().ok_or_else(|| {
                Fl511ParseError::SchemaChanged(format!("item2[{index}].location[1] is not numeric"))
            })?;
            if !latitude.is_finite()
                || !longitude.is_finite()
                || !(-90.0..=90.0).contains(&latitude)
                || !(-180.0..=180.0).contains(&longitude)
            {
                return Err(Fl511ParseError::SchemaChanged(format!(
                    "item2[{index}].location is outside valid coordinates"
                )));
            }
            let icon = object
                .get("icon")
                .and_then(Value::as_object)
                .and_then(|icon| icon.get("url"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Fl511ParseError::SchemaChanged(format!("item2[{index}].icon.url is missing"))
                })?;
            Ok(Fl511LayerEntry {
                item_id,
                latitude,
                longitude,
                state_hint: state_from_icon_url(icon),
            })
        })
        .collect()
}

pub fn parse_bridge_tooltip(body: &[u8]) -> Result<BridgeTooltip, Fl511ParseError> {
    let body = std::str::from_utf8(body)
        .map_err(|error| Fl511ParseError::InvalidTooltip(error.to_string()))?;
    let document = Html::parse_document(body);
    let bold_selector = Selector::parse(".map-tooltip table b, .map-tooltip h4 + table b")
        .expect("static selector is valid");
    let row_selector = Selector::parse(".map-tooltip table tr").expect("static selector is valid");
    let th_selector = Selector::parse("th").expect("static selector is valid");
    let td_selector = Selector::parse("td").expect("static selector is valid");

    let name = document
        .select(&bold_selector)
        .next()
        .map(element_text)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Fl511ParseError::InvalidTooltip("bridge name is missing".into()))?;

    let mut fields = BTreeMap::new();
    for row in document.select(&row_selector) {
        let Some(key) = row.select(&th_selector).next().map(element_text) else {
            continue;
        };
        let Some(value) = row.select(&td_selector).next().map(element_text) else {
            continue;
        };
        fields.insert(key.to_ascii_lowercase(), value);
    }
    // A missing/unrecognized status is represented explicitly as Unknown. This
    // is distinct from a layer-level schema failure, which returns an error.
    let state = fields
        .get("status")
        .map(|value| state_from_status(value))
        .unwrap_or(BridgeState::Unknown);

    Ok(BridgeTooltip {
        name,
        state,
        roadway: fields.remove("roadway"),
        location: fields.remove("location"),
        county: fields.remove("county"),
    })
}

fn element_text(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn state_from_status(value: &str) -> BridgeState {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    match normalized.as_str() {
        "bridge up" => BridgeState::Up,
        "bridge down" => BridgeState::Down,
        _ => BridgeState::Unknown,
    }
}

fn state_from_icon_url(value: &str) -> BridgeState {
    let filename = value
        .rsplit('/')
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase();
    match filename.as_str() {
        "map_drawbridgeup.svg" | "ic_drawbridgeup.svg" => BridgeState::Up,
        "map_drawbridgedown.svg" | "ic_drawbridgedown.svg" => BridgeState::Down,
        _ => BridgeState::Unknown,
    }
}

pub struct Fl511BridgeCollector {
    config: Fl511Config,
    fetcher: Arc<dyn HttpFetcher>,
}

impl Fl511BridgeCollector {
    pub fn new(config: Fl511Config) -> Result<Self, CollectorError> {
        Self::with_fetcher(config, Arc::new(SafeHttpFetcher::default()))
    }

    pub fn with_fetcher(
        config: Fl511Config,
        fetcher: Arc<dyn HttpFetcher>,
    ) -> Result<Self, CollectorError> {
        if config.bridges.is_empty() {
            return Err(CollectorError::Configuration(
                "FL511 requires at least one bridge selector".into(),
            ));
        }
        if !config.tooltip_url_template.contains("{id}") {
            return Err(CollectorError::Configuration(
                "FL511 tooltip_url_template must contain {id}".into(),
            ));
        }
        for bridge in &config.bridges {
            if bridge.key.trim().is_empty()
                || bridge.name_contains.trim().is_empty()
                || bridge.search_radius_meters <= 0.0
            {
                return Err(CollectorError::Configuration(format!(
                    "invalid FL511 bridge selector {:?}",
                    bridge.key
                )));
            }
        }
        Ok(Self { config, fetcher })
    }

    async fn discover(
        &self,
        entries: &[Fl511LayerEntry],
    ) -> Result<(Vec<Fl511Discovery>, Vec<&BridgeSelector>), CollectorError> {
        let mut discoveries = Vec::new();
        let mut missing = Vec::new();
        for selector in &self.config.bridges {
            let mut candidates: Vec<_> = entries
                .iter()
                .filter_map(|entry| {
                    let distance = haversine_meters(
                        selector.latitude,
                        selector.longitude,
                        entry.latitude,
                        entry.longitude,
                    );
                    (distance <= selector.search_radius_meters).then_some((entry, distance))
                })
                .collect();
            candidates.sort_by(|left, right| left.1.total_cmp(&right.1));

            let mut found = None;
            for (entry, distance_meters) in candidates
                .into_iter()
                .take(MAX_TOOLTIP_CANDIDATES_PER_SELECTOR)
            {
                let tooltip_url = self.tooltip_url(&entry.item_id)?;
                let Ok(response) = self
                    .fetcher
                    .get(&tooltip_url, None, &[("accept", "text/html")])
                    .await
                else {
                    continue;
                };
                let Ok(tooltip) = parse_bridge_tooltip(&response.body) else {
                    continue;
                };
                if tooltip
                    .name
                    .to_ascii_lowercase()
                    .contains(&selector.name_contains.to_ascii_lowercase())
                {
                    let (state, state_conflict) =
                        reconcile_bridge_state(tooltip.state, entry.state_hint);
                    found = Some(Fl511Discovery {
                        selector_key: selector.key.clone(),
                        relation: selector.relation,
                        item_id: entry.item_id.clone(),
                        name: tooltip.name,
                        state,
                        state_hint: entry.state_hint,
                        state_conflict,
                        roadway: tooltip.roadway,
                        location: tooltip.location,
                        county: tooltip.county,
                        latitude: entry.latitude,
                        longitude: entry.longitude,
                        distance_meters,
                        tooltip_url,
                    });
                    break;
                }
            }
            if let Some(found) = found {
                discoveries.push(found);
            } else {
                missing.push(selector);
            }
        }
        Ok((discoveries, missing))
    }

    fn tooltip_url(&self, item_id: &str) -> Result<Url, CollectorError> {
        // FL511 currently emits numeric IDs. Restrict substitution so a changed
        // upstream ID cannot alter the path or query string.
        if !item_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(CollectorError::SchemaChanged {
                collector: "fl511",
                detail: format!("unexpected non-numeric bridge itemId {item_id:?}"),
            });
        }
        Url::parse(&self.config.tooltip_url_template.replace("{id}", item_id)).map_err(|error| {
            CollectorError::Configuration(format!("invalid FL511 tooltip URL: {error}"))
        })
    }
}

#[async_trait]
impl Collector for Fl511BridgeCollector {
    fn name(&self) -> &'static str {
        "fl511-bridges"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        let response = self
            .fetcher
            .get(
                &self.config.layer_url,
                context.cursor.as_ref(),
                &[("accept", "application/json")],
            )
            .await?;
        if response.not_modified {
            return Ok(not_modified_batch(self.name(), response.cursor));
        }
        let entries =
            parse_bridge_layer(&response.body).map_err(|error| CollectorError::SchemaChanged {
                collector: "fl511",
                detail: error.to_string(),
            })?;
        let (discoveries, missing) = self.discover(&entries).await?;
        let has_unknown = discoveries
            .iter()
            .any(|bridge| bridge.state == BridgeState::Unknown);
        let conflict_count = discoveries
            .iter()
            .filter(|bridge| bridge.state_conflict)
            .count();

        let mut items = discoveries
            .into_iter()
            .map(discovery_item)
            .collect::<Vec<_>>();
        items.extend(missing.iter().copied().map(missing_item));
        let degraded = has_unknown || conflict_count > 0 || !missing.is_empty();
        Ok(CollectorBatch {
            source: self.name().into(),
            items,
            health: CollectorHealth {
                state: if degraded {
                    HealthState::Degraded
                } else {
                    HealthState::Healthy
                },
                checked_at: chrono::Utc::now(),
                message: degraded.then(|| {
                    format!(
                        "{} bridge selector(s) missing; {conflict_count} layer/tooltip conflict(s); unknown status values are preserved",
                        missing.len(),
                    )
                }),
            },
            cursor: response.cursor,
            not_modified: false,
        })
    }
}

fn discovery_item(bridge: Fl511Discovery) -> CollectorItem {
    let mut attributes = BTreeMap::new();
    attributes.insert("state".into(), json!(bridge.state));
    attributes.insert("layer_state_hint".into(), json!(bridge.state_hint));
    attributes.insert("state_conflict".into(), json!(bridge.state_conflict));
    attributes.insert("relation".into(), json!(bridge.relation));
    attributes.insert("selector_key".into(), json!(bridge.selector_key));
    attributes.insert("fl511_item_id".into(), json!(bridge.item_id));
    attributes.insert("distance_meters".into(), json!(bridge.distance_meters));
    if let Some(roadway) = bridge.roadway {
        attributes.insert("roadway".into(), json!(roadway));
    }
    if let Some(county) = bridge.county {
        attributes.insert("county".into(), json!(county));
    }
    CollectorItem {
        id: format!("fl511:bridge:{}", bridge.item_id),
        kind: ItemKind::Bridge,
        title: bridge.name,
        summary: Some(
            match bridge.state {
                BridgeState::Up => "Bridge Up",
                BridgeState::Down => "Bridge Down",
                BridgeState::Unknown => "Bridge status unknown",
            }
            .into(),
        ),
        observed_at: None,
        starts_at: None,
        ends_at: None,
        location: Some(Location {
            name: bridge.location,
            latitude: Some(bridge.latitude),
            longitude: Some(bridge.longitude),
        }),
        source: SourceLink {
            name: "Florida 511".into(),
            url: Some(bridge.tooltip_url),
        },
        attributes,
    }
}

fn reconcile_bridge_state(
    tooltip_state: BridgeState,
    layer_state_hint: BridgeState,
) -> (BridgeState, bool) {
    let known_disagreement = tooltip_state != BridgeState::Unknown
        && layer_state_hint != BridgeState::Unknown
        && tooltip_state != layer_state_hint;
    if known_disagreement {
        (BridgeState::Unknown, true)
    } else {
        // The tooltip is the explicit status field. An unknown tooltip remains
        // unknown instead of silently promoting the icon filename to truth.
        (tooltip_state, false)
    }
}

fn missing_item(selector: &BridgeSelector) -> CollectorItem {
    let mut attributes = BTreeMap::new();
    attributes.insert("state".into(), json!(BridgeState::Unknown));
    attributes.insert("relation".into(), json!(selector.relation));
    attributes.insert("selector_key".into(), json!(selector.key));
    CollectorItem {
        id: format!("fl511:bridge-selector:{}", selector.key),
        kind: ItemKind::Bridge,
        title: selector.name_contains.clone(),
        summary: Some("Bridge could not be discovered in the current FL511 layer".into()),
        observed_at: None,
        starts_at: None,
        ends_at: None,
        location: Some(Location::point(selector.latitude, selector.longitude)),
        source: SourceLink {
            name: "Florida 511".into(),
            url: Some(Url::parse(DEFAULT_LAYER_URL).expect("constant URL is valid")),
        },
        attributes,
    }
}

fn not_modified_batch(source: &str, cursor: CollectorCursor) -> CollectorBatch {
    CollectorBatch {
        source: source.into(),
        items: Vec::new(),
        health: CollectorHealth::healthy(),
        cursor,
        not_modified: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FetchResponse;
    use std::sync::Mutex;

    struct FixtureFetcher {
        seen_cursors: Mutex<Vec<Option<CollectorCursor>>>,
        brickell_conflict: bool,
    }

    #[async_trait]
    impl HttpFetcher for FixtureFetcher {
        async fn get(
            &self,
            url: &Url,
            cursor: Option<&CollectorCursor>,
            _headers: &[(&str, &str)],
        ) -> Result<FetchResponse, CollectorError> {
            self.seen_cursors.lock().unwrap().push(cursor.cloned());
            let (body, response_cursor) = match url.path() {
                "/map/mapIcons/Bridge" => (
                    include_bytes!("../fixtures/fl511-layer.json").to_vec(),
                    CollectorCursor {
                        etag: Some("\"layer-v2\"".into()),
                        last_modified: Some("Fri, 14 Aug 2026 20:00:00 GMT".into()),
                        metadata: BTreeMap::new(),
                    },
                ),
                "/tooltip/Bridge/253" => (
                    if self.brickell_conflict {
                        include_bytes!("../fixtures/fl511-tooltip-brickell-down.html").to_vec()
                    } else {
                        include_bytes!("../fixtures/fl511-tooltip-brickell-up.html").to_vec()
                    },
                    CollectorCursor::default(),
                ),
                "/tooltip/Bridge/261" => (
                    include_bytes!("../fixtures/fl511-tooltip-upstream-down.html").to_vec(),
                    CollectorCursor::default(),
                ),
                path => panic!("unexpected fixture request for {path}"),
            };
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                body,
                cursor: response_cursor,
                not_modified: false,
                content_type: None,
            })
        }
    }

    #[test]
    fn parses_layer_and_state_hints() {
        let entries = parse_bridge_layer(include_bytes!("../fixtures/fl511-layer.json")).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].item_id, "253");
        assert_eq!(entries[0].state_hint, BridgeState::Up);
        assert_eq!(entries[1].state_hint, BridgeState::Down);
    }

    #[test]
    fn parses_tooltip_name_fields_and_up_state() {
        let tooltip =
            parse_bridge_tooltip(include_bytes!("../fixtures/fl511-tooltip-brickell-up.html"))
                .unwrap();
        assert_eq!(tooltip.name, "Brickell Avenue Bridge");
        assert_eq!(tooltip.state, BridgeState::Up);
        assert_eq!(tooltip.roadway.as_deref(), Some("US-1 N"));
        assert_eq!(tooltip.county.as_deref(), Some("Miami-Dade"));
    }

    #[test]
    fn unknown_tooltip_status_is_explicit() {
        let tooltip =
            parse_bridge_tooltip(include_bytes!("../fixtures/fl511-tooltip-unknown.html")).unwrap();
        assert_eq!(tooltip.state, BridgeState::Unknown);
    }

    #[test]
    fn parses_upstream_bridge_down() {
        let tooltip = parse_bridge_tooltip(include_bytes!(
            "../fixtures/fl511-tooltip-upstream-down.html"
        ))
        .unwrap();
        assert_eq!(tooltip.name, "SW 2 Ave - Miami River");
        assert_eq!(tooltip.state, BridgeState::Down);
    }

    #[test]
    fn status_parser_rejects_negation_and_incidental_substrings() {
        assert_eq!(state_from_status("Bridge Up"), BridgeState::Up);
        assert_eq!(state_from_status("Not Bridge Up"), BridgeState::Unknown);
        assert_eq!(
            state_from_status("Bridge update pending"),
            BridgeState::Unknown
        );
        assert_eq!(
            state_from_icon_url("/Generated/map_drawBridgeDown.svg"),
            BridgeState::Down
        );
        assert_eq!(
            state_from_icon_url("/Generated/not_map_drawBridgeUp.svg"),
            BridgeState::Unknown
        );
    }

    #[test]
    fn layer_schema_change_is_an_error_not_an_empty_snapshot() {
        let error =
            parse_bridge_layer(include_bytes!("../fixtures/fl511-layer-schema-change.json"))
                .unwrap_err();
        assert!(matches!(error, Fl511ParseError::SchemaChanged(_)));
        assert!(error.to_string().contains("item2"));
    }

    #[tokio::test]
    async fn discovers_target_and_upstream_by_coordinate_then_name() {
        let fetcher = Arc::new(FixtureFetcher {
            seen_cursors: Mutex::new(Vec::new()),
            brickell_conflict: false,
        });
        let config = Fl511Config {
            layer_url: Url::parse(DEFAULT_LAYER_URL).unwrap(),
            tooltip_url_template: DEFAULT_TOOLTIP_TEMPLATE.into(),
            bridges: Fl511Config::brickell_and_upstream()
                .bridges
                .into_iter()
                .take(2)
                .collect(),
        };
        let collector = Fl511BridgeCollector::with_fetcher(config, fetcher.clone()).unwrap();
        let context = CollectContext {
            cursor: Some(CollectorCursor {
                etag: Some("\"layer-v1\"".into()),
                ..CollectorCursor::default()
            }),
        };

        let batch = collector.collect(&context).await.unwrap();

        assert_eq!(batch.items.len(), 2);
        assert_eq!(batch.cursor.etag.as_deref(), Some("\"layer-v2\""));
        assert_eq!(batch.items[0].attributes["state"], json!(BridgeState::Up));
        assert_eq!(
            batch.items[1].attributes["relation"],
            json!(BridgeRelation::Upstream)
        );
        let seen = fetcher.seen_cursors.lock().unwrap();
        assert_eq!(
            seen[0].as_ref().unwrap().etag.as_deref(),
            Some("\"layer-v1\"")
        );
        assert!(seen[1].is_none());
        assert!(seen[2].is_none());
    }

    #[tokio::test]
    async fn known_layer_tooltip_disagreement_fails_closed_and_degrades_health() {
        let fetcher = Arc::new(FixtureFetcher {
            seen_cursors: Mutex::new(Vec::new()),
            brickell_conflict: true,
        });
        let config = Fl511Config {
            layer_url: Url::parse(DEFAULT_LAYER_URL).unwrap(),
            tooltip_url_template: DEFAULT_TOOLTIP_TEMPLATE.into(),
            bridges: Fl511Config::brickell_and_upstream()
                .bridges
                .into_iter()
                .take(1)
                .collect(),
        };
        let collector = Fl511BridgeCollector::with_fetcher(config, fetcher).unwrap();

        let batch = collector.collect(&CollectContext::default()).await.unwrap();

        assert_eq!(batch.health.state, HealthState::Degraded);
        assert_eq!(
            batch.items[0].attributes["state"],
            json!(BridgeState::Unknown)
        );
        assert_eq!(batch.items[0].attributes["state_conflict"], json!(true));
        assert!(
            batch
                .health
                .message
                .as_deref()
                .is_some_and(|message| message.contains("1 layer/tooltip conflict"))
        );
    }
}
