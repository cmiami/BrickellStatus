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
    CollectorItem, HealthState, ItemKind, Location, SourceLink, geo::haversine_meters,
};

const AISSTREAM_ENDPOINT: &str = "wss://stream.aisstream.io/v0/stream";
const AISSTREAM_SOURCE_URL: &str = "https://aisstream.io/documentation.html";
const MAX_WEBSOCKET_MESSAGE_BYTES: usize = 128 * 1024;
const MAX_TRACKED_VESSELS: usize = 512;
const MAX_EXPOSED_TRACKS: usize = 1;
const MAX_HISTORY_TRACKS: usize = 64;
const MAX_HISTORY_POINTS_PER_VESSEL: usize = 121;
const HISTORY_SAMPLE_SECONDS: i64 = 30;
const MAX_REPORT_FUTURE_SKEW_SECONDS: i64 = 30;
const MIN_API_KEY_CHARS: usize = 8;
const MAX_API_KEY_CHARS: usize = 512;

/// Cursor metadata containing bounded, non-secret vessel courses for the map.
pub const AIS_VESSEL_TRACKS_CURSOR_KEY: &str = "vessel_tracks";

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
}

impl AisStreamSubscription {
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
        let state = self.state.read().await;
        if state.health != HealthState::Healthy {
            return Err(CollectorError::Request(
                state
                    .failure
                    .unwrap_or(StreamFailure::Starting)
                    .detail()
                    .into(),
            ));
        }
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
        let items = tracks
            .into_iter()
            .take(MAX_EXPOSED_TRACKS)
            .map(|track| track.item.clone())
            .collect();
        let mut cursor = CollectorCursor::default();
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
}

impl Default for StreamState {
    fn default() -> Self {
        Self {
            health: HealthState::Unknown,
            failure: Some(StreamFailure::Starting),
            tracks: BTreeMap::new(),
            histories: BTreeMap::new(),
        }
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
    distance_meters: f64,
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
    let rank = match (movement, intersects) {
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
    let mut state = state.write().await;
    if let Some(track) = normalize_value(&root, now, config, &state.tracks) {
        update_history(&mut state.histories, &track, now, config.history_retention);
        state.tracks.insert(track.mmsi, track);
        prune_tracks(&mut state.tracks, now, config.track_retention);
        prune_histories(&mut state.histories, now, config.history_retention);
    }
    Ok(())
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
    normalize_value(&root, received_at, config, existing)
}

fn normalize_value(
    root: &Value,
    received_at: DateTime<Utc>,
    config: &AisStreamConfig,
    existing: &BTreeMap<u32, VesselTrack>,
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
    let bearing = initial_bearing_degrees(
        latitude,
        longitude,
        config.subscription.bridge_latitude,
        config.subscription.bridge_longitude,
    );
    let course_delta = angular_difference_degrees(cog_degrees, bearing);
    let projected_miss_meters = distance_meters * course_delta.to_radians().sin().abs();
    let course_projects_to_bridge = course_delta <= 75.0 && projected_miss_meters <= 750.0;

    let previous = existing.get(&mmsi).map(|track| track.point);
    let closing_speed = previous.and_then(|point| {
        let elapsed = source_time.signed_duration_since(point.observed_at);
        let seconds = elapsed.num_milliseconds() as f64 / 1_000.0;
        (2.0..=180.0)
            .contains(&seconds)
            .then_some((point.distance_meters - distance_meters) / seconds)
    });
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

    let vessel_name = metadata
        .get("ShipName")
        .and_then(Value::as_str)
        .and_then(sanitize_vessel_name);
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
    let summary = format!(
        "{distance_text} from {} · {} · {eta_text}",
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
        (
            "projected_miss_meters".into(),
            json!(projected_miss_meters.round()),
        ),
        ("provider".into(), json!("aisstream")),
    ]);
    if let Some(name) = &vessel_name {
        attributes.insert("vessel_name".into(), json!(name));
    }
    if let Some((earliest, latest)) = eta {
        attributes.insert("eta_min_minutes".into(), json!(earliest));
        attributes.insert("eta_max_minutes".into(), json!(latest));
    }

    Some(VesselTrack {
        mmsi,
        observed_at: source_time,
        distance_meters,
        point: TrackPoint {
            observed_at: source_time,
            distance_meters,
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
    if !minutes.is_finite() || !(0.25..=60.0).contains(&minutes) {
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
            points: VecDeque::new(),
        });

    history.vessel_name = vessel_name;
    history.movement = movement;
    history.route_intersects = route_intersects;
    history.speed_knots = speed_knots;
    history.course_degrees = course_degrees;
    history.observed_at = track.observed_at;

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

fn initial_bearing_degrees(
    latitude: f64,
    longitude: f64,
    target_latitude: f64,
    target_longitude: f64,
) -> f64 {
    let source_latitude = latitude.to_radians();
    let target_latitude = target_latitude.to_radians();
    let longitude_delta = (target_longitude - longitude).to_radians();
    let y = longitude_delta.sin() * target_latitude.cos();
    let x = source_latitude.cos() * target_latitude.sin()
        - source_latitude.sin() * target_latitude.cos() * longitude_delta.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

fn angular_difference_degrees(left: f64, right: f64) -> f64 {
    let difference = (left - right).abs().rem_euclid(360.0);
    difference.min(360.0 - difference)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIVED: &str = "2026-08-14T16:20:10Z";

    fn config() -> AisStreamConfig {
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
            !format!("{:?}", AisStreamConfig::new(key, config().subscription)).contains("do-not")
        );
        let subscription = &config().subscription;
        assert_eq!(subscription.bounding_boxes().len(), 1);
        assert!((subscription.radius_kilometers() - 12.0).abs() < f64::EPSILON);
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
        let body = String::from_utf8(fixture("2026-08-14T16:20:00Z"))
            .expect("utf8 fixture")
            .replace("-80.1788", "-80.1760");
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
            .replace("\"Cog\": 274.0", "\"Cog\": 360.0");
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
