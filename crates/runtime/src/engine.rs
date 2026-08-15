use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::{
    AisConnectionStateDto, AisStreamStatusDto, AlertArea, AlertAreaSource, AppPreferences,
    AppSnapshot, AvailabilityDto, BridgeRelationDto, BridgeStateDto, BridgeStateIntervalDto,
    ChannelKindDto, ChannelPreference, ChannelSignalDto, ChannelSnapshot,
    CredentialFreeCollectorFactory, DecisionSnapshot, DeliveryStateDto, DestinationIdDto,
    DisplayTransport, EvidenceStateDto, EvidenceStrip, LocationSearchError, LocationSearchResult,
    LocationSearchService, MutationResult, ObservedBridgeStateDto, OutputSnapshot, OutputStateDto,
    PreferencesError, SourceHealth, SystemHealth, SystemStatusDto, UnitSystem, VesselTrackSnapshot,
    WhatsAppRecipientConsent, validate_preferences, whatsapp_consent_is_current,
};
use bridgestatus_collectors::{
    AIS_VESSEL_TRACKS_CURSOR_KEY, CollectContext, Collector, CollectorBatch, CollectorCursor,
    CollectorError, CollectorItem, HealthState, ItemKind,
};
use bridgestatus_model::{
    Availability, AvailabilityStatus, BridgeControllerState, BridgeObservation, ChannelId,
    Confidence, EtaRangeMinutes, Observation, ObservationId, OutboundProgressStage, SourceId,
    TimestampMillis, VesselMovement,
};
use bridgestatus_policy::{
    BridgeEvidence, BridgePrediction, BridgePredictor, ContributionDisposition, EvidenceKind,
    PredictionError,
};
use futures::{StreamExt, stream};
use jiff::{Timestamp, tz::TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tenders_storage::{StorageError, Store};
use thiserror::Error;
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::{MissedTickBehavior, timeout},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

const PREFERENCES_KEY: &str = "runtime.preferences";
const LIVE_STATE_KEY: &str = "runtime.live_state";

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub user_agent: String,
    pub poll_interval: Duration,
    pub collector_timeout: Duration,
    pub max_concurrent_collectors: usize,
    pub backoff_initial: Duration,
    pub backoff_max: Duration,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            user_agent: "PuenteGonorrea/0.1 (+https://github.com/cmiami/PuenteGonorrea)".into(),
            // The tick is the scheduler's granularity, not any source's rate.
            // It is set by the fastest consumer -- FL511 bridge status at 15s --
            // and every collector declares its own floor in the factory, so a
            // fast tick does not translate into fast polling for anything else.
            poll_interval: Duration::from_secs(15),
            collector_timeout: Duration::from_secs(20),
            max_concurrent_collectors: 4,
            backoff_initial: Duration::from_secs(15),
            backoff_max: Duration::from_secs(15 * 60),
        }
    }
}

impl RuntimeConfig {
    fn validate(&self) -> Result<(), RuntimeError> {
        if self.user_agent.trim().is_empty() {
            return Err(RuntimeError::Configuration(
                "runtime User-Agent cannot be empty".into(),
            ));
        }
        if self.poll_interval.is_zero()
            || self.collector_timeout.is_zero()
            || self.backoff_initial.is_zero()
            || self.backoff_max < self.backoff_initial
        {
            return Err(RuntimeError::Configuration(
                "poll, timeout, and backoff durations are invalid".into(),
            ));
        }
        if !(1..=32).contains(&self.max_concurrent_collectors) {
            return Err(RuntimeError::Configuration(
                "max_concurrent_collectors must be between 1 and 32".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct CollectorRegistration {
    pub id: String,
    pub channel_id: String,
    pub collector: Arc<dyn Collector>,
    pub minimum_interval: Duration,
    pub fail_closed_on_error: bool,
}

impl CollectorRegistration {
    pub fn new(
        id: impl Into<String>,
        channel_id: impl Into<String>,
        collector: Arc<dyn Collector>,
    ) -> Self {
        Self {
            id: id.into(),
            channel_id: channel_id.into(),
            collector,
            minimum_interval: Duration::ZERO,
            fail_closed_on_error: false,
        }
    }

    pub fn with_minimum_interval(mut self, minimum_interval: Duration) -> Self {
        self.minimum_interval = minimum_interval;
        self
    }

    /// Marks a connection-bound collector whose cached observations become
    /// unusable as soon as collection fails or reports non-healthy status.
    pub fn fail_closed_on_error(mut self) -> Self {
        self.fail_closed_on_error = true;
        self
    }
}

pub trait CollectorFactory: Send + Sync {
    fn build(
        &self,
        preferences: &AppPreferences,
    ) -> Result<Vec<CollectorRegistration>, RuntimeError>;

    fn set_aisstream_key(&self, _key: Option<String>) -> Result<(), RuntimeError> {
        Err(RuntimeError::Configuration(
            "this collector factory does not support AISStream secrets".into(),
        ))
    }

    /// Begins a reversible AISStream secret change. The runtime keeps this
    /// guard alive until the matching preferences/live-state transaction has
    /// committed; dropping it first restores the previous secret and live
    /// collectors. Implementations that own AIS secrets must override this.
    fn begin_aisstream_key_change(
        &self,
        _key: Option<String>,
    ) -> Result<AisStreamKeyChange, RuntimeError> {
        Err(RuntimeError::Configuration(
            "this collector factory does not support transactional AISStream secrets".into(),
        ))
    }

    /// Returns the host secret's actual presence when this factory owns an AIS
    /// secret boundary. `None` leaves the serializable flag untouched for
    /// factories that do not implement AIS.
    fn aisstream_key_configured(&self) -> Result<Option<bool>, RuntimeError> {
        Ok(None)
    }

    fn cancel(&self) {}
}

/// Opaque rollback guard for one host-secret mutation.
///
/// This type never contains serializable key material and its debug output is
/// intentionally redacted. Dropping an uncommitted guard restores the prior
/// factory state.
#[must_use = "commit the AISStream secret change only after durable state commits"]
pub struct AisStreamKeyChange {
    finish: Option<Box<dyn FnOnce(bool) + Send>>,
}

impl AisStreamKeyChange {
    pub(crate) fn new(finish: impl FnOnce(bool) + Send + 'static) -> Self {
        Self {
            finish: Some(Box::new(finish)),
        }
    }

    pub(crate) fn commit(mut self) {
        if let Some(finish) = self.finish.take() {
            finish(true);
        }
    }
}

impl fmt::Debug for AisStreamKeyChange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AisStreamKeyChange")
            .field("secret", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl Drop for AisStreamKeyChange {
    fn drop(&mut self) {
        if let Some(finish) = self.finish.take() {
            finish(false);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped_backoff: usize,
    pub finished_at: String,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Collector(#[from] CollectorError),
    #[error(transparent)]
    Preferences(#[from] PreferencesError),
    #[error(transparent)]
    LocationSearch(#[from] LocationSearchError),
    #[error(transparent)]
    Prediction(#[from] PredictionError),
    #[error("runtime configuration error: {0}")]
    Configuration(String),
    #[error("could not normalize collector data: {0}")]
    Normalization(String),
    #[error("invalid timestamp: {0}")]
    Time(String),
    #[error("runtime is shutting down")]
    ShuttingDown,
}

trait Clock: Send + Sync {
    fn now_millis(&self) -> i64;
}

#[derive(Debug)]
struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> i64 {
        Timestamp::now().as_millisecond()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SourceState {
    channel_id: String,
    #[serde(default)]
    items: Vec<CollectorItem>,
    #[serde(default)]
    cursor: CollectorCursor,
    reported_health: HealthState,
    health_message: Option<String>,
    last_attempt_ms: Option<i64>,
    last_success_ms: Option<i64>,
    next_eligible_ms: Option<i64>,
    failure_count: u32,
    last_error: Option<String>,
    #[serde(default)]
    fail_closed_on_error: bool,
    #[serde(default)]
    bridge_transitions: Vec<BridgeStateTransition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct BridgeStateTransition {
    bridge_key: String,
    bridge_name: String,
    relation: String,
    from_state: String,
    to_state: String,
    occurred_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DetectedOutboundProgress {
    bridge_key: String,
    bridge_name: String,
    stage: OutboundProgressStage,
    occurred_at_ms: i64,
}

impl SourceState {
    fn empty(channel_id: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            items: Vec::new(),
            cursor: CollectorCursor::default(),
            reported_health: HealthState::Unknown,
            health_message: None,
            last_attempt_ms: None,
            last_success_ms: None,
            next_eligible_ms: None,
            failure_count: 0,
            last_error: None,
            fail_closed_on_error: false,
            bridge_transitions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedRuntimeState {
    sources: BTreeMap<String, SourceState>,
    last_cycle_ms: Option<i64>,
    previous_prediction: Option<BridgePrediction>,
    #[serde(skip)]
    active_sources: BTreeMap<String, String>,
}

/// Two or more bridges reporting `unknown` in the same pass is the signature of
/// a failed FL511 fetch rather than anything happening on the river: a bascule
/// cannot become "unknown", only our view of it can. Recording those readings
/// splits genuine intervals in two and inflates the opening count, which is
/// exactly the noise that makes upstream spans look like bad predictors.
const CORRELATED_UNKNOWN_THRESHOLD: usize = 2;

pub struct RuntimeEngine {
    store: Store,
    /// Identifies this engine run in durable observations, so a restart is
    /// distinguishable after the fact from a real state change.
    session_id: String,
    config: RuntimeConfig,
    factory: Arc<dyn CollectorFactory>,
    clock: Arc<dyn Clock>,
    preferences: RwLock<AppPreferences>,
    state: Mutex<PersistedRuntimeState>,
    /// Serializes the tiny in-memory publication step without blocking reads
    /// while collectors or SQLite transactions are in flight.
    publication: RwLock<()>,
    /// Serializes durable preference/secret mutations and the short collector
    /// result commit, but is never held during collector network I/O.
    mutation: Mutex<()>,
    /// Prevents overlapping scheduled and user-requested refresh cycles.
    refresh: Mutex<()>,
    configuration_revision: AtomicU64,
    predictor: BridgePredictor,
    location_search: LocationSearchService,
    sqlite_version: String,
    shutdown: CancellationToken,
}

impl RuntimeEngine {
    pub async fn new(store: Store, config: RuntimeConfig) -> Result<Self, RuntimeError> {
        let factory = Arc::new(CredentialFreeCollectorFactory::new(
            config.user_agent.clone(),
        )?);
        Self::initialize(store, config, factory, Arc::new(SystemClock)).await
    }

    pub async fn with_factory(
        store: Store,
        config: RuntimeConfig,
        factory: Arc<dyn CollectorFactory>,
    ) -> Result<Self, RuntimeError> {
        Self::initialize(store, config, factory, Arc::new(SystemClock)).await
    }

    async fn initialize(
        store: Store,
        config: RuntimeConfig,
        factory: Arc<dyn CollectorFactory>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, RuntimeError> {
        config.validate()?;
        let now_ms = clock.now_millis();
        let stored_preferences = store.get_json::<AppPreferences>(PREFERENCES_KEY).await?;
        let mut preferences = stored_preferences.clone().unwrap_or_default();
        let secret_status_changed = if let Some(configured) = factory.aisstream_key_configured()? {
            let changed = preferences.ais.api_key_configured != configured;
            preferences.ais.api_key_configured = configured;
            changed
        } else {
            false
        };
        validate_preferences(&preferences)?;
        if stored_preferences.is_none() || secret_status_changed {
            store
                .set_json(PREFERENCES_KEY, &preferences, &iso_timestamp(now_ms)?)
                .await?;
        }

        let stored_state = store.get_json::<Value>(LIVE_STATE_KEY).await?;
        let mut state: PersistedRuntimeState = stored_state
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        let registrations = factory.build(&preferences)?;
        state.active_sources = active_source_map(&registrations)?;

        let sqlite_version = store.sqlite_version().await?;
        let location_search = LocationSearchService::new(&config.user_agent)?;
        Ok(Self {
            store,
            // A v7 id sorts by creation time, so ordering rows by session_id
            // matches the order the runs actually happened in.
            session_id: uuid::Uuid::now_v7().to_string(),
            config,
            factory,
            clock,
            preferences: RwLock::new(preferences),
            state: Mutex::new(state),
            publication: RwLock::new(()),
            mutation: Mutex::new(()),
            refresh: Mutex::new(()),
            configuration_revision: AtomicU64::new(0),
            predictor: BridgePredictor::default(),
            location_search,
            sqlite_version,
            shutdown: CancellationToken::new(),
        })
    }

    pub async fn get_preferences(&self) -> AppPreferences {
        self.preferences.read().await.clone()
    }

    /// Returns secret-free AISStream health from the AIS source itself, not a
    /// channel aggregate that could be made healthy by FL511.
    pub async fn get_aisstream_status(&self) -> Result<AisStreamStatusDto, RuntimeError> {
        let now_ms = self.clock.now_millis();
        let (preferences, state) = {
            let _publication = self.publication.read().await;
            (
                self.preferences.read().await.clone(),
                self.state.lock().await.clone(),
            )
        };
        aisstream_status(&preferences, &state, now_ms)
    }

    /// Performs an explicit, bounded location lookup for the area editor. This
    /// service is not invoked by the background scheduler.
    pub async fn search_locations(
        &self,
        query: &str,
    ) -> Result<Vec<LocationSearchResult>, RuntimeError> {
        Ok(self.location_search.search(query).await?)
    }

    pub async fn save_preferences(
        &self,
        preferences: AppPreferences,
    ) -> Result<MutationResult, RuntimeError> {
        let _mutation = self.mutation.lock().await;
        let mut preferences = preferences;
        if let Some(configured) = self.factory.aisstream_key_configured()? {
            preferences.ais.api_key_configured = configured;
        }
        validate_preferences(&preferences)?;
        let registrations = self.factory.build(&preferences)?;
        let active_sources = active_source_map(&registrations)?;
        let old = self.preferences.read().await.clone();
        let changed_channels = changed_channel_ids(&old, &preferences);
        let ais_configuration_changed = old.ais != preferences.ais;
        let now_ms = self.clock.now_millis();
        let mut next_state = self.state.lock().await.clone();
        next_state.sources.retain(|source_id, source| {
            !changed_channels.contains(&source.channel_id)
                && !(ais_configuration_changed && source_id.starts_with("aisstream."))
        });
        let bridge_changed = old
            .profile
            .channels
            .iter()
            .chain(&preferences.profile.channels)
            .any(|channel| {
                channel.kind == ChannelKindDto::Bridge && changed_channels.contains(&channel.id)
            });
        if bridge_changed || ais_configuration_changed {
            next_state.previous_prediction = None;
        }
        next_state.active_sources = active_sources;
        let updated_at = iso_timestamp(now_ms)?;
        let mut transaction = self.store.begin_transaction().await?;
        transaction
            .set_json(PREFERENCES_KEY, &preferences, &updated_at)
            .await?;
        transaction
            .set_json(LIVE_STATE_KEY, &next_state, &updated_at)
            .await?;
        transaction.commit().await?;

        // SQLite is now the source of truth. Publish the corresponding pair
        // behind one short guard so snapshots cannot mix revisions.
        let _publication = self.publication.write().await;
        *self.preferences.write().await = preferences;
        *self.state.lock().await = next_state;
        self.configuration_revision.fetch_add(1, Ordering::Release);
        Ok(MutationResult {
            ok: true,
            message: "Preferences saved. Changed collectors will restart on the next cycle.".into(),
        })
    }

    /// Replaces or clears the host-owned AISStream secret without restarting
    /// the runtime. Only the presence flag crosses the preferences boundary.
    pub async fn set_aisstream_key(
        &self,
        key: Option<String>,
    ) -> Result<MutationResult, RuntimeError> {
        let _mutation = self.mutation.lock().await;
        let configured = key.is_some();
        // The factory change stays reversible until the SQLite pair is
        // durable. Any `?` below drops this guard and restores the previous
        // secret plus its still-running collectors.
        let secret_change = self.factory.begin_aisstream_key_change(key)?;

        let mut preferences = self.preferences.read().await.clone();
        preferences.ais.api_key_configured = configured;
        validate_preferences(&preferences)?;
        let registrations = self.factory.build(&preferences)?;
        let active_sources = active_source_map(&registrations)?;
        let now_ms = self.clock.now_millis();
        let mut next_state = self.state.lock().await.clone();
        next_state
            .sources
            .retain(|source_id, _| !source_id.starts_with("aisstream."));
        next_state.previous_prediction = None;
        next_state.active_sources = active_sources;

        let updated_at = iso_timestamp(now_ms)?;
        let mut transaction = self.store.begin_transaction().await?;
        transaction
            .set_json(PREFERENCES_KEY, &preferences, &updated_at)
            .await?;
        transaction
            .set_json(LIVE_STATE_KEY, &next_state, &updated_at)
            .await?;
        transaction.commit().await?;

        // Committing the guard retires the previous provider session. From
        // this point the durable state and collector factory agree.
        secret_change.commit();

        let _publication = self.publication.write().await;
        *self.preferences.write().await = preferences;
        *self.state.lock().await = next_state;
        self.configuration_revision.fetch_add(1, Ordering::Release);
        Ok(MutationResult {
            ok: true,
            message: if configured {
                "AISStream key saved; the live adapter will connect on the next refresh.".into()
            } else {
                "AISStream key removed; live vessel collection stopped.".into()
            },
        })
    }

    pub async fn refresh_all(&self) -> Result<RefreshReport, RuntimeError> {
        let _refresh = self.refresh.lock().await;
        self.refresh_cycle(true).await
    }

    pub async fn refresh_sources(&self) -> Result<MutationResult, RuntimeError> {
        let report = self.refresh_all().await?;
        Ok(MutationResult {
            ok: report.failed == 0,
            message: format!(
                "Source refresh finished: {} succeeded, {} failed, {} skipped by backoff.",
                report.succeeded, report.failed, report.skipped_backoff
            ),
        })
    }

    async fn refresh_due(&self) -> Result<RefreshReport, RuntimeError> {
        let _refresh = self.refresh.lock().await;
        self.refresh_cycle(false).await
    }

    async fn refresh_cycle(&self, force: bool) -> Result<RefreshReport, RuntimeError> {
        if self.shutdown.is_cancelled() {
            return Err(RuntimeError::ShuttingDown);
        }
        let configuration_revision = self.configuration_revision.load(Ordering::Acquire);
        let now_ms = self.clock.now_millis();
        let preferences = self.preferences.read().await.clone();
        let registrations = self.factory.build(&preferences)?;
        let active_sources = active_source_map(&registrations)?;
        let mut next_state = self.state.lock().await.clone();
        next_state.active_sources = active_sources;
        let mut due = Vec::new();
        let mut skipped_backoff = 0;
        for registration in registrations {
            let source = next_state
                .sources
                .entry(registration.id.clone())
                .or_insert_with(|| SourceState::empty(&registration.channel_id));
            source.channel_id.clone_from(&registration.channel_id);
            source.fail_closed_on_error = registration.fail_closed_on_error;
            let eligible = force
                || source
                    .next_eligible_ms
                    .is_none_or(|eligible| eligible <= now_ms);
            if eligible {
                due.push((
                    registration,
                    CollectContext {
                        cursor: Some(source.cursor.clone()),
                    },
                ));
            } else {
                skipped_backoff += 1;
            }
        }

        let collector_timeout = self.config.collector_timeout;
        let shutdown = self.shutdown.clone();
        let executions = stream::iter(due.into_iter().map(|(registration, context)| {
            let shutdown = shutdown.clone();
            async move {
                let collector = Arc::clone(&registration.collector);
                let task = tokio::spawn(async move {
                    tokio::select! {
                        () = shutdown.cancelled() => CollectionOutcome::Cancelled,
                        result = timeout(collector_timeout, collector.collect(&context)) => {
                            match result {
                                Ok(Ok(batch)) => CollectionOutcome::Batch(batch),
                                Ok(Err(error)) => CollectionOutcome::Failed(error.to_string()),
                                Err(_) => CollectionOutcome::Failed(format!(
                                    "collector exceeded its {:?} runtime deadline",
                                    collector_timeout
                                )),
                            }
                        }
                    }
                });
                let outcome = match task.await {
                    Ok(outcome) => outcome,
                    Err(error) => CollectionOutcome::Failed(format!(
                        "collector task stopped unexpectedly: {error}"
                    )),
                };
                (registration, outcome)
            }
        }))
        .buffer_unordered(self.config.max_concurrent_collectors)
        .collect::<Vec<_>>()
        .await;

        if executions
            .iter()
            .any(|(_, outcome)| matches!(outcome, CollectionOutcome::Cancelled))
        {
            return Err(RuntimeError::ShuttingDown);
        }

        // A settings or credential save is allowed to complete while network
        // requests are in flight. Only publish results collected against the
        // same configuration snapshot; a superseded cycle is safely ignored
        // and the scheduler will collect the new configuration next.
        let _mutation = self.mutation.lock().await;
        if configuration_revision != self.configuration_revision.load(Ordering::Acquire) {
            let succeeded = executions
                .iter()
                .filter(|(_, outcome)| matches!(outcome, CollectionOutcome::Batch(_)))
                .count();
            let failed = executions.len().saturating_sub(succeeded);
            return Ok(RefreshReport {
                attempted: executions.len(),
                succeeded,
                failed,
                skipped_backoff,
                finished_at: iso_timestamp(now_ms)?,
            });
        }

        let mut succeeded = 0;
        let mut failed = 0;
        for (registration, outcome) in executions {
            let source = next_state
                .sources
                .entry(registration.id.clone())
                .or_insert_with(|| SourceState::empty(&registration.channel_id));
            source.channel_id.clone_from(&registration.channel_id);
            source.fail_closed_on_error = registration.fail_closed_on_error;
            source.last_attempt_ms = Some(now_ms);
            match outcome {
                CollectionOutcome::Batch(batch) => {
                    succeeded += 1;
                    source.cursor = batch.cursor;
                    source.reported_health = batch.health.state;
                    source.health_message = batch.health.message;
                    source.last_success_ms = Some(now_ms);
                    source.next_eligible_ms =
                        (!registration.minimum_interval.is_zero()).then(|| {
                            now_ms.saturating_add(duration_millis(registration.minimum_interval))
                        });
                    source.failure_count = 0;
                    source.last_error = None;
                    if !batch.not_modified {
                        if registration.id.starts_with("fl511.") {
                            update_bridge_transitions(
                                &source.items,
                                &batch.items,
                                now_ms,
                                &mut source.bridge_transitions,
                            );
                        }
                        source.items = batch.items;
                    }
                }
                CollectionOutcome::Failed(error) => {
                    failed += 1;
                    source.failure_count = source.failure_count.saturating_add(1);
                    source.next_eligible_ms = Some(now_ms.saturating_add(duration_millis(
                        backoff_for(&self.config, source.failure_count),
                    )));
                    source.last_error = Some(error);
                }
                CollectionOutcome::Cancelled => unreachable!("handled before state update"),
            }
        }
        next_state.last_cycle_ms = Some(now_ms);
        let (current_evidence, _) = bridge_evidence(&next_state, &preferences, now_ms)?;
        let prediction = self.predictor.evaluate(
            TimestampMillis(now_ms),
            &current_evidence,
            next_state.previous_prediction.as_ref(),
        )?;
        next_state.previous_prediction = Some(prediction);

        {
            let _publication = self.publication.write().await;
            *self.state.lock().await = next_state.clone();
        }
        self.persist_refresh(&next_state, now_ms).await?;
        Ok(RefreshReport {
            attempted: succeeded + failed,
            succeeded,
            failed,
            skipped_backoff,
            finished_at: iso_timestamp(now_ms)?,
        })
    }

    pub async fn get_snapshot(&self) -> Result<AppSnapshot, RuntimeError> {
        let now_ms = self.clock.now_millis();
        let (preferences, state) = {
            let _publication = self.publication.read().await;
            (
                self.preferences.read().await.clone(),
                self.state.lock().await.clone(),
            )
        };
        let mut snapshot = self.build_live_snapshot(&preferences, &state, now_ms)?;
        snapshot.bridge_intervals = self
            .store
            .list_recent_bridge_state_intervals(200)
            .await?
            .into_iter()
            .map(bridge_interval_dto)
            .collect::<Result<Vec<_>, _>>()?;
        snapshot.system.database_size_bytes = self.store.database_size_bytes().await?;
        Ok(snapshot)
    }

    pub fn spawn_scheduler(self: &Arc<Self>) -> SchedulerHandle {
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let engine = Arc::clone(self);
        let join = tokio::spawn(async move {
            let mut interval = tokio::time::interval(engine.config.poll_interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    () = task_cancellation.cancelled() => break,
                    () = engine.shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        match engine.refresh_due().await {
                            Ok(report) => debug!(?report, "collector cycle completed"),
                            Err(RuntimeError::ShuttingDown) => break,
                            Err(error) => warn!(%error, "collector cycle failed before completion"),
                        }
                    }
                }
            }
        });
        SchedulerHandle {
            cancellation,
            join: Some(join),
        }
    }

    pub fn cancel(&self) {
        self.factory.cancel();
        self.shutdown.cancel();
    }

    async fn persist_refresh(
        &self,
        state: &PersistedRuntimeState,
        now_ms: i64,
    ) -> Result<(), RuntimeError> {
        let updated_at = iso_timestamp(now_ms)?;
        let mut transaction = self.store.begin_transaction().await?;
        for source_id in state
            .active_sources
            .keys()
            .filter(|source_id| source_id.starts_with("fl511."))
        {
            let Some(source) = state.sources.get(source_id) else {
                continue;
            };
            let correlated_unknown = source
                .items
                .iter()
                .filter(|item| item.kind == ItemKind::Bridge)
                .filter(|item| {
                    item.attributes.get("state").and_then(Value::as_str) == Some("unknown")
                })
                .count()
                >= CORRELATED_UNKNOWN_THRESHOLD;
            for item in &source.items {
                if item.kind != ItemKind::Bridge {
                    continue;
                }
                let Some(bridge_key) = item.attributes.get("selector_key").and_then(Value::as_str)
                else {
                    continue;
                };
                let Some(relation @ ("target" | "upstream")) =
                    item.attributes.get("relation").and_then(Value::as_str)
                else {
                    continue;
                };
                let Some(state @ ("up" | "down" | "unknown")) =
                    item.attributes.get("state").and_then(Value::as_str)
                else {
                    continue;
                };
                // Hold the previous state rather than recording an acquisition
                // fault as an observation. A single unresolved bridge is still
                // written: that is durable evidence about one span, not a
                // correlated failure across the fetch.
                if state == "unknown" && correlated_unknown {
                    continue;
                }
                transaction
                    .record_bridge_state(tenders_storage::BridgeObservation {
                        source_id,
                        bridge_key,
                        bridge_name: &item.title,
                        relation,
                        state,
                        observed_at_ms: now_ms,
                        session_id: &self.session_id,
                    })
                    .await?;
            }
        }
        transaction
            .set_json(LIVE_STATE_KEY, state, &updated_at)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    fn build_live_snapshot(
        &self,
        preferences: &AppPreferences,
        state: &PersistedRuntimeState,
        now_ms: i64,
    ) -> Result<AppSnapshot, RuntimeError> {
        let (bridge_evidence, bridge_views) = bridge_evidence(state, preferences, now_ms)?;
        let prediction = self.predictor.evaluate(
            TimestampMillis(now_ms),
            &bridge_evidence,
            state.previous_prediction.as_ref(),
        )?;
        let decision = decision_snapshot(&prediction, &preferences.profile.quiet_hours.time_zone)?;
        let evidence =
            evidence_snapshots(&prediction, bridge_views, now_ms, preferences.unit_system)?;
        let channels = channel_snapshots(preferences, state, &decision, now_ms);
        let outputs = output_snapshots(preferences);
        let sources = source_health(preferences, state, now_ms)?;
        let collectors_total = sources.len();
        let collectors_online = sources
            .iter()
            .filter(|source| source.availability == AvailabilityDto::Fresh)
            .count();
        let system_status = if collectors_total == 0 || collectors_online == 0 {
            SystemStatusDto::Offline
        } else if collectors_online == collectors_total {
            SystemStatusDto::Nominal
        } else {
            SystemStatusDto::Degraded
        };
        let vessel_tracks = vessel_tracks(state, now_ms);
        let generated_at = iso_timestamp(now_ms)?;
        Ok(AppSnapshot {
            generated_at: generated_at.clone(),
            local_time_zone: preferences.profile.quiet_hours.time_zone.clone(),
            decision,
            evidence,
            channels,
            outputs,
            dispatches: Vec::new(),
            bridge_intervals: Vec::new(),
            vessel_tracks,
            system: SystemHealth {
                status: system_status,
                sqlite_version: self.sqlite_version.clone(),
                database_size_bytes: 0,
                engine_version: env!("CARGO_PKG_VERSION").into(),
                last_cycle_at: state
                    .last_cycle_ms
                    .map(iso_timestamp)
                    .transpose()?
                    .unwrap_or(generated_at),
                collectors_online,
                collectors_total,
                sources,
            },
        })
    }
}

enum CollectionOutcome {
    Batch(CollectorBatch),
    Failed(String),
    Cancelled,
}

pub struct SchedulerHandle {
    cancellation: CancellationToken,
    join: Option<JoinHandle<()>>,
}

impl SchedulerHandle {
    pub async fn stop(mut self) {
        self.cancellation.cancel();
        if let Some(join) = self.join.take()
            && let Err(error) = join.await
        {
            warn!(%error, "collector scheduler stopped unexpectedly");
        }
    }
}

impl Drop for SchedulerHandle {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

fn active_source_map(
    registrations: &[CollectorRegistration],
) -> Result<BTreeMap<String, String>, RuntimeError> {
    let mut active = BTreeMap::new();
    for registration in registrations {
        if registration.id.trim().is_empty() || registration.channel_id.trim().is_empty() {
            return Err(RuntimeError::Configuration(
                "collector registration ids cannot be empty".into(),
            ));
        }
        if active
            .insert(registration.id.clone(), registration.channel_id.clone())
            .is_some()
        {
            return Err(RuntimeError::Configuration(format!(
                "duplicate collector registration {:?}",
                registration.id
            )));
        }
    }
    Ok(active)
}

fn changed_channel_ids(old: &AppPreferences, new: &AppPreferences) -> BTreeSet<String> {
    let old_channels = old
        .profile
        .channels
        .iter()
        .map(|channel| (channel.id.as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    let new_channels = new
        .profile
        .channels
        .iter()
        .map(|channel| (channel.id.as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    let mut changed = old_channels
        .keys()
        .chain(new_channels.keys())
        .filter(|id| old_channels.get(**id) != new_channels.get(**id))
        .map(|id| (*id).to_owned())
        .collect::<BTreeSet<_>>();

    let old_areas = old
        .areas
        .iter()
        .map(|area| (area.id.as_str(), area))
        .collect::<BTreeMap<_, _>>();
    let new_areas = new
        .areas
        .iter()
        .map(|area| (area.id.as_str(), area))
        .collect::<BTreeMap<_, _>>();
    let changed_areas = old_areas
        .keys()
        .chain(new_areas.keys())
        .filter(|id| old_areas.get(**id) != new_areas.get(**id))
        .copied()
        .collect::<BTreeSet<_>>();
    if !changed_areas.is_empty() {
        for channel in old
            .profile
            .channels
            .iter()
            .chain(&new.profile.channels)
            .filter(|channel| {
                matches!(
                    channel.kind,
                    ChannelKindDto::Weather | ChannelKindDto::Official | ChannelKindDto::Hurricane
                )
            })
        {
            let selected = channel.scope.get("areaIds").and_then(Value::as_array);
            if selected.is_none_or(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .any(|area_id| changed_areas.contains(area_id))
            }) {
                changed.insert(channel.id.clone());
            }
        }
    }
    changed
}

fn backoff_for(config: &RuntimeConfig, failures: u32) -> Duration {
    let exponent = failures.saturating_sub(1).min(20);
    config
        .backoff_initial
        .saturating_mul(1_u32 << exponent)
        .min(config.backoff_max)
}

fn duration_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn iso_timestamp(milliseconds: i64) -> Result<String, RuntimeError> {
    Timestamp::from_millisecond(milliseconds)
        .map(|timestamp| timestamp.to_string())
        .map_err(|error| RuntimeError::Time(error.to_string()))
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn update_bridge_transitions(
    previous: &[CollectorItem],
    current: &[CollectorItem],
    now_ms: i64,
    transitions: &mut Vec<BridgeStateTransition>,
) {
    const RETENTION_MS: i64 = 45 * 60 * 1_000;
    const MAX_TRANSITIONS: usize = 64;

    for item in current.iter().filter(|item| item.kind == ItemKind::Bridge) {
        let Some(bridge_key) = item.attributes.get("selector_key").and_then(Value::as_str) else {
            continue;
        };
        let Some(relation @ ("target" | "upstream")) =
            item.attributes.get("relation").and_then(Value::as_str)
        else {
            continue;
        };
        let Some(to_state @ ("up" | "down" | "unknown")) =
            item.attributes.get("state").and_then(Value::as_str)
        else {
            continue;
        };
        let Some(previous_item) = previous.iter().find(|candidate| {
            candidate
                .attributes
                .get("selector_key")
                .and_then(Value::as_str)
                == Some(bridge_key)
        }) else {
            continue;
        };
        let Some(from_state @ ("up" | "down" | "unknown")) = previous_item
            .attributes
            .get("state")
            .and_then(Value::as_str)
        else {
            continue;
        };
        if from_state == to_state {
            continue;
        }
        transitions.push(BridgeStateTransition {
            bridge_key: bridge_key.into(),
            bridge_name: item.title.clone(),
            relation: relation.into(),
            from_state: from_state.into(),
            to_state: to_state.into(),
            occurred_at_ms: now_ms,
        });
    }

    let cutoff = now_ms.saturating_sub(RETENTION_MS);
    transitions.retain(|transition| transition.occurred_at_ms >= cutoff);
    if transitions.len() > MAX_TRANSITIONS {
        transitions.drain(..transitions.len() - MAX_TRANSITIONS);
    }
}

/// Metres from each upstream bascule down to the Brickell Avenue Bridge, taken
/// from the FL511 coordinates the selectors use.
///
/// An upstream opening only means something once it is turned into "when could
/// that vessel be here", and that needs a distance.
fn upstream_distance_meters(bridge_key: &str) -> Option<f64> {
    match bridge_key {
        "sw_2_ave" => Some(759.0),
        "sw_1_st" => Some(1_112.0),
        "w_flagler" => Some(1_223.0),
        "nw_5_st" => Some(1_932.0),
        "nw_12_ave" => Some(2_845.0),
        "nw_17_ave" => Some(3_744.0),
        "nw_22_ave" => Some(4_611.0),
        "nw_27_ave" => Some(5_574.0),
        _ => None,
    }
}

/// Miami River transit speeds. Tug-assisted commercial traffic works the low
/// end; the yachts and sailboats behind most openings work the high end. The
/// span between them is why this is a window and not a point.
const RIVER_SPEED_SLOW_KNOTS: f64 = 3.0;
const RIVER_SPEED_FAST_KNOTS: f64 = 6.0;
const METRES_PER_KNOT_MINUTE: f64 = 1_852.0 / 60.0;

/// When a vessel that just cleared `bridge_key` could reach Brickell.
///
/// The distance is real and the speed range is observed, so this is a far
/// better statement about an upstream opening than its age: a vessel that
/// passed SW 2 Ave eight minutes ago is *more* likely to be at Brickell now
/// than it was one minute after passing, not less.
fn outbound_eta(bridge_key: &str) -> Option<EtaRangeMinutes> {
    let metres = upstream_distance_meters(bridge_key)?;
    let fastest = metres / (RIVER_SPEED_FAST_KNOTS * METRES_PER_KNOT_MINUTE);
    let slowest = metres / (RIVER_SPEED_SLOW_KNOTS * METRES_PER_KNOT_MINUTE);
    Some(EtaRangeMinutes::new(
        fastest.floor().max(0.0) as u16,
        slowest.ceil().max(1.0) as u16,
    ))
}

/// Position in the river, counting upstream from the target.
///
/// Outbound detection reads a *decreasing* rank over time as a vessel working
/// downriver toward Brickell, so this ordering is load-bearing: get it wrong and
/// an outbound convoy reads as inbound. Ranks follow the FL511 longitudes, which
/// run monotonically west as the river climbs.
///
/// South Miami Avenue would sit between Brickell and SW 2 Ave, but FL511 does
/// not publish it, so the sequence steps over an opening we never observe.
fn upstream_rank(bridge_key: &str) -> Option<u8> {
    match bridge_key {
        "sw_2_ave" => Some(1),
        "sw_1_st" => Some(2),
        "w_flagler" => Some(3),
        "nw_5_st" => Some(4),
        "nw_12_ave" => Some(5),
        "nw_17_ave" => Some(6),
        "nw_22_ave" => Some(7),
        "nw_27_ave" => Some(8),
        _ => None,
    }
}

fn detect_outbound_progress(
    transitions: &[BridgeStateTransition],
    now_ms: i64,
) -> Option<DetectedOutboundProgress> {
    const WINDOW_MS: i64 = 30 * 60 * 1_000;
    let cutoff = now_ms.saturating_sub(WINDOW_MS);
    let openings = transitions
        .iter()
        .filter(|transition| {
            transition.relation == "upstream"
                && transition.from_state == "down"
                && transition.to_state == "up"
                && transition.occurred_at_ms >= cutoff
                && transition.occurred_at_ms <= now_ms
        })
        .filter_map(|transition| {
            upstream_rank(&transition.bridge_key).map(|rank| (transition, rank))
        })
        .collect::<Vec<_>>();

    let (earlier, later) = openings.iter().enumerate().find_map(|(index, earlier)| {
        openings[index + 1..]
            .iter()
            .rev()
            .find(|later| {
                earlier.1 > later.1
                    && earlier.0.occurred_at_ms < later.0.occurred_at_ms
                    && later.0.occurred_at_ms - earlier.0.occurred_at_ms <= WINDOW_MS
            })
            .map(|later| (*earlier, *later))
    })?;

    let target_opened_during_progress = transitions.iter().any(|transition| {
        transition.relation == "target"
            && transition.from_state == "down"
            && transition.to_state == "up"
            && transition.occurred_at_ms >= earlier.0.occurred_at_ms.saturating_sub(WINDOW_MS)
            && transition.occurred_at_ms <= now_ms
    });
    if target_opened_during_progress {
        return None;
    }

    Some(DetectedOutboundProgress {
        bridge_key: later.0.bridge_key.clone(),
        bridge_name: later.0.bridge_name.clone(),
        stage: if later.1 == 1 {
            OutboundProgressStage::VeryHigh
        } else {
            OutboundProgressStage::High
        },
        occurred_at_ms: later.0.occurred_at_ms,
    })
}

fn normalized_bridge_observation(
    item: &CollectorItem,
    channel_id: &str,
    source_id: &str,
    received_ms: i64,
    availability: Availability,
) -> Option<Observation> {
    let observed_ms = item
        .observed_at
        .as_ref()
        .or(item.starts_at.as_ref())
        .map_or(received_ms, |time| time.timestamp_millis());
    let data = bridge_fact(item)?;
    Some(Observation {
        id: ObservationId(format!("{source_id}:{}", item.id)),
        channel_id: ChannelId(channel_id.into()),
        source_id: SourceId(source_id.into()),
        observed_at: TimestampMillis(observed_ms),
        received_at: TimestampMillis(received_ms),
        expires_at: item
            .ends_at
            .as_ref()
            .map(|time| TimestampMillis(time.timestamp_millis())),
        availability,
        data,
    })
}

fn bridge_fact(item: &CollectorItem) -> Option<BridgeObservation> {
    if item.kind != ItemKind::Bridge {
        return None;
    }
    let relation = item.attributes.get("relation")?.as_str()?;
    let state = item.attributes.get("state")?.as_str()?;
    match relation {
        "target" => Some(BridgeObservation::Controller {
            state: match state {
                "up" => BridgeControllerState::Open,
                "down" => BridgeControllerState::Closed,
                _ => BridgeControllerState::Unknown,
            },
        }),
        "upstream" => None,
        "ais" => {
            let movement = match item.attributes.get("movement")?.as_str()? {
                "approaching" => VesselMovement::Approaching,
                "diverging" => VesselMovement::Diverging,
                "stationary" => VesselMovement::Stationary,
                "unknown" => VesselMovement::Unknown,
                _ => return None,
            };
            let eta = match (
                item.attributes
                    .get("eta_min_minutes")
                    .and_then(Value::as_u64),
                item.attributes
                    .get("eta_max_minutes")
                    .and_then(Value::as_u64),
            ) {
                (Some(earliest), Some(latest)) => Some(EtaRangeMinutes::new(
                    u16::try_from(earliest).ok()?,
                    u16::try_from(latest).ok()?,
                )),
                (None, None) => None,
                _ => return None,
            };
            Some(BridgeObservation::AisTrack {
                mmsi: item
                    .attributes
                    .get("mmsi")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                vessel_name: item
                    .attributes
                    .get("vessel_name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                movement,
                route_intersects: item.attributes.get("route_intersects")?.as_bool()?,
                eta,
                opening_propensity: None,
            })
        }
        _ => None,
    }
}

#[derive(Clone)]
struct BridgeEvidenceView {
    evidence: Option<BridgeEvidence>,
    item: CollectorItem,
    source_id: String,
    observed_ms: i64,
    availability: AvailabilityDto,
}

fn bridge_evidence(
    state: &PersistedRuntimeState,
    preferences: &AppPreferences,
    now_ms: i64,
) -> Result<(Vec<BridgeEvidence>, Vec<BridgeEvidenceView>), RuntimeError> {
    let Some(channel) = preferences
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::Bridge)
    else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut evidence = Vec::new();
    let mut views = Vec::new();
    for (source_id, source_channel) in &state.active_sources {
        if source_channel != &channel.id
            || !(source_id.starts_with("fl511.") || source_id.starts_with("aisstream."))
        {
            continue;
        }
        let Some(source) = state.sources.get(source_id) else {
            continue;
        };
        let outbound = detect_outbound_progress(&source.bridge_transitions, now_ms);
        let (availability, _) = source_availability(source, channel, now_ms);
        for item in &source.items {
            if item.kind != ItemKind::Bridge {
                continue;
            }
            let observed_ms = item.observed_at.as_ref().map_or_else(
                || source.last_success_ms.unwrap_or(now_ms),
                |time| time.timestamp_millis(),
            );
            let item_availability = if !bridge_item_is_current(item, channel, observed_ms, now_ms)
                && matches!(
                    availability,
                    AvailabilityDto::Fresh | AvailabilityDto::Delayed
                ) {
                AvailabilityDto::Stale
            } else {
                availability
            };
            let model_availability = Availability {
                status: model_status(item_availability),
                checked_at: TimestampMillis(now_ms),
                last_success_at: source.last_success_ms.map(TimestampMillis),
                detail: source
                    .last_error
                    .clone()
                    .or_else(|| source.health_message.clone()),
            };
            let relation = item
                .attributes
                .get("relation")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let observation = if relation == "upstream" {
                let selector_key = item.attributes.get("selector_key").and_then(Value::as_str);
                outbound
                    .as_ref()
                    .filter(|progress| selector_key == Some(progress.bridge_key.as_str()))
                    .map(|progress| Observation {
                        id: ObservationId(format!(
                            "{source_id}:outbound:{}",
                            progress.occurred_at_ms
                        )),
                        channel_id: ChannelId(channel.id.clone()),
                        source_id: SourceId(source_id.clone()),
                        observed_at: TimestampMillis(progress.occurred_at_ms),
                        received_at: TimestampMillis(now_ms),
                        expires_at: None,
                        availability: model_availability.clone(),
                        data: BridgeObservation::OutboundProgress {
                            bridge: progress.bridge_name.clone(),
                            stage: progress.stage,
                            eta: outbound_eta(&progress.bridge_key),
                        },
                    })
            } else {
                normalized_bridge_observation(
                    item,
                    &channel.id,
                    source_id,
                    observed_ms,
                    model_availability.clone(),
                )
            };
            let normalized = observation.map(|observation| {
                BridgeEvidence::from_observation(
                    &observation,
                    match relation {
                        "target" => Confidence::from_basis_points(9_900),
                        "ais" => Confidence::from_basis_points(8_500),
                        "upstream" => Confidence::CERTAIN,
                        _ => Confidence::ZERO,
                    },
                )
            });
            let view_observed_ms = normalized
                .as_ref()
                .map_or(observed_ms, |item| item.observed_at.0);
            evidence.extend(normalized.clone());
            views.push(BridgeEvidenceView {
                evidence: normalized,
                item: item.clone(),
                source_id: source_id.clone(),
                observed_ms: view_observed_ms,
                availability: item_availability,
            });
        }
    }
    Ok((evidence, views))
}

fn bridge_item_is_current(
    item: &CollectorItem,
    channel: &ChannelPreference,
    observed_ms: i64,
    now_ms: i64,
) -> bool {
    // Provider timestamps slightly ahead of the local clock are tolerated,
    // but a malformed/persisted far-future timestamp never becomes evidence.
    if observed_ms > now_ms.saturating_add(30_000) {
        return false;
    }
    if TimestampMillis(observed_ms).age_seconds_at(TimestampMillis(now_ms))
        > u64::from(channel.max_age_minutes) * 60
    {
        return false;
    }
    !item
        .ends_at
        .as_ref()
        .is_some_and(|ends| ends.timestamp_millis() < now_ms)
}

fn decision_snapshot(
    prediction: &BridgePrediction,
    local_time_zone: &str,
) -> Result<DecisionSnapshot, RuntimeError> {
    let state = bridge_state_dto(prediction.state);
    let (state_label, meaning, action) = decision_copy(state);
    let confidence_basis = {
        let labels = prediction
            .contributions
            .iter()
            .filter(|item| {
                matches!(
                    item.disposition,
                    ContributionDisposition::Applied | ContributionDisposition::Authoritative
                )
            })
            .map(|item| item.label.as_str())
            .take(3)
            .collect::<Vec<_>>();
        if labels.is_empty() {
            Some("Legal schedule only; no predictive source observations.".into())
        } else {
            Some(labels.join(" · "))
        }
    };
    let source_age_seconds = prediction
        .availability
        .last_success_at
        .map_or(0, |timestamp| {
            timestamp.age_seconds_at(prediction.evaluated_at)
        });
    Ok(DecisionSnapshot {
        channel_id: "bridge.brickell".into(),
        subject: "Brickell Avenue".into(),
        state,
        state_label: state_label.into(),
        meaning: meaning.into(),
        action: action.into(),
        eta_min: prediction.eta.map(|eta| eta.earliest),
        eta_max: prediction.eta.map(|eta| eta.latest),
        confidence_bps: Some(prediction.confidence.basis_points),
        confidence_label: Some(confidence_label(prediction.confidence.basis_points).into()),
        confidence_basis,
        next_legal_slot: prediction
            .schedule
            .next_ordinary_opening_at
            .map(|timestamp| format_local_slot(timestamp, local_time_zone))
            .transpose()?,
        opening_allowed_now: prediction.schedule.ordinary_opening_allowed,
        availability: availability_dto(prediction.availability.status),
        source_age_seconds,
    })
}

fn evidence_snapshots(
    prediction: &BridgePrediction,
    views: Vec<BridgeEvidenceView>,
    now_ms: i64,
    unit_system: UnitSystem,
) -> Result<Vec<EvidenceStrip>, RuntimeError> {
    let contributions = prediction
        .contributions
        .iter()
        .filter_map(|contribution| {
            contribution
                .observation_id
                .as_ref()
                .map(|id| (id.as_str(), contribution))
        })
        .collect::<BTreeMap<_, _>>();
    let corroborated = prediction.contributions.iter().any(|item| {
        item.kind == EvidenceKind::Corroboration
            && item.disposition == ContributionDisposition::Applied
    });
    views
        .into_iter()
        .map(|view| {
            let contribution = view
                .evidence
                .as_ref()
                .and_then(|item| contributions.get(item.observation_id.as_str()).copied());
            let relation = view
                .item
                .attributes
                .get("relation")
                .and_then(Value::as_str)
                .unwrap_or("bridge");
            let state_text = view
                .item
                .attributes
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let detail = if relation == "ais" {
                ais_evidence_detail(&view.item, unit_system).unwrap_or_else(|| {
                    view.item
                        .summary
                        .clone()
                        .unwrap_or_else(|| "AIS vessel position received".into())
                })
            } else {
                view.item
                    .summary
                    .clone()
                    .unwrap_or_else(|| format!("{relation} bridge status is {state_text}"))
            };
            let evidence_state = match view.availability {
                AvailabilityDto::Stale | AvailabilityDto::Offline => EvidenceStateDto::Stale,
                AvailabilityDto::Delayed => EvidenceStateDto::Pending,
                AvailabilityDto::Fresh if relation == "upstream" && state_text != "up" => {
                    EvidenceStateDto::Pending
                }
                AvailabilityDto::Fresh => EvidenceStateDto::Live,
            };
            Ok(EvidenceStrip {
                id: view.evidence.as_ref().map_or_else(
                    || view.item.id.clone(),
                    |item| item.observation_id.0.clone(),
                ),
                channel_id: "bridge.brickell".into(),
                source_label: if view.source_id.starts_with("aisstream.") {
                    "AISStream".into()
                } else {
                    "Florida 511".into()
                },
                source_id: view.source_id,
                title: view.item.title,
                detail,
                observed_at: iso_timestamp(view.observed_ms)?,
                age_seconds: TimestampMillis(view.observed_ms)
                    .age_seconds_at(TimestampMillis(now_ms)),
                availability: view.availability,
                contribution_bps: contribution
                    .map(|item| (item.applied_score * 10_000.0).round() as i32),
                state: evidence_state,
                corroborated: corroborated.then_some(true),
                interrupt: (relation == "target" && state_text == "up").then_some(true),
            })
        })
        .collect()
}

fn ais_evidence_detail(item: &CollectorItem, unit_system: UnitSystem) -> Option<String> {
    let distance_meters = item.attributes.get("distance_meters")?.as_f64()?;
    let speed_knots = item.attributes.get("sog_knots")?.as_f64()?;
    let movement = item
        .attributes
        .get("movement")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if !distance_meters.is_finite() || !speed_knots.is_finite() {
        return None;
    }
    let (distance, speed) = match unit_system {
        UnitSystem::Imperial => {
            let miles = distance_meters / 1_609.344;
            let distance = if miles < 0.5 {
                format!("{:.0} ft", distance_meters * 3.280_84)
            } else {
                format!("{miles:.1} mi")
            };
            (distance, format!("{:.1} mph", speed_knots * 1.150_779))
        }
        UnitSystem::Metric => {
            let distance = if distance_meters < 1_000.0 {
                format!("{distance_meters:.0} m")
            } else {
                format!("{:.1} km", distance_meters / 1_000.0)
            };
            (distance, format!("{:.1} km/h", speed_knots * 1.852))
        }
    };
    let eta = match (
        item.attributes
            .get("eta_min_minutes")
            .and_then(Value::as_u64),
        item.attributes
            .get("eta_max_minutes")
            .and_then(Value::as_u64),
    ) {
        (Some(minimum), Some(maximum)) => format!("ETA {minimum}–{maximum} min"),
        _ => "ETA unavailable".into(),
    };
    Some(format!(
        "{distance} from target bridge · {movement} · {speed} · {eta}"
    ))
}

// Channel-specific activation, summaries, and snapshot presentation are kept
// together so scheduler and persistence work remain readable in this module.
include!("engine/channel_rules.rs");
fn source_label(kind: ChannelKindDto, _channel: &ChannelPreference) -> &'static str {
    match kind {
        ChannelKindDto::Bridge => "Configured live sources + confidence model",
        ChannelKindDto::Weather => "Open-Meteo",
        ChannelKindDto::Official => "National Weather Service",
        ChannelKindDto::Hurricane => "National Hurricane Center",
        ChannelKindDto::News => "Configured RSS/Atom",
        ChannelKindDto::Earthquake => "U.S. Geological Survey",
        ChannelKindDto::Markets => "Yahoo Finance chart",
        ChannelKindDto::System => "Local runtime",
    }
}

fn model_status(value: AvailabilityDto) -> AvailabilityStatus {
    match value {
        AvailabilityDto::Fresh => AvailabilityStatus::Live,
        AvailabilityDto::Delayed => AvailabilityStatus::Degraded,
        AvailabilityDto::Stale => AvailabilityStatus::Stale,
        AvailabilityDto::Offline => AvailabilityStatus::Offline,
    }
}

fn availability_dto(value: AvailabilityStatus) -> AvailabilityDto {
    match value {
        AvailabilityStatus::Live => AvailabilityDto::Fresh,
        AvailabilityStatus::Degraded => AvailabilityDto::Delayed,
        AvailabilityStatus::Stale => AvailabilityDto::Stale,
        AvailabilityStatus::Offline | AvailabilityStatus::Disabled => AvailabilityDto::Offline,
    }
}

fn bridge_state_dto(value: bridgestatus_model::BridgeState) -> BridgeStateDto {
    match value {
        bridgestatus_model::BridgeState::Clear => BridgeStateDto::Clear,
        bridgestatus_model::BridgeState::Watch => BridgeStateDto::Possible,
        bridgestatus_model::BridgeState::Likely => BridgeStateDto::Likely,
        bridgestatus_model::BridgeState::Open => BridgeStateDto::Open,
    }
}

fn bridge_interval_dto(
    interval: tenders_storage::BridgeStateInterval,
) -> Result<BridgeStateIntervalDto, RuntimeError> {
    let relation = match interval.relation.as_str() {
        "target" => BridgeRelationDto::Target,
        "upstream" => BridgeRelationDto::Upstream,
        _ => {
            return Err(RuntimeError::Normalization(
                "unknown bridge relation".into(),
            ));
        }
    };
    let state = match interval.state.as_str() {
        "up" => ObservedBridgeStateDto::Up,
        "down" => ObservedBridgeStateDto::Down,
        "unknown" => ObservedBridgeStateDto::Unknown,
        _ => {
            return Err(RuntimeError::Normalization(
                "unknown observed bridge state".into(),
            ));
        }
    };
    let river_order = upstream_rank(&interval.bridge_key).unwrap_or(0);
    Ok(BridgeStateIntervalDto {
        river_order,
        source_id: interval.source_id,
        bridge_key: interval.bridge_key,
        bridge_name: interval.bridge_name,
        relation,
        state,
        started_at: iso_timestamp(interval.started_at_ms)?,
        ended_at: interval.ended_at_ms.map(iso_timestamp).transpose()?,
    })
}

fn decision_copy(state: BridgeStateDto) -> (&'static str, &'static str, &'static str) {
    match state {
        BridgeStateDto::Clear => (
            "No opening likely",
            "No current predictive evidence points to an opening.",
            "No opening activity is currently detected.",
        ),
        BridgeStateDto::Possible => (
            "Opening possible",
            "The legal window or weak evidence makes an opening possible, not predicted.",
            "Schedule context is present; predictive evidence remains below the alert threshold.",
        ),
        BridgeStateDto::Likely => (
            "Opening likely",
            "Current predictive evidence indicates a meaningful opening risk.",
            "Predictive evidence is above the Likely threshold.",
        ),
        BridgeStateDto::Open => (
            "Bridge open",
            "Florida 511 reports the span open to marine traffic.",
            "Florida 511 reports the roadway span open for marine traffic.",
        ),
    }
}

fn confidence_label(basis_points: u16) -> &'static str {
    match basis_points {
        0..=2_499 => "Low estimate",
        2_500..=5_799 => "Moderate estimate",
        5_800..=8_199 => "High estimate",
        _ => "Very high estimate",
    }
}

fn format_local_slot(
    timestamp: TimestampMillis,
    local_time_zone: &str,
) -> Result<String, RuntimeError> {
    let time_zone =
        TimeZone::get(local_time_zone).map_err(|error| RuntimeError::Time(error.to_string()))?;
    let zoned = Timestamp::from_millisecond(timestamp.0)
        .map_err(|error| RuntimeError::Time(error.to_string()))?
        .to_zoned(time_zone);
    let hour = zoned.hour();
    let suffix = if hour < 12 { "AM" } else { "PM" };
    let display_hour = match hour % 12 {
        0 => 12,
        value => value,
    };
    Ok(format!("{display_hour}:{:02} {suffix}", zoned.minute()))
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
