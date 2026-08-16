use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use url::Url;

/// Conditional-request metadata. A successful response replaces these values;
/// a 304 response preserves them.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CollectorCursor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// Reserved for collectors which need more than HTTP validators.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct CollectContext {
    pub cursor: Option<CollectorCursor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Bridge,
    WeatherCurrent,
    WeatherHourly,
    /// One 15-minute precipitation bin. Hourly buckets can only answer "some
    /// time in the next hour"; these can answer "in eight minutes", which is
    /// the difference between a forecast and a warning.
    WeatherMinutely,
    OfficialAlert,
    TropicalCyclone,
    Earthquake,
    News,
    MarketQuote,
    /// A scheduled ship movement. Unlike [`ItemKind::Bridge`], which reports a
    /// bridge that has already moved, this is a forward-looking event: a Miami
    /// River transit that will require the bascule bridges to open.
    VesselMovement,
    /// A pointer to one observed radar composite: where the imagery lives, not
    /// the imagery itself. Items are persisted as JSON, so tile bytes must
    /// never travel this way.
    RadarFrame,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Location {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
}

impl Location {
    pub fn point(latitude: f64, longitude: f64) -> Self {
        Self {
            name: None,
            latitude: Some(latitude),
            longitude: Some(longitude),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceLink {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<Url>,
}

/// Source-normalized observation. Flexible `attributes` retain source-specific
/// facts while the rest of the application receives predictable envelope fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectorItem {
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    pub source: SourceLink,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Healthy,
    Degraded,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectorHealth {
    pub state: HealthState,
    pub checked_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl CollectorHealth {
    pub fn healthy() -> Self {
        Self {
            state: HealthState::Healthy,
            checked_at: Utc::now(),
            message: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectorBatch {
    pub source: String,
    pub items: Vec<CollectorItem>,
    pub health: CollectorHealth,
    pub cursor: CollectorCursor,
    /// True means the origin returned HTTP 304 and `items` intentionally contains
    /// no replacement snapshot.
    #[serde(default)]
    pub not_modified: bool,
}

#[derive(Debug, Error)]
pub enum CollectorError {
    #[error("invalid collector configuration: {0}")]
    Configuration(String),
    #[error("unsafe URL rejected: {0}")]
    UnsafeUrl(String),
    #[error("request failed: {0}")]
    Request(String),
    #[error("request to {url} timed out after {limit:?}")]
    Timeout {
        url: String,
        limit: std::time::Duration,
    },
    #[error("upstream returned HTTP {status}: {url}")]
    Http { status: u16, url: String },
    #[error("response from {url} exceeded {limit} bytes")]
    BodyTooLarge { url: String, limit: usize },
    #[error("upstream schema changed for {collector}: {detail}")]
    SchemaChanged {
        collector: &'static str,
        detail: String,
    },
    #[error("could not parse {collector}: {detail}")]
    Parse {
        collector: &'static str,
        detail: String,
    },
    #[error("redirect from {from} has no valid Location header")]
    InvalidRedirect { from: String },
    #[error("too many redirects (limit {0})")]
    TooManyRedirects(usize),
    #[error("DNS lookup failed for {host}: {detail}")]
    Dns { host: String, detail: String },
}

#[cfg(feature = "native")]
impl From<reqwest::Error> for CollectorError {
    fn from(error: reqwest::Error) -> Self {
        let detail = if error.is_timeout() {
            "request timed out"
        } else if error.is_connect() {
            "connection failed"
        } else if error.is_request() {
            "request construction failed"
        } else if error.is_body() {
            "response body failed"
        } else if error.is_decode() {
            "response decoding failed"
        } else if error.status().is_some() {
            "upstream returned an HTTP error"
        } else {
            "transport error"
        };
        Self::Request(detail.into())
    }
}

#[async_trait]
pub trait Collector: Send + Sync {
    fn name(&self) -> &'static str;
    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError>;
}
