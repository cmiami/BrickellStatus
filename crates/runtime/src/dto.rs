use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeStateDto {
    Clear,
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

/// Where a channel sits against every other channel right now.
///
/// Computed once, in the engine, from typed facts. Every surface reads this
/// rather than deriving its own ranking: the panel, the notification path and
/// the console previously each decided urgency separately, and two of the three
/// discarded it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPriorityDto {
    /// Higher wins. Comparable across channel kinds.
    pub score: u16,
    pub urgency: UrgencyDto,
    /// Minutes until this affects the reader; `None` when the timing is unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imminence_minutes: Option<u16>,
    /// Observed rather than predicted.
    pub confirmed: bool,
}

impl Default for ChannelPriorityDto {
    fn default() -> Self {
        Self {
            score: 0,
            urgency: UrgencyDto::Routine,
            imminence_minutes: None,
            confirmed: false,
        }
    }
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
    Sports,
    Earthquake,
    Markets,
    System,
}

// No `Eq`: the signal now carries a price series, and float equality is not an
// equivalence relation. Nothing compares signals for identity anyway — that is
// what `band` and `material_key` are for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Coarse identity of *how much* and *how soon*, for channels whose signal
    /// is a measurement rather than an authored alert.
    ///
    /// A forecast that reads 62% at 40 minutes and then 95% at 5 minutes is the
    /// same sentence with different numbers, and both of the app's dedupe paths
    /// got it wrong in opposite directions: notifications hashed the digits away
    /// and never re-alerted, while the panel hashed the raw payload and
    /// re-alerted on every refresh. Banding the numbers gives both one identity
    /// that changes when the situation does and holds still when it does not.
    ///
    /// `None` for kinds whose material is an authored event; those already have
    /// a stable identity of their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<String>,
    /// Minutes until this signal's material starts to affect the reader.
    ///
    /// This is the one term that reorders channels against each other, so it
    /// must only be set by a rule that genuinely knows the answer. `None` costs
    /// the channel the imminence bonus, which is the correct price for not
    /// being able to say when something matters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imminence_minutes: Option<u16>,
    /// Recent values behind the headline, oldest first, in the signal's own
    /// units — a shape to draw, not a record to read from. Surfaces plot it;
    /// nothing computes with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<f64>,
    /// The level the change beside the series is measured against.
    ///
    /// Carried with the series because a plot without it is a shape with no
    /// meaning: a reader sees the price wander and cannot see which side of the
    /// day's starting point it wandered on. The series' own first sample is not
    /// a substitute — that is the open, and a quote that gapped down overnight
    /// can climb all session while still printing a loss.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_close: Option<f64>,
}

/// One independently current item within a channel.
///
/// A channel is a subscription and a notice is one slide. Keeping that
/// distinction in the snapshot means two earthquakes or two headlines do not
/// collapse into a count while only the first one reaches the screen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelNoticeDto {
    /// Stable identity from the collector item and channel subscription.
    pub key: String,
    /// The provider-owned page or document behind this slide, when the
    /// collector supplied one. Surfaces may link to it but must not invent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub signal: ChannelSignalDto,
    /// Item-level ranking used when notices from different channels interleave.
    pub priority: ChannelPriorityDto,
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
    /// Every current item, highest priority first. `signal` remains the first
    /// notice as a compatibility view for existing consumers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<ChannelNoticeDto>,
    /// Cross-channel ranking. See [`ChannelPriorityDto`].
    #[serde(default)]
    pub priority: ChannelPriorityDto,
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
    /// Motion and corridor projection at this fix. Older persisted cursors did
    /// not carry these fields, so history ingestion falls back to the track's
    /// latest values only when they are absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_knots: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub course_degrees: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s_meters: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_meters: Option<f64>,
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
    /// Broadcast ship-type word, when the vessel has sent a static report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vessel_class: Option<String>,
    /// Behavioral standing: `underway`, `moored`, `waiting`, `holding`,
    /// `off_channel`, or `deep_draft`. A surface showing traffic under way
    /// filters on this rather than inventing a second drift threshold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posture: Option<String>,
    /// Signed channel meters to the target span: positive upriver, negative
    /// seaward, measured along the charted centerline rather than as the crow
    /// flies. Absent for a fix taken outside corridor mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s_meters: Option<f64>,
    /// Which charted branch the fix projected onto: `river`, `north_approach`
    /// or `south_approach`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Radio call sign, when broadcast.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_sign: Option<String>,
    /// Permanent hull identity, unlike the MMSI which follows the licence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imo_number: Option<u32>,
    /// Skipper-entered destination. A hull naming a river berth has said where
    /// it is going, which is stronger evidence than any inference from course.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    /// Hull size from the vessel's static report. Absent until one arrives —
    /// a hull with no reported size must be drawn neutral, not guessed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_meters: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beam_meters: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draught_meters: Option<f64>,
    /// Learned likelihood this hull forces an opening, in basis points, from
    /// the durable vessel ledger. Absent means unproven, which is not the
    /// same as "fits under" and must not be displayed as one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opening_propensity: Option<u16>,
    /// Durable fact: this MMSI has at least one resolved Brickell passage
    /// during which the bridge went up. This is history, not a claim about the
    /// vessel's current course.
    #[serde(default)]
    pub known_opener: bool,
    /// Current-track judgement that this vessel is both committed to Brickell
    /// and likely to need the span raised. A known opener moving away is false;
    /// an unrecorded sailing rig approaching the bridge can be true.
    #[serde(default)]
    pub likely_to_open_brickell: bool,
    /// Minutes until this vessel reaches the span, as a range. Absent when it
    /// is not closing on the span at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_min_minutes: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_max_minutes: Option<u16>,
    /// Whether 33 CFR 117.261 lets this hull be passed outside the ordinary
    /// schedule. Tugs, tows and pilot craft are the commercial working traffic
    /// the bridge opens for when it would make a yacht wait.
    #[serde(default)]
    pub schedule_exempt: bool,
    /// When this vessel could actually be passed: its arrival for exempt
    /// traffic, otherwise the first ordinary opening at or after it. Absent
    /// when there is no ETA to reason from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_opening_at: Option<String>,
    /// True when the schedule pushed the opening later than the arrival, which
    /// is the difference between "it turns up" and "it gets through".
    #[serde(default)]
    pub waits_for_slot: bool,
    pub points: Vec<VesselTrackPoint>,
}

/// One vessel observed crossing the target bridge line.
///
/// Openings are otherwise unexplained events: the span is up and nobody can
/// say what for. A crossing recorded inside an up interval is the answer, when
/// the vessel was broadcasting at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCrossingDto {
    pub mmsi: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessel_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessel_class: Option<String>,
    /// "upriver" or "downriver".
    pub direction: String,
    pub crossed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_knots: Option<f64>,
    /// `opened`, `fits_under`, `unknown`, or absent while still unresolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// When the crossing was matched to a bridge state. Absent while pending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

/// Durable Brickell impact for one selected vessel.
///
/// Live position and AIS identity stay on [`VesselTrackSnapshot`]; this is the
/// slower learned record loaded only when a map vessel is selected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VesselDetailDto {
    pub mmsi: String,
    pub transits_opened: u64,
    pub transits_fits_under: u64,
    pub transits_unknown: u64,
    pub transits_pending: u64,
    pub first_seen_at: String,
    pub last_seen_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_crossing_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_opened_at: Option<String>,
    /// Beta(1,1)-smoothed likelihood, in basis points. Absent until at least
    /// one crossing has a bridge-impact outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening_propensity: Option<u16>,
    pub recent_crossings: Vec<BridgeCrossingDto>,
}

/// One vessel in the durable catalog of hulls observed raising Brickell.
///
/// Unlike [`VesselTrackSnapshot`], this record remains available when the
/// vessel has no position in the one-hour Map window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownOpenerDto {
    pub mmsi: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessel_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessel_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_sign: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imo_number: Option<u32>,
    pub transits_opened: u64,
    pub transits_fits_under: u64,
    pub first_seen_at: String,
    pub last_seen_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_opened_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening_propensity: Option<u16>,
}

/// The AIS corridor as drawable geometry, published so a surface highlights
/// exactly the water the collector subscribes to and tests fixes against.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiverCorridorDto {
    pub bridge_latitude: f64,
    pub bridge_longitude: f64,
    /// Whether an AIS source is actually running. False means the geometry is
    /// still true but no vessel is being received, which a surface must say
    /// rather than present as empty water.
    pub ais_live: bool,
    pub branches: Vec<RiverCorridorBranchDto>,
}

/// A named point on the corridor: a bascule, the mouth, or a charted mark.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiverStationDto {
    pub label: String,
    /// `target`, `bridge`, `mouth`, or `waypoint`.
    pub kind: String,
    /// FL511 selector key when this station is a bascule the app watches, so a
    /// surface can join it to live bridge state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_key: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    /// Signed channel metres from the span: positive upriver, negative seaward.
    pub s_meters: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiverCorridorBranchDto {
    pub id: String,
    pub label: String,
    /// Half-width of the tracked water either side of the centerline.
    pub corridor_offset_meters: f64,
    /// `[latitude, longitude]` waypoints, mouth-first.
    pub centerline: Vec<[f64; 2]>,
    /// Named points along this branch, mouth-first.
    pub stations: Vec<RiverStationDto>,
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
    /// Position counting upstream from the target, 0 for the target itself.
    /// Surfaces exist so a client can present spans in river order rather than
    /// inventing an ordering that would drift from the engine's.
    pub river_order: u8,
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
    /// Every locally learned Brickell opener, including vessels that have no
    /// current AIS position and therefore cannot appear on the live map.
    #[serde(default)]
    pub known_openers: Vec<KnownOpenerDto>,
    /// The river this app reasons about. Always present; see `ais_live` for
    /// whether anything is currently being received on it.
    pub river_corridor: RiverCorridorDto,
    /// Recent bridge-line crossings, newest first, so an opening can be
    /// attributed to the hull that caused it.
    #[serde(default)]
    pub bridge_crossings: Vec<BridgeCrossingDto>,
    pub system: SystemHealth,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPreference {
    pub id: String,
    pub kind: ChannelKindDto,
    pub title: String,
    pub enabled: bool,
    /// Retained for stored-profile compatibility. The runtime normalizes these
    /// former alert/carousel controls to one automatic policy on load and save.
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
    pub full_refresh_every: u32,
    /// Which way up the board is mounted.
    ///
    /// `#[serde(default)]` so a profile written before this existed still
    /// loads, and lands upright — which is what it was.
    #[serde(default)]
    pub orientation: DisplayOrientation,
}

/// Which edge of the panel the reader treats as the top.
///
/// Half a turn is the only rotation offered because it is the only one the
/// hardware allows: the glass is bonded to the board, so turning the board over
/// is how it sits differently, and a quarter turn would hand 250x122 hardware a
/// 122x250 image that the firmware refuses on size.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayOrientation {
    #[default]
    Upright,
    Inverted,
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
    /// Whether the source should run. Follows the key rather than a switch:
    /// storing a key turns it on, clearing one turns it off. Asking twice only
    /// created a state where a key is present and nothing happens.
    pub enabled: bool,
    pub provider: AisProvider,
    pub api_key_configured: bool,
}

impl Default for AisSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: AisProvider::Aisstream,
            api_key_configured: false,
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
