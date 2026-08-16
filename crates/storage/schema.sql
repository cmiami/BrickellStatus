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
