use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeStateDto {
    Clear,
    Possible,
    Likely,
    Open,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrgencyDto {
    Routine,
    HeadsUp,
    Action,
    Emergency,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityDto {
    Fresh,
    Delayed,
    Stale,
    Offline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AisConnectionStateDto {
    Disabled,
    NeedsKey,
    Armed,
    Live,
    Rejected,
    Disconnected,
}

/// Secret-free runtime health for the optional AISStream source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AisStreamStatusDto {
    pub enabled: bool,
    pub provider: AisProvider,
    pub api_key_configured: bool,
    pub source_registered: bool,
    pub connection_state: AisConnectionStateDto,
    pub availability: AvailabilityDto,
    pub radius_kilometers: f64,
    pub last_success_at: Option<String>,
    pub last_position_at: Option<String>,
    pub fresh_vessel_count: usize,
    pub detail: String,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStateDto {
    Pending,
    Accepted,
    Delivered,
    Failed,
    Suppressed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfacePresence {
    Home,
    Rotation,
    ActiveOnly,
    MessagesOnly,
    Off,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptPreset {
    Recommended,
    ConfirmedOnly,
    Meaningful,
    Off,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationIdDto {
    Epaper,
    Whatsapp,
    Desktop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceStrip {
    pub id: String,
    pub channel_id: String,
    pub source_id: String,
    pub source_label: String,
    pub title: String,
    pub detail: String,
    pub observed_at: String,
    pub age_seconds: u64,
    pub availability: AvailabilityDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contribution_bps: Option<i32>,
    pub state: EvidenceStateDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corroborated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupt: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStateDto {
    Live,
    Pending,
    Stale,
    Disabled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionSnapshot {
    pub channel_id: String,
    pub subject: String,
    pub state: BridgeStateDto,
    pub state_label: String,
    pub meaning: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_min: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_max: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_bps: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_basis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_legal_slot: Option<String>,
    pub opening_allowed_now: bool,
    pub availability: AvailabilityDto,
    pub source_age_seconds: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKindDto {
    Bridge,
    Weather,
    Official,
    Hurricane,
    News,
    Earthquake,
    Markets,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSignalDto {
    /// Provider-authored event title, normalized and bounded for delivery.
    pub headline: String,
    /// Concise provider detail or a typed fact summary; never a raw payload.
    pub detail: String,
    /// Concise factual status detail for the current channel signal.
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSnapshot {
    pub id: String,
    pub kind: ChannelKindDto,
    pub title: String,
    pub source_label: String,
    pub availability: AvailabilityDto,
    pub age_seconds: u64,
    /// True only when every configured source for this channel is currently
    /// usable (fresh or delayed). Bridge channels additionally require a
    /// current, healthy, non-conflicting authoritative target-down report.
    /// Consumers must not infer a resolved event from an inactive snapshot
    /// while this is false.
    pub coverage_complete: bool,
    pub summary: String,
    /// Deterministic identity for the currently actionable material. Unlike a
    /// count-only summary, this changes when one same-count alert replaces
    /// another and stays stable across source polling with identical content.
    pub material_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<ChannelSignalDto>,
    pub enabled: bool,
    pub active: bool,
    pub presence: SurfacePresence,
    pub interrupt_preset: InterruptPreset,
    pub destinations: Vec<DestinationIdDto>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStateDto {
    Ready,
    Degraded,
    Offline,
    Unconfigured,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSnapshot {
    pub id: DestinationIdDto,
    pub title: String,
    pub state: OutputStateDto,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accepted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_state: Option<DeliveryStateDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchRecord {
    pub id: String,
    pub incident_id: String,
    pub material_revision: u32,
    pub at: String,
    pub channel_id: String,
    pub title: String,
    pub state: String,
    pub urgency: UrgencyDto,
    pub destinations: Vec<DestinationIdDto>,
    pub delivery_state: DeliveryStateDto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemStatusDto {
    Nominal,
    Degraded,
    Offline,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHealth {
    pub source_id: String,
    pub channel_id: String,
    pub availability: AvailabilityDto,
    pub detail: String,
    pub failure_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemHealth {
    pub status: SystemStatusDto,
    pub sqlite_version: String,
    pub database_size_bytes: u64,
    pub engine_version: String,
    pub last_cycle_at: String,
    pub collectors_online: usize,
    pub collectors_total: usize,
    pub sources: Vec<SourceHealth>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VesselMovementDto {
    Approaching,
    Stationary,
    Unknown,
    Diverging,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VesselTrackPoint {
    pub latitude: f64,
    pub longitude: f64,
    pub observed_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VesselTrackSnapshot {
    pub mmsi: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessel_name: Option<String>,
    pub movement: VesselMovementDto,
    pub route_intersects: bool,
    pub speed_knots: f64,
    pub course_degrees: f64,
    pub observed_at: String,
    pub points: Vec<VesselTrackPoint>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedBridgeStateDto {
    Up,
    Down,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeRelationDto {
    Target,
    Upstream,
}

/// One uninterrupted FL511 observation interval exposed to the operator log.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStateIntervalDto {
    pub source_id: String,
    pub bridge_key: String,
    pub bridge_name: String,
    pub relation: BridgeRelationDto,
    pub state: ObservedBridgeStateDto,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub generated_at: String,
    pub local_time_zone: String,
    pub decision: DecisionSnapshot,
    pub evidence: Vec<EvidenceStrip>,
    pub channels: Vec<ChannelSnapshot>,
    pub outputs: Vec<OutputSnapshot>,
    pub dispatches: Vec<DispatchRecord>,
    pub bridge_intervals: Vec<BridgeStateIntervalDto>,
    pub vessel_tracks: Vec<VesselTrackSnapshot>,
    pub system: SystemHealth,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPreference {
    pub id: String,
    pub kind: ChannelKindDto,
    pub title: String,
    pub enabled: bool,
    pub presence: SurfacePresence,
    pub interrupt_preset: InterruptPreset,
    pub destinations: Vec<DestinationIdDto>,
    pub max_age_minutes: u32,
    pub max_items: usize,
    pub rotation_seconds: u32,
    pub scope: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertAreaSource {
    Preset,
    Search,
    Device,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertArea {
    pub id: String,
    pub label: String,
    pub latitude: f64,
    pub longitude: f64,
    pub time_zone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_area: Option<String>,
    pub source: AlertAreaSource,
    pub enabled: bool,
    pub weather_enabled: bool,
    pub official_alerts_enabled: bool,
    pub tropical_context_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationSearchResult {
    pub id: String,
    pub label: String,
    pub latitude: f64,
    pub longitude: f64,
    pub time_zone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_area: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietHours {
    pub enabled: bool,
    pub start: String,
    pub end: String,
    pub time_zone: String,
    pub bypass_emergency: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePreset {
    BridgeFirst,
    MiamiWatch,
    FullSignalDesk,
    QuietWatch,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitSystem {
    #[default]
    Imperial,
    Metric,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyProfile {
    pub id: String,
    pub name: String,
    pub preset: ProfilePreset,
    pub home_channel_id: String,
    pub quiet_hours: QuietHours,
    pub channels: Vec<ChannelPreference>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayTransport {
    Auto,
    Usb,
    Ble,
    Preview,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplaySettings {
    pub transport: DisplayTransport,
    pub serial_port: String,
    pub ble_name: String,
    pub dwell_seconds: u32,
    pub return_home_after: u32,
    pub full_refresh_every: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhatsAppRecipientConsent {
    #[default]
    NotRecorded,
    OptedIn,
    Unsubscribed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppSettings {
    pub enabled: bool,
    pub phone_number_id: String,
    pub recipient: String,
    pub graph_version: String,
    pub template_name: String,
    pub language_code: String,
    pub token_configured: bool,
    pub consent: WhatsAppRecipientConsent,
    pub consent_recipient: Option<String>,
    pub consent_recorded_at_millis: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AisProvider {
    #[default]
    Aisstream,
}

/// Non-secret AIS configuration. The API key itself is owned by the desktop
/// host's secret store and never crosses this serializable boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AisSettings {
    pub enabled: bool,
    pub provider: AisProvider,
    pub api_key_configured: bool,
    pub radius_kilometers: f64,
}

impl Default for AisSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: AisProvider::Aisstream,
            api_key_configured: false,
            radius_kilometers: 12.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    pub unit_system: UnitSystem,
    pub areas: Vec<AlertArea>,
    pub profile: PolicyProfile,
    pub display: DisplaySettings,
    pub whatsapp: WhatsAppSettings,
    pub ais: AisSettings,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub ok: bool,
    pub message: String,
}
