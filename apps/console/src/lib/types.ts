export type BridgeState =
  | 'clear'
  | 'likely'
  | 'open';

export type Urgency = 'routine' | 'heads_up' | 'action' | 'emergency';
export type Availability = 'fresh' | 'delayed' | 'stale' | 'offline';
export type DeliveryState = 'pending' | 'accepted' | 'delivered' | 'failed' | 'suppressed';
export type SurfacePresence = 'home' | 'rotation' | 'active_only' | 'messages_only' | 'off';
export type InterruptPreset = 'recommended' | 'confirmed_only' | 'meaningful' | 'off' | 'custom';
export type DestinationId = 'epaper' | 'whatsapp' | 'desktop';
export type UnitSystem = 'imperial' | 'metric';

export interface EvidenceStrip {
  id: string;
  channelId: string;
  sourceId: string;
  sourceLabel: string;
  title: string;
  detail: string;
  observedAt: string;
  ageSeconds: number;
  availability: Availability;
  contributionBps?: number;
  state: 'live' | 'pending' | 'stale' | 'disabled';
  corroborated?: boolean;
  interrupt?: boolean;
}

export interface DecisionSnapshot {
  channelId: string;
  subject: string;
  state: BridgeState;
  stateLabel: string;
  meaning: string;
  action: string;
  etaMin?: number;
  etaMax?: number;
  confidenceBps?: number;
  confidenceLabel?: string;
  confidenceBasis?: string;
  nextLegalSlot?: string;
  openingAllowedNow: boolean;
  availability: Availability;
  sourceAgeSeconds: number;
}

export interface ChannelSignal {
  headline: string;
  detail: string;
  action: string;
  severity?: string;
  expiresAt?: string;
  band?: string;
  imminenceMinutes?: number;
  // Recent values behind the headline, oldest first — a shape to draw.
  series?: number[];
}

// How this channel ranks against every other, computed once in the engine.
// Ordering across kinds is a property of the score, not of the surface reading
// it, so the console sorts by it rather than inventing its own hierarchy.
export interface ChannelPriority {
  score: number;
  urgency: Urgency;
  imminenceMinutes?: number;
  confirmed: boolean;
}

export interface ChannelSnapshot {
  id: string;
  kind:
    | 'bridge'
    | 'weather'
    | 'official'
    | 'hurricane'
    | 'news'
    | 'sports'
    | 'earthquake'
    | 'markets'
    | 'system';
  title: string;
  sourceLabel: string;
  availability: Availability;
  ageSeconds: number;
  coverageComplete: boolean;
  summary: string;
  materialKey: string;
  signal?: ChannelSignal;
  priority: ChannelPriority;
  enabled: boolean;
  active: boolean;
  presence: SurfacePresence;
  interruptPreset: InterruptPreset;
  destinations: DestinationId[];
}

export interface OutputSnapshot {
  id: DestinationId;
  title: string;
  state: 'ready' | 'degraded' | 'offline' | 'unconfigured';
  detail: string;
  lastAcceptedAt?: string;
  deliveryState?: DeliveryState;
}

export interface DispatchRecord {
  id: string;
  incidentId: string;
  materialRevision: number;
  at: string;
  channelId: string;
  title: string;
  state: string;
  urgency: Urgency;
  destinations: DestinationId[];
  deliveryState: DeliveryState;
}

export interface SystemHealth {
  status: 'nominal' | 'degraded' | 'offline';
  sqliteVersion: string;
  databaseSizeBytes: number;
  engineVersion: string;
  lastCycleAt: string;
  collectorsOnline: number;
  collectorsTotal: number;
  sources: SourceHealth[];
}

export interface SourceHealth {
  sourceId: string;
  channelId: string;
  availability: Availability;
  detail: string;
  failureCount: number;
  lastAttemptAt?: string;
  lastSuccessAt?: string;
}

export interface VesselTrackPoint {
  latitude: number;
  longitude: number;
  observedAt: string;
}

/** Behavioural standing, decided by the engine from the track itself. */
export type VesselPosture =
  | 'underway'
  | 'waiting'
  | 'holding'
  | 'moored'
  | 'off_channel'
  | 'deep_draft';

export interface VesselTrack {
  mmsi: string;
  vesselName?: string;
  movement: 'approaching' | 'stationary' | 'unknown' | 'diverging';
  routeIntersects: boolean;
  speedKnots: number;
  courseDegrees: number;
  observedAt: string;
  /** Broadcast ship-type word, when the vessel has sent a static report. */
  vesselClass?: string;
  posture?: VesselPosture;
  /** Signed channel metres to the span: positive upriver, negative seaward. */
  sMeters?: number;
  /** Which charted branch the fix projected onto. */
  branch?: 'river' | 'north_approach' | 'government_cut' | 'south_approach';
  /** Broadcast identity, present only when the vessel has sent a static report. */
  callSign?: string;
  imoNumber?: number;
  /** Skipper-entered destination, when it is something meaningful. */
  destination?: string;
  /** Hull size from the static report; absent until one is received. */
  lengthMeters?: number;
  beamMeters?: number;
  draughtMeters?: number;
  /**
   * Learned likelihood this hull forces an opening, in basis points. Absent
   * means unproven, which is not the same as "fits under".
   */
  openingPropensity?: number;
  /** Minutes until this vessel reaches the span. Absent when it is not closing. */
  etaMinMinutes?: number;
  etaMaxMinutes?: number;
  /** Whether 33 CFR 117.261 lets this hull be passed outside the schedule. */
  scheduleExempt?: boolean;
  /** When it could actually be passed, schedule included. */
  predictedOpeningAt?: string;
  /** True when the schedule pushed the opening later than the arrival. */
  waitsForSlot?: boolean;
  points: VesselTrackPoint[];
}

/** A named point on the corridor: a bascule, the mouth, or a charted mark. */
export interface RiverStation {
  label: string;
  kind: 'target' | 'bridge' | 'mouth' | 'waypoint';
  /** FL511 selector key when this station is a bascule the app watches. */
  bridgeKey?: string;
  latitude: number;
  longitude: number;
  /** Signed channel metres from the span: positive upriver, negative seaward. */
  sMeters: number;
}

export interface RiverCorridorBranch {
  id: string;
  label: string;
  /** Half-width of the tracked water either side of the centreline. */
  corridorOffsetMeters: number;
  /** `[latitude, longitude]` waypoints, mouth-first. */
  centerline: [number, number][];
  /** Named points along this branch, mouth-first. */
  stations: RiverStation[];
}

/** One vessel observed crossing the target bridge line. */
export interface BridgeCrossing {
  mmsi: string;
  vesselName?: string;
  vesselClass?: string;
  direction: 'upriver' | 'downriver';
  crossedAt: string;
  speedKnots?: number;
  /** `opened`, `fits_under`, `unknown`, or absent while still unresolved. */
  outcome?: 'opened' | 'fits_under' | 'unknown';
}

export interface RiverCorridor {
  bridgeLatitude: number;
  bridgeLongitude: number;
  /** Whether an AIS source is actually running and receiving vessels. */
  aisLive: boolean;
  branches: RiverCorridorBranch[];
}

export interface AppSnapshot {
  generatedAt: string;
  localTimeZone: string;
  decision: DecisionSnapshot;
  evidence: EvidenceStrip[];
  channels: ChannelSnapshot[];
  outputs: OutputSnapshot[];
  dispatches: DispatchRecord[];
  bridgeIntervals: BridgeStateInterval[];
  vesselTracks: VesselTrack[];
  /** Always present; `aisLive` says whether vessels are being received. */
  riverCorridor: RiverCorridor;
  /** Recent bridge-line crossings, newest first. */
  bridgeCrossings: BridgeCrossing[];
  system: SystemHealth;
}

export interface BridgeStateInterval {
  sourceId: string;
  bridgeKey: string;
  bridgeName: string;
  relation: 'target' | 'upstream';
  /** Position upstream from the target; 0 is the target itself. */
  riverOrder: number;
  state: 'up' | 'down' | 'unknown';
  startedAt: string;
  endedAt?: string;
}

export interface ChannelPreference {
  id: string;
  kind: ChannelSnapshot['kind'];
  title: string;
  enabled: boolean;
  presence: SurfacePresence;
  interruptPreset: InterruptPreset;
  destinations: DestinationId[];
  maxAgeMinutes: number;
  maxItems: number;
  rotationSeconds: number;
  scope: Record<string, string | number | boolean | string[]>;
}

export interface QuietHours {
  enabled: boolean;
  start: string;
  end: string;
  timeZone: string;
  bypassEmergency: boolean;
}

export interface AlertArea {
  id: string;
  label: string;
  latitude: number;
  longitude: number;
  timeZone: string;
  countryCode?: string;
  adminArea?: string;
  source: 'preset' | 'search' | 'device' | 'manual';
  enabled: boolean;
  weatherEnabled: boolean;
  officialAlertsEnabled: boolean;
  tropicalContextEnabled: boolean;
}

export interface LocationSearchResult {
  id: string;
  label: string;
  latitude: number;
  longitude: number;
  timeZone: string;
  countryCode?: string;
  adminArea?: string;
}

export interface LocationMapPoint {
  id: string;
  label: string;
  latitude: number;
  longitude: number;
  detail?: string;
  kind?: 'saved' | 'candidate' | 'bridge' | 'vessel';
  enabled?: boolean;
  draggable?: boolean;
  courseDegrees?: number;
}

export interface PolicyProfile {
  id: string;
  name: string;
  preset: 'bridge_first' | 'miami_watch' | 'full_signal_desk' | 'quiet_watch' | 'custom';
  homeChannelId: string;
  quietHours: QuietHours;
  channels: ChannelPreference[];
}

export interface DisplaySettings {
  transport: 'auto' | 'usb' | 'ble' | 'preview';
  serialPort: string;
  bleName: string;
  dwellSeconds: number;
  fullRefreshEvery: number;
  orientation: 'upright' | 'inverted';
}

export interface WhatsAppSettings {
  enabled: boolean;
  phoneNumberId: string;
  recipient: string;
  graphVersion: string;
  templateName: string;
  languageCode: string;
  tokenConfigured: boolean;
  consent: 'not_recorded' | 'opted_in' | 'unsubscribed';
  consentRecipient?: string | null;
  consentRecordedAtMillis?: number | null;
}

export interface AisSettings {
  enabled: boolean;
  provider: 'aisstream';
  apiKeyConfigured: boolean;
}

export interface AisStreamStatus {
  configured: boolean;
  enabled: boolean;
  state: 'disabled' | 'missing_key' | 'ready' | 'live' | 'degraded';
  detail: string;
  lastPositionAt?: string;
  vesselsInRange?: number;
}

export interface DisplayConnectionStatus {
  state: 'connected' | 'connecting' | 'disconnected' | 'unavailable' | 'error';
  transport: 'usb' | 'ble' | null;
  deviceName?: string;
  detail: string;
  lastFrameAt?: string;
  lastAckAt?: string;
  /** The panel the connected board reported. Absent until one has spoken. */
  panel?: PanelModel;
}

export interface DisplayDeviceCandidate {
  id: string;
  name: string;
  transport: 'usb' | 'ble';
  detail: string;
  signalStrength?: number;
}

export interface AppPreferences {
  unitSystem: UnitSystem;
  areas: AlertArea[];
  profile: PolicyProfile;
  ais: AisSettings;
  display: DisplaySettings;
  whatsapp: WhatsAppSettings;
}

export interface MutationResult {
  ok: boolean;
  message: string;
}

export interface EinkPreview {
  channelId: string;
  png: number[];
}

/// Which e-paper board is attached. Reported by the board itself; never asked.
export type PanelModel = 'e213' | 'e290';

/// Which E213 panel controller a build carries. The one thing about the
/// hardware that cannot be read back, and so the only thing ever remembered.
export type PanelRevision = 'original' | 'v11';

export type FlashRequirement =
  | { state: 'noDevice' }
  | { state: 'upToDate'; build: string }
  | { state: 'unknownBuild' }
  | {
      state: 'required';
      reason:
        | { kind: 'notResponding' }
        | { kind: 'buildMismatch'; device: string; bundled: string }
        | { kind: 'wrongBoard'; board: PanelModel };
    };

export interface FirmwareVariantSummary {
  id: string;
  label: string;
  panel: PanelModel;
  panelRevision?: PanelRevision;
  totalBytes: number;
}

export interface FirmwareStatus {
  port?: string;
  bundledBuild?: string;
  /** The board that answered, when one has. */
  board?: PanelModel;
  /** The build to write to it, decided from what the board reported. */
  recommendedVariantId?: string;
  variants: FirmwareVariantSummary[];
  requirement: FlashRequirement;
  unavailable?: string;
}

/** Device pixels each panel draws, for anything that has to state the size. */
export const PANEL_GEOMETRY: Record<PanelModel, { width: number; height: number; label: string }> = {
  e213: { width: 250, height: 122, label: 'E213' },
  e290: { width: 296, height: 128, label: 'E290' }
};

// One observed radar composite, as a MapLibre raster template rather than as
// imagery: the pixels are fetched by the map, straight from the tile host.
export interface RadarLayer {
  tileUrlTemplate: string;
  observedAt: string;
  ageSeconds: number;
  maxZoom: number;
  attribution: string;
}

/** What the running platform can physically do, from `get_platform_capabilities`. */
export interface PlatformCapabilities {
  /** Whether a USB serial panel connection can be opened at all. */
  usbDisplay: boolean;
  /** Whether this build ships firmware it could write to a board. */
  firmwareFlashing: boolean;
}
