PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL CHECK (json_valid(value_json)),
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS bridge_state_intervals (
    source_id TEXT NOT NULL,
    bridge_key TEXT NOT NULL,
    bridge_name TEXT NOT NULL,
    relation TEXT NOT NULL CHECK (relation IN ('target', 'upstream')),
    state TEXT NOT NULL CHECK (state IN ('up', 'down', 'unknown')),
    started_at_ms INTEGER NOT NULL,
    ended_at_ms INTEGER,
    -- Engine run that observed this interval. Distinguishes a genuine bridge
    -- transition from an artifact of the app restarting, which otherwise write
    -- identical rows. NULL for rows recorded before sessions were tracked.
    session_id TEXT,
    PRIMARY KEY (source_id, bridge_key, started_at_ms),
    CHECK (ended_at_ms IS NULL OR ended_at_ms >= started_at_ms)
);

-- Booked river movements, kept so the transit offset can be learned.
--
-- The pilots' board publishes boarding times, not bridge times, and turning one
-- into a Brickell ETA needs an offset measured against openings that actually
-- happened. The bridge side of that pair was already durable in
-- bridge_state_intervals; this is the other side. Without it the app observed
-- both halves every ten minutes, used them live, and discarded the predictor --
-- so the offset its own collector calls uncalibrated could never stop being so.
--
-- Keyed by the movement rather than by the fetch: the same booking reappears on
-- the board for hours and may be revised, and what calibration needs is one row
-- per movement carrying the schedule it settled on.
CREATE TABLE IF NOT EXISTS river_transits (
    source_id TEXT NOT NULL,
    movement_key TEXT NOT NULL,
    vessel TEXT NOT NULL,
    action TEXT NOT NULL,
    -- Direction past the bridge; NULL when the board did not say.
    river_direction TEXT,
    -- Pilot boarding time from the board, which is what an offset is measured
    -- from.
    scheduled_at_ms INTEGER NOT NULL,
    -- The collector's uncalibrated guess, retained so a learned offset can be
    -- compared against the placeholder it replaces.
    estimated_bridge_at_ms INTEGER,
    estimated_offset_minutes INTEGER,
    first_seen_at_ms INTEGER NOT NULL,
    last_seen_at_ms INTEGER NOT NULL,
    session_id TEXT,
    PRIMARY KEY (source_id, movement_key)
);

CREATE INDEX IF NOT EXISTS river_transits_schedule
    ON river_transits(scheduled_at_ms);

CREATE UNIQUE INDEX IF NOT EXISTS bridge_state_intervals_current
    ON bridge_state_intervals(source_id, bridge_key)
    WHERE ended_at_ms IS NULL;

CREATE INDEX IF NOT EXISTS bridge_state_intervals_training
    ON bridge_state_intervals(bridge_key, state, started_at_ms);

-- Per-vessel opening ledger, learned from observed bridge-line crossings.
--
-- AIS never broadcasts air draft, so whether a hull needs the span raised is
-- learned per MMSI: a crossing while the target bridge was recorded up counts
-- toward `transits_opened`, a crossing while it stayed down counts toward
-- `transits_fits_under`, and the ratio feeds the predictor's
-- opening-propensity factor. Identity fields are the vessel's own public AIS
-- broadcast.
CREATE TABLE IF NOT EXISTS ais_vessel_ledger (
    mmsi TEXT PRIMARY KEY NOT NULL,
    name TEXT,
    vessel_class TEXT,
    length_meters REAL,
    draught_meters REAL,
    transits_opened INTEGER NOT NULL DEFAULT 0 CHECK (transits_opened >= 0),
    transits_fits_under INTEGER NOT NULL DEFAULT 0 CHECK (transits_fits_under >= 0),
    first_seen_ms INTEGER NOT NULL,
    last_seen_ms INTEGER NOT NULL,
    CHECK (last_seen_ms >= first_seen_ms)
);

-- One vessel passing the Brickell bridge line, either direction. Outcome is
-- resolved later against bridge_state_intervals: 'opened' when the crossing
-- sits inside a recorded up interval, 'fits_under' when the span verifiably
-- stayed down around it, 'unknown' when the record has a gap (an unresolved
-- outcome never trains the ledger).
CREATE TABLE IF NOT EXISTS ais_transits (
    mmsi TEXT NOT NULL,
    crossed_at_ms INTEGER NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('upriver', 'downriver')),
    speed_knots REAL,
    outcome TEXT CHECK (outcome IN ('opened', 'fits_under', 'unknown')),
    resolved_at_ms INTEGER,
    session_id TEXT,
    PRIMARY KEY (mmsi, crossed_at_ms)
);

CREATE INDEX IF NOT EXISTS ais_transits_unresolved
    ON ais_transits(crossed_at_ms)
    WHERE outcome IS NULL;

-- Where hulls actually ran, kept for a week so the charted centreline can be
-- calibrated against observed water rather than traced once and trusted
-- (docs/AIS_DISCOVERY.md §6). The projection is stored beside the raw fix:
-- `offset_meters` is the distance from the charted branch, so a leg the chart
-- has in the wrong place shows up as a run of fixes that all miss the same
-- way. Fixes are thinned to one per vessel per 30 s on the way in, which is
-- finer than any hull changes position, and pruned at seven days.
CREATE TABLE IF NOT EXISTS ais_track_fixes (
    mmsi TEXT NOT NULL,
    observed_at_ms INTEGER NOT NULL,
    latitude REAL NOT NULL,
    longitude REAL NOT NULL,
    speed_knots REAL,
    course_degrees REAL,
    -- Charted branch this fix projected onto, and its channel coordinates.
    branch TEXT,
    s_meters REAL,
    offset_meters REAL,
    posture TEXT,
    session_id TEXT,
    PRIMARY KEY (mmsi, observed_at_ms)
);

CREATE INDEX IF NOT EXISTS ais_track_fixes_recent
    ON ais_track_fixes(observed_at_ms);

-- Ledger seed: the four vessels whose crossings were observed end-to-end with
-- FL511 confirmation during the 2026-08-17 discovery session
-- (docs/AIS_DISCOVERY.md §5). Field observations of public broadcasts, not
-- fixtures: three tugs crossed inside Brickell's 17:50:30Z up interval, and
-- one Class B ran the river under closed spans.
INSERT OR IGNORE INTO ais_vessel_ledger
    (mmsi, name, vessel_class, transits_opened, transits_fits_under, first_seen_ms, last_seen_ms)
VALUES
    ('367705810', 'SARA', 'tug', 1, 0, 1786986780000, 1786989240000),
    ('371705000', 'COSTA V', 'tug', 1, 0, 1786986780000, 1786989300000),
    ('367705830', 'PEPIN', 'tug', 1, 0, 1786987140000, 1786989360000),
    ('338215012', 'BRIGHT SIDE', 'pleasure craft', 0, 1, 1786987770000, 1786987908000);

INSERT OR IGNORE INTO ais_transits
    (mmsi, crossed_at_ms, direction, speed_knots, outcome, resolved_at_ms, session_id)
VALUES
    ('367705810', 1786989240000, 'downriver', 3.4, 'opened', 1786989360000, 'discovery-2026-08-17'),
    ('371705000', 1786989300000, 'downriver', 4.6, 'opened', 1786989360000, 'discovery-2026-08-17'),
    ('367705830', 1786989360000, 'downriver', 3.9, 'opened', 1786989360000, 'discovery-2026-08-17'),
    ('338215012', 1786987770000, 'upriver', 6.4, 'fits_under', 1786987908000, 'discovery-2026-08-17');

CREATE TABLE IF NOT EXISTS incidents (
    id TEXT PRIMARY KEY NOT NULL,
    channel_id TEXT NOT NULL,
    state TEXT NOT NULL,
    urgency TEXT NOT NULL,
    material_revision INTEGER NOT NULL DEFAULT 1 CHECK (material_revision > 0),
    fingerprint TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    opened_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE INDEX IF NOT EXISTS incidents_channel_updated
    ON incidents(channel_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS delivery_outbox (
    id TEXT PRIMARY KEY NOT NULL,
    route_id TEXT NOT NULL,
    incident_id TEXT NOT NULL,
    material_revision INTEGER NOT NULL CHECK (material_revision > 0),
    action TEXT NOT NULL,
    request_json TEXT NOT NULL CHECK (json_valid(request_json)),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'leased', 'accepted', 'delivered', 'failed', 'suppressed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at TEXT NOT NULL,
    lease_until TEXT,
    provider_message_id TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(route_id, incident_id, material_revision, action)
);

CREATE INDEX IF NOT EXISTS delivery_outbox_ready
    ON delivery_outbox(status, next_attempt_at);

CREATE TABLE IF NOT EXISTS incident_history (
    incident_id TEXT NOT NULL,
    material_revision INTEGER NOT NULL,
    state TEXT NOT NULL,
    urgency TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (incident_id, material_revision),
    FOREIGN KEY (incident_id) REFERENCES incidents(id) ON DELETE CASCADE
);
