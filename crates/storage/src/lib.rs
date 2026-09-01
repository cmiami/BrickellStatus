//! Durable storage for BrickellStatus.
//!
//! SQLite persists runtime settings and retry-safe outbound work.

use std::{path::Path, str::FromStr, time::Duration};

use serde::{Serialize, de::DeserializeOwned};
use sqlx::{
    Row, Sqlite, SqlitePool, Transaction,
    sqlite::{
        SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePoolOptions,
        SqliteSynchronous,
    },
};
use thiserror::Error;
use uuid::Uuid;

const MINIMUM_SQLITE: (u64, u64, u64) = (3, 51, 3);
const SCHEMA_SQL: &str = include_str!("../schema.sql");

/// A successful FL511 reading more than two minutes after the previous one is
/// a new observation run, not proof that the old state held through the gap.
pub const BRIDGE_CONTINUITY_MAX_GAP_MS: i64 = 2 * 60 * 1_000;

/// Raw fixes are durable at most once per hull per half-minute.
pub const AIS_TRACK_FIX_MIN_SPACING_MS: i64 = 30 * 1_000;

/// Default raw AIS history horizon. The caller still supplies an explicit
/// cutoff to pruning, so installations can retain more or less, but a full
/// year gives corridor calibration seasonal depth instead of a one-week
/// glimpse. Fixes for a hull proven to open the bridge are never pruned.
pub const DEFAULT_AIS_TRACK_RETENTION_MS: i64 = 365 * 24 * 60 * 60 * 1_000;

/// Default horizon for minute-level forecast evaluation samples.
pub const DEFAULT_FORECAST_RETENTION_MS: i64 = 2 * 365 * 24 * 60 * 60 * 1_000;

/// The least history any pruning pass may leave behind: four weeks.
///
/// Bridge intervals, crossings, the vessel ledger and pilots-board movements
/// are never pruned at all. Raw fixes and forecast samples default to a year
/// and two years. This floor exists so that no caller, preference, or future
/// default can cut the learnable record below the span the calibration needs
/// to see a full month of weekday and weekend openings.
pub const MIN_HISTORY_RETENTION_MS: i64 = 28 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SQLite {found} is unsafe for this WAL workload; {required} or newer is required")]
    UnsafeSqlite { found: String, required: String },
    #[error("invalid SQLite version string: {0}")]
    InvalidSqliteVersion(String),
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

/// An atomic group of storage writes.
///
/// Call [`StoreTransaction::commit`] to make the writes durable. Dropping an
/// uncommitted transaction rolls every write in the group back.
pub struct StoreTransaction<'a> {
    inner: Transaction<'a, Sqlite>,
}

#[derive(Debug, Clone)]
pub struct IncidentRecord<'a, T> {
    pub id: Uuid,
    pub channel_id: &'a str,
    pub state: &'a str,
    pub urgency: &'a str,
    pub material_revision: i64,
    pub fingerprint: &'a str,
    pub payload: &'a T,
    pub opened_at: &'a str,
    pub updated_at: &'a str,
    pub resolved_at: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct OutboxRecord<'a, T> {
    pub id: Uuid,
    pub route_id: &'a str,
    pub incident_id: Uuid,
    pub material_revision: i64,
    pub action: &'a str,
    pub request: &'a T,
    pub next_attempt_at: &'a str,
    pub created_at: &'a str,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxLease {
    pub id: String,
    pub route_id: String,
    pub incident_id: String,
    pub material_revision: i64,
    pub action: String,
    pub urgency: Option<String>,
    pub request_json: String,
    pub attempts: i64,
}

/// Credential-free durable delivery history for operator-facing ledgers.
///
/// The request JSON contains the provider-independent notice and destination,
/// but never credentials or provider authorization headers. Callers must map
/// it into a redacted view instead of exposing this row directly.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxHistoryRow {
    pub id: String,
    pub route_id: String,
    pub incident_id: String,
    pub material_revision: i64,
    pub action: String,
    pub urgency: Option<String>,
    pub request_json: String,
    pub status: String,
    pub attempts: i64,
    pub provider_message_id: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// One uninterrupted FL511 state interval for a configured bridge.
#[derive(Debug, Clone, Eq, PartialEq, sqlx::FromRow)]
pub struct BridgeStateInterval {
    pub source_id: String,
    pub bridge_key: String,
    pub bridge_name: String,
    pub relation: String,
    pub state: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    /// Last successful source reading that explicitly confirmed `state`.
    pub last_confirmed_at_ms: i64,
    /// `state_change`, `session_start`, `continuity_gap`, or the initial/legacy
    /// equivalent. Training can distinguish observation boundaries from real
    /// bridge movement.
    pub start_reason: String,
    /// Engine run that recorded the interval; NULL on rows written before
    /// sessions were tracked.
    pub session_id: Option<String>,
}

/// One FL511 reading for one bridge.
///
/// Grouped rather than passed positionally: the call carries four adjacent
/// string fields, and transposing `relation` and `state` would write a
/// well-formed row that means something else entirely.
#[derive(Clone, Copy, Debug)]
pub struct BridgeObservation<'a> {
    pub source_id: &'a str,
    pub bridge_key: &'a str,
    pub bridge_name: &'a str,
    pub relation: &'a str,
    pub state: &'a str,
    pub observed_at_ms: i64,
    /// Engine run that took the reading.
    pub session_id: &'a str,
}

/// One booked river movement, as the pilots' board published it.
///
/// Recorded so the transit offset between a boarding time and a Brickell
/// opening can be measured later. The collector emits an explicitly
/// uncalibrated estimate and says the real offset has to be learned from
/// observed openings; nothing was keeping the movements, so it never could be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiverTransitObservation<'a> {
    pub source_id: &'a str,
    /// Stable identity of the movement, not of the fetch that saw it.
    pub movement_key: &'a str,
    pub vessel: &'a str,
    pub action: &'a str,
    pub river_direction: Option<&'a str>,
    pub scheduled_at_ms: i64,
    pub estimated_bridge_at_ms: Option<i64>,
    pub estimated_offset_minutes: Option<i64>,
    pub observed_at_ms: i64,
    pub session_id: &'a str,
}

/// One recorded movement, for calibration.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct RiverTransitRecord {
    pub vessel: String,
    pub action: String,
    pub river_direction: Option<String>,
    pub scheduled_at_ms: i64,
    pub estimated_bridge_at_ms: Option<i64>,
    pub estimated_offset_minutes: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PruneReport {
    pub scrubbed_destinations: u64,
    pub outbox_rows: u64,
    pub incidents: u64,
    pub track_fixes: u64,
}

/// A stored position fix as read back for calibration and training.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ObservedTrackFix {
    pub mmsi: String,
    pub observed_at_ms: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub speed_knots: Option<f64>,
    pub course_degrees: Option<f64>,
    pub branch: Option<String>,
    pub s_meters: Option<f64>,
    pub offset_meters: Option<f64>,
    pub posture: Option<String>,
}

/// One observed AIS position fix, with the channel coordinates it projected
/// to at the time it was seen.
#[derive(Clone, Copy, Debug)]
pub struct AisTrackFix<'a> {
    pub mmsi: &'a str,
    pub observed_at_ms: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub speed_knots: Option<f64>,
    pub course_degrees: Option<f64>,
    pub branch: Option<&'a str>,
    pub s_meters: Option<f64>,
    pub offset_meters: Option<f64>,
    pub posture: Option<&'a str>,
    pub session_id: &'a str,
}

/// Static AIS identity observed for any hull in range, whether or not it has
/// crossed Brickell yet.
#[derive(Clone, Copy, Debug)]
pub struct AisVesselObservation<'a> {
    pub mmsi: &'a str,
    pub name: Option<&'a str>,
    pub vessel_class: Option<&'a str>,
    pub call_sign: Option<&'a str>,
    pub imo_number: Option<u32>,
    pub destination: Option<&'a str>,
    pub length_meters: Option<f64>,
    pub beam_meters: Option<f64>,
    pub draught_meters: Option<f64>,
    pub observed_at_ms: i64,
}

/// One observed AIS bridge-line crossing, ready to become ledger history.
#[derive(Clone, Copy, Debug)]
pub struct AisCrossingObservation<'a> {
    pub mmsi: &'a str,
    pub vessel_name: Option<&'a str>,
    pub vessel_class: Option<&'a str>,
    pub length_meters: Option<f64>,
    pub draught_meters: Option<f64>,
    /// "upriver" or "downriver".
    pub direction: &'a str,
    pub crossed_at_ms: i64,
    pub speed_knots: f64,
    pub session_id: &'a str,
}

/// One recorded bridge-line crossing, with the vessel's identity attached.
///
/// This is what lets an opening be attributed to a hull rather than left as an
/// unexplained event. `outcome` is `opened` once the crossing has been matched
/// to a recorded up interval.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AisCrossingRecord {
    pub mmsi: String,
    pub name: Option<String>,
    pub vessel_class: Option<String>,
    pub direction: String,
    pub crossed_at_ms: i64,
    pub speed_knots: Option<f64>,
    pub outcome: Option<String>,
    pub resolved_at_ms: Option<i64>,
}

/// A vessel's learned opening record.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AisLedgerEntry {
    pub mmsi: String,
    pub name: Option<String>,
    pub vessel_class: Option<String>,
    pub call_sign: Option<String>,
    pub imo_number: Option<i64>,
    pub destination: Option<String>,
    pub length_meters: Option<f64>,
    pub beam_meters: Option<f64>,
    pub draught_meters: Option<f64>,
    pub transits_opened: i64,
    pub transits_fits_under: i64,
    pub transits_unknown: i64,
    pub transits_pending: i64,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    pub last_crossing_at_ms: Option<i64>,
    pub last_opened_at_ms: Option<i64>,
}

/// One vessel's durable Brickell record, including its newest crossings.
#[derive(Clone, Debug)]
pub struct AisVesselHistory {
    pub ledger: AisLedgerEntry,
    pub recent_crossings: Vec<AisCrossingRecord>,
}

/// One versioned forecast evaluation retained for later accuracy analysis.
#[derive(Clone, Copy, Debug)]
pub struct ForecastSample<'a> {
    pub target_key: &'a str,
    pub evaluated_at_ms: i64,
    pub model_version: &'a str,
    pub state: &'a str,
    pub predictive_score_bps: i64,
    pub confidence_bps: i64,
    pub eta_min_minutes: Option<i64>,
    pub eta_max_minutes: Option<i64>,
    pub schedule_mode: &'a str,
    /// Compact JSON object mapping evidence labels to applied basis points.
    pub contribution_bps_json: &'a str,
    /// Compact JSON object carrying source age/availability at evaluation.
    pub source_freshness_json: &'a str,
    pub session_id: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct ForecastSampleRecord {
    pub target_key: String,
    pub evaluated_at_ms: i64,
    pub minute_bucket_ms: i64,
    pub model_version: String,
    pub state: String,
    pub predictive_score_bps: i64,
    pub confidence_bps: i64,
    pub eta_min_minutes: Option<i64>,
    pub eta_max_minutes: Option<i64>,
    pub schedule_mode: String,
    pub contribution_bps_json: String,
    pub source_freshness_json: String,
    pub session_id: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LegacyLearningImportReport {
    pub vessels_added: u64,
    pub transits_added: u64,
    pub track_fixes_added: u64,
    pub river_transits_added: u64,
}

#[derive(Debug, sqlx::FromRow)]
struct LegacyVesselRow {
    mmsi: String,
    name: Option<String>,
    vessel_class: Option<String>,
    length_meters: Option<f64>,
    draught_meters: Option<f64>,
    first_seen_ms: i64,
    last_seen_ms: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct LegacyTransitRow {
    mmsi: String,
    crossed_at_ms: i64,
    direction: String,
    speed_knots: Option<f64>,
    outcome: Option<String>,
    resolved_at_ms: Option<i64>,
    session_id: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct LegacyTrackFixRow {
    mmsi: String,
    observed_at_ms: i64,
    latitude: f64,
    longitude: f64,
    speed_knots: Option<f64>,
    course_degrees: Option<f64>,
    branch: Option<String>,
    s_meters: Option<f64>,
    offset_meters: Option<f64>,
    posture: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct LegacyRiverTransitRow {
    source_id: String,
    movement_key: String,
    vessel: String,
    action: String,
    river_direction: Option<String>,
    scheduled_at_ms: i64,
    estimated_bridge_at_ms: Option<i64>,
    estimated_offset_minutes: Option<i64>,
    first_seen_at_ms: i64,
    last_seen_at_ms: i64,
    session_id: Option<String>,
}

impl StoreTransaction<'_> {
    /// Upserts one JSON setting as part of this transaction.
    pub async fn set_json<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        updated_at: &str,
    ) -> Result<(), StorageError> {
        set_json_on(&mut self.inner, key, value, updated_at).await
    }

    /// Records a booked movement, or refreshes the one already held.
    ///
    /// Upserted on the movement rather than appended per fetch: the same
    /// booking sits on the board for hours and may be revised, and calibration
    /// wants the schedule it settled on, with the window over which it was
    /// visible. `first_seen_at_ms` is preserved so a revision cannot make a
    /// movement look newly announced.
    pub async fn record_river_transit(
        &mut self,
        observation: RiverTransitObservation<'_>,
    ) -> Result<(), StorageError> {
        let RiverTransitObservation {
            source_id,
            movement_key,
            vessel,
            action,
            river_direction,
            scheduled_at_ms,
            estimated_bridge_at_ms,
            estimated_offset_minutes,
            observed_at_ms,
            session_id,
        } = observation;
        sqlx::query(
            r#"
            INSERT INTO river_transits(
                source_id, movement_key, vessel, action, river_direction,
                scheduled_at_ms, estimated_bridge_at_ms, estimated_offset_minutes,
                first_seen_at_ms, last_seen_at_ms, session_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)
            ON CONFLICT(source_id, movement_key) DO UPDATE SET
                vessel = excluded.vessel,
                action = excluded.action,
                river_direction = excluded.river_direction,
                scheduled_at_ms = excluded.scheduled_at_ms,
                estimated_bridge_at_ms = excluded.estimated_bridge_at_ms,
                estimated_offset_minutes = excluded.estimated_offset_minutes,
                last_seen_at_ms = excluded.last_seen_at_ms,
                session_id = excluded.session_id
            "#,
        )
        .bind(source_id)
        .bind(movement_key)
        .bind(vessel)
        .bind(action)
        .bind(river_direction)
        .bind(scheduled_at_ms)
        .bind(estimated_bridge_at_ms)
        .bind(estimated_offset_minutes)
        .bind(observed_at_ms)
        .bind(session_id)
        .execute(&mut *self.inner)
        .await?;
        Ok(())
    }

    /// Records one observed position fix. Re-offering a fix already held is
    /// how the caller works — the live window it reads from overlaps every
    /// cycle — so a repeat is ignored rather than treated as an error.
    pub async fn record_ais_track_fix(&mut self, fix: AisTrackFix<'_>) -> Result<(), StorageError> {
        let AisTrackFix {
            mmsi,
            observed_at_ms,
            latitude,
            longitude,
            speed_knots,
            course_degrees,
            branch,
            s_meters,
            offset_meters,
            posture,
            session_id,
        } = fix;
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO ais_track_fixes(
                mmsi, observed_at_ms, latitude, longitude, speed_knots,
                course_degrees, branch, s_meters, offset_meters, posture,
                session_id
            )
            SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11
            WHERE NOT EXISTS (
                SELECT 1 FROM ais_track_fixes existing
                WHERE existing.mmsi = ?1
                  AND existing.observed_at_ms > ?2 - ?12
                  AND existing.observed_at_ms < ?2 + ?12
            )
            "#,
        )
        .bind(mmsi)
        .bind(observed_at_ms)
        .bind(latitude)
        .bind(longitude)
        .bind(speed_knots)
        .bind(course_degrees)
        .bind(branch)
        .bind(s_meters)
        .bind(offset_meters)
        .bind(posture)
        .bind(session_id)
        .bind(AIS_TRACK_FIX_MIN_SPACING_MS)
        .execute(&mut *self.inner)
        .await?;
        Ok(())
    }

    /// Adds or refreshes one hull in the durable AIS catalog.
    ///
    /// Position reports usually arrive before static identity reports. A bare
    /// update therefore never blanks a field learned earlier; a later static
    /// packet fills it in, while destination follows the latest non-empty
    /// broadcast because it describes the current voyage.
    pub async fn record_ais_vessel_observation(
        &mut self,
        observation: AisVesselObservation<'_>,
    ) -> Result<(), StorageError> {
        let AisVesselObservation {
            mmsi,
            name,
            vessel_class,
            call_sign,
            imo_number,
            destination,
            length_meters,
            beam_meters,
            draught_meters,
            observed_at_ms,
        } = observation;
        sqlx::query(
            r#"
            INSERT INTO ais_vessel_ledger(
                mmsi, name, vessel_class, call_sign, imo_number, destination,
                length_meters, beam_meters, draught_meters,
                first_seen_ms, last_seen_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
            ON CONFLICT(mmsi) DO UPDATE SET
                name = COALESCE(excluded.name, name),
                vessel_class = COALESCE(excluded.vessel_class, vessel_class),
                call_sign = COALESCE(excluded.call_sign, call_sign),
                imo_number = COALESCE(excluded.imo_number, imo_number),
                destination = COALESCE(excluded.destination, destination),
                length_meters = COALESCE(excluded.length_meters, length_meters),
                beam_meters = COALESCE(excluded.beam_meters, beam_meters),
                draught_meters = COALESCE(excluded.draught_meters, draught_meters),
                first_seen_ms = MIN(first_seen_ms, excluded.first_seen_ms),
                last_seen_ms = MAX(last_seen_ms, excluded.last_seen_ms)
            "#,
        )
        .bind(mmsi)
        .bind(name)
        .bind(vessel_class)
        .bind(call_sign)
        .bind(imo_number.map(i64::from))
        .bind(destination)
        .bind(length_meters)
        .bind(beam_meters)
        .bind(draught_meters)
        .bind(observed_at_ms)
        .execute(&mut *self.inner)
        .await?;
        Ok(())
    }

    /// Records one bridge-line crossing and keeps the vessel's ledger row
    /// current. The crossing is inserted un-resolved; whether the span was up
    /// for it is judged later, once FL511's interval around that moment has
    /// settled.
    pub async fn record_ais_crossing(
        &mut self,
        observation: AisCrossingObservation<'_>,
    ) -> Result<(), StorageError> {
        let AisCrossingObservation {
            mmsi,
            vessel_name,
            vessel_class,
            length_meters,
            draught_meters,
            direction,
            crossed_at_ms,
            speed_knots,
            session_id,
        } = observation;
        sqlx::query(
            r#"
            INSERT INTO ais_vessel_ledger(
                mmsi, name, vessel_class, length_meters, draught_meters,
                first_seen_ms, last_seen_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
            ON CONFLICT(mmsi) DO UPDATE SET
                name = COALESCE(excluded.name, name),
                vessel_class = COALESCE(excluded.vessel_class, vessel_class),
                length_meters = COALESCE(excluded.length_meters, length_meters),
                draught_meters = COALESCE(excluded.draught_meters, draught_meters),
                first_seen_ms = MIN(first_seen_ms, excluded.first_seen_ms),
                last_seen_ms = MAX(last_seen_ms, excluded.last_seen_ms)
            "#,
        )
        .bind(mmsi)
        .bind(vessel_name)
        .bind(vessel_class)
        .bind(length_meters)
        .bind(draught_meters)
        .bind(crossed_at_ms)
        .execute(&mut *self.inner)
        .await?;
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO ais_transits(
                mmsi, crossed_at_ms, direction, speed_knots, session_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(mmsi)
        .bind(crossed_at_ms)
        .bind(direction)
        .bind(speed_knots)
        .bind(session_id)
        .execute(&mut *self.inner)
        .await?;
        Ok(())
    }

    /// Extends a continuously confirmed interval, or starts a new one when
    /// FL511 changes, the engine session changes, or observations resume after
    /// a gap. The latter two boundaries are intentionally not smoothed over.
    pub async fn record_bridge_state(
        &mut self,
        observation: BridgeObservation<'_>,
    ) -> Result<(), StorageError> {
        let BridgeObservation {
            source_id,
            bridge_key,
            bridge_name,
            relation,
            state,
            observed_at_ms,
            session_id,
        } = observation;
        let current = sqlx::query_as::<_, BridgeStateInterval>(
            r#"
            SELECT source_id, bridge_key, bridge_name, relation, state,
                   started_at_ms, ended_at_ms, last_confirmed_at_ms,
                   start_reason, session_id
            FROM bridge_state_intervals
            WHERE source_id = ?1 AND bridge_key = ?2 AND ended_at_ms IS NULL
            "#,
        )
        .bind(source_id)
        .bind(bridge_key)
        .fetch_optional(&mut *self.inner)
        .await?;

        let same_identity_and_state = current.as_ref().is_some_and(|interval| {
            interval.state == state
                && interval.bridge_name == bridge_name
                && interval.relation == relation
        });
        let same_session = current
            .as_ref()
            .and_then(|interval| interval.session_id.as_deref())
            == Some(session_id);
        let within_continuity = current.as_ref().is_some_and(|interval| {
            observed_at_ms >= interval.last_confirmed_at_ms
                && observed_at_ms - interval.last_confirmed_at_ms <= BRIDGE_CONTINUITY_MAX_GAP_MS
        });

        if same_identity_and_state && same_session && within_continuity {
            sqlx::query(
                r#"
                UPDATE bridge_state_intervals
                SET last_confirmed_at_ms = MAX(last_confirmed_at_ms, ?3)
                WHERE source_id = ?1 AND bridge_key = ?2 AND ended_at_ms IS NULL
                "#,
            )
            .bind(source_id)
            .bind(bridge_key)
            .bind(observed_at_ms)
            .execute(&mut *self.inner)
            .await?;
            return Ok(());
        }

        let start_reason = match current.as_ref() {
            None => "initial_observation",
            Some(interval) if interval.session_id.as_deref() != Some(session_id) => "session_start",
            Some(interval)
                if observed_at_ms < interval.last_confirmed_at_ms
                    || observed_at_ms - interval.last_confirmed_at_ms
                        > BRIDGE_CONTINUITY_MAX_GAP_MS =>
            {
                "continuity_gap"
            }
            Some(_) => "state_change",
        };

        if let Some(interval) = current.as_ref() {
            let ended_at_ms = if matches!(start_reason, "session_start" | "continuity_gap") {
                interval.last_confirmed_at_ms
            } else {
                observed_at_ms
            };
            sqlx::query(
                r#"
                UPDATE bridge_state_intervals
                SET ended_at_ms = MAX(started_at_ms, ?3)
                WHERE source_id = ?1 AND bridge_key = ?2 AND ended_at_ms IS NULL
                "#,
            )
            .bind(source_id)
            .bind(bridge_key)
            .bind(ended_at_ms)
            .execute(&mut *self.inner)
            .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO bridge_state_intervals(
                source_id, bridge_key, bridge_name, relation, state, started_at_ms,
                last_confirmed_at_ms, start_reason, session_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8)
            "#,
        )
        .bind(source_id)
        .bind(bridge_key)
        .bind(bridge_name)
        .bind(relation)
        .bind(state)
        .bind(observed_at_ms)
        .bind(start_reason)
        .bind(session_id)
        .execute(&mut *self.inner)
        .await?;
        Ok(())
    }

    /// Commits every write performed through this transaction.
    pub async fn commit(self) -> Result<(), StorageError> {
        self.inner.commit().await?;
        Ok(())
    }

    /// Explicitly rolls every write performed through this transaction back.
    pub async fn rollback(self) -> Result<(), StorageError> {
        self.inner.rollback().await?;
        Ok(())
    }
}

impl Store {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::from_str(&format!(
            "sqlite://{}",
            path.as_ref().to_string_lossy()
        ))?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        // Alert policy, recipient routing, and outbox transitions are small
        // writes whose power-loss durability matters more than bulk-write
        // throughput. FULL keeps the "SQLite is the source of truth" promise
        // honest across host crashes and abrupt power loss.
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await?;

        let store = Self { pool };
        store.assert_safe_sqlite().await?;
        store.create_schema().await?;
        Ok(store)
    }

    pub async fn in_memory() -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Memory)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let store = Self { pool };
        store.assert_safe_sqlite().await?;
        store.create_schema().await?;
        Ok(store)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Begins an atomic group of storage writes.
    pub async fn begin_transaction(&self) -> Result<StoreTransaction<'_>, StorageError> {
        Ok(StoreTransaction {
            inner: self.pool.begin().await?,
        })
    }

    pub async fn sqlite_version(&self) -> Result<String, StorageError> {
        Ok(
            sqlx::query_scalar::<Sqlite, String>("SELECT sqlite_version()")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn database_size_bytes(&self) -> Result<u64, StorageError> {
        let page_count = sqlx::query_scalar::<Sqlite, i64>("PRAGMA page_count")
            .fetch_one(&self.pool)
            .await?;
        let page_size = sqlx::query_scalar::<Sqlite, i64>("PRAGMA page_size")
            .fetch_one(&self.pool)
            .await?;
        Ok(u64::try_from(page_count.saturating_mul(page_size)).unwrap_or_default())
    }

    async fn assert_safe_sqlite(&self) -> Result<(), StorageError> {
        let version = self.sqlite_version().await?;
        let parsed = parse_version(&version)?;
        if parsed < MINIMUM_SQLITE {
            return Err(StorageError::UnsafeSqlite {
                found: version,
                required: "3.51.3".to_owned(),
            });
        }
        Ok(())
    }

    async fn create_schema(&self) -> Result<(), StorageError> {
        sqlx::raw_sql(SCHEMA_SQL).execute(&self.pool).await?;
        self.ensure_bridge_learning_columns().await?;
        self.ensure_vessel_catalog_columns().await?;
        self.ensure_forecast_minute_key().await?;
        self.backfill_vessel_catalog_from_tracks().await?;
        Ok(())
    }

    /// Adds bridge observation-continuity fields to databases created by an
    /// earlier release.
    ///
    /// `schema.sql` is applied with CREATE TABLE IF NOT EXISTS, so an existing
    /// table is left untouched by it and needs the column added explicitly.
    /// SQLite has no ADD COLUMN IF NOT EXISTS, hence the pragma check.
    async fn ensure_bridge_learning_columns(&self) -> Result<(), StorageError> {
        if !self
            .table_has_column("bridge_state_intervals", "session_id")
            .await?
        {
            sqlx::query("ALTER TABLE bridge_state_intervals ADD COLUMN session_id TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self
            .table_has_column("bridge_state_intervals", "last_confirmed_at_ms")
            .await?
        {
            // Zero is a crash-safe intermediate default. The unconditional
            // backfill below immediately replaces it, and also repairs a
            // migration interrupted between these two statements.
            sqlx::query(
                "ALTER TABLE bridge_state_intervals ADD COLUMN last_confirmed_at_ms INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        sqlx::query(
            r#"
            UPDATE bridge_state_intervals
            SET last_confirmed_at_ms = started_at_ms
            WHERE last_confirmed_at_ms < started_at_ms
            "#,
        )
        .execute(&self.pool)
        .await?;
        if !self
            .table_has_column("bridge_state_intervals", "start_reason")
            .await?
        {
            // Historical rows contain no durable last-success timestamp, so
            // they are marked legacy and excluded from new coverage-based
            // outcome resolution rather than assigned invented continuity.
            sqlx::query(
                "ALTER TABLE bridge_state_intervals ADD COLUMN start_reason TEXT NOT NULL DEFAULT 'legacy'",
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Extends the durable hull catalog without rebuilding or discarding the
    /// opening history already accumulated in an older database.
    async fn ensure_vessel_catalog_columns(&self) -> Result<(), StorageError> {
        for (name, statement) in [
            (
                "call_sign",
                "ALTER TABLE ais_vessel_ledger ADD COLUMN call_sign TEXT",
            ),
            (
                "imo_number",
                "ALTER TABLE ais_vessel_ledger ADD COLUMN imo_number INTEGER",
            ),
            (
                "destination",
                "ALTER TABLE ais_vessel_ledger ADD COLUMN destination TEXT",
            ),
            (
                "beam_meters",
                "ALTER TABLE ais_vessel_ledger ADD COLUMN beam_meters REAL",
            ),
        ] {
            if !self.table_has_column("ais_vessel_ledger", name).await? {
                sqlx::query(statement).execute(&self.pool).await?;
            }
        }
        Ok(())
    }

    /// Early development builds keyed samples by exact evaluation time. Fold
    /// any same-minute rows down to the latest write before enforcing the
    /// shipped one-row-per-target-minute contract.
    async fn ensure_forecast_minute_key(&self) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            DELETE FROM bridge_forecast_samples
            WHERE rowid NOT IN (
                SELECT MAX(rowid)
                FROM bridge_forecast_samples
                GROUP BY target_key, minute_bucket_ms
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS bridge_forecast_samples_minute
            ON bridge_forecast_samples(target_key, minute_bucket_ms)
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Older builds retained raw fixes without cataloging hulls until they
    /// crossed Brickell. Promote every MMSI already present in that history so
    /// the richer catalog begins with the data the app has, not only vessels
    /// that happen to be online after upgrade.
    async fn backfill_vessel_catalog_from_tracks(&self) -> Result<(), StorageError> {
        const MIGRATION_KEY: &str = "storage.ais_catalog_track_backfill.v1";
        let complete =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM settings WHERE key = ?1)")
                .bind(MIGRATION_KEY)
                .fetch_one(&self.pool)
                .await?;
        if complete {
            return Ok(());
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO ais_vessel_ledger(mmsi, first_seen_ms, last_seen_ms)
            SELECT mmsi, MIN(observed_at_ms), MAX(observed_at_ms)
            FROM ais_track_fixes
            GROUP BY mmsi
            ON CONFLICT(mmsi) DO UPDATE SET
                first_seen_ms = MIN(first_seen_ms, excluded.first_seen_ms),
                last_seen_ms = MAX(last_seen_ms, excluded.last_seen_ms)
            "#,
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO settings(key, value_json, updated_at)
            VALUES (?1, 'true', 'schema-migration')
            "#,
        )
        .bind(MIGRATION_KEY)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn table_has_column(&self, table: &str, column: &str) -> Result<bool, StorageError> {
        let columns = sqlx::query("SELECT name FROM pragma_table_info(?1)")
            .bind(table)
            .fetch_all(&self.pool)
            .await?;
        Ok(columns
            .iter()
            .any(|row| row.get::<String, _>("name") == column))
    }

    async fn database_has_table(pool: &SqlitePool, table: &str) -> Result<bool, StorageError> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        )
        .bind(table)
        .fetch_one(pool)
        .await?)
    }

    /// Calculates the standard one-year AIS fix cutoff while leaving callers
    /// free to supply a different retention policy to [`Store::prune_history`].
    pub fn default_ais_track_cutoff_ms(now_ms: i64) -> i64 {
        now_ms.saturating_sub(DEFAULT_AIS_TRACK_RETENTION_MS)
    }

    /// Calculates the standard two-year forecast-sample cutoff.
    pub fn default_forecast_cutoff_ms(now_ms: i64) -> i64 {
        now_ms.saturating_sub(DEFAULT_FORECAST_RETENTION_MS)
    }

    /// Pulls a requested pruning cutoff back so that at least
    /// [`MIN_HISTORY_RETENTION_MS`] of history survives it.
    pub fn bounded_history_cutoff_ms(now_ms: i64, requested_cutoff_ms: i64) -> i64 {
        requested_cutoff_ms.min(now_ms.saturating_sub(MIN_HISTORY_RETENTION_MS))
    }

    /// Merges the learning tables from the app's pre-rename database.
    ///
    /// The source is opened read-only and bridge intervals are deliberately
    /// not copied: the old rows lack confirmation timestamps and cannot prove
    /// continuous FL511 coverage. Catalog identity, crossing outcomes, raw AIS
    /// fixes and booked movements are useful independent observations and are
    /// merged idempotently. Ledger totals are then recomputed from the merged
    /// crossing rows, avoiding double-counting schema seed vessels.
    pub async fn import_legacy_learning(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<LegacyLearningImportReport, StorageError> {
        let path = path.as_ref();
        if !path.is_file() {
            return Ok(LegacyLearningImportReport::default());
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .foreign_keys(false)
            .busy_timeout(Duration::from_secs(5));
        let legacy = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        let vessels = if Self::database_has_table(&legacy, "ais_vessel_ledger").await? {
            sqlx::query_as::<_, LegacyVesselRow>(
                r#"
                SELECT mmsi, name, vessel_class, length_meters, draught_meters,
                       first_seen_ms, last_seen_ms
                FROM ais_vessel_ledger
                "#,
            )
            .fetch_all(&legacy)
            .await?
        } else {
            Vec::new()
        };
        let transits = if Self::database_has_table(&legacy, "ais_transits").await? {
            sqlx::query_as::<_, LegacyTransitRow>(
                r#"
                SELECT mmsi, crossed_at_ms, direction, speed_knots, outcome,
                       resolved_at_ms, session_id
                FROM ais_transits
                "#,
            )
            .fetch_all(&legacy)
            .await?
        } else {
            Vec::new()
        };
        let track_fixes = if Self::database_has_table(&legacy, "ais_track_fixes").await? {
            sqlx::query_as::<_, LegacyTrackFixRow>(
                r#"
                SELECT mmsi, observed_at_ms, latitude, longitude, speed_knots,
                       course_degrees, branch, s_meters, offset_meters, posture,
                       session_id
                FROM ais_track_fixes
                "#,
            )
            .fetch_all(&legacy)
            .await?
        } else {
            Vec::new()
        };
        let river_transits = if Self::database_has_table(&legacy, "river_transits").await? {
            sqlx::query_as::<_, LegacyRiverTransitRow>(
                r#"
                SELECT source_id, movement_key, vessel, action, river_direction,
                       scheduled_at_ms, estimated_bridge_at_ms,
                       estimated_offset_minutes, first_seen_at_ms,
                       last_seen_at_ms, session_id
                FROM river_transits
                "#,
            )
            .fetch_all(&legacy)
            .await?
        } else {
            Vec::new()
        };
        legacy.close().await;

        let mut report = LegacyLearningImportReport::default();
        let mut transaction = self.pool.begin().await?;
        for vessel in vessels {
            report.vessels_added = report.vessels_added.saturating_add(
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO ais_vessel_ledger(
                        mmsi, name, vessel_class, length_meters, draught_meters,
                        first_seen_ms, last_seen_ms
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    "#,
                )
                .bind(&vessel.mmsi)
                .bind(&vessel.name)
                .bind(&vessel.vessel_class)
                .bind(vessel.length_meters)
                .bind(vessel.draught_meters)
                .bind(vessel.first_seen_ms)
                .bind(vessel.last_seen_ms)
                .execute(&mut *transaction)
                .await?
                .rows_affected(),
            );
            sqlx::query(
                r#"
                UPDATE ais_vessel_ledger
                SET name = COALESCE(name, ?2),
                    vessel_class = COALESCE(vessel_class, ?3),
                    length_meters = COALESCE(length_meters, ?4),
                    draught_meters = COALESCE(draught_meters, ?5),
                    first_seen_ms = MIN(first_seen_ms, ?6),
                    last_seen_ms = MAX(last_seen_ms, ?7)
                WHERE mmsi = ?1
                "#,
            )
            .bind(&vessel.mmsi)
            .bind(&vessel.name)
            .bind(&vessel.vessel_class)
            .bind(vessel.length_meters)
            .bind(vessel.draught_meters)
            .bind(vessel.first_seen_ms)
            .bind(vessel.last_seen_ms)
            .execute(&mut *transaction)
            .await?;
        }

        for transit in transits {
            // A crossing can exist in a very old database without a catalog
            // row. Retain it under its stable MMSI rather than dropping it.
            report.vessels_added = report.vessels_added.saturating_add(
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO ais_vessel_ledger(
                        mmsi, first_seen_ms, last_seen_ms
                    ) VALUES (?1, ?2, ?2)
                    "#,
                )
                .bind(&transit.mmsi)
                .bind(transit.crossed_at_ms)
                .execute(&mut *transaction)
                .await?
                .rows_affected(),
            );
            report.transits_added = report.transits_added.saturating_add(
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO ais_transits(
                        mmsi, crossed_at_ms, direction, speed_knots, outcome,
                        resolved_at_ms, session_id
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    "#,
                )
                .bind(&transit.mmsi)
                .bind(transit.crossed_at_ms)
                .bind(&transit.direction)
                .bind(transit.speed_knots)
                .bind(&transit.outcome)
                .bind(transit.resolved_at_ms)
                .bind(&transit.session_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected(),
            );
            sqlx::query(
                r#"
                UPDATE ais_transits
                SET speed_knots = COALESCE(speed_knots, ?3),
                    outcome = CASE
                        WHEN outcome IS NULL OR outcome = 'unknown'
                        THEN COALESCE(?4, outcome)
                        ELSE outcome
                    END,
                    resolved_at_ms = COALESCE(resolved_at_ms, ?5),
                    session_id = COALESCE(session_id, ?6)
                WHERE mmsi = ?1 AND crossed_at_ms = ?2
                "#,
            )
            .bind(&transit.mmsi)
            .bind(transit.crossed_at_ms)
            .bind(transit.speed_knots)
            .bind(&transit.outcome)
            .bind(transit.resolved_at_ms)
            .bind(&transit.session_id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                r#"
                UPDATE ais_vessel_ledger
                SET first_seen_ms = MIN(first_seen_ms, ?2),
                    last_seen_ms = MAX(last_seen_ms, ?2)
                WHERE mmsi = ?1
                "#,
            )
            .bind(&transit.mmsi)
            .bind(transit.crossed_at_ms)
            .execute(&mut *transaction)
            .await?;
        }

        for fix in track_fixes {
            report.vessels_added = report.vessels_added.saturating_add(
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO ais_vessel_ledger(
                        mmsi, first_seen_ms, last_seen_ms
                    ) VALUES (?1, ?2, ?2)
                    "#,
                )
                .bind(&fix.mmsi)
                .bind(fix.observed_at_ms)
                .execute(&mut *transaction)
                .await?
                .rows_affected(),
            );
            sqlx::query(
                r#"
                UPDATE ais_vessel_ledger
                SET first_seen_ms = MIN(first_seen_ms, ?2),
                    last_seen_ms = MAX(last_seen_ms, ?2)
                WHERE mmsi = ?1
                "#,
            )
            .bind(&fix.mmsi)
            .bind(fix.observed_at_ms)
            .execute(&mut *transaction)
            .await?;
            report.track_fixes_added = report.track_fixes_added.saturating_add(
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO ais_track_fixes(
                        mmsi, observed_at_ms, latitude, longitude, speed_knots,
                        course_degrees, branch, s_meters, offset_meters, posture,
                        session_id
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                    "#,
                )
                .bind(&fix.mmsi)
                .bind(fix.observed_at_ms)
                .bind(fix.latitude)
                .bind(fix.longitude)
                .bind(fix.speed_knots)
                .bind(fix.course_degrees)
                .bind(&fix.branch)
                .bind(fix.s_meters)
                .bind(fix.offset_meters)
                .bind(&fix.posture)
                .bind(&fix.session_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected(),
            );
        }

        for movement in river_transits {
            report.river_transits_added = report.river_transits_added.saturating_add(
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO river_transits(
                        source_id, movement_key, vessel, action, river_direction,
                        scheduled_at_ms, estimated_bridge_at_ms,
                        estimated_offset_minutes, first_seen_at_ms,
                        last_seen_at_ms, session_id
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                    "#,
                )
                .bind(&movement.source_id)
                .bind(&movement.movement_key)
                .bind(&movement.vessel)
                .bind(&movement.action)
                .bind(&movement.river_direction)
                .bind(movement.scheduled_at_ms)
                .bind(movement.estimated_bridge_at_ms)
                .bind(movement.estimated_offset_minutes)
                .bind(movement.first_seen_at_ms)
                .bind(movement.last_seen_at_ms)
                .bind(&movement.session_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected(),
            );
            sqlx::query(
                r#"
                UPDATE river_transits
                SET vessel = CASE WHEN ?10 >= last_seen_at_ms THEN ?3 ELSE vessel END,
                    action = CASE WHEN ?10 >= last_seen_at_ms THEN ?4 ELSE action END,
                    river_direction = CASE
                        WHEN ?10 >= last_seen_at_ms THEN COALESCE(?5, river_direction)
                        ELSE river_direction
                    END,
                    scheduled_at_ms = CASE
                        WHEN ?10 >= last_seen_at_ms THEN ?6 ELSE scheduled_at_ms
                    END,
                    estimated_bridge_at_ms = CASE
                        WHEN ?10 >= last_seen_at_ms
                        THEN COALESCE(?7, estimated_bridge_at_ms)
                        ELSE estimated_bridge_at_ms
                    END,
                    estimated_offset_minutes = CASE
                        WHEN ?10 >= last_seen_at_ms
                        THEN COALESCE(?8, estimated_offset_minutes)
                        ELSE estimated_offset_minutes
                    END,
                    first_seen_at_ms = MIN(first_seen_at_ms, ?9),
                    last_seen_at_ms = MAX(last_seen_at_ms, ?10),
                    session_id = COALESCE(session_id, ?11)
                WHERE source_id = ?1 AND movement_key = ?2
                "#,
            )
            .bind(&movement.source_id)
            .bind(&movement.movement_key)
            .bind(&movement.vessel)
            .bind(&movement.action)
            .bind(&movement.river_direction)
            .bind(movement.scheduled_at_ms)
            .bind(movement.estimated_bridge_at_ms)
            .bind(movement.estimated_offset_minutes)
            .bind(movement.first_seen_at_ms)
            .bind(movement.last_seen_at_ms)
            .bind(&movement.session_id)
            .execute(&mut *transaction)
            .await?;
        }

        sqlx::query(
            r#"
            UPDATE ais_vessel_ledger
            SET transits_opened = (
                    SELECT COUNT(*) FROM ais_transits transit
                    WHERE transit.mmsi = ais_vessel_ledger.mmsi
                      AND transit.outcome = 'opened'
                ),
                transits_fits_under = (
                    SELECT COUNT(*) FROM ais_transits transit
                    WHERE transit.mmsi = ais_vessel_ledger.mmsi
                      AND transit.outcome = 'fits_under'
                )
            "#,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(report)
    }

    pub async fn set_json<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        updated_at: &str,
    ) -> Result<(), StorageError> {
        let mut connection = self.pool.acquire().await?;
        set_json_on(&mut connection, key, value, updated_at).await
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, StorageError> {
        let value =
            sqlx::query_scalar::<Sqlite, String>("SELECT value_json FROM settings WHERE key = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        value
            .map(|json| serde_json::from_str(&json).map_err(StorageError::from))
            .transpose()
    }

    /// Returns recorded bridge intervals in chronological order for training.
    pub async fn list_bridge_state_intervals(
        &self,
        source_id: &str,
        bridge_key: &str,
    ) -> Result<Vec<BridgeStateInterval>, StorageError> {
        Ok(sqlx::query_as::<_, BridgeStateInterval>(
            r#"
            SELECT source_id, bridge_key, bridge_name, relation, state,
                   started_at_ms, ended_at_ms, last_confirmed_at_ms,
                   start_reason, session_id
            FROM bridge_state_intervals
            WHERE source_id = ?1 AND bridge_key = ?2
            ORDER BY started_at_ms
            "#,
        )
        .bind(source_id)
        .bind(bridge_key)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Returns the newest recorded FL511 bridge intervals across every
    /// configured target and upstream bridge.
    pub async fn list_recent_bridge_state_intervals(
        &self,
        limit: u32,
    ) -> Result<Vec<BridgeStateInterval>, StorageError> {
        Ok(sqlx::query_as::<_, BridgeStateInterval>(
            r#"
            SELECT source_id, bridge_key, bridge_name, relation, state,
                   started_at_ms, ended_at_ms, last_confirmed_at_ms,
                   start_reason, session_id
            FROM bridge_state_intervals
            ORDER BY started_at_ms DESC
            LIMIT ?1
            "#,
        )
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Judges pending bridge-line crossings against the recorded target
    /// intervals and folds settled outcomes into the vessel ledger.
    ///
    /// An `up` interval must have begun no more than fifteen minutes before the
    /// crossing and be explicitly confirmed through thirty seconds after it.
    /// That conservative lead window prevents a long-running unrelated opening
    /// from teaching the catalog that every later hull required the bridge. A
    /// `down` interval must be confirmed from a minute before through two
    /// minutes after. An open-ended row is never extended to `now` by
    /// assumption. A crossing that still matches neither after 45 minutes falls
    /// into an FL511 gap and is closed as `unknown`, which never trains the
    /// ledger.
    pub async fn resolve_ais_transits(&self, now_ms: i64) -> Result<u64, StorageError> {
        const SETTLE_MS: i64 = 3 * 60 * 1_000;
        const GIVE_UP_MS: i64 = 45 * 60 * 1_000;
        const OPEN_CONFIRM_AFTER_MS: i64 = 30 * 1_000;
        const OPEN_MAX_LEAD_MS: i64 = 15 * 60 * 1_000;
        let mut transaction = self.pool.begin().await?;
        let mut resolved = 0_u64;

        let opened = sqlx::query_as::<_, (String,)>(
            r#"
            UPDATE ais_transits
            SET outcome = 'opened', resolved_at_ms = ?2
            WHERE outcome IS NULL AND crossed_at_ms <= ?2 - ?1
              AND EXISTS (
                SELECT 1 FROM bridge_state_intervals b
                WHERE b.relation = 'target' AND b.state = 'up'
                  AND b.session_id IS NOT NULL
                  AND b.start_reason != 'legacy'
                  AND b.started_at_ms <= ais_transits.crossed_at_ms
                  AND b.started_at_ms >= ais_transits.crossed_at_ms - ?4
                  AND b.last_confirmed_at_ms >= ais_transits.crossed_at_ms + ?3
                  AND (b.ended_at_ms IS NULL
                       OR b.ended_at_ms >= ais_transits.crossed_at_ms + ?3)
              )
            RETURNING mmsi
            "#,
        )
        .bind(SETTLE_MS)
        .bind(now_ms)
        .bind(OPEN_CONFIRM_AFTER_MS)
        .bind(OPEN_MAX_LEAD_MS)
        .fetch_all(&mut *transaction)
        .await?;
        for (mmsi,) in opened {
            resolved += 1;
            sqlx::query(
                "UPDATE ais_vessel_ledger SET transits_opened = transits_opened + 1 WHERE mmsi = ?1",
            )
            .bind(&mmsi)
            .execute(&mut *transaction)
            .await?;
        }

        let fits_under = sqlx::query_as::<_, (String,)>(
            r#"
            UPDATE ais_transits
            SET outcome = 'fits_under', resolved_at_ms = ?2
            WHERE outcome IS NULL AND crossed_at_ms <= ?2 - ?1
              AND EXISTS (
                SELECT 1 FROM bridge_state_intervals b
                WHERE b.relation = 'target' AND b.state = 'down'
                  AND b.session_id IS NOT NULL
                  AND b.start_reason != 'legacy'
                  AND b.started_at_ms <= ais_transits.crossed_at_ms - 60000
                  AND b.last_confirmed_at_ms >= ais_transits.crossed_at_ms + 120000
                  AND (b.ended_at_ms IS NULL
                       OR b.ended_at_ms >= ais_transits.crossed_at_ms + 120000)
              )
            RETURNING mmsi
            "#,
        )
        .bind(SETTLE_MS)
        .bind(now_ms)
        .fetch_all(&mut *transaction)
        .await?;
        for (mmsi,) in fits_under {
            resolved += 1;
            sqlx::query(
                "UPDATE ais_vessel_ledger SET transits_fits_under = transits_fits_under + 1 WHERE mmsi = ?1",
            )
            .bind(&mmsi)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            r#"
            UPDATE ais_transits
            SET outcome = 'unknown', resolved_at_ms = ?2
            WHERE outcome IS NULL AND crossed_at_ms <= ?2 - ?1
            "#,
        )
        .bind(GIVE_UP_MS)
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(resolved)
    }

    /// The durable vessel catalog. Hulls with a learned crossing outcome come
    /// first so a bounded predictor read cannot evict known openers behind a
    /// busy day of newly seen bay traffic; each group is most-recent first.
    pub async fn list_ais_ledger(&self, limit: u32) -> Result<Vec<AisLedgerEntry>, StorageError> {
        Ok(sqlx::query_as::<_, AisLedgerEntry>(
            r#"
            SELECT
                l.mmsi,
                l.name,
                l.vessel_class,
                l.call_sign,
                l.imo_number,
                l.destination,
                l.length_meters,
                l.beam_meters,
                l.draught_meters,
                l.transits_opened,
                l.transits_fits_under,
                COALESCE(t.transits_unknown, 0) AS transits_unknown,
                COALESCE(t.transits_pending, 0) AS transits_pending,
                l.first_seen_ms,
                l.last_seen_ms,
                t.last_crossing_at_ms,
                t.last_opened_at_ms
            FROM ais_vessel_ledger l
            LEFT JOIN (
                SELECT
                    mmsi,
                    SUM(CASE WHEN outcome = 'unknown' THEN 1 ELSE 0 END) AS transits_unknown,
                    SUM(CASE WHEN outcome IS NULL THEN 1 ELSE 0 END) AS transits_pending,
                    MAX(crossed_at_ms) AS last_crossing_at_ms,
                    MAX(CASE WHEN outcome = 'opened' THEN crossed_at_ms END) AS last_opened_at_ms
                FROM ais_transits
                GROUP BY mmsi
            ) t ON t.mmsi = l.mmsi
            ORDER BY (l.transits_opened + l.transits_fits_under > 0) DESC,
                     l.last_seen_ms DESC
            LIMIT ?1
            "#,
        )
        .bind(i64::from(limit.clamp(1, 2_000)))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Every vessel with at least one confirmed bridge-up passage.
    ///
    /// This is the durable opener catalog, not a recent-position query, so it
    /// deliberately has no time cutoff. The set is naturally small and must
    /// not lose an older opener merely because the bay was busy this hour.
    pub async fn list_known_ais_openers(&self) -> Result<Vec<AisLedgerEntry>, StorageError> {
        Ok(sqlx::query_as::<_, AisLedgerEntry>(
            r#"
            SELECT
                l.mmsi,
                l.name,
                l.vessel_class,
                l.call_sign,
                l.imo_number,
                l.destination,
                l.length_meters,
                l.beam_meters,
                l.draught_meters,
                l.transits_opened,
                l.transits_fits_under,
                COALESCE(t.transits_unknown, 0) AS transits_unknown,
                COALESCE(t.transits_pending, 0) AS transits_pending,
                l.first_seen_ms,
                l.last_seen_ms,
                t.last_crossing_at_ms,
                t.last_opened_at_ms
            FROM ais_vessel_ledger l
            LEFT JOIN (
                SELECT
                    mmsi,
                    SUM(CASE WHEN outcome = 'unknown' THEN 1 ELSE 0 END) AS transits_unknown,
                    SUM(CASE WHEN outcome IS NULL THEN 1 ELSE 0 END) AS transits_pending,
                    MAX(crossed_at_ms) AS last_crossing_at_ms,
                    MAX(CASE WHEN outcome = 'opened' THEN crossed_at_ms END) AS last_opened_at_ms
                FROM ais_transits
                GROUP BY mmsi
            ) t ON t.mmsi = l.mmsi
            WHERE l.transits_opened > 0
            ORDER BY COALESCE(t.last_opened_at_ms, l.last_seen_ms) DESC,
                     l.mmsi ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// One hull's durable identity and Brickell crossing history.
    ///
    /// This is deliberately separate from the broad one-hour map snapshot:
    /// selecting a vessel reads the already-recorded ledger and does not alter
    /// AIS collection, retention, or which live tracks are published.
    pub async fn get_ais_vessel_history(
        &self,
        mmsi: &str,
        recent_limit: u32,
    ) -> Result<Option<AisVesselHistory>, StorageError> {
        let ledger = sqlx::query_as::<_, AisLedgerEntry>(
            r#"
            SELECT
                l.mmsi,
                l.name,
                l.vessel_class,
                l.call_sign,
                l.imo_number,
                l.destination,
                l.length_meters,
                l.beam_meters,
                l.draught_meters,
                l.transits_opened,
                l.transits_fits_under,
                COALESCE(t.transits_unknown, 0) AS transits_unknown,
                COALESCE(t.transits_pending, 0) AS transits_pending,
                l.first_seen_ms,
                l.last_seen_ms,
                t.last_crossing_at_ms,
                t.last_opened_at_ms
            FROM ais_vessel_ledger l
            LEFT JOIN (
                SELECT
                    mmsi,
                    SUM(CASE WHEN outcome = 'unknown' THEN 1 ELSE 0 END) AS transits_unknown,
                    SUM(CASE WHEN outcome IS NULL THEN 1 ELSE 0 END) AS transits_pending,
                    MAX(crossed_at_ms) AS last_crossing_at_ms,
                    MAX(CASE WHEN outcome = 'opened' THEN crossed_at_ms END) AS last_opened_at_ms
                FROM ais_transits
                WHERE mmsi = ?1
                GROUP BY mmsi
            ) t ON t.mmsi = l.mmsi
            WHERE l.mmsi = ?1
            "#,
        )
        .bind(mmsi)
        .fetch_optional(&self.pool)
        .await?;
        let Some(ledger) = ledger else {
            return Ok(None);
        };

        let recent_crossings = sqlx::query_as::<_, AisCrossingRecord>(
            r#"
            SELECT
                t.mmsi          AS mmsi,
                l.name          AS name,
                l.vessel_class  AS vessel_class,
                t.direction     AS direction,
                t.crossed_at_ms AS crossed_at_ms,
                t.speed_knots   AS speed_knots,
                t.outcome       AS outcome,
                t.resolved_at_ms AS resolved_at_ms
            FROM ais_transits t
            LEFT JOIN ais_vessel_ledger l ON l.mmsi = t.mmsi
            WHERE t.mmsi = ?1
            ORDER BY t.crossed_at_ms DESC
            LIMIT ?2
            "#,
        )
        .bind(mmsi)
        .bind(i64::from(recent_limit.clamp(1, 100)))
        .fetch_all(&self.pool)
        .await?;

        Ok(Some(AisVesselHistory {
            ledger,
            recent_crossings,
        }))
    }

    /// Recent bridge-line crossings, newest first, with vessel identity.
    ///
    /// Left-joined against the ledger so a crossing by a hull that has never
    /// broadcast a static report still returns, with a null name, rather than
    /// vanishing from the record of what went through the bridge.
    pub async fn list_recent_ais_crossings(
        &self,
        limit: u32,
    ) -> Result<Vec<AisCrossingRecord>, StorageError> {
        Ok(sqlx::query_as::<_, AisCrossingRecord>(
            r#"
            SELECT
                t.mmsi          AS mmsi,
                l.name          AS name,
                l.vessel_class  AS vessel_class,
                t.direction     AS direction,
                t.crossed_at_ms AS crossed_at_ms,
                t.speed_knots   AS speed_knots,
                t.outcome       AS outcome,
                t.resolved_at_ms AS resolved_at_ms
            FROM ais_transits t
            LEFT JOIN ais_vessel_ledger l ON l.mmsi = t.mmsi
            ORDER BY t.crossed_at_ms DESC
            LIMIT ?1
            "#,
        )
        .bind(i64::from(limit.clamp(1, 500)))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Persists one compact, versioned forecast evaluation.
    ///
    /// Repeating the exact evaluation instant is idempotent. The runtime owns
    /// the cadence (normally one periodic sample per minute plus a material
    /// state change), while the stored minute bucket makes later grouping
    /// cheap and unambiguous.
    pub async fn record_forecast_sample(
        &self,
        sample: ForecastSample<'_>,
    ) -> Result<(), StorageError> {
        let ForecastSample {
            target_key,
            evaluated_at_ms,
            model_version,
            state,
            predictive_score_bps,
            confidence_bps,
            eta_min_minutes,
            eta_max_minutes,
            schedule_mode,
            contribution_bps_json,
            source_freshness_json,
            session_id,
        } = sample;
        let minute_bucket_ms = evaluated_at_ms - evaluated_at_ms.rem_euclid(60_000);
        sqlx::query(
            r#"
            INSERT INTO bridge_forecast_samples(
                target_key, evaluated_at_ms, minute_bucket_ms, model_version,
                state, predictive_score_bps, confidence_bps, eta_min_minutes,
                eta_max_minutes, schedule_mode, contribution_bps_json,
                source_freshness_json, session_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(target_key, minute_bucket_ms) DO UPDATE SET
                evaluated_at_ms = excluded.evaluated_at_ms,
                model_version = excluded.model_version,
                state = excluded.state,
                predictive_score_bps = excluded.predictive_score_bps,
                confidence_bps = excluded.confidence_bps,
                eta_min_minutes = excluded.eta_min_minutes,
                eta_max_minutes = excluded.eta_max_minutes,
                schedule_mode = excluded.schedule_mode,
                contribution_bps_json = excluded.contribution_bps_json,
                source_freshness_json = excluded.source_freshness_json,
                session_id = excluded.session_id
            "#,
        )
        .bind(target_key)
        .bind(evaluated_at_ms)
        .bind(minute_bucket_ms)
        .bind(model_version)
        .bind(state)
        .bind(predictive_score_bps)
        .bind(confidence_bps)
        .bind(eta_min_minutes)
        .bind(eta_max_minutes)
        .bind(schedule_mode)
        .bind(contribution_bps_json)
        .bind(source_freshness_json)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Forecast evaluations since a cutoff, oldest first, for calibration.
    pub async fn forecast_samples_since(
        &self,
        target_key: &str,
        since_ms: i64,
        limit: u32,
    ) -> Result<Vec<ForecastSampleRecord>, StorageError> {
        Ok(sqlx::query_as::<_, ForecastSampleRecord>(
            r#"
            SELECT target_key, evaluated_at_ms, minute_bucket_ms, model_version,
                   state, predictive_score_bps, confidence_bps,
                   eta_min_minutes, eta_max_minutes, schedule_mode,
                   contribution_bps_json, source_freshness_json, session_id
            FROM bridge_forecast_samples
            WHERE target_key = ?1 AND evaluated_at_ms >= ?2
            ORDER BY evaluated_at_ms
            LIMIT ?3
            "#,
        )
        .bind(target_key)
        .bind(since_ms)
        .bind(i64::from(limit.clamp(1, 100_000)))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Bounds forecast history independently from delivery and raw AIS data.
    pub async fn prune_forecast_samples(&self, before_ms: i64) -> Result<u64, StorageError> {
        Ok(
            sqlx::query("DELETE FROM bridge_forecast_samples WHERE evaluated_at_ms < ?1")
                .bind(before_ms)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }

    /// Persists an incident and its immutable material revision atomically.
    pub async fn upsert_incident<T: Serialize>(
        &self,
        incident: &IncidentRecord<'_, T>,
    ) -> Result<(), StorageError> {
        let payload = serde_json::to_string(incident.payload)?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO incidents(
                id, channel_id, state, urgency, material_revision, fingerprint,
                payload_json, opened_at, updated_at, resolved_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                state = excluded.state,
                urgency = excluded.urgency,
                material_revision = excluded.material_revision,
                fingerprint = excluded.fingerprint,
                payload_json = excluded.payload_json,
                updated_at = excluded.updated_at,
                resolved_at = excluded.resolved_at
            WHERE excluded.material_revision >= incidents.material_revision
            "#,
        )
        .bind(incident.id.to_string())
        .bind(incident.channel_id)
        .bind(incident.state)
        .bind(incident.urgency)
        .bind(incident.material_revision)
        .bind(incident.fingerprint)
        .bind(&payload)
        .bind(incident.opened_at)
        .bind(incident.updated_at)
        .bind(incident.resolved_at)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO incident_history(
                incident_id, material_revision, state, urgency, fingerprint,
                payload_json, recorded_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(incident_id, material_revision) DO NOTHING
            "#,
        )
        .bind(incident.id.to_string())
        .bind(incident.material_revision)
        .bind(incident.state)
        .bind(incident.urgency)
        .bind(incident.fingerprint)
        .bind(payload)
        .bind(incident.updated_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Adds a retry-safe delivery. A duplicate material action is suppressed by
    /// the database uniqueness constraint and returns `false`.
    pub async fn enqueue<T: Serialize>(
        &self,
        entry: &OutboxRecord<'_, T>,
    ) -> Result<bool, StorageError> {
        let request = serde_json::to_string(entry.request)?;
        let result = sqlx::query(
            r#"
            INSERT INTO delivery_outbox(
                id, route_id, incident_id, material_revision, action,
                request_json, next_attempt_at, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            ON CONFLICT(route_id, incident_id, material_revision, action) DO NOTHING
            "#,
        )
        .bind(entry.id.to_string())
        .bind(entry.route_id)
        .bind(entry.incident_id.to_string())
        .bind(entry.material_revision)
        .bind(entry.action)
        .bind(request)
        .bind(entry.next_attempt_at)
        .bind(entry.created_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Commits a material incident revision, its retry-safe outbound request,
    /// and the dispatch cursor in one transaction.
    ///
    /// Keeping these three writes together prevents a restart between the
    /// outbox insert and cursor update from inventing a second incident for the
    /// same transition.
    pub async fn commit_delivery_transition<I: Serialize, O: Serialize, S: Serialize>(
        &self,
        incident: &IncidentRecord<'_, I>,
        entry: &OutboxRecord<'_, O>,
        setting_key: &str,
        setting_value: &S,
        updated_at: &str,
    ) -> Result<bool, StorageError> {
        let incident_payload = serde_json::to_string(incident.payload)?;
        let request = serde_json::to_string(entry.request)?;
        let setting_json = serde_json::to_string(setting_value)?;
        let incident_id = incident.id.to_string();
        let outbox_id = entry.id.to_string();
        let mut transaction = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO incidents(
                id, channel_id, state, urgency, material_revision, fingerprint,
                payload_json, opened_at, updated_at, resolved_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                state = excluded.state,
                urgency = excluded.urgency,
                material_revision = excluded.material_revision,
                fingerprint = excluded.fingerprint,
                payload_json = excluded.payload_json,
                updated_at = excluded.updated_at,
                resolved_at = excluded.resolved_at
            WHERE excluded.material_revision >= incidents.material_revision
            "#,
        )
        .bind(&incident_id)
        .bind(incident.channel_id)
        .bind(incident.state)
        .bind(incident.urgency)
        .bind(incident.material_revision)
        .bind(incident.fingerprint)
        .bind(&incident_payload)
        .bind(incident.opened_at)
        .bind(incident.updated_at)
        .bind(incident.resolved_at)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO incident_history(
                incident_id, material_revision, state, urgency, fingerprint,
                payload_json, recorded_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(incident_id, material_revision) DO NOTHING
            "#,
        )
        .bind(&incident_id)
        .bind(incident.material_revision)
        .bind(incident.state)
        .bind(incident.urgency)
        .bind(incident.fingerprint)
        .bind(&incident_payload)
        .bind(incident.updated_at)
        .execute(&mut *transaction)
        .await?;

        let outbox = sqlx::query(
            r#"
            INSERT INTO delivery_outbox(
                id, route_id, incident_id, material_revision, action,
                request_json, next_attempt_at, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            ON CONFLICT(route_id, incident_id, material_revision, action) DO NOTHING
            "#,
        )
        .bind(outbox_id)
        .bind(entry.route_id)
        .bind(&incident_id)
        .bind(entry.material_revision)
        .bind(entry.action)
        .bind(request)
        .bind(entry.next_attempt_at)
        .bind(entry.created_at)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO settings(key, value_json, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(setting_key)
        .bind(setting_json)
        .bind(updated_at)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(outbox.rows_affected() == 1)
    }

    /// Atomically leases the next due outbox record to one worker.
    pub async fn lease_next(
        &self,
        now: &str,
        lease_until: &str,
    ) -> Result<Option<OutboxLease>, StorageError> {
        let leased = sqlx::query(
            r#"
            UPDATE delivery_outbox
            SET status = 'leased', lease_until = ?2, attempts = attempts + 1,
                updated_at = ?1
            WHERE id = (
                SELECT id
                FROM delivery_outbox
                WHERE next_attempt_at <= ?1
                  AND (
                        status IN ('pending', 'failed')
                        OR (status = 'leased' AND lease_until IS NOT NULL AND lease_until < ?1)
                      )
                ORDER BY next_attempt_at, created_at
                LIMIT 1
            )
            RETURNING id, route_id, incident_id, material_revision, action,
                   (
                     SELECT urgency FROM incident_history
                     WHERE incident_history.incident_id = incident_id
                       AND incident_history.material_revision = material_revision
                     LIMIT 1
                   ) AS urgency,
                   request_json, attempts - 1 AS attempts
            "#,
        )
        .bind(now)
        .bind(lease_until)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| OutboxLease {
            id: row.get("id"),
            route_id: row.get("route_id"),
            incident_id: row.get("incident_id"),
            material_revision: row.get("material_revision"),
            action: row.get("action"),
            urgency: row.get("urgency"),
            request_json: row.get("request_json"),
            attempts: row.get("attempts"),
        });
        Ok(leased)
    }

    pub async fn mark_outbox(
        &self,
        id: &str,
        status: &str,
        updated_at: &str,
        provider_message_id: Option<&str>,
        error: Option<&str>,
        next_attempt_at: Option<&str>,
    ) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        mark_outbox_on(
            &mut transaction,
            id,
            status,
            updated_at,
            provider_message_id,
            error,
            next_attempt_at,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Marks an outbox row and upserts related JSON state in one transaction.
    ///
    /// The outbox update has exactly the same terminal address-scrubbing
    /// behavior as [`Store::mark_outbox`].
    #[allow(clippy::too_many_arguments)]
    pub async fn mark_outbox_and_set_json<T: Serialize>(
        &self,
        id: &str,
        status: &str,
        updated_at: &str,
        provider_message_id: Option<&str>,
        error: Option<&str>,
        next_attempt_at: Option<&str>,
        setting_key: &str,
        setting_value: &T,
    ) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        mark_outbox_on(
            &mut transaction,
            id,
            status,
            updated_at,
            provider_message_id,
            error,
            next_attempt_at,
        )
        .await?;
        set_json_on(&mut transaction, setting_key, setting_value, updated_at).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Cancels every unsent row for a route and scrubs recipient addresses
    /// from all of that route's persisted envelopes and legacy incident
    /// payloads. Used when consent, recipient, credentials, or route identity
    /// changes.
    pub async fn suppress_route_and_scrub(
        &self,
        route_id: &str,
        updated_at: &str,
        reason: &str,
    ) -> Result<u64, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let suppressed =
            suppress_route_and_scrub_on(&mut transaction, route_id, updated_at, reason).await?;
        transaction.commit().await?;
        Ok(suppressed)
    }

    /// Cancels/scrubs a route and replaces its JSON tracker atomically.
    ///
    /// This is used when a credential or route identity changes: either both
    /// old recipient work and its material-edge tracker disappear, or neither
    /// change is committed.
    pub async fn suppress_route_and_set_json<T: Serialize>(
        &self,
        route_id: &str,
        updated_at: &str,
        reason: &str,
        setting_key: &str,
        setting_value: &T,
    ) -> Result<u64, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let suppressed =
            suppress_route_and_scrub_on(&mut transaction, route_id, updated_at, reason).await?;
        set_json_on(&mut transaction, setting_key, setting_value, updated_at).await?;
        transaction.commit().await?;
        Ok(suppressed)
    }

    /// Returns newest-first durable delivery outcomes without exposing secrets.
    pub async fn list_outbox_history(
        &self,
        limit: usize,
    ) -> Result<Vec<OutboxHistoryRow>, StorageError> {
        let bounded_limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
        sqlx::query_as::<_, OutboxHistoryRow>(
            r#"
            SELECT delivery_outbox.id, route_id, delivery_outbox.incident_id,
                   delivery_outbox.material_revision, action,
                   (
                     SELECT urgency FROM incident_history
                     WHERE incident_history.incident_id = delivery_outbox.incident_id
                       AND incident_history.material_revision = delivery_outbox.material_revision
                     LIMIT 1
                   ) AS urgency,
                   request_json, status, attempts, provider_message_id,
                   last_error, created_at, delivery_outbox.updated_at
            FROM delivery_outbox
            -- rowid breaks ties on insertion order. Two rows enqueued inside the
            -- same timestamp tick otherwise come back in whatever order SQLite
            -- chooses, which makes "the newest row" a coin flip and any caller
            -- reading history[0] intermittently wrong.
            ORDER BY delivery_outbox.updated_at DESC, created_at DESC,
                     delivery_outbox.rowid DESC
            LIMIT ?1
            "#,
        )
        .bind(bounded_limit)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::from)
    }

    /// Recovers acknowledgement state after a provider accepted a message but
    /// the subsequent tracker-setting write was interrupted. This query never
    /// treats pending, failed, leased, or suppressed work as announced.
    pub async fn outbox_revision_was_accepted(
        &self,
        route_id: &str,
        incident_id: Uuid,
        material_revision: i64,
    ) -> Result<bool, StorageError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM delivery_outbox
                WHERE route_id = ?1
                  AND incident_id = ?2
                  AND material_revision = ?3
                  AND status IN ('accepted', 'delivered')
            )
            "#,
        )
        .bind(route_id)
        .bind(incident_id.to_string())
        .bind(material_revision)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::from)
    }

    /// Scrubs terminal delivery addresses and bounds historical storage.
    /// Active incidents and retryable outbox rows are never removed.
    pub async fn prune_history(
        &self,
        delivery_cutoff: &str,
        track_cutoff_ms: i64,
    ) -> Result<PruneReport, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let mut scrubbed_destinations = sqlx::query(
            r#"
            UPDATE delivery_outbox
            SET request_json = json_set(request_json, '$.destination.address', '[redacted]')
            WHERE (
                    status IN ('accepted', 'delivered', 'suppressed')
                    OR (status = 'failed' AND next_attempt_at >= '9999-01-01')
                  )
              AND COALESCE(json_extract(request_json, '$.destination.address'), '') != '[redacted]'
            "#,
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        scrubbed_destinations = scrubbed_destinations.saturating_add(
            sqlx::query(
                r#"
                UPDATE incidents
                SET payload_json = json_set(
                    payload_json,
                    '$.destination.address',
                    '[redacted]'
                )
                WHERE id IN (
                    SELECT incident_id FROM delivery_outbox
                    WHERE status IN ('accepted', 'delivered', 'suppressed')
                       OR (status = 'failed' AND next_attempt_at >= '9999-01-01')
                )
                AND COALESCE(json_extract(payload_json, '$.destination.address'), '') != '[redacted]'
                "#,
            )
            .execute(&mut *transaction)
            .await?
            .rows_affected(),
        );
        scrubbed_destinations = scrubbed_destinations.saturating_add(
            sqlx::query(
                r#"
                UPDATE incident_history
                SET payload_json = json_set(
                    payload_json,
                    '$.destination.address',
                    '[redacted]'
                )
                WHERE incident_id IN (
                    SELECT incident_id FROM delivery_outbox
                    WHERE status IN ('accepted', 'delivered', 'suppressed')
                       OR (status = 'failed' AND next_attempt_at >= '9999-01-01')
                )
                AND COALESCE(json_extract(payload_json, '$.destination.address'), '') != '[redacted]'
                "#,
            )
            .execute(&mut *transaction)
            .await?
            .rows_affected(),
        );
        let outbox_rows = sqlx::query(
            r#"
            DELETE FROM delivery_outbox
            WHERE updated_at < ?1
              AND (
                    status IN ('accepted', 'delivered', 'suppressed')
                    OR (status = 'failed' AND next_attempt_at >= '9999-01-01')
                  )
            "#,
        )
        .bind(delivery_cutoff)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let incidents = sqlx::query(
            r#"
            DELETE FROM incidents
            WHERE state = 'resolved'
              AND updated_at < ?1
              AND NOT EXISTS (
                    SELECT 1 FROM delivery_outbox
                    WHERE delivery_outbox.incident_id = incidents.id
                  )
            "#,
        )
        .bind(delivery_cutoff)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let track_fixes = sqlx::query(
            r#"
            DELETE FROM ais_track_fixes
            WHERE observed_at_ms < ?1
              AND NOT EXISTS (
                    SELECT 1 FROM ais_vessel_ledger vessel
                    WHERE vessel.mmsi = ais_track_fixes.mmsi
                      AND vessel.transits_opened > 0
                  )
            "#,
        )
        .bind(track_cutoff_ms)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;
        Ok(PruneReport {
            scrubbed_destinations,
            outbox_rows,
            incidents,
            track_fixes,
        })
    }

    /// Observed fixes since a cutoff, oldest first, for calibrating the
    /// charted centreline and training per-vessel behaviour against water
    /// that was actually run.
    pub async fn track_fixes_since(
        &self,
        since_ms: i64,
    ) -> Result<Vec<ObservedTrackFix>, StorageError> {
        let rows = sqlx::query_as::<_, ObservedTrackFix>(
            r#"
            SELECT mmsi, observed_at_ms, latitude, longitude, speed_knots,
                   course_degrees, branch, s_meters, offset_meters, posture
            FROM ais_track_fixes
            WHERE observed_at_ms >= ?1
            ORDER BY observed_at_ms
            "#,
        )
        .bind(since_ms)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

async fn mark_outbox_on(
    connection: &mut SqliteConnection,
    id: &str,
    status: &str,
    updated_at: &str,
    provider_message_id: Option<&str>,
    error: Option<&str>,
    next_attempt_at: Option<&str>,
) -> Result<(), StorageError> {
    let terminal = matches!(status, "accepted" | "delivered" | "suppressed")
        || status == "failed" && next_attempt_at.is_some_and(|next| next >= "9999-01-01");
    sqlx::query(
        r#"
        UPDATE delivery_outbox
        SET status = ?2,
            updated_at = ?3,
            provider_message_id = ?4,
            last_error = ?5,
            next_attempt_at = COALESCE(?6, next_attempt_at),
            lease_until = NULL,
            request_json = CASE
                WHEN ?2 IN ('accepted', 'delivered', 'suppressed')
                  OR (?2 = 'failed' AND COALESCE(?6, '') >= '9999-01-01')
                THEN json_set(request_json, '$.destination.address', '[redacted]')
                ELSE request_json
            END
        WHERE id = ?1
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(updated_at)
    .bind(provider_message_id)
    .bind(error)
    .bind(next_attempt_at)
    .execute(&mut *connection)
    .await?;
    if terminal {
        sqlx::query(
            r#"
            UPDATE incidents
            SET payload_json = json_set(
                payload_json,
                '$.destination.address',
                '[redacted]'
            )
            WHERE id IN (
                SELECT incident_id FROM delivery_outbox WHERE id = ?1
            )
            "#,
        )
        .bind(id)
        .execute(&mut *connection)
        .await?;
        sqlx::query(
            r#"
            UPDATE incident_history
            SET payload_json = json_set(
                payload_json,
                '$.destination.address',
                '[redacted]'
            )
            WHERE incident_id IN (
                SELECT incident_id FROM delivery_outbox WHERE id = ?1
            )
            "#,
        )
        .bind(id)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn suppress_route_and_scrub_on(
    connection: &mut SqliteConnection,
    route_id: &str,
    updated_at: &str,
    reason: &str,
) -> Result<u64, StorageError> {
    let suppressed = sqlx::query(
        r#"
        UPDATE delivery_outbox
        SET status = 'suppressed', updated_at = ?2, last_error = ?3,
            lease_until = NULL
        WHERE route_id = ?1
          AND status IN ('pending', 'leased', 'failed')
        "#,
    )
    .bind(route_id)
    .bind(updated_at)
    .bind(reason)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    sqlx::query(
        r#"
        UPDATE delivery_outbox
        SET request_json = json_set(request_json, '$.destination.address', '[redacted]')
        WHERE route_id = ?1
        "#,
    )
    .bind(route_id)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        r#"
        UPDATE incidents
        SET payload_json = json_set(payload_json, '$.destination.address', '[redacted]')
        WHERE id IN (
            SELECT incident_id FROM delivery_outbox WHERE route_id = ?1
        )
        "#,
    )
    .bind(route_id)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        r#"
        UPDATE incident_history
        SET payload_json = json_set(payload_json, '$.destination.address', '[redacted]')
        WHERE incident_id IN (
            SELECT incident_id FROM delivery_outbox WHERE route_id = ?1
        )
        "#,
    )
    .bind(route_id)
    .execute(&mut *connection)
    .await?;
    Ok(suppressed)
}

async fn set_json_on<T: Serialize>(
    connection: &mut SqliteConnection,
    key: &str,
    value: &T,
    updated_at: &str,
) -> Result<(), StorageError> {
    let value = serde_json::to_string(value)?;
    sqlx::query(
        r#"
        INSERT INTO settings(key, value_json, updated_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(key)
    .bind(value)
    .bind(updated_at)
    .execute(connection)
    .await?;
    Ok(())
}

fn parse_version(value: &str) -> Result<(u64, u64, u64), StorageError> {
    let mut parts = value.split('.');
    let major = parts
        .next()
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| StorageError::InvalidSqliteVersion(value.to_owned()))?;
    let minor = parts
        .next()
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| StorageError::InvalidSqliteVersion(value.to_owned()))?;
    let patch = parts
        .next()
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| StorageError::InvalidSqliteVersion(value.to_owned()))?;
    Ok((major, minor, patch))
}

#[cfg(test)]
mod tests {
    #[test]
    fn no_pruning_cutoff_can_undercut_four_weeks_of_history() {
        let now_ms = 1_787_000_000_000_i64;
        let day_ms = 24 * 60 * 60 * 1_000;
        // A caller asking to keep only a week is held to four.
        assert_eq!(
            super::Store::bounded_history_cutoff_ms(now_ms, now_ms - 7 * day_ms),
            now_ms - 28 * day_ms
        );
        // A caller keeping a year is left alone.
        assert_eq!(
            super::Store::bounded_history_cutoff_ms(now_ms, now_ms - 365 * day_ms),
            now_ms - 365 * day_ms
        );
        assert_eq!(super::MIN_HISTORY_RETENTION_MS, 28 * day_ms);
    }

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn ais_crossings_resolve_against_bridge_intervals_and_train_the_ledger() {
        let store = Store::in_memory().await.unwrap();
        let base = 1_800_000_000_000_i64;

        // The target span: confirmed down, confirmed up, then down again.
        let mut transaction = store.begin_transaction().await.unwrap();
        for (state, at) in [
            ("down", base),
            ("down", base + 120_000),
            ("down", base + 180_000),
            ("up", base + 240_000),
            ("up", base + 300_000),
            ("down", base + 360_000),
        ] {
            transaction
                .record_bridge_state(BridgeObservation {
                    source_id: "fl511.bridge",
                    bridge_key: "brickell",
                    bridge_name: "Brickell Avenue Bridge",
                    relation: "target",
                    state,
                    observed_at_ms: at,
                    session_id: "test",
                })
                .await
                .unwrap();
        }
        // One crossing during the up interval, one squarely inside a down
        // interval, one too fresh to settle, and one from before the record
        // began — a genuine gap.
        for (mmsi, crossed_at) in [
            ("111000111", base + 270_000),
            ("222000222", base + 60_000),
            ("333000333", base + 420_000),
            ("444000444", base - 300_000),
        ] {
            transaction
                .record_ais_crossing(AisCrossingObservation {
                    mmsi,
                    vessel_name: Some("TEST VESSEL"),
                    vessel_class: Some("tug"),
                    length_meters: Some(30.0),
                    draught_meters: None,
                    direction: "downriver",
                    crossed_at_ms: crossed_at,
                    speed_knots: 4.5,
                    session_id: "test",
                })
                .await
                .unwrap();
        }
        transaction.commit().await.unwrap();

        // Resolve shortly after the second crossing settles; the third is too
        // recent. Only explicit confirmations, never the open-ended row by
        // itself, establish coverage around the first two.
        let resolved = store.resolve_ais_transits(base + 540_000).await.unwrap();
        assert_eq!(resolved, 2);
        let ledger = store.list_ais_ledger(100).await.unwrap();
        let entry = |mmsi: &str| {
            ledger
                .iter()
                .find(|entry| entry.mmsi == mmsi)
                .expect("ledger row")
                .clone()
        };
        assert_eq!(entry("111000111").transits_opened, 1);
        assert_eq!(entry("111000111").transits_fits_under, 0);
        assert_eq!(entry("222000222").transits_fits_under, 1);
        assert_eq!(entry("333000333").transits_opened, 0);
        assert_eq!(entry("333000333").transits_fits_under, 0);

        // The discovery seed rows ride in with the schema.
        assert_eq!(entry("367705810").transits_opened, 1);
        assert_eq!(entry("338215012").transits_fits_under, 1);

        // Confirm the final down state through two minutes after the fresh
        // crossing. It may then settle later from that recorded coverage; the
        // pre-record crossing still matches no interval and trains nothing.
        let mut confirmation = store.begin_transaction().await.unwrap();
        for at in [base + 480_000, base + 540_000] {
            confirmation
                .record_bridge_state(BridgeObservation {
                    source_id: "fl511.bridge",
                    bridge_key: "brickell",
                    bridge_name: "Brickell Avenue Bridge",
                    relation: "target",
                    state: "down",
                    observed_at_ms: at,
                    session_id: "test",
                })
                .await
                .unwrap();
        }
        confirmation.commit().await.unwrap();
        store
            .resolve_ais_transits(base + 420_000 + 46 * 60_000)
            .await
            .unwrap();
        let ledger = store.list_ais_ledger(100).await.unwrap();
        let entry = |mmsi: &str| ledger.iter().find(|entry| entry.mmsi == mmsi).unwrap();
        assert_eq!(entry("333000333").transits_fits_under, 1);
        assert_eq!(
            entry("444000444").transits_opened + entry("444000444").transits_fits_under,
            0
        );
        let unresolved: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM ais_transits WHERE outcome IS NULL")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(unresolved.0, 0);
    }

    #[tokio::test]
    async fn long_running_up_interval_does_not_claim_an_unrelated_crossing() {
        let store = Store::in_memory().await.unwrap();
        let base = 1_800_100_000_000_i64;
        let crossing_at = base + 20 * 60_000;
        let mut transaction = store.begin_transaction().await.unwrap();

        for minute in (0..=20).step_by(2).chain(std::iter::once(21)) {
            transaction
                .record_bridge_state(BridgeObservation {
                    source_id: "fl511.bridge",
                    bridge_key: "brickell",
                    bridge_name: "Brickell Avenue Bridge",
                    relation: "target",
                    state: "up",
                    observed_at_ms: base + minute * 60_000,
                    session_id: "test",
                })
                .await
                .unwrap();
        }
        transaction
            .record_ais_crossing(AisCrossingObservation {
                mmsi: "555000555",
                vessel_name: Some("LATE ARRIVAL"),
                vessel_class: Some("pleasure"),
                length_meters: Some(18.0),
                draught_meters: None,
                direction: "downriver",
                crossed_at_ms: crossing_at,
                speed_knots: 5.0,
                session_id: "test",
            })
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        assert_eq!(
            store
                .resolve_ais_transits(crossing_at + 3 * 60_000)
                .await
                .unwrap(),
            0
        );
        let ledger = store.list_ais_ledger(100).await.unwrap();
        let vessel = ledger
            .iter()
            .find(|entry| entry.mmsi == "555000555")
            .unwrap();
        assert_eq!(vessel.transits_opened, 0);
        assert_eq!(vessel.transits_pending, 1);
    }

    #[tokio::test]
    async fn existing_database_opens_and_settings_round_trip() {
        let directory = tempdir().unwrap();
        let store = Store::open(directory.path().join("test.sqlite"))
            .await
            .unwrap();
        store
            .set_json(
                "profile",
                &json!({"name": "Bridge First"}),
                "2026-08-14T00:00:00Z",
            )
            .await
            .unwrap();
        let profile: serde_json::Value = store.get_json("profile").await.unwrap().unwrap();
        assert_eq!(profile["name"], "Bridge First");
        drop(store);

        let reopened = Store::open(directory.path().join("test.sqlite"))
            .await
            .unwrap();
        assert_eq!(
            reopened
                .get_json::<serde_json::Value>("profile")
                .await
                .unwrap(),
            Some(json!({"name": "Bridge First"}))
        );
    }

    #[tokio::test]
    async fn old_bridge_rows_migrate_without_inventing_confirmation_coverage() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("old.sqlite");
        let old = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::raw_sql(
            r#"
            CREATE TABLE bridge_state_intervals (
                source_id TEXT NOT NULL,
                bridge_key TEXT NOT NULL,
                bridge_name TEXT NOT NULL,
                relation TEXT NOT NULL,
                state TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                session_id TEXT,
                PRIMARY KEY (source_id, bridge_key, started_at_ms)
            );
            INSERT INTO bridge_state_intervals VALUES (
                'fl511.bridge', 'brickell', 'Brickell Avenue Bridge',
                'target', 'up', 1000, NULL, 'old-run'
            );
            "#,
        )
        .execute(&old)
        .await
        .unwrap();
        old.close().await;

        let store = Store::open(&path).await.unwrap();
        let migrated = store
            .list_bridge_state_intervals("fl511.bridge", "brickell")
            .await
            .unwrap();
        assert_eq!(migrated[0].last_confirmed_at_ms, 1_000);
        assert_eq!(migrated[0].start_reason, "legacy");

        let mut transaction = store.begin_transaction().await.unwrap();
        transaction
            .record_bridge_state(BridgeObservation {
                source_id: "fl511.bridge",
                bridge_key: "brickell",
                bridge_name: "Brickell Avenue Bridge",
                relation: "target",
                state: "up",
                observed_at_ms: 2_000,
                session_id: "new-run",
            })
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let intervals = store
            .list_bridge_state_intervals("fl511.bridge", "brickell")
            .await
            .unwrap();
        assert_eq!(intervals[0].ended_at_ms, Some(1_000));
        assert_eq!(intervals[1].start_reason, "session_start");
    }

    #[tokio::test]
    async fn vessel_catalog_keeps_static_identity_and_forecasts_are_capped_per_minute() {
        let store = Store::in_memory().await.unwrap();
        let mut transaction = store.begin_transaction().await.unwrap();
        transaction
            .record_ais_vessel_observation(AisVesselObservation {
                mmsi: "367999111",
                name: Some("MIAMI STAR"),
                vessel_class: Some("passenger"),
                call_sign: Some("WDF1234"),
                imo_number: Some(9_876_543),
                destination: Some("MIAMI RIVER"),
                length_meters: Some(42.0),
                beam_meters: Some(9.5),
                draught_meters: Some(2.4),
                observed_at_ms: 1_000,
            })
            .await
            .unwrap();
        transaction
            .record_ais_vessel_observation(AisVesselObservation {
                mmsi: "367999111",
                name: None,
                vessel_class: None,
                call_sign: None,
                imo_number: None,
                destination: None,
                length_meters: None,
                beam_meters: None,
                draught_meters: None,
                observed_at_ms: 2_000,
            })
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let catalog = store.list_ais_ledger(100).await.unwrap();
        let vessel = catalog
            .iter()
            .find(|entry| entry.mmsi == "367999111")
            .unwrap();
        assert_eq!(vessel.name.as_deref(), Some("MIAMI STAR"));
        assert_eq!(vessel.call_sign.as_deref(), Some("WDF1234"));
        assert_eq!(vessel.imo_number, Some(9_876_543));
        assert_eq!((vessel.first_seen_ms, vessel.last_seen_ms), (1_000, 2_000));

        for (evaluated_at_ms, state, score) in [(1_000, "clear", 1_200), (59_000, "likely", 6_800)]
        {
            store
                .record_forecast_sample(ForecastSample {
                    target_key: "brickell",
                    evaluated_at_ms,
                    model_version: "bridge-v2",
                    state,
                    predictive_score_bps: score,
                    confidence_bps: score,
                    eta_min_minutes: Some(6),
                    eta_max_minutes: Some(9),
                    schedule_mode: "on_signal",
                    contribution_bps_json: r#"{"ais":4200,"upstream":2600}"#,
                    source_freshness_json: r#"{"aisSeconds":3,"fl511Seconds":8}"#,
                    session_id: "run-a",
                })
                .await
                .unwrap();
        }
        let samples = store
            .forecast_samples_since("brickell", 0, 100)
            .await
            .unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].evaluated_at_ms, 59_000);
        assert_eq!(samples[0].state, "likely");
    }

    #[tokio::test]
    async fn selected_vessel_history_joins_impact_counts_and_newest_crossings() {
        let store = Store::in_memory().await.unwrap();
        let mmsi = "367999112";
        let base = 1_800_200_000_000_i64;
        let mut transaction = store.begin_transaction().await.unwrap();
        transaction
            .record_ais_vessel_observation(AisVesselObservation {
                mmsi,
                name: Some("BRICKELL RUNNER"),
                vessel_class: Some("tug"),
                call_sign: Some("WDF5678"),
                imo_number: Some(9_765_432),
                destination: Some("MIAMI RIVER"),
                length_meters: Some(31.0),
                beam_meters: Some(8.0),
                draught_meters: Some(2.2),
                observed_at_ms: base,
            })
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        for (index, (direction, outcome)) in [
            ("upriver", None),
            ("downriver", Some("unknown")),
            ("upriver", Some("fits_under")),
            ("downriver", Some("opened")),
        ]
        .into_iter()
        .enumerate()
        {
            let crossed_at_ms = base + (index as i64 + 1) * 60_000;
            sqlx::query(
                r#"
                INSERT INTO ais_transits(
                    mmsi, crossed_at_ms, direction, speed_knots,
                    outcome, resolved_at_ms, session_id
                ) VALUES (?1, ?2, ?3, 5.0, ?4, ?5, 'detail-test')
                "#,
            )
            .bind(mmsi)
            .bind(crossed_at_ms)
            .bind(direction)
            .bind(outcome)
            .bind(outcome.map(|_| crossed_at_ms + 30_000))
            .execute(store.pool())
            .await
            .unwrap();
        }
        sqlx::query(
            "UPDATE ais_vessel_ledger SET transits_opened = 1, transits_fits_under = 1 WHERE mmsi = ?1",
        )
        .bind(mmsi)
        .execute(store.pool())
        .await
        .unwrap();

        let history = store
            .get_ais_vessel_history(mmsi, 3)
            .await
            .unwrap()
            .expect("catalogued vessel");
        assert_eq!(history.ledger.name.as_deref(), Some("BRICKELL RUNNER"));
        assert_eq!(history.ledger.transits_opened, 1);
        assert_eq!(history.ledger.transits_fits_under, 1);
        assert_eq!(history.ledger.transits_unknown, 1);
        assert_eq!(history.ledger.transits_pending, 1);
        assert_eq!(history.ledger.last_crossing_at_ms, Some(base + 240_000));
        assert_eq!(history.ledger.last_opened_at_ms, Some(base + 240_000));
        assert_eq!(history.recent_crossings.len(), 3);
        assert_eq!(
            history.recent_crossings[0].outcome.as_deref(),
            Some("opened")
        );
        assert_eq!(
            history.recent_crossings[0].resolved_at_ms,
            Some(base + 270_000)
        );
        assert_eq!(
            history.recent_crossings[2].outcome.as_deref(),
            Some("unknown")
        );
        let known_openers = store.list_known_ais_openers().await.unwrap();
        assert!(
            known_openers.iter().any(|entry| entry.mmsi == mmsi),
            "a vessel with one confirmed opening remains in the durable catalog"
        );
        assert!(
            known_openers.iter().all(|entry| entry.transits_opened > 0),
            "fits-under-only and unresolved vessels are not called known openers"
        );
        assert!(
            store
                .get_ais_vessel_history("999999998", 3)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn legacy_learning_import_is_idempotent_and_recounts_crossing_outcomes() {
        let directory = tempdir().unwrap();
        let legacy_path = directory.path().join("legacy.sqlite");
        let legacy = Store::open(&legacy_path).await.unwrap();
        let mut transaction = legacy.begin_transaction().await.unwrap();
        transaction
            .record_ais_vessel_observation(AisVesselObservation {
                mmsi: "367888999",
                name: Some("RIVER WORKHORSE"),
                vessel_class: Some("tug"),
                call_sign: None,
                imo_number: None,
                destination: Some("MIAMI RIVER"),
                length_meters: Some(27.0),
                beam_meters: Some(8.0),
                draught_meters: Some(2.8),
                observed_at_ms: 10_000,
            })
            .await
            .unwrap();
        transaction
            .record_ais_crossing(AisCrossingObservation {
                mmsi: "367888999",
                vessel_name: Some("RIVER WORKHORSE"),
                vessel_class: Some("tug"),
                length_meters: Some(27.0),
                draught_meters: Some(2.8),
                direction: "downriver",
                crossed_at_ms: 20_000,
                speed_knots: 3.8,
                session_id: "legacy-run",
            })
            .await
            .unwrap();
        transaction
            .record_ais_track_fix(AisTrackFix {
                mmsi: "367888999",
                observed_at_ms: 19_000,
                latitude: 25.769,
                longitude: -80.191,
                speed_knots: Some(3.8),
                course_degrees: Some(95.0),
                branch: Some("river"),
                s_meters: Some(120.0),
                offset_meters: Some(4.0),
                posture: Some("underway"),
                session_id: "legacy-run",
            })
            .await
            .unwrap();
        transaction
            .record_river_transit(RiverTransitObservation {
                source_id: "bbpilots.board",
                movement_key: "river-workhorse-1",
                vessel: "RIVER WORKHORSE",
                action: "sailing",
                river_direction: Some("downriver"),
                scheduled_at_ms: 5_000,
                estimated_bridge_at_ms: Some(20_000),
                estimated_offset_minutes: Some(15),
                observed_at_ms: 4_000,
                session_id: "legacy-run",
            })
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        sqlx::query(
            "UPDATE ais_transits SET outcome = 'opened', resolved_at_ms = 21000 WHERE mmsi = '367888999'",
        )
        .execute(legacy.pool())
        .await
        .unwrap();
        drop(legacy);

        let store = Store::in_memory().await.unwrap();
        let first = store.import_legacy_learning(&legacy_path).await.unwrap();
        assert_eq!(first.vessels_added, 1);
        assert_eq!(first.transits_added, 1);
        assert_eq!(first.track_fixes_added, 1);
        assert_eq!(first.river_transits_added, 1);
        let second = store.import_legacy_learning(&legacy_path).await.unwrap();
        assert_eq!(second, LegacyLearningImportReport::default());

        let catalog = store.list_ais_ledger(100).await.unwrap();
        let imported = catalog
            .iter()
            .find(|entry| entry.mmsi == "367888999")
            .unwrap();
        assert_eq!(imported.name.as_deref(), Some("RIVER WORKHORSE"));
        assert_eq!(imported.transits_opened, 1);
        assert_eq!(imported.last_opened_at_ms, Some(20_000));
        assert_eq!(store.track_fixes_since(0).await.unwrap().len(), 1);
        let movements: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM river_transits")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(movements, 1);
    }

    #[tokio::test]
    async fn transaction_commits_related_json_state() {
        let store = Store::in_memory().await.unwrap();
        let preferences = json!({"profile": "bridge-first"});
        let state = json!({"cycle": 7});

        let mut transaction = store.begin_transaction().await.unwrap();
        transaction
            .set_json("app.preferences", &preferences, "2026-08-14T00:00:01Z")
            .await
            .unwrap();
        transaction
            .set_json("runtime.state", &state, "2026-08-14T00:00:01Z")
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        assert_eq!(
            store
                .get_json::<serde_json::Value>("app.preferences")
                .await
                .unwrap(),
            Some(preferences)
        );
        assert_eq!(
            store
                .get_json::<serde_json::Value>("runtime.state")
                .await
                .unwrap(),
            Some(state)
        );
    }

    #[tokio::test]
    async fn bridge_state_changes_close_the_prior_interval() {
        let store = Store::in_memory().await.unwrap();

        let mut transaction = store.begin_transaction().await.unwrap();
        transaction
            .record_bridge_state(BridgeObservation {
                source_id: "fl511.bridge.brickell",
                bridge_key: "sw_2_ave",
                bridge_name: "SW 2 Ave Bridge",
                relation: "upstream",
                state: "down",
                observed_at_ms: 1_000,
                session_id: "run-a",
            })
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        let mut unchanged = store.begin_transaction().await.unwrap();
        unchanged
            .record_bridge_state(BridgeObservation {
                source_id: "fl511.bridge.brickell",
                bridge_key: "sw_2_ave",
                bridge_name: "SW 2 Ave Bridge",
                relation: "upstream",
                state: "down",
                observed_at_ms: 2_000,
                session_id: "run-a",
            })
            .await
            .unwrap();
        unchanged.commit().await.unwrap();

        let mut changed = store.begin_transaction().await.unwrap();
        changed
            .record_bridge_state(BridgeObservation {
                source_id: "fl511.bridge.brickell",
                bridge_key: "sw_2_ave",
                bridge_name: "SW 2 Ave Bridge",
                relation: "upstream",
                state: "up",
                observed_at_ms: 3_000,
                session_id: "run-b",
            })
            .await
            .unwrap();
        changed.commit().await.unwrap();

        let intervals = store
            .list_bridge_state_intervals("fl511.bridge.brickell", "sw_2_ave")
            .await
            .unwrap();
        assert_eq!(intervals.len(), 2);
        assert_eq!(intervals[0].state, "down");
        assert_eq!(intervals[0].started_at_ms, 1_000);
        assert_eq!(intervals[0].last_confirmed_at_ms, 2_000);
        assert_eq!(intervals[0].ended_at_ms, Some(2_000));
        // A new engine run starts a new observation interval even if its first
        // reading differs. Downtime is not classified as a bridge transition.
        assert_eq!(intervals[0].session_id.as_deref(), Some("run-a"));
        assert_eq!(intervals[1].session_id.as_deref(), Some("run-b"));
        assert_eq!(intervals[1].start_reason, "session_start");
        assert_eq!(intervals[1].state, "up");
        assert_eq!(intervals[1].started_at_ms, 3_000);
        assert_eq!(intervals[1].ended_at_ms, None);

        let recent = store.list_recent_bridge_state_intervals(1).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].bridge_key, "sw_2_ave");
        assert_eq!(recent[0].state, "up");
    }

    #[tokio::test]
    async fn outbox_material_action_is_retry_safe() {
        let store = Store::in_memory().await.unwrap();
        let incident_id = Uuid::now_v7();
        store
            .upsert_incident(&IncidentRecord {
                id: incident_id,
                channel_id: "bridge.brickell",
                state: "likely",
                urgency: "heads_up",
                material_revision: 1,
                fingerprint: "likely-8-12",
                payload: &json!({"eta": [8, 12]}),
                opened_at: "2026-08-14T00:00:00Z",
                updated_at: "2026-08-14T00:00:00Z",
                resolved_at: None,
            })
            .await
            .unwrap();
        let entry = OutboxRecord {
            id: Uuid::now_v7(),
            route_id: "whatsapp.primary",
            incident_id,
            material_revision: 1,
            action: "stage_change",
            request: &json!({"template": "bridge_likely"}),
            next_attempt_at: "2026-08-14T00:00:00Z",
            created_at: "2026-08-14T00:00:00Z",
        };
        assert!(store.enqueue(&entry).await.unwrap());
        assert!(!store.enqueue(&entry).await.unwrap());
        let lease = store
            .lease_next("2026-08-14T00:00:00Z", "2026-08-14T00:01:00Z")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.route_id, "whatsapp.primary");
    }

    #[tokio::test]
    async fn expired_outbox_lease_is_recovered_after_worker_restart() {
        let store = Store::in_memory().await.unwrap();
        let incident_id = Uuid::now_v7();
        store
            .upsert_incident(&IncidentRecord {
                id: incident_id,
                channel_id: "weather.miami",
                state: "active",
                urgency: "recommended",
                material_revision: 1,
                fingerprint: "rain-70-percent",
                payload: &json!({"summary": "Rain threshold crossed"}),
                opened_at: "2026-08-14T00:00:00Z",
                updated_at: "2026-08-14T00:00:00Z",
                resolved_at: None,
            })
            .await
            .unwrap();
        let entry = OutboxRecord {
            id: Uuid::now_v7(),
            route_id: "whatsapp.primary",
            incident_id,
            material_revision: 1,
            action: "material_update",
            request: &json!({"summary": "Rain threshold crossed"}),
            next_attempt_at: "2026-08-14T00:00:00Z",
            created_at: "2026-08-14T00:00:00Z",
        };
        assert!(store.enqueue(&entry).await.unwrap());

        let first = store
            .lease_next("2026-08-14T00:00:00Z", "2026-08-14T00:01:00Z")
            .await
            .unwrap()
            .expect("pending row should be leased");
        assert!(
            store
                .lease_next("2026-08-14T00:00:30Z", "2026-08-14T00:01:30Z")
                .await
                .unwrap()
                .is_none(),
            "a live lease must not be stolen"
        );

        let recovered = store
            .lease_next("2026-08-14T00:01:01Z", "2026-08-14T00:02:01Z")
            .await
            .unwrap()
            .expect("an expired lease must be recoverable after a crash");
        assert_eq!(recovered.id, first.id);
        assert_eq!(recovered.attempts, 1);
    }

    #[tokio::test]
    async fn outbox_history_is_newest_first_and_contains_no_auth_material() {
        let store = Store::in_memory().await.unwrap();
        let incident_id = Uuid::now_v7();
        store
            .upsert_incident(&IncidentRecord {
                id: incident_id,
                channel_id: "official.miami",
                state: "active",
                urgency: "confirmed_only",
                material_revision: 1,
                fingerprint: "warning-1",
                payload: &json!({"summary": "Tornado warning"}),
                opened_at: "2026-08-14T00:00:00Z",
                updated_at: "2026-08-14T00:00:00Z",
                resolved_at: None,
            })
            .await
            .unwrap();
        let request = json!({
            "notice": {"subject": "Tornado warning"},
            "destination": {"address": "+13055550123"}
        });
        let outbox_id = Uuid::now_v7();
        assert!(
            store
                .enqueue(&OutboxRecord {
                    id: outbox_id,
                    route_id: "meta.whatsapp.cloud",
                    incident_id,
                    material_revision: 1,
                    action: "material_update",
                    request: &request,
                    next_attempt_at: "2026-08-14T00:00:00Z",
                    created_at: "2026-08-14T00:00:00Z",
                })
                .await
                .unwrap()
        );
        store
            .mark_outbox(
                &outbox_id.to_string(),
                "accepted",
                "2026-08-14T00:00:03Z",
                Some("wamid.safe-id"),
                None,
                None,
            )
            .await
            .unwrap();

        let history = store.list_outbox_history(20).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "accepted");
        assert_eq!(
            history[0].provider_message_id.as_deref(),
            Some("wamid.safe-id")
        );
        assert!(history[0].request_json.contains("Tornado warning"));
        assert!(!history[0].request_json.contains("+13055550123"));
        assert!(
            !history[0]
                .request_json
                .to_ascii_lowercase()
                .contains("authorization")
        );
    }

    #[tokio::test]
    async fn outbox_mark_and_json_state_commit_or_roll_back_together() {
        let store = Store::in_memory().await.unwrap();
        let incident_id = Uuid::now_v7();
        let address = "+13055550123";
        store
            .upsert_incident(&IncidentRecord {
                id: incident_id,
                channel_id: "bridge.brickell",
                state: "active",
                urgency: "recommended",
                material_revision: 1,
                fingerprint: "bridge-opening",
                payload: &json!({"destination": {"address": address}}),
                opened_at: "2026-08-14T00:00:00Z",
                updated_at: "2026-08-14T00:00:00Z",
                resolved_at: None,
            })
            .await
            .unwrap();
        let outbox_id = Uuid::now_v7();
        store
            .enqueue(&OutboxRecord {
                id: outbox_id,
                route_id: "meta.whatsapp.cloud",
                incident_id,
                material_revision: 1,
                action: "material_update",
                request: &json!({"destination": {"address": address}}),
                next_attempt_at: "2026-08-14T00:00:00Z",
                created_at: "2026-08-14T00:00:00Z",
            })
            .await
            .unwrap();
        let original_cursor = json!({"revision": 1});
        let accepted_cursor = json!({"revision": 2});
        store
            .set_json("delivery.cursor", &original_cursor, "2026-08-14T00:00:00Z")
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TRIGGER reject_delivery_cursor
            BEFORE UPDATE ON settings
            WHEN NEW.key = 'delivery.cursor'
            BEGIN
                SELECT RAISE(ABORT, 'injected settings failure');
            END
            "#,
        )
        .execute(store.pool())
        .await
        .unwrap();

        let error = store
            .mark_outbox_and_set_json(
                &outbox_id.to_string(),
                "accepted",
                "2026-08-14T00:00:01Z",
                Some("wamid.atomic"),
                None,
                None,
                "delivery.cursor",
                &accepted_cursor,
            )
            .await
            .expect_err("the injected settings failure must abort the outbox mark");
        assert!(matches!(error, StorageError::Database(_)));

        let history = store.list_outbox_history(1).await.unwrap();
        assert_eq!(history[0].status, "pending");
        assert!(history[0].request_json.contains(address));
        assert_eq!(
            store
                .get_json::<serde_json::Value>("delivery.cursor")
                .await
                .unwrap(),
            Some(original_cursor)
        );
        let incident_payload = sqlx::query_scalar::<Sqlite, String>(
            "SELECT payload_json FROM incidents WHERE id = ?1",
        )
        .bind(incident_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(incident_payload.contains(address));

        sqlx::query("DROP TRIGGER reject_delivery_cursor")
            .execute(store.pool())
            .await
            .unwrap();
        store
            .mark_outbox_and_set_json(
                &outbox_id.to_string(),
                "accepted",
                "2026-08-14T00:00:02Z",
                Some("wamid.atomic"),
                None,
                None,
                "delivery.cursor",
                &accepted_cursor,
            )
            .await
            .unwrap();

        let history = store.list_outbox_history(1).await.unwrap();
        assert_eq!(history[0].status, "accepted");
        assert!(!history[0].request_json.contains(address));
        assert_eq!(
            store
                .get_json::<serde_json::Value>("delivery.cursor")
                .await
                .unwrap(),
            Some(accepted_cursor)
        );
        let incident_payload = sqlx::query_scalar::<Sqlite, String>(
            "SELECT payload_json FROM incidents WHERE id = ?1",
        )
        .bind(incident_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(!incident_payload.contains(address));
    }

    #[tokio::test]
    async fn delivery_transition_commits_incident_outbox_and_cursor_together() {
        let store = Store::in_memory().await.unwrap();
        let incident_id = Uuid::now_v7();
        let request = json!({"notice": {"subject": "Bridge likely"}});
        let cursor = json!({"channels": {"bridge.brickell": {"revision": 1}}});
        let inserted = store
            .commit_delivery_transition(
                &IncidentRecord {
                    id: incident_id,
                    channel_id: "bridge.brickell",
                    state: "active",
                    urgency: "recommended",
                    material_revision: 1,
                    fingerprint: "likely:7-10:high",
                    payload: &request,
                    opened_at: "2026-08-14T00:00:00Z",
                    updated_at: "2026-08-14T00:00:00Z",
                    resolved_at: None,
                },
                &OutboxRecord {
                    id: Uuid::now_v7(),
                    route_id: "meta.whatsapp.cloud",
                    incident_id,
                    material_revision: 1,
                    action: "material_update",
                    request: &request,
                    next_attempt_at: "2026-08-14T00:00:00Z",
                    created_at: "2026-08-14T00:00:00Z",
                },
                "dispatch.cursor",
                &cursor,
                "2026-08-14T00:00:00Z",
            )
            .await
            .unwrap();

        assert!(inserted);
        assert_eq!(
            store
                .get_json::<serde_json::Value>("dispatch.cursor")
                .await
                .unwrap(),
            Some(cursor)
        );
        let lease = store
            .lease_next("2026-08-14T00:00:00Z", "2026-08-14T00:01:00Z")
            .await
            .unwrap()
            .expect("the same transaction should publish an outbox row");
        assert_eq!(lease.incident_id, incident_id.to_string());
        let incident_count = sqlx::query_scalar::<Sqlite, i64>(
            "SELECT COUNT(*) FROM incidents WHERE id = ?1 AND material_revision = 1",
        )
        .bind(incident_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(incident_count, 1);
    }

    #[tokio::test]
    async fn route_suppression_and_tracker_reset_are_atomic() {
        let store = Store::in_memory().await.unwrap();
        let incident_id = Uuid::now_v7();
        let outbox_id = Uuid::now_v7();
        store
            .upsert_incident(&IncidentRecord {
                id: incident_id,
                channel_id: "bridge.brickell",
                state: "active",
                urgency: "recommended",
                material_revision: 1,
                fingerprint: "likely",
                payload: &json!({"destination": {"address": "+13055550123"}}),
                opened_at: "2026-08-14T00:00:00Z",
                updated_at: "2026-08-14T00:00:00Z",
                resolved_at: None,
            })
            .await
            .unwrap();
        store
            .enqueue(&OutboxRecord {
                id: outbox_id,
                route_id: "meta.whatsapp.cloud",
                incident_id,
                material_revision: 1,
                action: "material_update",
                request: &json!({"destination": {"address": "+13055550123"}}),
                next_attempt_at: "2026-08-14T00:00:00Z",
                created_at: "2026-08-14T00:00:00Z",
            })
            .await
            .unwrap();
        let old_tracker = json!({"channels": {"bridge.brickell": {"active": true}}});
        let reset_tracker = json!({"channels": {}});
        store
            .set_json(
                "desktop.whatsapp.dispatch",
                &old_tracker,
                "2026-08-14T00:00:00Z",
            )
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TRIGGER reject_tracker_reset
            BEFORE UPDATE ON settings
            WHEN NEW.key = 'desktop.whatsapp.dispatch'
            BEGIN
                SELECT RAISE(ABORT, 'injected tracker failure');
            END
            "#,
        )
        .execute(store.pool())
        .await
        .unwrap();

        store
            .suppress_route_and_set_json(
                "meta.whatsapp.cloud",
                "2026-08-14T00:00:01Z",
                "credential replaced",
                "desktop.whatsapp.dispatch",
                &reset_tracker,
            )
            .await
            .expect_err("tracker failure must roll route suppression back");
        assert_eq!(
            store.list_outbox_history(1).await.unwrap()[0].status,
            "pending"
        );
        assert_eq!(
            store
                .get_json::<serde_json::Value>("desktop.whatsapp.dispatch")
                .await
                .unwrap(),
            Some(old_tracker)
        );

        sqlx::query("DROP TRIGGER reject_tracker_reset")
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            store
                .suppress_route_and_set_json(
                    "meta.whatsapp.cloud",
                    "2026-08-14T00:00:02Z",
                    "credential replaced",
                    "desktop.whatsapp.dispatch",
                    &reset_tracker,
                )
                .await
                .unwrap(),
            1
        );
        let row = &store.list_outbox_history(1).await.unwrap()[0];
        assert_eq!(row.status, "suppressed");
        assert!(!row.request_json.contains("+13055550123"));
        assert_eq!(
            store
                .get_json::<serde_json::Value>("desktop.whatsapp.dispatch")
                .await
                .unwrap(),
            Some(reset_tracker)
        );
    }

    #[tokio::test]
    async fn pruning_scrubs_terminal_pii_and_preserves_retryable_work() {
        let store = Store::in_memory().await.unwrap();
        let old_incident = Uuid::now_v7();
        store
            .upsert_incident(&IncidentRecord {
                id: old_incident,
                channel_id: "weather.miami",
                state: "resolved",
                urgency: "recommended",
                material_revision: 1,
                fingerprint: "rain",
                payload: &json!({
                    "summary": "rain",
                    "destination": {"address": "+13055550123"}
                }),
                opened_at: "2026-01-01T00:00:00Z",
                updated_at: "2026-01-01T00:00:00Z",
                resolved_at: Some("2026-01-01T00:00:00Z"),
            })
            .await
            .unwrap();
        let old_outbox = Uuid::now_v7();
        store
            .enqueue(&OutboxRecord {
                id: old_outbox,
                route_id: "meta.whatsapp.cloud",
                incident_id: old_incident,
                material_revision: 1,
                action: "resolved",
                request: &json!({"destination": {"address": "+13055550123"}}),
                next_attempt_at: "2026-01-01T00:00:00Z",
                created_at: "2026-01-01T00:00:00Z",
            })
            .await
            .unwrap();
        store
            .mark_outbox(
                &old_outbox.to_string(),
                "accepted",
                "2026-01-01T00:00:01Z",
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let incident_payload = sqlx::query_scalar::<Sqlite, String>(
            "SELECT payload_json FROM incidents WHERE id = ?1",
        )
        .bind(old_incident.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
        let history_payload = sqlx::query_scalar::<Sqlite, String>(
            "SELECT payload_json FROM incident_history WHERE incident_id = ?1",
        )
        .bind(old_incident.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(!incident_payload.contains("+13055550123"));
        assert!(!history_payload.contains("+13055550123"));

        let retryable = Uuid::now_v7();
        store
            .enqueue(&OutboxRecord {
                id: retryable,
                route_id: "meta.whatsapp.cloud",
                incident_id: Uuid::now_v7(),
                material_revision: 1,
                action: "material_update",
                request: &json!({"destination": {"address": "+13055550999"}}),
                next_attempt_at: "2026-01-01T00:05:00Z",
                created_at: "2026-01-01T00:00:00Z",
            })
            .await
            .unwrap();

        let report = store
            .prune_history("2026-04-01T00:00:00Z", 0)
            .await
            .unwrap();
        assert_eq!(report.outbox_rows, 1);
        assert_eq!(report.incidents, 1);
        let remaining = store.list_outbox_history(20).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, retryable.to_string());
        assert!(remaining[0].request_json.contains("+13055550999"));
    }

    #[tokio::test]
    async fn observed_tracks_are_deduplicated_and_known_opener_history_is_preserved() {
        let store = Store::in_memory().await.unwrap();
        let week_ms = 7 * 24 * 60 * 60 * 1_000;
        let now_ms = 1_787_000_000_000_i64;
        fn fix(mmsi: &str, observed_at_ms: i64) -> AisTrackFix<'_> {
            AisTrackFix {
                mmsi,
                observed_at_ms,
                latitude: 25.7699,
                longitude: -80.190_05,
                speed_knots: Some(4.2),
                course_degrees: Some(271.0),
                branch: Some("river"),
                s_meters: Some(-12.0),
                offset_meters: Some(31.0),
                posture: Some("underway"),
                session_id: "session-a",
            }
        }

        let mut transaction = store.begin_transaction().await.unwrap();
        transaction
            .record_ais_track_fix(fix("367123456", now_ms))
            .await
            .unwrap();
        transaction
            .record_ais_track_fix(fix("367123456", now_ms + 15_000))
            .await
            .unwrap();
        transaction
            .record_ais_track_fix(fix("367123456", now_ms + 30_000))
            .await
            .unwrap();
        // The live window is re-offered whole every cycle, so the same fix
        // arriving again must not fail or double-count.
        transaction
            .record_ais_track_fix(fix("367123456", now_ms))
            .await
            .unwrap();
        transaction
            .record_ais_track_fix(fix("367123456", now_ms - week_ms - 1))
            .await
            .unwrap();
        // SARA is a schema-seeded opener. Its full historical run survives
        // ordinary raw-fix retention so the bridge-opening corridor remains
        // learnable indefinitely.
        transaction
            .record_ais_track_fix(fix("367705810", now_ms - week_ms - 1))
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        let held = store.track_fixes_since(0).await.unwrap();
        assert_eq!(
            held.len(),
            4,
            "exact and nearby repeats must be ignored; the 30 s boundary is retained"
        );
        assert!(held.iter().any(|fix| fix.observed_at_ms == now_ms + 30_000));
        assert!(held.iter().all(|fix| fix.offset_meters == Some(31.0)));
        assert!(
            held.iter()
                .all(|fix| fix.branch.as_deref() == Some("river"))
        );

        let report = store
            .prune_history("2026-04-01T00:00:00Z", now_ms - week_ms)
            .await
            .unwrap();
        assert_eq!(report.track_fixes, 1, "only the ordinary old fix goes");
        let remaining = store.track_fixes_since(0).await.unwrap();
        assert_eq!(remaining.len(), 3);
        assert!(
            remaining
                .iter()
                .any(|fix| fix.mmsi == "367705810" && fix.observed_at_ms < now_ms)
        );
    }

    #[tokio::test]
    async fn route_revocation_suppresses_pending_work_and_scrubs_every_copy() {
        let store = Store::in_memory().await.unwrap();
        let incident_id = Uuid::now_v7();
        let payload = json!({"destination": {"address": "+13055550123"}});
        store
            .upsert_incident(&IncidentRecord {
                id: incident_id,
                channel_id: "official.miami",
                state: "active",
                urgency: "confirmed_only",
                material_revision: 1,
                fingerprint: "warning",
                payload: &payload,
                opened_at: "2026-08-14T00:00:00Z",
                updated_at: "2026-08-14T00:00:00Z",
                resolved_at: None,
            })
            .await
            .unwrap();
        store
            .enqueue(&OutboxRecord {
                id: Uuid::now_v7(),
                route_id: "meta.whatsapp.cloud",
                incident_id,
                material_revision: 1,
                action: "material_update",
                request: &payload,
                next_attempt_at: "2026-08-14T00:00:00Z",
                created_at: "2026-08-14T00:00:00Z",
            })
            .await
            .unwrap();

        assert_eq!(
            store
                .suppress_route_and_scrub(
                    "meta.whatsapp.cloud",
                    "2026-08-14T00:00:01Z",
                    "recipient changed",
                )
                .await
                .unwrap(),
            1
        );
        let outbox = store.list_outbox_history(10).await.unwrap();
        assert_eq!(outbox[0].status, "suppressed");
        assert!(!outbox[0].request_json.contains("+13055550123"));
        let incident = sqlx::query_scalar::<Sqlite, String>(
            "SELECT payload_json FROM incidents WHERE id = ?1",
        )
        .bind(incident_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
        let history = sqlx::query_scalar::<Sqlite, String>(
            "SELECT payload_json FROM incident_history WHERE incident_id = ?1",
        )
        .bind(incident_id.to_string())
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(!incident.contains("+13055550123"));
        assert!(!history.contains("+13055550123"));
    }

    #[test]
    fn compares_versions_numerically() {
        assert_eq!(parse_version("3.51.3").unwrap(), (3, 51, 3));
        assert!(parse_version("not-a-version").is_err());
    }

    /// The board republishes the same booking for hours and sometimes revises
    /// it. Calibration wants one row per movement carrying the schedule it
    /// settled on, and it wants to know when the movement was first announced —
    /// a revision that reset that would make a long-planned transit look like it
    /// appeared minutes ago.
    #[tokio::test]
    async fn a_revised_movement_updates_its_schedule_without_losing_when_it_appeared() {
        let store = Store::in_memory().await.unwrap();
        let observe = |scheduled: i64, seen: i64| RiverTransitObservation {
            source_id: "bbpilots.bridge.brickell",
            movement_key: "bbp-movement-1",
            vessel: "MV EXAMPLE",
            action: "departure",
            river_direction: Some("outbound"),
            scheduled_at_ms: scheduled,
            estimated_bridge_at_ms: Some(scheduled + 20 * 60_000),
            estimated_offset_minutes: Some(20),
            observed_at_ms: seen,
            session_id: "session-a",
        };

        let mut transaction = store.begin_transaction().await.unwrap();
        transaction
            .record_river_transit(observe(1_000_000, 900_000))
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        // Same movement, later fetch, schedule pushed back half an hour.
        let mut transaction = store.begin_transaction().await.unwrap();
        transaction
            .record_river_transit(observe(2_800_000, 2_000_000))
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        let row: (i64, i64, i64, String) = sqlx::query_as(
            "SELECT scheduled_at_ms, first_seen_at_ms, last_seen_at_ms, vessel \
             FROM river_transits",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            row.0, 2_800_000,
            "the revised schedule is what calibration reads"
        );
        assert_eq!(row.1, 900_000, "first sighting survives the revision");
        assert_eq!(row.2, 2_000_000, "last sighting moves forward");
        assert_eq!(row.3, "MV EXAMPLE");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM river_transits")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "one row per movement, not one per fetch");
    }

    #[tokio::test]
    async fn reports_live_sqlite_allocation() {
        let store = Store::in_memory().await.unwrap();
        assert!(store.database_size_bytes().await.unwrap() > 0);
    }
}
