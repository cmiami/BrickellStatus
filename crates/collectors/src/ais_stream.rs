//! Optional, backend-only AISStream vessel-position adapter.
//!
//! AISStream explicitly requires its API key to stay on a backend. The secret
//! type in this module cannot be serialized and redacts its `Debug` output.
//! The collector maintains one bounded WebSocket subscription in the
//! background and exposes only a small, fresh snapshot through the ordinary
//! collector contract.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{sync::RwLock, time::timeout};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroize;

use crate::{
    CollectContext, Collector, CollectorBatch, CollectorCursor, CollectorError, CollectorHealth,
    CollectorItem, HealthState, ItemKind, Location, SourceLink,
    geo::haversine_meters,
    river::{self, RiverBranch},
};

const AISSTREAM_ENDPOINT: &str = "wss://stream.aisstream.io/v0/stream";
const AISSTREAM_SOURCE_URL: &str = "https://aisstream.io/documentation.html";
const MAX_WEBSOCKET_MESSAGE_BYTES: usize = 128 * 1024;
const MAX_TRACKED_VESSELS: usize = 512;
const MAX_EXPOSED_TRACKS: usize = 3;
const MAX_HISTORY_TRACKS: usize = 64;
const MAX_HISTORY_POINTS_PER_VESSEL: usize = 121;
const HISTORY_SAMPLE_SECONDS: i64 = 30;
const MAX_REPORT_FUTURE_SKEW_SECONDS: i64 = 30;
const MIN_API_KEY_CHARS: usize = 8;
const MAX_API_KEY_CHARS: usize = 512;
const MAX_PENDING_CROSSINGS: usize = 32;
/// The Miami River is maintained to roughly 4.6 m; a hull drawing this much
/// water cannot enter it, whatever its course claims.
const RIVER_MAX_DRAUGHT_METERS: f64 = 4.5;
/// A vessel whose fixes stay inside this displacement for the moored window
/// is tied up, whatever its NavigationalStatus claims.
const MOORED_DISPLACEMENT_METERS: f64 = 50.0;
const MOORED_WINDOW_SECONDS: i64 = 10 * 60;

/// Cursor metadata containing bounded, non-secret vessel courses for the map.
pub const AIS_VESSEL_TRACKS_CURSOR_KEY: &str = "vessel_tracks";
/// Cursor metadata carrying bridge-line crossings observed since the previous
/// collect, for durable transit/ledger recording by the runtime.
pub const AIS_CROSSINGS_CURSOR_KEY: &str = "ais_crossings";

/// One vessel passing the Brickell bridge line, in either direction.
///
/// Crossings are the raw material of the per-vessel opening ledger: the
/// runtime joins them against recorded bridge state to learn which hulls
/// force an opening and which fit under.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AisCrossing {
    pub mmsi: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessel_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessel_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_meters: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draught_meters: Option<f64>,
    /// "upriver" or "downriver".
    pub direction: String,
    pub crossed_at: DateTime<Utc>,
    pub speed_knots: f64,
}

struct SecretInner(String);

impl Drop for SecretInner {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Backend-only AISStream API key.
///
/// The value is deliberately neither `Serialize` nor `Deserialize`, and its
/// `Debug` representation never contains key material.
#[derive(Clone)]
pub struct AisStreamApiKey(Arc<SecretInner>);

impl AisStreamApiKey {
    pub fn new(value: impl Into<String>) -> Result<Self, CollectorError> {
        let value = value.into();
        let characters = value.chars().count();
        if value.trim() != value
            || !(MIN_API_KEY_CHARS..=MAX_API_KEY_CHARS).contains(&characters)
            || value.chars().any(char::is_control)
        {
            return Err(CollectorError::Configuration(
                "AISStream API key has an invalid shape".into(),
            ));
        }
        Ok(Self(Arc::new(SecretInner(value))))
    }

    fn expose(&self) -> &str {
        &self.0.0
    }
}

impl fmt::Debug for AisStreamApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AisStreamApiKey([REDACTED])")
    }
}

/// Public, non-secret geometry for one bounded AISStream subscription.
#[derive(Clone, Debug, PartialEq)]
pub struct AisStreamSubscription {
    bridge_label: String,
    bridge_latitude: f64,
    bridge_longitude: f64,
    radius_kilometers: f64,
    bounding_boxes: Vec<[[f64; 2]; 2]>,
    /// Whether vessel reasoning runs in Miami River channel coordinates.
    corridor: bool,
}

impl AisStreamSubscription {
    /// Chooses the right geometry for a bridge target: the Brickell Avenue
    /// Bridge gets the surveyed Miami River corridor (river tiles plus the two
    /// marked entrance channels); any other target keeps the generic square.
    pub fn for_bridge(
        bridge_label: impl Into<String>,
        bridge_latitude: f64,
        bridge_longitude: f64,
        radius_kilometers: f64,
    ) -> Result<Self, CollectorError> {
        if river::is_brickell_target(bridge_latitude, bridge_longitude) {
            let mut subscription = Self::around_bridge(
                bridge_label,
                bridge_latitude,
                bridge_longitude,
                radius_kilometers,
            )?;
            subscription.bounding_boxes = river::corridor_bounding_boxes();
            subscription.corridor = true;
            Ok(subscription)
        } else {
            Self::around_bridge(
                bridge_label,
                bridge_latitude,
                bridge_longitude,
                radius_kilometers,
            )
        }
    }

    /// Builds a square subscription around a bridge point. Radius is bounded to
    /// avoid accidentally subscribing to a city, country, or the entire world.
    pub fn around_bridge(
        bridge_label: impl Into<String>,
        bridge_latitude: f64,
        bridge_longitude: f64,
        radius_kilometers: f64,
    ) -> Result<Self, CollectorError> {
        let bridge_label = bridge_label.into();
        if bridge_label.trim().is_empty() || bridge_label.chars().count() > 120 {
            return Err(CollectorError::Configuration(
                "AIS bridge label must contain 1 to 120 characters".into(),
            ));
        }
        if !valid_coordinate(bridge_latitude, bridge_longitude) {
            return Err(CollectorError::Configuration(
                "AIS bridge coordinates are invalid".into(),
            ));
        }
        if !radius_kilometers.is_finite() || !(2.0..=30.0).contains(&radius_kilometers) {
            return Err(CollectorError::Configuration(
                "AIS subscription radius must be between 2 and 30 kilometers".into(),
            ));
        }

        let latitude_delta = radius_kilometers / 111.32;
        let longitude_scale = bridge_latitude.to_radians().cos().abs();
        if longitude_scale < 0.1 {
            return Err(CollectorError::Configuration(
                "AIS bridge latitude is too close to a pole".into(),
            ));
        }
        let longitude_delta = radius_kilometers / (111.32 * longitude_scale);
        let south = (bridge_latitude - latitude_delta).max(-90.0);
        let north = (bridge_latitude + latitude_delta).min(90.0);
        let west = (bridge_longitude - longitude_delta).max(-180.0);
        let east = (bridge_longitude + longitude_delta).min(180.0);

        Ok(Self {
            bridge_label,
            bridge_latitude,
            bridge_longitude,
            radius_kilometers,
            bounding_boxes: vec![[[south, west], [north, east]]],
            corridor: false,
        })
    }

    pub fn radius_kilometers(&self) -> f64 {
        self.radius_kilometers
    }

    pub fn bounding_boxes(&self) -> &[[[f64; 2]; 2]] {
        &self.bounding_boxes
    }

    fn contains(&self, latitude: f64, longitude: f64) -> bool {
        self.bounding_boxes.iter().any(|bounds| {
            let south = bounds[0][0].min(bounds[1][0]);
            let north = bounds[0][0].max(bounds[1][0]);
            let west = bounds[0][1].min(bounds[1][1]);
            let east = bounds[0][1].max(bounds[1][1]);
            (south..=north).contains(&latitude) && (west..=east).contains(&longitude)
        })
    }
}

/// Live adapter configuration. Its `Debug` output redacts the API key.
#[derive(Clone)]
pub struct AisStreamConfig {
    api_key: AisStreamApiKey,
    subscription: AisStreamSubscription,
    /// Oldest position report still treated as describing the present.
    ///
    /// This has to clear the transmitter's own reporting interval or the
    /// vessels that matter most are structurally invisible. Class B carriage --
    /// the yachts and sailboats behind most Brickell openings -- reports every
    /// three minutes below two knots, and Class A drops to three minutes at
    /// anchor or moored. A vessel *stopped and waiting at the bridge* is
    /// therefore the slowest reporter there is, and a window shorter than its
    /// interval discards it every time.
    max_report_age: Duration,
    track_retention: Duration,
    history_retention: Duration,
    idle_timeout: Duration,
    reconnect_initial: Duration,
    reconnect_max: Duration,
}

impl AisStreamConfig {
    pub fn new(api_key: AisStreamApiKey, subscription: AisStreamSubscription) -> Self {
        Self {
            api_key,
            subscription,
            // Twice the three-minute Class B interval, so a queued vessel
            // survives a missed report rather than vanishing between them.
            max_report_age: Duration::from_secs(6 * 60),
            track_retention: Duration::from_secs(6 * 60),
            history_retention: Duration::from_secs(60 * 60),
            idle_timeout: Duration::from_secs(90),
            reconnect_initial: Duration::from_secs(1),
            reconnect_max: Duration::from_secs(60),
        }
    }

    pub fn subscription(&self) -> &AisStreamSubscription {
        &self.subscription
    }
}

impl fmt::Debug for AisStreamConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AisStreamConfig")
            .field("api_key", &"[REDACTED]")
            .field("subscription", &self.subscription)
            .field("max_report_age", &self.max_report_age)
            .field("track_retention", &self.track_retention)
            .field("history_retention", &self.history_retention)
            .field("idle_timeout", &self.idle_timeout)
            .field("reconnect_initial", &self.reconnect_initial)
            .field("reconnect_max", &self.reconnect_max)
            .finish()
    }
}

/// A continuously maintained AISStream collector.
///
/// The first `collect` starts the backend WebSocket reader. Subsequent calls
/// return its latest bounded snapshot without reconnecting or exposing the key.
pub struct AisStreamCollector {
    config: AisStreamConfig,
    state: Arc<RwLock<StreamState>>,
    started: AtomicBool,
    cancellation: CancellationToken,
}

impl AisStreamCollector {
    pub fn new(config: AisStreamConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(StreamState::default())),
            started: AtomicBool::new(false),
            cancellation: CancellationToken::new(),
        }
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    async fn ensure_started(&self, cursor: Option<&CollectorCursor>) {
        if self
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        if let Some(encoded) =
            cursor.and_then(|cursor| cursor.metadata.get(AIS_VESSEL_TRACKS_CURSOR_KEY))
            && let Ok(histories) = serde_json::from_str::<Vec<VesselHistory>>(encoded)
        {
            let mut state = self.state.write().await;
            state.histories =
                validated_histories(histories, Utc::now(), self.config.history_retention);
        }
        let config = self.config.clone();
        let state = Arc::clone(&self.state);
        let cancellation = self.cancellation.clone();
        tokio::spawn(async move {
            run_stream(config, state, cancellation).await;
        });
    }
}

impl Drop for AisStreamCollector {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[async_trait]
impl Collector for AisStreamCollector {
    fn name(&self) -> &'static str {
        "aisstream"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        self.ensure_started(context.cursor.as_ref()).await;
        let now = Utc::now();
        let mut state = self.state.write().await;
        if state.health != HealthState::Healthy {
            return Err(CollectorError::Request(
                state
                    .failure
                    .unwrap_or(StreamFailure::Starting)
                    .detail()
                    .into(),
            ));
        }
        let crossings = state.crossings.drain(..).collect::<Vec<_>>();
        let state = tokio::sync::RwLockWriteGuard::downgrade(state);
        let cutoff = now
            - TimeDelta::from_std(self.config.track_retention)
                .unwrap_or_else(|_| TimeDelta::seconds(60));
        let mut tracks = state
            .tracks
            .values()
            .filter(|track| track.observed_at >= cutoff)
            .collect::<Vec<_>>();
        tracks.sort_by(|left, right| {
            track_priority(left)
                .cmp(&track_priority(right))
                .then_with(|| left.distance_meters.total_cmp(&right.distance_meters))
                .then_with(|| left.mmsi.cmp(&right.mmsi))
        });
        let fresh_vessel_count = tracks.len();
        let approaching_count = tracks
            .iter()
            .filter(|track| track_priority(track).0 == 0)
            .count();
        let retention = TimeDelta::from_std(self.config.track_retention)
            .unwrap_or_else(|_| TimeDelta::seconds(60));
        let fresh_vessel_expirations = tracks
            .iter()
            .map(|track| (track.observed_at + retention).timestamp_millis())
            .collect::<Vec<_>>();
        let latest_position_at = tracks
            .iter()
            .map(|track| track.observed_at.timestamp_millis())
            .max();
        let mut items: Vec<CollectorItem> = tracks
            .into_iter()
            .take(MAX_EXPOSED_TRACKS)
            .map(|track| track.item.clone())
            .collect();
        // The lead approaching vessel announces the rest of the queue, so a
        // convoy or a stack of waiting yachts reads as one story.
        if approaching_count > 1
            && let Some(lead) = items.first_mut()
        {
            let queued = approaching_count - 1;
            lead.attributes.insert("queued_count".into(), json!(queued));
            if let Some(summary) = lead.summary.as_mut() {
                summary.push_str(&format!(" · +{queued} queued"));
            }
        }
        let mut cursor = CollectorCursor::default();
        if !crossings.is_empty()
            && let Ok(encoded) = serde_json::to_string(&crossings)
        {
            cursor
                .metadata
                .insert(AIS_CROSSINGS_CURSOR_KEY.into(), encoded);
        }
        cursor
            .metadata
            .insert("fresh_vessel_count".into(), fresh_vessel_count.to_string());
        if let Ok(expirations) = serde_json::to_string(&fresh_vessel_expirations) {
            cursor
                .metadata
                .insert("fresh_vessel_expirations_ms".into(), expirations);
        }
        if let Some(latest_position_at) = latest_position_at {
            cursor
                .metadata
                .insert("last_position_at_ms".into(), latest_position_at.to_string());
        }
        let mut histories = state.histories.values().cloned().collect::<Vec<_>>();
        histories.sort_by_key(|track| std::cmp::Reverse(track.observed_at));
        histories.truncate(MAX_HISTORY_TRACKS);
        if let Ok(encoded) = serde_json::to_string(&histories) {
            cursor
                .metadata
                .insert(AIS_VESSEL_TRACKS_CURSOR_KEY.into(), encoded);
        }

        Ok(CollectorBatch {
            source: "AISStream".into(),
            items,
            health: CollectorHealth {
                state: HealthState::Healthy,
                checked_at: now,
                message: None,
            },
            cursor,
            not_modified: false,
        })
    }
}

struct StreamState {
    health: HealthState,
    failure: Option<StreamFailure>,
    tracks: BTreeMap<u32, VesselTrack>,
    histories: BTreeMap<u32, VesselHistory>,
    statics: BTreeMap<u32, VesselStatic>,
    crossings: VecDeque<AisCrossing>,
}

impl Default for StreamState {
    fn default() -> Self {
        Self {
            health: HealthState::Unknown,
            failure: Some(StreamFailure::Starting),
            tracks: BTreeMap::new(),
            histories: BTreeMap::new(),
            statics: BTreeMap::new(),
            crossings: VecDeque::new(),
        }
    }
}

/// Identity and hull facts accumulated from static AIS messages.
#[derive(Clone, Debug, Default)]
struct VesselStatic {
    name: Option<String>,
    /// Radio call sign, broadcast alongside the name. The only human-readable
    /// identity many working hulls give beyond their MMSI.
    call_sign: Option<String>,
    /// IMO number: the hull's permanent global identity, unlike an MMSI, which
    /// follows the radio licence and changes with owner or flag.
    imo_number: Option<u32>,
    /// Skipper-entered destination. Free text, frequently blank or padding,
    /// but a hull that types a river berth has declared it is coming through.
    destination: Option<String>,
    ship_type: Option<u16>,
    length_meters: Option<f64>,
    /// Beam from the reported dimensions. Drawn hulls need a width as well as
    /// a length, and a tug and a barge of the same length are not the same
    /// shape on the water.
    beam_meters: Option<f64>,
    draught_meters: Option<f64>,
    updated_at: Option<DateTime<Utc>>,
}

impl VesselStatic {
    /// Friendly class word for surfaces; AIS ship-type first digit families.
    fn class_word(&self) -> Option<&'static str> {
        let word = match self.ship_type? {
            30 => "fishing",
            31 | 32 => "tug + tow",
            36 => "sailing",
            37 => "pleasure craft",
            50 => "pilot",
            52 => "tug",
            60..=69 => "passenger",
            70..=79 => "cargo",
            80..=89 => "tanker",
            _ => return Some("vessel"),
        };
        Some(word)
    }

    /// Hulls this deep cannot enter the river at all.
    fn river_capable(&self) -> bool {
        self.draught_meters
            .is_none_or(|draught| draught < RIVER_MAX_DRAUGHT_METERS)
    }
}

#[derive(Clone)]
struct VesselTrack {
    mmsi: u32,
    observed_at: DateTime<Utc>,
    distance_meters: f64,
    item: CollectorItem,
    point: TrackPoint,
}

#[derive(Clone, Copy)]
struct TrackPoint {
    observed_at: DateTime<Utc>,
    /// Channel distance to the bridge in corridor mode, straight-line
    /// distance otherwise: the quantity whose shrinkage means "closing".
    distance_meters: f64,
    /// Signed channel coordinate, corridor mode only.
    s_meters: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VesselHistory {
    mmsi: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    vessel_name: Option<String>,
    movement: String,
    route_intersects: bool,
    speed_knots: f64,
    course_degrees: f64,
    observed_at: DateTime<Utc>,
    /// Broadcast ship-type word, when the vessel has sent a static report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vessel_class: Option<String>,
    /// Behavioral standing: `underway`, `moored`, `waiting`, `holding`,
    /// `off_channel`, or `deep_draft`. This is what separates a vessel on
    /// passage from a hull tied up beside the channel, so a surface can drop
    /// the moored fleet without re-deriving the test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    posture: Option<String>,
    /// Signed channel meters to the Brickell span: positive upriver, negative
    /// seaward. Absent when the fix was not projected in corridor mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s_meters: Option<f64>,
    /// Which charted branch the fix projected onto: `river`, `north_approach`
    /// or `south_approach`. Seaward traffic is only placeable on a diagram
    /// once this says which entrance channel it is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    call_sign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    imo_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    destination: Option<String>,
    /// Hull dimensions from the vessel's static report, absent until one is
    /// received. A hull with no reported size is drawn at a neutral size
    /// rather than guessed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    length_meters: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    beam_meters: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    draught_meters: Option<f64>,
    /// Minutes until this vessel reaches the span, as a range. Absent when the
    /// vessel is not closing on it, which is not the same as "far away".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    eta_min_minutes: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    eta_max_minutes: Option<u16>,
    points: VecDeque<VesselHistoryPoint>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VesselHistoryPoint {
    latitude: f64,
    longitude: f64,
    observed_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Movement {
    Approaching,
    Stationary,
    Unknown,
    Diverging,
}

impl Movement {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Approaching => "approaching",
            Self::Diverging => "diverging",
            Self::Stationary => "stationary",
            Self::Unknown => "unknown",
        }
    }
}

fn track_priority(track: &VesselTrack) -> (u8, u16) {
    let movement = track
        .item
        .attributes
        .get("movement")
        .and_then(Value::as_str);
    let intersects = track
        .item
        .attributes
        .get("route_intersects")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let posture = track.item.attributes.get("posture").and_then(Value::as_str);
    let rank = match (movement, intersects) {
        _ if matches!(posture, Some("moored" | "off_channel" | "deep_draft")) => 5,
        (Some("approaching"), true) => 0,
        (Some("approaching"), false) => 1,
        (Some("stationary"), _) => 2,
        (Some("unknown"), _) => 3,
        _ => 4,
    };
    let eta = track
        .item
        .attributes
        .get("eta_min_minutes")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(u16::MAX);
    (rank, eta)
}

async fn run_stream(
    config: AisStreamConfig,
    state: Arc<RwLock<StreamState>>,
    cancellation: CancellationToken,
) {
    let mut reconnect_delay = config.reconnect_initial;
    loop {
        if cancellation.is_cancelled() {
            return;
        }
        let connected_at = tokio::time::Instant::now();
        let result = run_connection(&config, &state, &cancellation).await;
        if matches!(result, ConnectionExit::Cancelled) {
            return;
        }
        {
            let mut state = state.write().await;
            state.health = HealthState::Degraded;
            state.failure = Some(match result {
                ConnectionExit::Rejected => StreamFailure::Rejected,
                ConnectionExit::Unavailable => StreamFailure::Unavailable,
                ConnectionExit::Cancelled => unreachable!(),
            });
        }
        if connected_at.elapsed() >= Duration::from_secs(30) {
            reconnect_delay = config.reconnect_initial;
        }
        tokio::select! {
            () = cancellation.cancelled() => return,
            () = tokio::time::sleep(reconnect_delay) => {}
        }
        reconnect_delay = reconnect_delay.saturating_mul(2).min(config.reconnect_max);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConnectionExit {
    Cancelled,
    Rejected,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamFailure {
    Starting,
    Rejected,
    Unavailable,
}

impl StreamFailure {
    const fn detail(self) -> &'static str {
        match self {
            Self::Starting => "AISStream connection is starting",
            Self::Rejected => "AISStream rejected the subscription",
            Self::Unavailable => "AISStream connection is unavailable",
        }
    }
}

async fn run_connection(
    config: &AisStreamConfig,
    state: &Arc<RwLock<StreamState>>,
    cancellation: &CancellationToken,
) -> ConnectionExit {
    // tungstenite's rustls connector resolves the process-default provider,
    // falling back to whichever one the rustls crate features name. That
    // fallback panics if a future dependency ever enables aws-lc-rs alongside
    // ring, so name the provider here rather than inherit an ambiguity.
    // Installing is idempotent.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let websocket_config = WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(4 * 1024)
        .max_write_buffer_size(64 * 1024)
        .max_message_size(Some(MAX_WEBSOCKET_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_WEBSOCKET_MESSAGE_BYTES));
    let connection = tokio::select! {
        () = cancellation.cancelled() => return ConnectionExit::Cancelled,
        result = timeout(
            Duration::from_secs(15),
            connect_async_with_config(AISSTREAM_ENDPOINT, Some(websocket_config), false),
        ) => result,
    };
    let Ok(Ok((mut websocket, _response))) = connection else {
        return ConnectionExit::Unavailable;
    };

    let subscription = WireSubscription {
        api_key: config.api_key.expose(),
        bounding_boxes: config.subscription.bounding_boxes(),
        filter_message_types: &[
            "PositionReport",
            "StandardClassBPositionReport",
            "ExtendedClassBPositionReport",
            "ShipStaticData",
            "StaticDataReport",
        ],
    };
    let Ok(payload) = serde_json::to_string(&subscription) else {
        return ConnectionExit::Unavailable;
    };
    let send = tokio::select! {
        () = cancellation.cancelled() => return ConnectionExit::Cancelled,
        result = timeout(Duration::from_secs(2), websocket.send(Message::Text(payload.into()))) => result,
    };
    if !matches!(send, Ok(Ok(()))) {
        return ConnectionExit::Unavailable;
    }
    {
        let mut state = state.write().await;
        state.health = HealthState::Healthy;
        state.failure = None;
    }

    loop {
        let incoming = tokio::select! {
            () = cancellation.cancelled() => {
                let _ = timeout(Duration::from_secs(1), websocket.close(None)).await;
                return ConnectionExit::Cancelled;
            }
            result = timeout(config.idle_timeout, websocket.next()) => result,
        };
        let Ok(Some(Ok(message))) = incoming else {
            return ConnectionExit::Unavailable;
        };
        match message {
            Message::Text(text) => {
                if let Err(exit) = handle_payload(text.as_bytes(), config, state).await {
                    return exit;
                }
            }
            Message::Binary(bytes) => {
                if let Err(exit) = handle_payload(&bytes, config, state).await {
                    return exit;
                }
            }
            Message::Ping(payload) => {
                let pong = tokio::select! {
                    () = cancellation.cancelled() => {
                        let _ = timeout(Duration::from_secs(1), websocket.close(None)).await;
                        return ConnectionExit::Cancelled;
                    }
                    result = timeout(
                        Duration::from_secs(2),
                        websocket.send(Message::Pong(payload)),
                    ) => result,
                };
                if !matches!(pong, Ok(Ok(()))) {
                    return ConnectionExit::Unavailable;
                }
            }
            Message::Close(_) => return ConnectionExit::Unavailable,
            Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

async fn handle_payload(
    body: &[u8],
    config: &AisStreamConfig,
    state: &Arc<RwLock<StreamState>>,
) -> Result<(), ConnectionExit> {
    if body.len() > MAX_WEBSOCKET_MESSAGE_BYTES {
        return Err(ConnectionExit::Unavailable);
    }
    let Ok(root) = serde_json::from_slice::<Value>(body) else {
        return Ok(());
    };
    if root.get("error").is_some() {
        return Err(ConnectionExit::Rejected);
    }
    let now = Utc::now();
    let mut guard = state.write().await;
    let state = &mut *guard;
    if let Some((mmsi, update)) = normalize_static(&root, now) {
        apply_static(&mut state.statics, mmsi, update, now);
        return Ok(());
    }
    if let Some(track) = normalize_value(
        &root,
        now,
        config,
        &state.tracks,
        &state.statics,
        &state.histories,
    ) {
        record_crossing(
            &mut state.crossings,
            state.tracks.get(&track.mmsi),
            &track,
            state.statics.get(&track.mmsi),
        );
        update_history(&mut state.histories, &track, now, config.history_retention);
        state.tracks.insert(track.mmsi, track);
        prune_tracks(&mut state.tracks, now, config.track_retention);
        prune_histories(&mut state.histories, now, config.history_retention);
    }
    Ok(())
}

/// Extracts identity/hull facts from the two static AIS message kinds.
fn normalize_static(root: &Value, received_at: DateTime<Utc>) -> Option<(u32, VesselStatic)> {
    let message_type = root.get("MessageType")?.as_str()?;
    if !matches!(message_type, "ShipStaticData" | "StaticDataReport") {
        return None;
    }
    let report = root.get("Message")?.get(message_type)?;
    let mmsi = report
        .get("UserID")
        .and_then(Value::as_u64)
        .or_else(|| {
            root.get("MetaData")
                .and_then(|metadata| metadata.get("MMSI"))
                .and_then(Value::as_u64)
        })
        .and_then(|value| u32::try_from(value).ok())?;
    if !(100_000_000..=999_999_999).contains(&mmsi) {
        return None;
    }
    let mut update = VesselStatic {
        updated_at: Some(received_at),
        ..VesselStatic::default()
    };
    if message_type == "ShipStaticData" {
        update.name = report
            .get("Name")
            .and_then(Value::as_str)
            .and_then(sanitize_vessel_name);
        update.call_sign = report
            .get("CallSign")
            .and_then(Value::as_str)
            .and_then(sanitize_call_sign);
        update.imo_number = report
            .get("ImoNumber")
            .and_then(Value::as_u64)
            .and_then(valid_imo_number);
        update.destination = report
            .get("Destination")
            .and_then(Value::as_str)
            .and_then(sanitize_destination);
        update.ship_type = report
            .get("Type")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok());
        update.length_meters = dimension_length(report.get("Dimension"));
        update.beam_meters = dimension_beam(report.get("Dimension"));
        update.draught_meters = report
            .get("MaximumStaticDraught")
            .and_then(finite_number)
            .filter(|draught| (0.0..=30.0).contains(draught));
    } else {
        let part_a = report.get("ReportA");
        if part_a
            .and_then(|part| part.get("Valid"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            update.name = part_a
                .and_then(|part| part.get("Name"))
                .and_then(Value::as_str)
                .and_then(sanitize_vessel_name);
        }
        let part_b = report.get("ReportB");
        if part_b
            .and_then(|part| part.get("Valid"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            update.ship_type = part_b
                .and_then(|part| part.get("ShipType"))
                .and_then(Value::as_u64)
                .and_then(|value| u16::try_from(value).ok());
            update.length_meters = dimension_length(part_b.and_then(|part| part.get("Dimension")));
            update.beam_meters = dimension_beam(part_b.and_then(|part| part.get("Dimension")));
        }
    }
    (update.name.is_some()
        || update.ship_type.is_some()
        || update.length_meters.is_some()
        || update.beam_meters.is_some()
        || update.draught_meters.is_some())
    .then_some((mmsi, update))
}

fn dimension_length(dimension: Option<&Value>) -> Option<f64> {
    let dimension = dimension?;
    let bow = dimension.get("A").and_then(finite_number)?;
    let stern = dimension.get("B").and_then(finite_number)?;
    let length = bow + stern;
    ((1.0..=500.0).contains(&length)).then_some(length)
}

/// Beam, from the port and starboard offsets of the reported reference point.
fn dimension_beam(dimension: Option<&Value>) -> Option<f64> {
    let dimension = dimension?;
    let port = dimension.get("C").and_then(finite_number)?;
    let starboard = dimension.get("D").and_then(finite_number)?;
    let beam = port + starboard;
    ((1.0..=100.0).contains(&beam)).then_some(beam)
}

fn apply_static(
    statics: &mut BTreeMap<u32, VesselStatic>,
    mmsi: u32,
    update: VesselStatic,
    now: DateTime<Utc>,
) {
    let entry = statics.entry(mmsi).or_default();
    if update.name.is_some() {
        entry.name = update.name;
    }
    if update.ship_type.is_some() {
        entry.ship_type = update.ship_type;
    }
    if update.call_sign.is_some() {
        entry.call_sign = update.call_sign;
    }
    if update.imo_number.is_some() {
        entry.imo_number = update.imo_number;
    }
    // Destination is the one static field a skipper retypes mid-voyage, so a
    // later report replaces it rather than merging; but a report that omits it
    // is silence, not a clearing.
    if update.destination.is_some() {
        entry.destination = update.destination;
    }
    if update.beam_meters.is_some() {
        entry.beam_meters = update.beam_meters;
    }
    if update.length_meters.is_some() {
        entry.length_meters = update.length_meters;
    }
    if update.draught_meters.is_some() {
        entry.draught_meters = update.draught_meters;
    }
    entry.updated_at = Some(now);
    while statics.len() > MAX_TRACKED_VESSELS {
        let Some(oldest) = statics
            .iter()
            .min_by_key(|(_, value)| value.updated_at)
            .map(|(key, _)| *key)
        else {
            break;
        };
        statics.remove(&oldest);
    }
}

/// Detects the previous→current fix straddling the bridge line.
fn record_crossing(
    crossings: &mut VecDeque<AisCrossing>,
    previous: Option<&VesselTrack>,
    current: &VesselTrack,
    vessel_static: Option<&VesselStatic>,
) {
    let Some(previous) = previous else { return };
    let (Some(before), Some(after)) = (previous.point.s_meters, current.point.s_meters) else {
        return;
    };
    // Both fixes must sit near the span; a sign flip across a long gap is a
    // teleporting receiver, not a passage.
    if before.signum() == after.signum()
        || before.abs() > 600.0
        || after.abs() > 600.0
        || current
            .point
            .observed_at
            .signed_duration_since(previous.point.observed_at)
            > TimeDelta::minutes(12)
    {
        return;
    }
    let span = (after - before).abs();
    let fraction = if span > 1.0 { before.abs() / span } else { 0.5 };
    let elapsed = current
        .point
        .observed_at
        .signed_duration_since(previous.point.observed_at);
    let crossed_at = previous.point.observed_at
        + TimeDelta::milliseconds((elapsed.num_milliseconds() as f64 * fraction) as i64);
    let speed_knots = current
        .item
        .attributes
        .get("sog_knots")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    crossings.push_back(AisCrossing {
        mmsi: current.mmsi.to_string(),
        vessel_name: vessel_static
            .and_then(|value| value.name.clone())
            .or_else(|| {
                current
                    .item
                    .attributes
                    .get("vessel_name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            }),
        vessel_class: vessel_static
            .and_then(VesselStatic::class_word)
            .map(ToOwned::to_owned),
        length_meters: vessel_static.and_then(|value| value.length_meters),
        draught_meters: vessel_static.and_then(|value| value.draught_meters),
        direction: if after > before {
            "upriver"
        } else {
            "downriver"
        }
        .into(),
        crossed_at,
        speed_knots,
    });
    while crossings.len() > MAX_PENDING_CROSSINGS {
        let _ = crossings.pop_front();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct WireSubscription<'a> {
    #[serde(rename = "APIKey")]
    api_key: &'a str,
    bounding_boxes: &'a [[[f64; 2]; 2]],
    filter_message_types: &'a [&'a str],
}

#[cfg(test)]
fn normalize_message(
    body: &[u8],
    received_at: DateTime<Utc>,
    config: &AisStreamConfig,
    existing: &BTreeMap<u32, VesselTrack>,
) -> Option<VesselTrack> {
    let root: Value = serde_json::from_slice(body).ok()?;
    normalize_value(
        &root,
        received_at,
        config,
        existing,
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
}

/// How one fix relates to the bridge, in whichever geometry applies.
struct FixClassification {
    distance_meters: f64,
    s_signed: Option<f64>,
    movement: Movement,
    posture: Option<&'static str>,
    route_intersects: bool,
    eta: Option<(u16, u16)>,
    branch: Option<&'static str>,
    offset_meters: Option<f64>,
}

fn normalize_value(
    root: &Value,
    received_at: DateTime<Utc>,
    config: &AisStreamConfig,
    existing: &BTreeMap<u32, VesselTrack>,
    statics: &BTreeMap<u32, VesselStatic>,
    histories: &BTreeMap<u32, VesselHistory>,
) -> Option<VesselTrack> {
    if root.get("error").is_some() {
        return None;
    }
    let message_type = root.get("MessageType")?.as_str()?;
    if !matches!(
        message_type,
        "PositionReport" | "StandardClassBPositionReport" | "ExtendedClassBPositionReport"
    ) {
        return None;
    }
    let report = root.get("Message")?.get(message_type)?;
    if report.get("Valid")?.as_bool() != Some(true) {
        return None;
    }
    let mmsi = report
        .get("UserID")?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())?;
    if !(100_000_000..=999_999_999).contains(&mmsi) {
        return None;
    }
    let latitude = finite_number(report.get("Latitude")?)?;
    let longitude = finite_number(report.get("Longitude")?)?;
    if !valid_coordinate(latitude, longitude) || !config.subscription.contains(latitude, longitude)
    {
        return None;
    }
    let sog_knots = finite_number(report.get("Sog")?)?;
    let cog_degrees = finite_number(report.get("Cog")?)?;
    // AIS encodes 102.3 knots and 360 degrees as "not available".
    if !(0.0..102.3).contains(&sog_knots) || !(0.0..360.0).contains(&cog_degrees) {
        return None;
    }

    let metadata = root.get("MetaData").or_else(|| root.get("Metadata"))?;
    let source_time = metadata
        .get("time_utc")
        .or_else(|| metadata.get("TimeUtc"))?
        .as_str()
        .and_then(parse_aisstream_time)?;
    let age = received_at.signed_duration_since(source_time);
    let max_age = TimeDelta::from_std(config.max_report_age).ok()?;
    if age < TimeDelta::seconds(-MAX_REPORT_FUTURE_SKEW_SECONDS) || age > max_age {
        return None;
    }

    let vessel_static = statics.get(&mmsi);
    let previous = existing.get(&mmsi).map(|track| track.point);
    let classification = if config.subscription.corridor {
        classify_corridor_fix(
            latitude,
            longitude,
            sog_knots,
            cog_degrees,
            source_time,
            previous,
            vessel_static,
            histories.get(&mmsi),
        )
    } else {
        classify_square_fix(
            latitude,
            longitude,
            sog_knots,
            cog_degrees,
            source_time,
            previous,
            config,
        )?
    };
    let FixClassification {
        distance_meters,
        s_signed,
        movement,
        posture,
        route_intersects,
        eta,
        branch,
        offset_meters,
    } = classification;

    let vessel_name = metadata
        .get("ShipName")
        .and_then(Value::as_str)
        .and_then(sanitize_vessel_name)
        .or_else(|| vessel_static.and_then(|value| value.name.clone()));
    let vessel_class = vessel_static.and_then(VesselStatic::class_word);
    let title = vessel_name.as_ref().map_or_else(
        || format!("Vessel {mmsi}"),
        |name| format!("{name} · {mmsi}"),
    );
    let distance_text = if distance_meters < 1_000.0 {
        format!("{distance_meters:.0} m")
    } else {
        format!("{:.1} km", distance_meters / 1_000.0)
    };
    let eta_text = eta.map_or_else(
        || "ETA unavailable".into(),
        |(earliest, latest)| format!("ETA {earliest}–{latest} min"),
    );
    let class_text = vessel_class.map_or_else(String::new, |word| format!("{word} · "));
    let summary = format!(
        "{distance_text} from {} · {class_text}{} · {eta_text}",
        config.subscription.bridge_label,
        movement.as_str()
    );
    let expires_at = source_time + TimeDelta::from_std(config.track_retention).ok()?;
    let mut attributes = BTreeMap::from([
        ("relation".into(), json!("ais")),
        ("state".into(), json!(movement.as_str())),
        ("movement".into(), json!(movement.as_str())),
        ("route_intersects".into(), json!(route_intersects)),
        ("mmsi".into(), json!(mmsi.to_string())),
        ("distance_meters".into(), json!(distance_meters.round())),
        ("sog_knots".into(), json!(sog_knots)),
        ("cog_degrees".into(), json!(cog_degrees)),
        ("provider".into(), json!("aisstream")),
    ]);
    if let Some(name) = &vessel_name {
        attributes.insert("vessel_name".into(), json!(name));
    }
    if let Some(word) = vessel_class {
        attributes.insert("vessel_class".into(), json!(word));
    }
    if let Some(call_sign) = vessel_static.and_then(|value| value.call_sign.as_deref()) {
        attributes.insert("call_sign".into(), json!(call_sign));
    }
    if let Some(imo) = vessel_static.and_then(|value| value.imo_number) {
        attributes.insert("imo_number".into(), json!(imo));
    }
    if let Some(destination) = vessel_static.and_then(|value| value.destination.as_deref()) {
        attributes.insert("destination".into(), json!(destination));
    }
    if let Some(length) = vessel_static.and_then(|value| value.length_meters) {
        attributes.insert("length_meters".into(), json!(length.round()));
    }
    if let Some(beam) = vessel_static.and_then(|value| value.beam_meters) {
        attributes.insert("beam_meters".into(), json!(beam.round()));
    }
    if let Some(draught) = vessel_static.and_then(|value| value.draught_meters) {
        attributes.insert("draught_meters".into(), json!(draught));
    }
    if let Some(posture) = posture {
        attributes.insert("posture".into(), json!(posture));
    }
    if let Some(branch) = branch {
        attributes.insert("branch".into(), json!(branch));
    }
    if let Some(s_signed) = s_signed {
        attributes.insert("s_meters".into(), json!(s_signed.round()));
    }
    if let Some(offset) = offset_meters {
        attributes.insert("offset_meters".into(), json!(offset.round()));
    }
    if let Some((earliest, latest)) = eta {
        attributes.insert("eta_min_minutes".into(), json!(earliest));
        attributes.insert("eta_max_minutes".into(), json!(latest));
        attributes.insert("rung".into(), json!(warning_rung(earliest)));
    }

    Some(VesselTrack {
        mmsi,
        observed_at: source_time,
        distance_meters,
        point: TrackPoint {
            observed_at: source_time,
            distance_meters,
            s_meters: s_signed,
        },
        item: CollectorItem {
            id: format!("aisstream:{mmsi}"),
            kind: ItemKind::Bridge,
            title,
            summary: Some(summary),
            observed_at: Some(source_time),
            starts_at: None,
            ends_at: Some(expires_at),
            location: Some(Location {
                name: vessel_name,
                latitude: Some(latitude),
                longitude: Some(longitude),
            }),
            source: SourceLink {
                name: "AISStream".into(),
                url: Url::parse(AISSTREAM_SOURCE_URL).ok(),
            },
            attributes,
        },
    })
}

/// The original square-radius classification, kept for non-Brickell bridges.
fn classify_square_fix(
    latitude: f64,
    longitude: f64,
    sog_knots: f64,
    cog_degrees: f64,
    source_time: DateTime<Utc>,
    previous: Option<TrackPoint>,
    config: &AisStreamConfig,
) -> Option<FixClassification> {
    let distance_meters = haversine_meters(
        latitude,
        longitude,
        config.subscription.bridge_latitude,
        config.subscription.bridge_longitude,
    );
    if !distance_meters.is_finite()
        || distance_meters > config.subscription.radius_kilometers * 1_000.0 * 1.5
    {
        return None;
    }
    let bearing = crate::geo::initial_bearing_degrees(
        latitude,
        longitude,
        config.subscription.bridge_latitude,
        config.subscription.bridge_longitude,
    );
    let course_delta = crate::geo::angular_difference_degrees(cog_degrees, bearing);
    let projected_miss_meters = distance_meters * course_delta.to_radians().sin().abs();
    let course_projects_to_bridge = course_delta <= 75.0 && projected_miss_meters <= 750.0;
    let closing_speed = closing_speed_meters_per_second(previous, distance_meters, source_time);
    let movement = if sog_knots <= 0.5 {
        Movement::Stationary
    } else if closing_speed.is_some_and(|speed| speed > 0.25) {
        Movement::Approaching
    } else if closing_speed.is_some_and(|speed| speed < -0.25) {
        Movement::Diverging
    } else if course_projects_to_bridge {
        Movement::Approaching
    } else if course_delta >= 110.0 {
        Movement::Diverging
    } else {
        Movement::Unknown
    };
    let route_intersects = movement == Movement::Approaching
        && (course_projects_to_bridge || (distance_meters <= 500.0 && course_delta <= 100.0));
    let eta = estimate_eta(
        distance_meters,
        sog_knots,
        course_delta,
        closing_speed,
        route_intersects,
    );
    Some(FixClassification {
        distance_meters,
        s_signed: None,
        movement,
        posture: None,
        route_intersects,
        eta,
        branch: None,
        offset_meters: None,
    })
}

/// Channel-coordinate classification for the Miami River corridor.
///
/// Everything follows from `(s, offset)`: corridor membership from the
/// offset, closing from `|s|` shrinking between fixes, direction from the
/// channel's own bridgeward bearing rather than the straight-line bearing a
/// bending river invalidates.
#[allow(clippy::too_many_arguments)]
fn classify_corridor_fix(
    latitude: f64,
    longitude: f64,
    sog_knots: f64,
    cog_degrees: f64,
    source_time: DateTime<Utc>,
    previous: Option<TrackPoint>,
    vessel_static: Option<&VesselStatic>,
    history: Option<&VesselHistory>,
) -> FixClassification {
    let fix = river::project(latitude, longitude);
    let distance_meters = fix.channel_distance_meters();
    let in_corridor = fix.in_corridor();
    let river_capable = vessel_static.is_none_or(VesselStatic::river_capable);
    let closing_speed = closing_speed_meters_per_second(previous, distance_meters, source_time);
    let course_delta =
        crate::geo::angular_difference_degrees(cog_degrees, fix.bridgeward_bearing_degrees);
    let moored = sog_knots <= 0.5 && held_station(history, latitude, longitude, source_time);

    let movement = if sog_knots <= 0.5 {
        Movement::Stationary
    } else if !in_corridor {
        Movement::Unknown
    } else if closing_speed.is_some_and(|speed| speed > 0.25) {
        Movement::Approaching
    } else if closing_speed.is_some_and(|speed| speed < -0.25) {
        Movement::Diverging
    } else if course_delta <= 60.0 {
        Movement::Approaching
    } else if course_delta >= 120.0 {
        Movement::Diverging
    } else {
        Movement::Unknown
    };

    let posture = Some(if !river_capable {
        "deep_draft"
    } else if moored {
        "moored"
    } else if !in_corridor {
        "off_channel"
    } else if sog_knots <= 0.5 {
        if distance_meters <= 500.0 {
            "waiting"
        } else {
            "holding"
        }
    } else {
        "underway"
    });

    // On the river trunk every continuing course crosses the span; on an
    // approach channel, commitment builds as the vessel nears the mouth, and
    // a sailing rig is river-intent on any channel (the mast is why the
    // bridge exists).
    let committed = match fix.branch {
        RiverBranch::River => true,
        RiverBranch::NorthApproach | RiverBranch::GovernmentCut | RiverBranch::SouthApproach => {
            distance_meters <= 1_600.0
                || vessel_static.and_then(|value| value.ship_type) == Some(36)
        }
    };
    let route_intersects =
        movement == Movement::Approaching && in_corridor && river_capable && committed;
    let eta = estimate_eta(
        distance_meters,
        sog_knots,
        course_delta,
        closing_speed,
        route_intersects,
    );

    FixClassification {
        distance_meters,
        s_signed: Some(fix.s_meters),
        movement,
        posture,
        route_intersects,
        eta,
        branch: Some(fix.branch.as_str()),
        offset_meters: Some(fix.offset_meters),
    }
}

fn closing_speed_meters_per_second(
    previous: Option<TrackPoint>,
    distance_meters: f64,
    source_time: DateTime<Utc>,
) -> Option<f64> {
    previous.and_then(|point| {
        let elapsed = source_time.signed_duration_since(point.observed_at);
        let seconds = elapsed.num_milliseconds() as f64 / 1_000.0;
        // Class B transponders can go many minutes between received fixes;
        // a six-minute-old baseline still says which way the gap is moving.
        (2.0..=360.0)
            .contains(&seconds)
            .then_some((point.distance_meters - distance_meters) / seconds)
    })
}

/// Behavioral moored test: the vessel has held one spot for the whole window.
///
/// NavigationalStatus is decorative in practice — tugs underway at five knots
/// broadcast "moored" — so mooring is judged from the track itself. A vessel
/// that only *recently* stopped (a yacht holding at the span for the opening)
/// has moving history inside the window and correctly stays un-moored.
fn held_station(
    history: Option<&VesselHistory>,
    latitude: f64,
    longitude: f64,
    source_time: DateTime<Utc>,
) -> bool {
    let Some(history) = history else { return false };
    let window_start = source_time - TimeDelta::seconds(MOORED_WINDOW_SECONDS);
    if !history
        .points
        .front()
        .is_some_and(|oldest| oldest.observed_at <= window_start)
    {
        return false;
    }
    history.points.iter().all(|point| {
        point.observed_at < window_start
            || haversine_meters(point.latitude, point.longitude, latitude, longitude)
                <= MOORED_DISPLACEMENT_METERS
    })
}

/// Countdown rung for display surfaces, from the earliest supported ETA.
fn warning_rung(earliest_minutes: u16) -> &'static str {
    match earliest_minutes {
        0..=2 => "imminent",
        3 => "T-3",
        4 => "T-4",
        5 => "T-5",
        6..=10 => "T-10",
        11..=15 => "T-15",
        16..=20 => "T-20",
        21..=30 => "T-30",
        _ => "T-30+",
    }
}

fn estimate_eta(
    distance_meters: f64,
    sog_knots: f64,
    course_delta: f64,
    closing_speed: Option<f64>,
    route_intersects: bool,
) -> Option<(u16, u16)> {
    if !route_intersects || distance_meters <= 50.0 {
        return None;
    }
    let projected_speed = sog_knots * 0.514_444 * course_delta.to_radians().cos().max(0.0);
    let meters_per_second = closing_speed
        .filter(|speed| *speed > 0.25)
        .unwrap_or(projected_speed);
    if !meters_per_second.is_finite() || meters_per_second <= 0.25 {
        return None;
    }
    let minutes = distance_meters / meters_per_second / 60.0;
    // The corridor reaches ~46 minutes upriver and ~35 out the entrance
    // channels; anything slower is drift, not an approach.
    if !minutes.is_finite() || !(0.25..=75.0).contains(&minutes) {
        return None;
    }
    let earliest = (minutes * 0.75).floor().max(1.0) as u16;
    let latest = (minutes * 1.35).ceil().clamp(f64::from(earliest), 90.0) as u16;
    Some((earliest, latest))
}

fn prune_tracks(tracks: &mut BTreeMap<u32, VesselTrack>, now: DateTime<Utc>, retention: Duration) {
    let cutoff = now - TimeDelta::from_std(retention).unwrap_or_else(|_| TimeDelta::seconds(60));
    tracks.retain(|_, track| track.observed_at >= cutoff);
    while tracks.len() > MAX_TRACKED_VESSELS {
        let Some(oldest) = tracks
            .iter()
            .min_by_key(|(_, track)| track.observed_at)
            .map(|(mmsi, _)| *mmsi)
        else {
            break;
        };
        tracks.remove(&oldest);
    }
}

fn update_history(
    histories: &mut BTreeMap<u32, VesselHistory>,
    track: &VesselTrack,
    now: DateTime<Utc>,
    retention: Duration,
) {
    let Some(location) = track.item.location.as_ref() else {
        return;
    };
    let (Some(latitude), Some(longitude)) = (location.latitude, location.longitude) else {
        return;
    };
    let movement = track
        .item
        .attributes
        .get("movement")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let route_intersects = track
        .item
        .attributes
        .get("route_intersects")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let speed_knots = track
        .item
        .attributes
        .get("sog_knots")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let course_degrees = track
        .item
        .attributes
        .get("cog_degrees")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let vessel_name = track
        .item
        .attributes
        .get("vessel_name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let vessel_class = track
        .item
        .attributes
        .get("vessel_class")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let posture = track
        .item
        .attributes
        .get("posture")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let s_meters = track.point.s_meters;
    let branch = track
        .item
        .attributes
        .get("branch")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let text = |key: &str| {
        track
            .item
            .attributes
            .get(key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    };
    let call_sign = text("call_sign");
    let destination = text("destination");
    let imo_number = track
        .item
        .attributes
        .get("imo_number")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let number = |key: &str| track.item.attributes.get(key).and_then(Value::as_f64);
    let length_meters = number("length_meters");
    let beam_meters = number("beam_meters");
    let draught_meters = number("draught_meters");
    let eta_min_minutes = track
        .item
        .attributes
        .get("eta_min_minutes")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok());
    let eta_max_minutes = track
        .item
        .attributes
        .get("eta_max_minutes")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok());
    let point = VesselHistoryPoint {
        latitude,
        longitude,
        observed_at: track.observed_at,
    };
    let history = histories
        .entry(track.mmsi)
        .or_insert_with(|| VesselHistory {
            mmsi: track.mmsi.to_string(),
            vessel_name: vessel_name.clone(),
            movement: movement.clone(),
            route_intersects,
            speed_knots,
            course_degrees,
            observed_at: track.observed_at,
            vessel_class: vessel_class.clone(),
            posture: posture.clone(),
            s_meters,
            branch: branch.clone(),
            call_sign: call_sign.clone(),
            imo_number,
            destination: destination.clone(),
            length_meters,
            beam_meters,
            draught_meters,
            eta_min_minutes,
            eta_max_minutes,
            points: VecDeque::new(),
        });

    history.vessel_name = vessel_name;
    history.movement = movement;
    history.route_intersects = route_intersects;
    history.speed_knots = speed_knots;
    history.course_degrees = course_degrees;
    history.observed_at = track.observed_at;
    // A static report can arrive long after the first position, and a vessel
    // that stops broadcasting its class has not lost it; keep the last known
    // identity rather than blanking it on a bare position report.
    if vessel_class.is_some() {
        history.vessel_class = vessel_class;
    }
    history.posture = posture;
    history.s_meters = s_meters;
    history.branch = branch;
    // Identity arrives on a static report that may lag the first position by
    // minutes; never blank a known hull on a bare position update.
    if call_sign.is_some() {
        history.call_sign = call_sign;
    }
    if imo_number.is_some() {
        history.imo_number = imo_number;
    }
    if destination.is_some() {
        history.destination = destination;
    }
    // Dimensions arrive with a static report that may lag the first position
    // by minutes; never blank a known hull size on a bare position update.
    if length_meters.is_some() {
        history.length_meters = length_meters;
    }
    if beam_meters.is_some() {
        history.beam_meters = beam_meters;
    }
    if draught_meters.is_some() {
        history.draught_meters = draught_meters;
    }
    history.eta_min_minutes = eta_min_minutes;
    history.eta_max_minutes = eta_max_minutes;

    let bucket = track.observed_at.timestamp() / HISTORY_SAMPLE_SECONDS;
    if history
        .points
        .back()
        .is_some_and(|existing| existing.observed_at.timestamp() / HISTORY_SAMPLE_SECONDS == bucket)
    {
        let _ = history.points.pop_back();
    }
    history.points.push_back(point);

    let cutoff = now - TimeDelta::from_std(retention).unwrap_or_else(|_| TimeDelta::hours(1));
    while history
        .points
        .front()
        .is_some_and(|point| point.observed_at < cutoff)
    {
        let _ = history.points.pop_front();
    }
    while history.points.len() > MAX_HISTORY_POINTS_PER_VESSEL {
        let _ = history.points.pop_front();
    }
}

fn prune_histories(
    histories: &mut BTreeMap<u32, VesselHistory>,
    now: DateTime<Utc>,
    retention: Duration,
) {
    let cutoff = now - TimeDelta::from_std(retention).unwrap_or_else(|_| TimeDelta::hours(1));
    histories.retain(|_, history| history.observed_at >= cutoff && !history.points.is_empty());
    while histories.len() > MAX_HISTORY_TRACKS {
        let Some(oldest) = histories
            .iter()
            .min_by_key(|(_, history)| history.observed_at)
            .map(|(mmsi, _)| *mmsi)
        else {
            break;
        };
        histories.remove(&oldest);
    }
}

fn validated_histories(
    histories: Vec<VesselHistory>,
    now: DateTime<Utc>,
    retention: Duration,
) -> BTreeMap<u32, VesselHistory> {
    let cutoff = now - TimeDelta::from_std(retention).unwrap_or_else(|_| TimeDelta::hours(1));
    let latest = now + TimeDelta::seconds(MAX_REPORT_FUTURE_SKEW_SECONDS);
    let mut output = BTreeMap::new();
    for mut history in histories.into_iter().take(MAX_HISTORY_TRACKS) {
        let Ok(mmsi) = history.mmsi.parse::<u32>() else {
            continue;
        };
        if !(100_000_000..=999_999_999).contains(&mmsi)
            || history.observed_at < cutoff
            || history.observed_at > latest
            || !history.speed_knots.is_finite()
            || !history.course_degrees.is_finite()
            || !(0.0..102.3).contains(&history.speed_knots)
            || !(0.0..360.0).contains(&history.course_degrees)
            || !matches!(
                history.movement.as_str(),
                "approaching" | "stationary" | "unknown" | "diverging"
            )
        {
            continue;
        }
        history.points.retain(|point| {
            point.observed_at >= cutoff
                && point.observed_at <= latest
                && valid_coordinate(point.latitude, point.longitude)
        });
        while history.points.len() > MAX_HISTORY_POINTS_PER_VESSEL {
            let _ = history.points.pop_front();
        }
        if !history.points.is_empty() {
            output.insert(mmsi, history);
        }
    }
    output
}

fn parse_aisstream_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .or_else(|_| DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f %z UTC"))
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

/// Call signs are short alphanumeric identifiers; anything else is padding.
fn sanitize_call_sign(value: &str) -> Option<String> {
    let sign = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    (2..=16).contains(&sign.len()).then_some(sign)
}

/// IMO numbers occupy a fixed range; 0 is the field's "not set".
fn valid_imo_number(value: u64) -> Option<u32> {
    (1_000_000..=9_999_999)
        .contains(&value)
        .then_some(value as u32)
}

/// Destination is free text a skipper types, so it arrives padded, blank, or
/// as a placeholder. Anything that is not letters and digits is not a place.
fn sanitize_destination(value: &str) -> Option<String> {
    let destination = sanitize_vessel_name(value)?;
    let meaningful = destination
        .chars()
        .any(|character| character.is_ascii_alphanumeric());
    let placeholder = destination.eq_ignore_ascii_case("none")
        || destination.eq_ignore_ascii_case("unknown")
        || destination.eq_ignore_ascii_case("n/a");
    (meaningful && !placeholder).then_some(destination)
}

fn sanitize_vessel_name(value: &str) -> Option<String> {
    let mut name = value
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    name.truncate(name.floor_char_boundary(80));
    let name = name.trim_matches(|character: char| character.is_whitespace() || character == '@');
    (!name.is_empty()).then(|| name.to_owned())
}

fn finite_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

fn valid_coordinate(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIVED: &str = "2026-08-14T16:20:10Z";

    /// Production geometry: the Brickell target resolves to the corridor.
    fn config() -> AisStreamConfig {
        AisStreamConfig::new(
            AisStreamApiKey::new("fixture-secret-key").expect("valid fixture key"),
            AisStreamSubscription::for_bridge("Brickell Avenue Bridge", 25.7699, -80.19005, 12.0)
                .expect("valid fixture subscription"),
        )
    }

    /// The generic square geometry any non-Brickell bridge still gets.
    fn square_config() -> AisStreamConfig {
        AisStreamConfig::new(
            AisStreamApiKey::new("fixture-secret-key").expect("valid fixture key"),
            AisStreamSubscription::around_bridge(
                "Brickell Avenue Bridge",
                25.7699,
                -80.19005,
                12.0,
            )
            .expect("valid fixture subscription"),
        )
    }

    fn fixture(value: &str) -> Vec<u8> {
        include_str!("../fixtures/aisstream-position-report.json")
            .replace("2026-08-14T16:20:00Z", value)
            .into_bytes()
    }

    #[test]
    fn secret_debug_is_redacted_and_subscription_is_bounded() {
        let key = AisStreamApiKey::new("do-not-print-this-key").expect("valid key");
        assert_eq!(format!("{key:?}"), "AisStreamApiKey([REDACTED])");
        assert!(
            !format!(
                "{:?}",
                AisStreamConfig::new(key, square_config().subscription)
            )
            .contains("do-not")
        );
        let square = &square_config().subscription;
        assert_eq!(square.bounding_boxes().len(), 1);
        assert!((square.radius_kilometers() - 12.0).abs() < f64::EPSILON);

        // The Brickell target swaps in the surveyed corridor tiles while any
        // other bridge keeps the square.
        let corridor = &config().subscription;
        assert!(corridor.bounding_boxes().len() >= 5);
        let elsewhere =
            AisStreamSubscription::for_bridge("Las Olas", 26.119, -80.119, 12.0).expect("valid");
        assert_eq!(elsewhere.bounding_boxes().len(), 1);
    }

    #[test]
    fn fresh_position_report_becomes_typed_bridge_item() {
        let received = RECEIVED.parse::<DateTime<Utc>>().expect("fixture time");
        let track = normalize_message(
            &fixture("2026-08-14T16:20:00Z"),
            received,
            &config(),
            &BTreeMap::new(),
        )
        .expect("fresh valid report");
        assert_eq!(track.mmsi, 367719770);
        assert_eq!(track.item.kind, ItemKind::Bridge);
        assert_eq!(track.item.attributes["relation"], json!("ais"));
        assert_eq!(track.item.attributes["movement"], json!("approaching"));
        assert_eq!(track.item.attributes["route_intersects"], json!(true));
        assert!(
            track.item.attributes["distance_meters"]
                .as_f64()
                .is_some_and(|value| value > 100.0)
        );
        assert!(track.item.attributes["eta_min_minutes"].as_u64().is_some());
        assert_eq!(track.item.attributes["branch"], json!("river"));
        assert_eq!(track.item.attributes["posture"], json!("underway"));
        assert!(
            track.item.attributes["s_meters"]
                .as_f64()
                .is_some_and(|s| s > 1_500.0 && s < 2_200.0)
        );
        assert!(
            track.item.attributes["rung"]
                .as_str()
                .is_some_and(|rung| rung.starts_with("T-"))
        );
    }

    #[test]
    fn bridge_line_crossing_is_detected_and_queued() {
        let received = RECEIVED.parse::<DateTime<Utc>>().expect("fixture time");
        // Just upriver of the span, heading down.
        let before_body = String::from_utf8(fixture("2026-08-14T16:19:00Z"))
            .expect("utf8")
            .replace("-80.2040", "-80.1925")
            .replace("25.7762", "25.76944");
        let before = normalize_message(
            before_body.as_bytes(),
            received,
            &config(),
            &BTreeMap::new(),
        )
        .expect("upriver fix");
        assert!(
            before.point.s_meters.is_some_and(|s| s > 0.0),
            "s={:?}",
            before.point.s_meters
        );

        // One minute later, between the span and the mouth.
        let after_body = String::from_utf8(fixture("2026-08-14T16:20:00Z"))
            .expect("utf8")
            .replace("-80.2040", "-80.1878")
            .replace("25.7762", "25.77038");
        let existing = BTreeMap::from([(before.mmsi, before.clone())]);
        let after = normalize_message(after_body.as_bytes(), received, &config(), &existing)
            .expect("seaward fix");
        assert!(after.point.s_meters.is_some_and(|s| s < 0.0));

        let mut crossings = VecDeque::new();
        record_crossing(&mut crossings, Some(&before), &after, None);
        let crossing = crossings.pop_front().expect("crossing recorded");
        assert_eq!(crossing.direction, "downriver");
        assert_eq!(crossing.mmsi, "367719770");
        assert!(crossing.crossed_at >= before.point.observed_at);
        assert!(crossing.crossed_at <= after.point.observed_at);
        assert!(crossings.is_empty());
    }

    #[test]
    fn long_held_station_reads_moored_and_recent_stop_reads_waiting() {
        let received = RECEIVED.parse::<DateTime<Utc>>().expect("fixture time");
        let source_time = "2026-08-14T16:20:00Z"
            .parse::<DateTime<Utc>>()
            .expect("fixture time");
        let point = |minutes_ago: i64| VesselHistoryPoint {
            latitude: 25.7762,
            longitude: -80.2040,
            observed_at: source_time - TimeDelta::minutes(minutes_ago),
        };
        let history = |minutes: &[i64]| VesselHistory {
            mmsi: "367719770".into(),
            vessel_name: None,
            movement: "stationary".into(),
            route_intersects: false,
            speed_knots: 0.0,
            course_degrees: 129.0,
            observed_at: source_time,
            vessel_class: None,
            posture: None,
            s_meters: None,
            branch: None,
            call_sign: None,
            imo_number: None,
            destination: None,
            length_meters: None,
            beam_meters: None,
            draught_meters: None,
            eta_min_minutes: None,
            eta_max_minutes: None,
            points: minutes.iter().copied().map(point).collect(),
        };

        let stopped = String::from_utf8(fixture("2026-08-14T16:20:00Z"))
            .expect("utf8")
            .replace("\"Sog\": 5.4", "\"Sog\": 0.0");
        let root: Value = serde_json::from_slice(stopped.as_bytes()).expect("json");

        // Pinned to the same berth for the full window: moored, suppressed.
        let histories = BTreeMap::from([(367719770, history(&[12, 8, 4, 1]))]);
        let track = normalize_value(
            &root,
            received,
            &config(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &histories,
        )
        .expect("moored track");
        assert_eq!(track.item.attributes["posture"], json!("moored"));
        assert_eq!(track.item.attributes["movement"], json!("stationary"));

        // Only recently stopped mid-river: not moored, still a live story.
        let histories = BTreeMap::from([(367719770, history(&[4, 1]))]);
        let track = normalize_value(
            &root,
            received,
            &config(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &histories,
        )
        .expect("stopped track");
        assert_eq!(track.item.attributes["posture"], json!("holding"));
    }

    #[test]
    fn static_data_enriches_class_and_deep_draught_excludes_the_river() {
        let received = RECEIVED.parse::<DateTime<Utc>>().expect("fixture time");
        let static_body = json!({
            "MessageType": "ShipStaticData",
            "MetaData": { "MMSI": 367719770 },
            "Message": { "ShipStaticData": {
                "UserID": 367719770,
                "Name": "RIVER RUNNER",
                "Type": 52,
                "Dimension": { "A": 20.0, "B": 8.0, "C": 4.0, "D": 4.0 },
                "MaximumStaticDraught": 3.1
            }}
        });
        let (mmsi, update) = normalize_static(&static_body, received).expect("static parsed");
        assert_eq!(mmsi, 367719770);
        let mut statics = BTreeMap::new();
        apply_static(&mut statics, mmsi, update, received);
        assert_eq!(statics[&mmsi].class_word(), Some("tug"));
        assert_eq!(statics[&mmsi].length_meters, Some(28.0));

        let root: Value =
            serde_json::from_slice(&fixture("2026-08-14T16:20:00Z")).expect("json fixture");
        let track = normalize_value(
            &root,
            received,
            &config(),
            &BTreeMap::new(),
            &statics,
            &BTreeMap::new(),
        )
        .expect("enriched track");
        assert_eq!(track.item.attributes["vessel_class"], json!("tug"));
        assert_eq!(track.item.attributes["length_meters"], json!(28.0));
        assert_eq!(track.item.attributes["route_intersects"], json!(true));

        // The same hull drawing eight meters cannot enter the river at all.
        statics.get_mut(&mmsi).expect("entry").draught_meters = Some(8.0);
        let track = normalize_value(
            &root,
            received,
            &config(),
            &BTreeMap::new(),
            &statics,
            &BTreeMap::new(),
        )
        .expect("deep track");
        assert_eq!(track.item.attributes["posture"], json!("deep_draft"));
        assert_eq!(track.item.attributes["route_intersects"], json!(false));
        assert!(!track.item.attributes.contains_key("eta_min_minutes"));
    }

    #[test]
    fn distance_history_overrides_a_noisy_course_with_observed_divergence() {
        let received = RECEIVED.parse::<DateTime<Utc>>().expect("fixture time");
        let first = normalize_message(
            &fixture("2026-08-14T16:19:30Z"),
            received,
            &config(),
            &BTreeMap::new(),
        )
        .expect("first report");
        let mut existing = BTreeMap::from([(first.mmsi, first)]);
        // Same course words, but the hull has moved ~300 channel meters
        // upriver between fixes: observed channel motion wins over COG.
        let body = String::from_utf8(fixture("2026-08-14T16:20:00Z"))
            .expect("utf8 fixture")
            .replace("-80.2040", "-80.2062")
            .replace("25.7762", "25.7779");
        let second = normalize_message(body.as_bytes(), received, &config(), &existing)
            .expect("second report");
        assert_eq!(second.item.attributes["movement"], json!("diverging"));
        assert_eq!(second.item.attributes["route_intersects"], json!(false));
        assert!(!second.item.attributes.contains_key("eta_min_minutes"));
        existing.insert(second.mmsi, second);
    }

    #[test]
    fn malformed_stale_and_unavailable_navigation_reports_fail_closed() {
        let received = RECEIVED.parse::<DateTime<Utc>>().expect("fixture time");
        let mut stale = String::from_utf8(fixture("2026-08-14T16:10:00Z")).expect("utf8");
        assert!(
            normalize_message(stale.as_bytes(), received, &config(), &BTreeMap::new()).is_none()
        );

        stale = String::from_utf8(fixture("2026-08-14T16:20:00Z"))
            .expect("utf8")
            .replace("\"Valid\": true", "\"Valid\": false");
        assert!(
            normalize_message(stale.as_bytes(), received, &config(), &BTreeMap::new()).is_none()
        );

        let unavailable_course = String::from_utf8(fixture("2026-08-14T16:20:00Z"))
            .expect("utf8")
            .replace("\"Cog\": 129.0", "\"Cog\": 360.0");
        assert!(
            normalize_message(
                unavailable_course.as_bytes(),
                received,
                &config(),
                &BTreeMap::new()
            )
            .is_none()
        );

        assert!(normalize_message(b"{not json", received, &config(), &BTreeMap::new()).is_none());
    }

    #[tokio::test]
    async fn upstream_error_is_detected_without_retaining_its_text() {
        let state = Arc::new(RwLock::new(StreamState::default()));
        assert_eq!(
            handle_payload(br#"{"error":"Api Key Is Not Valid"}"#, &config(), &state).await,
            Err(ConnectionExit::Rejected)
        );
    }

    #[test]
    fn provider_go_timestamp_is_accepted() {
        let parsed = parse_aisstream_time("2026-08-15 13:42:10.123456789 +0000 UTC")
            .expect("AISStream Go time.Time value");
        assert_eq!(parsed.to_rfc3339(), "2026-08-15T13:42:10.123456789+00:00");
    }

    #[test]
    fn vessel_history_keeps_one_point_per_sample_bucket_and_round_trips() {
        let first_received = "2026-08-14T16:20:10Z"
            .parse::<DateTime<Utc>>()
            .expect("fixture time");
        let first = normalize_message(
            &fixture("2026-08-14T16:20:00Z"),
            first_received,
            &config(),
            &BTreeMap::new(),
        )
        .expect("first track");
        let second_received = "2026-08-14T16:20:25Z"
            .parse::<DateTime<Utc>>()
            .expect("fixture time");
        let second = normalize_message(
            &fixture("2026-08-14T16:20:20Z"),
            second_received,
            &config(),
            &BTreeMap::from([(first.mmsi, first.clone())]),
        )
        .expect("same-bucket track");
        let third_received = "2026-08-14T16:20:45Z"
            .parse::<DateTime<Utc>>()
            .expect("fixture time");
        let third = normalize_message(
            &fixture("2026-08-14T16:20:40Z"),
            third_received,
            &config(),
            &BTreeMap::from([(second.mmsi, second.clone())]),
        )
        .expect("next-bucket track");

        let mut histories = BTreeMap::new();
        update_history(
            &mut histories,
            &first,
            first_received,
            Duration::from_secs(3_600),
        );
        update_history(
            &mut histories,
            &second,
            second_received,
            Duration::from_secs(3_600),
        );
        update_history(
            &mut histories,
            &third,
            third_received,
            Duration::from_secs(3_600),
        );

        let history = histories.get(&first.mmsi).expect("vessel history");
        assert_eq!(history.points.len(), 2);
        assert_eq!(
            history.points.front().unwrap().observed_at,
            second.observed_at
        );
        assert_eq!(
            history.points.back().unwrap().observed_at,
            third.observed_at
        );

        let encoded = serde_json::to_string(&histories.into_values().collect::<Vec<_>>())
            .expect("history cursor JSON");
        let decoded =
            serde_json::from_str::<Vec<VesselHistory>>(&encoded).expect("history cursor decode");
        let restored = validated_histories(decoded, third_received, Duration::from_secs(3_600));
        assert_eq!(restored.get(&first.mmsi).unwrap().points.len(), 2);
    }

    #[tokio::test]
    async fn collection_succeeds_only_while_the_live_socket_boundary_is_healthy() {
        let collector = AisStreamCollector::new(config());
        // Keep this a WebSocket-free state-machine test.
        collector.started.store(true, Ordering::Release);
        assert!(collector.collect(&CollectContext::default()).await.is_err());

        let received = Utc::now();
        let report_time = received.to_rfc3339();
        let track = normalize_message(
            &fixture(&report_time),
            received,
            &collector.config,
            &BTreeMap::new(),
        )
        .expect("current fixture track");
        {
            let mut state = collector.state.write().await;
            state.health = HealthState::Healthy;
            state.tracks.insert(track.mmsi, track);
        }
        let batch = collector
            .collect(&CollectContext::default())
            .await
            .expect("healthy current socket boundary");
        assert_eq!(batch.cursor.metadata["fresh_vessel_count"], "1");
        assert_eq!(
            serde_json::from_str::<Vec<i64>>(&batch.cursor.metadata["fresh_vessel_expirations_ms"])
                .expect("bounded expiration vector")
                .len(),
            1
        );
        assert_eq!(
            batch.cursor.metadata["last_position_at_ms"],
            received.timestamp_millis().to_string()
        );

        collector.state.write().await.health = HealthState::Degraded;
        assert!(collector.collect(&CollectContext::default()).await.is_err());
    }
}
