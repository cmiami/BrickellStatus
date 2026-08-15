export type BridgeState =
  | 'clear'
  | 'possible'
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
}

export interface ChannelSnapshot {
  id: string;
  kind: 'bridge' | 'weather' | 'official' | 'hurricane' | 'news' | 'earthquake' | 'markets' | 'system';
  title: string;
  sourceLabel: string;
  availability: Availability;
  ageSeconds: number;
  coverageComplete: boolean;
  summary: string;
  materialKey: string;
  signal?: ChannelSignal;
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

export interface VesselTrack {
  mmsi: string;
  vesselName?: string;
  movement: 'approaching' | 'stationary' | 'unknown' | 'diverging';
  routeIntersects: boolean;
  speedKnots: number;
  courseDegrees: number;
  observedAt: string;
  points: VesselTrackPoint[];
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
  system: SystemHealth;
}

export interface BridgeStateInterval {
  sourceId: string;
  bridgeKey: string;
  bridgeName: string;
  relation: 'target' | 'upstream';
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
  returnHomeAfter: number;
  fullRefreshEvery: number;
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
  radiusKilometers: number;
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
