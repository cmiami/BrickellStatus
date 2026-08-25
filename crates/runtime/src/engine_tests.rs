use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use async_trait::async_trait;
use brickellstatus_collectors::{CollectorHealth, HealthState, Location, SourceLink};
use serde_json::json;
use tokio::sync::Notify;
use url::Url;

use super::*;

#[derive(Debug)]
struct FixedClock(AtomicI64);

impl FixedClock {
    fn advance(&self, milliseconds: i64) {
        self.0.fetch_add(milliseconds, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct StaticFactory {
    collector: Arc<dyn Collector>,
}

impl CollectorFactory for StaticFactory {
    fn build(
        &self,
        preferences: &AppPreferences,
    ) -> Result<Vec<CollectorRegistration>, RuntimeError> {
        Ok(vec![CollectorRegistration::new(
            "fl511.bridge.brickell",
            preferences.profile.home_channel_id.clone(),
            self.collector.clone(),
        )])
    }
}

struct CadencedFactory {
    collector: Arc<dyn Collector>,
    minimum_interval: Duration,
}

impl CollectorFactory for CadencedFactory {
    fn build(
        &self,
        preferences: &AppPreferences,
    ) -> Result<Vec<CollectorRegistration>, RuntimeError> {
        Ok(vec![
            CollectorRegistration::new(
                "fixture.cadenced",
                preferences.profile.home_channel_id.clone(),
                self.collector.clone(),
            )
            .with_minimum_interval(self.minimum_interval),
        ])
    }
}

struct SequenceCollector {
    calls: AtomicUsize,
    fail_after_first: bool,
}

struct BlockingCollector {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl Collector for BlockingCollector {
    fn name(&self) -> &'static str {
        "fixture-blocking"
    }

    async fn collect(&self, _context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(CollectorBatch {
            source: self.name().into(),
            items: Vec::new(),
            health: CollectorHealth::healthy(),
            cursor: CollectorCursor::default(),
            not_modified: false,
        })
    }
}

#[async_trait]
impl Collector for SequenceCollector {
    fn name(&self) -> &'static str {
        "fixture-fl511"
    }

    async fn collect(&self, _context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_after_first && call > 0 {
            return Err(CollectorError::Configuration("fixture offline".into()));
        }
        Ok(CollectorBatch {
            source: self.name().into(),
            items: vec![bridge_item("253", "Brickell Avenue Bridge", "target", "up")],
            health: CollectorHealth {
                state: HealthState::Healthy,
                checked_at: chrono_now_for_fixture(),
                message: None,
            },
            cursor: CollectorCursor {
                etag: Some("\"fixture-v1\"".into()),
                ..CollectorCursor::default()
            },
            not_modified: false,
        })
    }
}

struct DownFl511Collector;

#[async_trait]
impl Collector for DownFl511Collector {
    fn name(&self) -> &'static str {
        "fixture-fl511-down"
    }

    async fn collect(&self, _context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        Ok(CollectorBatch {
            source: self.name().into(),
            items: vec![bridge_item(
                "253",
                "Brickell Avenue Bridge",
                "target",
                "down",
            )],
            health: CollectorHealth {
                state: HealthState::Healthy,
                checked_at: chrono_now_for_fixture(),
                message: None,
            },
            cursor: CollectorCursor::default(),
            not_modified: false,
        })
    }
}

struct OpenThenUnknownFl511Collector {
    calls: AtomicUsize,
}

#[async_trait]
impl Collector for OpenThenUnknownFl511Collector {
    fn name(&self) -> &'static str {
        "fixture-fl511-open-then-unknown"
    }

    async fn collect(&self, _context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        let first = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
        Ok(CollectorBatch {
            source: self.name().into(),
            items: vec![bridge_item(
                "253",
                "Brickell Avenue Bridge",
                "target",
                if first { "up" } else { "unknown" },
            )],
            health: CollectorHealth {
                state: if first {
                    HealthState::Healthy
                } else {
                    HealthState::Degraded
                },
                checked_at: chrono_now_for_fixture(),
                message: (!first).then(|| "target bridge state is unknown".into()),
            },
            cursor: CollectorCursor::default(),
            not_modified: false,
        })
    }
}

struct QuietAisCollector;

#[async_trait]
impl Collector for QuietAisCollector {
    fn name(&self) -> &'static str {
        "fixture-aisstream-quiet"
    }

    async fn collect(&self, _context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        Ok(CollectorBatch {
            source: self.name().into(),
            items: Vec::new(),
            health: CollectorHealth {
                state: HealthState::Healthy,
                checked_at: chrono_now_for_fixture(),
                message: None,
            },
            cursor: CollectorCursor::default(),
            not_modified: false,
        })
    }
}

struct AisConnectionLossCollector {
    calls: AtomicUsize,
}

#[async_trait]
impl Collector for AisConnectionLossCollector {
    fn name(&self) -> &'static str {
        "fixture-aisstream"
    }

    async fn collect(&self, _context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            return Err(CollectorError::Request(
                "AISStream live connection is unavailable".into(),
            ));
        }
        Ok(CollectorBatch {
            source: self.name().into(),
            items: vec![ais_bridge_item()],
            health: CollectorHealth {
                state: HealthState::Healthy,
                checked_at: chrono_now_for_fixture(),
                message: None,
            },
            cursor: CollectorCursor::default(),
            not_modified: false,
        })
    }
}

struct Fl511AndAisFactory {
    fl511: Arc<dyn Collector>,
    ais: Arc<dyn Collector>,
}

impl CollectorFactory for Fl511AndAisFactory {
    fn build(
        &self,
        preferences: &AppPreferences,
    ) -> Result<Vec<CollectorRegistration>, RuntimeError> {
        Ok(vec![
            CollectorRegistration::new(
                "fl511.bridge.brickell",
                &preferences.profile.home_channel_id,
                Arc::clone(&self.fl511),
            ),
            CollectorRegistration::new(
                "aisstream.bridge.brickell",
                &preferences.profile.home_channel_id,
                Arc::clone(&self.ais),
            )
            .fail_closed_on_error(),
        ])
    }
}

fn chrono_now_for_fixture() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(1_786_741_200_000).unwrap()
}

fn bridge_item(id: &str, title: &str, relation: &str, state: &str) -> CollectorItem {
    let selector_key = if relation == "target" { "brickell" } else { id };
    CollectorItem {
        id: format!("fl511:bridge:{id}"),
        kind: ItemKind::Bridge,
        title: title.into(),
        summary: Some(format!("Bridge {state}")),
        observed_at: None,
        starts_at: None,
        ends_at: None,
        location: Some(Location::point(25.7699, -80.19005)),
        source: SourceLink {
            name: "Florida 511".into(),
            url: Some(Url::parse("https://fl511.com/").unwrap()),
        },
        attributes: BTreeMap::from([
            ("relation".into(), json!(relation)),
            ("selector_key".into(), json!(selector_key)),
            ("state".into(), json!(state)),
            ("state_conflict".into(), json!(false)),
        ]),
    }
}

fn ais_bridge_item() -> CollectorItem {
    CollectorItem {
        id: "aisstream:367719770".into(),
        kind: ItemKind::Bridge,
        title: "RIVER RUNNER · 367719770".into(),
        summary: Some("1.2 km from Brickell Avenue Bridge · approaching · ETA 6–10 min".into()),
        observed_at: Some(chrono_now_for_fixture()),
        starts_at: None,
        ends_at: Some(chrono_now_for_fixture() + chrono::TimeDelta::minutes(3)),
        location: Some(Location::point(25.76975, -80.1788)),
        source: SourceLink {
            name: "AISStream".into(),
            url: Some(Url::parse("https://aisstream.io/documentation.html").unwrap()),
        },
        attributes: BTreeMap::from([
            ("relation".into(), json!("ais")),
            ("state".into(), json!("approaching")),
            ("movement".into(), json!("approaching")),
            ("route_intersects".into(), json!(true)),
            ("mmsi".into(), json!("367719770")),
            ("vessel_name".into(), json!("RIVER RUNNER")),
            ("distance_meters".into(), json!(1_240)),
            ("eta_min_minutes".into(), json!(6)),
            ("eta_max_minutes".into(), json!(10)),
        ]),
    }
}

#[test]
fn ais_collector_item_normalizes_to_real_predictor_evidence() {
    let item = ais_bridge_item();

    assert_eq!(
        bridge_fact(&item, &BTreeMap::new()),
        Some(BridgeObservation::AisTrack {
            mmsi: Some("367719770".into()),
            vessel_name: Some("RIVER RUNNER".into()),
            movement: VesselMovement::Approaching,
            route_intersects: true,
            schedule_exempt: false,
            eta: Some(EtaRangeMinutes::new(6, 10)),
            opening_propensity: None,
        })
    );
}

#[test]
fn live_tug_exemption_reaches_predictor_evidence() {
    let mut item = ais_bridge_item();
    item.attributes.insert("vessel_class".into(), json!("tug"));

    let Some(BridgeObservation::AisTrack {
        schedule_exempt, ..
    }) = bridge_fact(&item, &BTreeMap::new())
    else {
        panic!("expected an AIS track");
    };

    assert!(schedule_exempt);
}

#[test]
fn ledger_propensity_and_sailing_prior_reach_the_ais_observation() {
    let item = ais_bridge_item();

    // A hull the ledger has watched scores as itself.
    let propensities = BTreeMap::from([("367719770".to_string(), 6_667_u16)]);
    let Some(BridgeObservation::AisTrack {
        opening_propensity, ..
    }) = bridge_fact(&item, &propensities)
    else {
        panic!("expected an AIS track");
    };
    assert_eq!(
        opening_propensity,
        Some(Confidence::from_basis_points(6_667))
    );

    // A sailing rig the ledger has never seen is still a near-certain opener.
    let mut sailing = ais_bridge_item();
    sailing
        .attributes
        .insert("vessel_class".into(), json!("sailing"));
    let Some(BridgeObservation::AisTrack {
        opening_propensity, ..
    }) = bridge_fact(&sailing, &BTreeMap::new())
    else {
        panic!("expected an AIS track");
    };
    assert_eq!(
        opening_propensity,
        Some(Confidence::from_basis_points(9_000))
    );
}

#[test]
fn a_known_opener_prearms_only_while_approaching_in_the_corridor() {
    let mut item = ais_bridge_item();
    item.attributes
        .insert("route_intersects".into(), json!(false));
    item.attributes.insert("posture".into(), json!("underway"));
    let known = BTreeMap::from([("367719770".to_string(), 6_667_u16)]);
    let route_intersects = |item: &CollectorItem, propensities: &BTreeMap<String, u16>| {
        let Some(BridgeObservation::AisTrack {
            route_intersects, ..
        }) = bridge_fact(item, propensities)
        else {
            panic!("expected an AIS track");
        };
        route_intersects
    };

    assert!(route_intersects(&item, &known));
    assert!(
        !route_intersects(
            &item,
            &BTreeMap::from([("367719770".to_string(), 5_000_u16)])
        ),
        "a hull below the shared known-opener boundary must keep the raw route result"
    );
    item.attributes
        .insert("posture".into(), json!("off_channel"));
    assert!(
        !route_intersects(&item, &known),
        "opening history cannot pull an off-channel hull onto the route"
    );
}

#[test]
fn a_known_opener_prearm_gets_an_eta_only_from_live_corridor_motion() {
    let mut item = ais_bridge_item();
    item.attributes
        .insert("route_intersects".into(), json!(false));
    item.attributes.insert("posture".into(), json!("underway"));
    item.attributes
        .insert("distance_meters".into(), json!(3_200));
    item.attributes.insert("sog_knots".into(), json!(6.0));
    item.attributes.remove("eta_min_minutes");
    item.attributes.remove("eta_max_minutes");
    let known = BTreeMap::from([("367719770".to_string(), 6_667_u16)]);

    let eta = |item: &CollectorItem, propensities: &BTreeMap<String, u16>| {
        let Some(BridgeObservation::AisTrack {
            route_intersects,
            eta,
            ..
        }) = bridge_fact(item, propensities)
        else {
            panic!("expected an AIS track");
        };
        (route_intersects, eta)
    };

    assert_eq!(
        eta(&item, &known),
        (true, Some(EtaRangeMinutes::new(12, 24)))
    );
    assert_eq!(
        eta(
            &item,
            &BTreeMap::from([("367719770".to_string(), 5_000_u16)])
        ),
        (false, None)
    );

    item.attributes
        .insert("posture".into(), json!("off_channel"));
    assert_eq!(eta(&item, &known), (false, None));

    item.attributes.insert("posture".into(), json!("moored"));
    assert_eq!(eta(&item, &known), (false, None));

    item.attributes.insert("posture".into(), json!("underway"));
    item.attributes.insert("movement".into(), json!("unknown"));
    assert_eq!(eta(&item, &known), (false, None));
}

fn transition(
    bridge_key: &str,
    relation: &str,
    from_state: &str,
    to_state: &str,
    occurred_at_ms: i64,
) -> BridgeStateTransition {
    BridgeStateTransition {
        bridge_key: bridge_key.into(),
        bridge_name: bridge_key.replace('_', " "),
        relation: relation.into(),
        from_state: from_state.into(),
        to_state: to_state.into(),
        occurred_at_ms,
    }
}

#[test]
fn ordered_outbound_openings_raise_high_then_very_high_confidence() {
    let now_ms = 1_786_741_200_000;
    let high = vec![
        transition("w_flagler", "upstream", "down", "up", now_ms - 8 * 60_000),
        transition("sw_1_st", "upstream", "down", "up", now_ms - 4 * 60_000),
    ];
    let progress = detect_outbound_progress(&high, now_ms).expect("outbound sequence");
    assert_eq!(progress.stage, OutboundProgressStage::High);
    assert_eq!(progress.bridge_key, "sw_1_st");

    let mut very_high = high;
    very_high.push(transition(
        "sw_2_ave",
        "upstream",
        "down",
        "up",
        now_ms - 60_000,
    ));
    let progress = detect_outbound_progress(&very_high, now_ms).expect("nearest upstream reached");
    assert_eq!(progress.stage, OutboundProgressStage::VeryHigh);
    assert_eq!(progress.bridge_key, "sw_2_ave");
}

#[test]
fn a_single_or_inbound_upstream_opening_is_not_outbound_evidence() {
    let now_ms = 1_786_741_200_000;
    assert!(
        detect_outbound_progress(
            &[transition(
                "sw_2_ave",
                "upstream",
                "down",
                "up",
                now_ms - 60_000,
            )],
            now_ms,
        )
        .is_none()
    );

    let inbound = vec![
        transition("brickell", "target", "down", "up", now_ms - 10 * 60_000),
        transition("sw_2_ave", "upstream", "down", "up", now_ms - 7 * 60_000),
        transition("sw_1_st", "upstream", "down", "up", now_ms - 3 * 60_000),
    ];
    assert!(detect_outbound_progress(&inbound, now_ms).is_none());
}

#[test]
fn expired_ais_positions_are_not_evidence_or_fresh_vessel_counts() {
    let now_ms = 1_786_741_200_000;
    let mut preferences = AppPreferences::default();
    preferences.ais.enabled = true;
    preferences.ais.api_key_configured = true;
    let channel_id = preferences.profile.home_channel_id.clone();
    let source_id = format!("aisstream.{channel_id}");
    let mut item = ais_bridge_item();
    item.ends_at = chrono::DateTime::from_timestamp_millis(now_ms - 1);
    let mut source = healthy_source_state(&channel_id, item, now_ms);
    source.fail_closed_on_error = true;
    source.cursor.metadata.insert(
        "fresh_vessel_expirations_ms".into(),
        format!("[{}]", now_ms - 1),
    );
    source
        .cursor
        .metadata
        .insert("last_position_at_ms".into(), now_ms.to_string());
    let state = PersistedRuntimeState {
        active_sources: BTreeMap::from([(source_id.clone(), channel_id)]),
        sources: BTreeMap::from([(source_id, source)]),
        ..PersistedRuntimeState::default()
    };

    let (evidence, views) = bridge_evidence(&state, &preferences, now_ms).unwrap();
    assert_eq!(views[0].availability, AvailabilityDto::Stale);
    assert_eq!(evidence[0].availability, AvailabilityStatus::Stale);
    let status = aisstream_status(&preferences, &state, now_ms).unwrap();
    assert_eq!(status.connection_state, AisConnectionStateDto::Armed);
    assert_eq!(status.fresh_vessel_count, 0);
    assert_eq!(
        status.last_position_at.as_deref(),
        Some(iso_timestamp(now_ms).unwrap().as_str())
    );
}

fn weather_hourly_item(now_ms: i64, probability: f64, gust_kmh: f64) -> CollectorItem {
    CollectorItem {
        id: "open-meteo:hourly:fixture".into(),
        kind: ItemKind::WeatherHourly,
        title: "Hourly forecast".into(),
        summary: Some(format!("Rain probability {probability:.0}%")),
        observed_at: Some(chrono::DateTime::from_timestamp_millis(now_ms).unwrap()),
        starts_at: Some(chrono::DateTime::from_timestamp_millis(now_ms + 30 * 60_000).unwrap()),
        ends_at: None,
        location: Some(Location::point(25.7617, -80.1918)),
        source: SourceLink {
            name: "Open-Meteo".into(),
            url: Some(Url::parse("https://open-meteo.com/").unwrap()),
        },
        attributes: BTreeMap::from([
            ("precipitation_probability".into(), json!(probability)),
            ("wind_gusts_10m".into(), json!(gust_kmh)),
            (
                "units".into(),
                json!({"precipitation_probability": "%", "wind_gusts_10m": "km/h"}),
            ),
        ]),
    }
}

fn market_quote_item(change_percent: f64, delay_minutes: Option<u64>) -> CollectorItem {
    let mut attributes = BTreeMap::from([
        ("symbol".into(), json!("AMD")),
        ("label".into(), json!("AMD")),
        ("price".into(), json!(172.40)),
        ("previous_close".into(), json!(161.94)),
        ("change_percent".into(), json!(change_percent)),
        ("currency".into(), json!("USD")),
        ("session_label".into(), json!("OPEN")),
    ]);
    if let Some(delay_minutes) = delay_minutes {
        attributes.insert("provider_delay_minutes".into(), json!(delay_minutes));
        attributes.insert("delay_semantics".into(), json!("provider_reported"));
    } else {
        attributes.insert("delay_semantics".into(), json!("not_reported"));
    }
    CollectorItem {
        id: "yahoo-chart:AMD".into(),
        kind: ItemKind::MarketQuote,
        title: "AMD".into(),
        summary: None,
        observed_at: Some(chrono_now_for_fixture()),
        starts_at: None,
        ends_at: None,
        location: None,
        source: SourceLink {
            name: "Yahoo Finance chart · unofficial".into(),
            url: Some(Url::parse("https://finance.yahoo.com/quote/AMD").unwrap()),
        },
        attributes,
    }
}

fn news_item(id: &str, title: &str, now_ms: i64) -> CollectorItem {
    CollectorItem {
        id: id.into(),
        kind: ItemKind::News,
        title: title.into(),
        summary: None,
        observed_at: Some(chrono::DateTime::from_timestamp_millis(now_ms).unwrap()),
        starts_at: None,
        ends_at: None,
        location: None,
        source: SourceLink {
            name: "Fixture feed".into(),
            url: Some(Url::parse("https://example.com/feed.xml").unwrap()),
        },
        attributes: BTreeMap::new(),
    }
}

/// A storm in the Bahamas, roughly 400 km from Miami and so inside the range
/// that decides whether this channel activates.
fn tropical_item(id: &str) -> CollectorItem {
    tropical_item_at(id, 24.0, -76.0)
}

fn tropical_item_at(id: &str, latitude: f64, longitude: f64) -> CollectorItem {
    CollectorItem {
        id: format!("nhc:{id}"),
        kind: ItemKind::TropicalCyclone,
        title: "TS Fixture".into(),
        summary: Some("TS Fixture · 50 kt".into()),
        observed_at: Some(chrono_now_for_fixture()),
        starts_at: None,
        ends_at: None,
        location: Some(Location::point(latitude, longitude)),
        source: SourceLink {
            name: "National Hurricane Center".into(),
            url: Some(Url::parse("https://www.nhc.noaa.gov/cyclones/").unwrap()),
        },
        // The area context collector attaches these to every tropical item; the
        // locality rule reads them rather than the whole preference set.
        attributes: BTreeMap::from([(
            "area_points".into(),
            json!([{"lat": 25.7699, "lon": -80.19005}]),
        )]),
    }
}

fn earthquake_item(id: &str, now_ms: i64) -> CollectorItem {
    CollectorItem {
        id: id.into(),
        kind: ItemKind::Earthquake,
        title: "M 7.2 fixture earthquake".into(),
        summary: Some("Fixture earthquake".into()),
        observed_at: Some(chrono::DateTime::from_timestamp_millis(now_ms).unwrap()),
        starts_at: None,
        ends_at: None,
        location: Some(Location::point(18.4, -66.1)),
        source: SourceLink {
            name: "USGS".into(),
            url: Some(
                Url::parse("https://earthquake.usgs.gov/earthquakes/eventpage/fixture").unwrap(),
            ),
        },
        attributes: BTreeMap::from([
            ("magnitude".into(), json!(7.2)),
            ("status".into(), json!("reviewed")),
        ]),
    }
}

fn official_alert_item(
    id: &str,
    message_type: &str,
    status: &str,
    ends_ms: Option<i64>,
) -> CollectorItem {
    CollectorItem {
        id: id.into(),
        kind: ItemKind::OfficialAlert,
        title: "Flash Flood Warning".into(),
        summary: Some("Fixture official warning".into()),
        observed_at: Some(chrono_now_for_fixture()),
        starts_at: None,
        ends_at: ends_ms.map(|value| {
            chrono::DateTime::from_timestamp_millis(value).expect("valid fixture timestamp")
        }),
        location: Some(Location::point(25.7617, -80.1918)),
        source: SourceLink {
            name: "National Weather Service".into(),
            url: Some(Url::parse("https://api.weather.gov/alerts/fixture").unwrap()),
        },
        attributes: BTreeMap::from([
            ("instruction".into(), json!("Move to higher ground now.")),
            ("message_type".into(), json!(message_type)),
            ("status".into(), json!(status)),
            ("severity".into(), json!("Severe")),
        ]),
    }
}

fn healthy_source_state(
    channel_id: &str,
    item: CollectorItem,
    last_success_ms: i64,
) -> SourceState {
    let mut source = SourceState::empty(channel_id);
    source.items = vec![item];
    source.reported_health = HealthState::Healthy;
    source.last_attempt_ms = Some(last_success_ms);
    source.last_success_ms = Some(last_success_ms);
    source
}

fn clear_decision() -> DecisionSnapshot {
    DecisionSnapshot {
        channel_id: "bridge.brickell".into(),
        subject: "Brickell Avenue".into(),
        state: BridgeStateDto::Clear,
        state_label: "No opening likely".into(),
        meaning: "No actionable bridge evidence.".into(),
        action: "No opening activity is currently detected.".into(),
        eta_min: None,
        eta_max: None,
        confidence_bps: Some(0),
        confidence_label: Some("Low estimate".into()),
        confidence_basis: None,
        next_legal_slot: None,
        opening_allowed_now: false,
        availability: AvailabilityDto::Offline,
        source_age_seconds: 0,
    }
}

async fn engine_with(collector: Arc<dyn Collector>, clock: Arc<FixedClock>) -> RuntimeEngine {
    RuntimeEngine::initialize(
        Store::in_memory().await.unwrap(),
        RuntimeConfig::default(),
        Arc::new(StaticFactory { collector }),
        clock,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn preference_save_does_not_wait_for_collector_network_io() {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let engine = Arc::new(
        engine_with(
            Arc::new(BlockingCollector {
                started: Arc::clone(&started),
                release: Arc::clone(&release),
            }),
            clock,
        )
        .await,
    );
    let refresh = tokio::spawn({
        let engine = Arc::clone(&engine);
        async move { engine.refresh_all().await }
    });
    started.notified().await;

    let mut preferences = engine.get_preferences().await;
    preferences.profile.name = "Saved during refresh".into();
    tokio::time::timeout(
        Duration::from_millis(250),
        engine.save_preferences(preferences),
    )
    .await
    .expect("preference save must not queue behind collector I/O")
    .unwrap();

    release.notify_one();
    refresh.await.unwrap().unwrap();
    assert_eq!(
        engine.get_preferences().await.profile.name,
        "Saved during refresh"
    );
    assert!(engine.state.lock().await.sources.is_empty());
}

async fn install_live_state_write_failure(store: &Store) {
    sqlx::query(
        r#"
            CREATE TRIGGER fail_runtime_live_state_write
            BEFORE INSERT ON settings
            WHEN NEW.key = 'runtime.live_state'
            BEGIN
                SELECT RAISE(ABORT, 'fixture live-state write failure');
            END
            "#,
    )
    .execute(store.pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn live_fl511_becomes_authoritative_bridge_evidence() {
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let collector = Arc::new(SequenceCollector {
        calls: AtomicUsize::new(0),
        fail_after_first: false,
    });
    let engine = engine_with(collector, clock).await;
    let report = engine.refresh_all().await.unwrap();
    let snapshot = engine.get_snapshot().await.unwrap();
    assert_eq!(report.succeeded, 1);
    assert_eq!(snapshot.decision.state, BridgeStateDto::Open);
    assert_eq!(snapshot.evidence.len(), 1);
    assert!(
        snapshot
            .evidence
            .iter()
            .any(|item| item.source_label == "Bridge status reporting")
    );
    let intervals = engine
        .store
        .list_bridge_state_intervals("fl511.bridge.brickell", "brickell")
        .await
        .unwrap();
    assert_eq!(intervals.len(), 1);
    assert_eq!(intervals[0].state, "up");
    assert_eq!(intervals[0].ended_at_ms, None);
    assert_eq!(snapshot.bridge_intervals.len(), 1);
    assert_eq!(
        snapshot.bridge_intervals[0].state,
        ObservedBridgeStateDto::Up
    );
    assert_eq!(
        snapshot.bridge_intervals[0].relation,
        BridgeRelationDto::Target
    );
    assert_eq!(snapshot.system.collectors_online, 1);
}

#[tokio::test]
async fn snapshot_publishes_the_tracked_corridor_whenever_ais_is_running() {
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let engine = RuntimeEngine::initialize(
        Store::in_memory().await.unwrap(),
        RuntimeConfig::default(),
        Arc::new(Fl511AndAisFactory {
            fl511: Arc::new(OpenThenUnknownFl511Collector {
                calls: AtomicUsize::new(0),
            }),
            ais: Arc::new(QuietAisCollector),
        }),
        clock.clone(),
    )
    .await
    .unwrap();
    engine.refresh_all().await.unwrap();

    // Without this the live surface has no water to draw and silently renders
    // nothing, which is indistinguishable from "no vessels".
    let snapshot = engine.get_snapshot().await.unwrap();
    let corridor = snapshot.river_corridor;
    assert!(corridor.ais_live, "an active AIS source must report live");
    assert!((corridor.bridge_latitude - 25.7699).abs() < 0.001);
    let river = corridor
        .branches
        .iter()
        .find(|branch| branch.id == "river")
        .expect("the trunk is always published");
    assert!(river.centerline.len() >= 2);
    assert_eq!(river.corridor_offset_meters, 120.0);
    assert_eq!(corridor.branches.len(), 4);

    // Stations are what a diagram names, and the target must be findable by
    // FL511's own key so its live state can be joined to it.
    let brickell = river
        .stations
        .iter()
        .find(|station| station.bridge_key.as_deref() == Some("brickell"))
        .expect("the target span is a station");
    assert_eq!(brickell.kind, "target");
    assert!(brickell.s_meters.abs() < 1.0);
    assert!(
        river
            .stations
            .iter()
            .any(|station| station.bridge_key.as_deref() == Some("sw_2_ave"))
    );
    // Upstream bascules sit at positive channel metres, seaward marks negative.
    let sw2 = river
        .stations
        .iter()
        .find(|station| station.bridge_key.as_deref() == Some("sw_2_ave"))
        .unwrap();
    assert!(sw2.s_meters > 0.0);
    // Government Cut is its own branch now. The north approach used to carry
    // this mark, which is what dragged its centerline across Dodge Island.
    let cut = corridor
        .branches
        .iter()
        .find(|branch| branch.id == "government_cut")
        .unwrap()
        .stations
        .iter()
        .find(|station| station.label == "Government Cut")
        .expect("the cut names its seaward marks");
    assert!(cut.s_meters < 0.0);
}

#[tokio::test]
async fn known_opener_history_and_current_opening_likelihood_are_independent() {
    let now_ms = 1_786_741_200_000;
    let clock = Arc::new(FixedClock(AtomicI64::new(now_ms)));
    let engine = engine_with(Arc::new(DownFl511Collector), clock).await;

    sqlx::query(
        r#"
        INSERT INTO ais_vessel_ledger(
            mmsi, name, vessel_class, transits_opened, first_seen_ms, last_seen_ms
        ) VALUES
            ('367000101', 'PAST OPENER', 'yacht', 1, ?1, ?1),
            ('367000102', 'RETURNING OPENER', 'yacht', 1, ?1, ?1),
            ('367000105', 'FAR OPENER', 'yacht', 1, ?1, ?1),
            ('367000106', 'STALE OPENER', 'yacht', 1, ?1, ?1),
            ('367000107', 'PREARMED OPENER', 'yacht', 1, ?1, ?1)
        "#,
    )
    .bind(now_ms - 86_400_000)
    .execute(engine.store.pool())
    .await
    .unwrap();

    let observed_at = iso_timestamp(now_ms).unwrap();
    let stale_at = iso_timestamp(now_ms - 7 * 60_000).unwrap();
    let track = |mmsi: &str, movement: &str, vessel_class: &str, eta: Option<u16>, at: &str| {
        serde_json::from_value::<VesselTrackSnapshot>(json!({
            "mmsi": mmsi,
            "movement": movement,
            "routeIntersects": true,
            "speedKnots": 5.0,
            "courseDegrees": 270.0,
            "observedAt": at,
            "vesselClass": vessel_class,
            "posture": "underway",
            "etaMinMinutes": eta,
            "etaMaxMinutes": eta.map(|minutes| minutes + 2),
            "points": [{
                "latitude": 25.76975,
                "longitude": -80.185,
                "observedAt": at
            }]
        }))
        .unwrap()
    };
    let mut prearmed = track("367000107", "approaching", "yacht", None, &observed_at);
    prearmed.route_intersects = false;
    prearmed.s_meters = Some(-1_200.0);
    let tracks = vec![
        track("367000101", "diverging", "yacht", None, &observed_at),
        track("367000102", "approaching", "yacht", Some(8), &observed_at),
        track("367000103", "approaching", "sailing", Some(8), &observed_at),
        track("367000104", "approaching", "yacht", Some(8), &observed_at),
        track("367000105", "approaching", "yacht", Some(21), &observed_at),
        track("367000106", "approaching", "yacht", Some(8), &stale_at),
        prearmed,
    ];

    let mut source = SourceState::empty("bridge.brickell");
    source.cursor.metadata.insert(
        AIS_VESSEL_TRACKS_CURSOR_KEY.into(),
        serde_json::to_string(&tracks).unwrap(),
    );
    let propensities = engine.load_ais_propensities().await.unwrap();
    let mut state = engine.state.lock().await;
    state
        .active_sources
        .insert("aisstream.bridge.brickell".into(), "bridge.brickell".into());
    state
        .sources
        .insert("aisstream.bridge.brickell".into(), source);
    state.ais_propensities = propensities;
    drop(state);

    let flags = engine
        .get_snapshot()
        .await
        .unwrap()
        .vessel_tracks
        .into_iter()
        .map(|track| {
            (
                track.mmsi,
                (track.known_opener, track.likely_to_open_brickell),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        flags,
        BTreeMap::from([
            ("367000101".into(), (true, false)),
            ("367000102".into(), (true, true)),
            ("367000103".into(), (false, true)),
            ("367000104".into(), (false, false)),
            ("367000105".into(), (true, false)),
            ("367000106".into(), (true, false)),
            ("367000107".into(), (true, true)),
        ])
    );
}

#[tokio::test]
async fn corridor_is_published_even_when_the_ais_channel_is_switched_off() {
    // The failure this guards: with AIS disabled an earlier build sent no
    // corridor at all, so the live surface rendered an empty space that looked
    // exactly like a broken page rather than a disabled source.
    // Only FL511 is registered here; there is no AIS source at all.
    let engine = engine_with(
        Arc::new(OpenThenUnknownFl511Collector {
            calls: AtomicUsize::new(0),
        }),
        Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000))),
    )
    .await;

    let corridor = engine.get_snapshot().await.unwrap().river_corridor;
    assert!(!corridor.ais_live, "no AIS source is running here");
    assert_eq!(corridor.branches.len(), 4);
    assert!(
        corridor
            .branches
            .iter()
            .any(|branch| !branch.stations.is_empty()),
        "the river is still described when nothing is watching it"
    );
}

#[tokio::test]
async fn prior_open_cannot_resolve_from_unknown_degraded_fl511_with_usable_ais() {
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let engine = RuntimeEngine::initialize(
        Store::in_memory().await.unwrap(),
        RuntimeConfig::default(),
        Arc::new(Fl511AndAisFactory {
            fl511: Arc::new(OpenThenUnknownFl511Collector {
                calls: AtomicUsize::new(0),
            }),
            ais: Arc::new(QuietAisCollector),
        }),
        clock.clone(),
    )
    .await
    .unwrap();

    let first = engine.refresh_all().await.unwrap();
    assert_eq!(first.succeeded, 2);
    let open = engine.get_snapshot().await.unwrap();
    assert_eq!(open.decision.state, BridgeStateDto::Open);
    assert!(
        open.channels
            .iter()
            .find(|channel| channel.id == "bridge.brickell")
            .unwrap()
            .active
    );

    clock.advance(10_000);
    let second = engine.refresh_all().await.unwrap();
    assert_eq!(second.succeeded, 2);
    let unresolved = engine.get_snapshot().await.unwrap();
    let bridge = unresolved
        .channels
        .iter()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    assert_eq!(bridge.availability, AvailabilityDto::Delayed);
    assert!(!bridge.active);
    assert!(
        !bridge.coverage_complete,
        "an unknown/degraded FL511 target cannot establish an all-clear"
    );

    let preferences = engine.get_preferences().await;
    let bridge_preference = preferences
        .profile
        .channels
        .iter()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    let state = engine.state.lock().await;
    assert_eq!(
        source_availability(
            &state.sources["aisstream.bridge.brickell"],
            bridge_preference,
            clock.now_millis(),
        )
        .0,
        AvailabilityDto::Fresh,
        "healthy sibling evidence remains usable for positive prediction"
    );
}

#[test]
fn bridge_resolution_requires_current_healthy_nonconflicting_target_down() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let channel = preferences
        .profile
        .channels
        .iter()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    let source_id = "fl511.bridge.brickell";
    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([(source_id.into(), channel.id.clone())]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(
        source_id.into(),
        healthy_source_state(
            &channel.id,
            bridge_item("253", "Brickell Avenue Bridge", "target", "down"),
            now_ms,
        ),
    );

    assert!(bridge_resolution_confirmed(channel, &state, now_ms));

    state.sources.get_mut(source_id).unwrap().items[0]
        .attributes
        .insert("state".into(), json!("unknown"));
    assert!(!bridge_resolution_confirmed(channel, &state, now_ms));

    let target = &mut state.sources.get_mut(source_id).unwrap().items[0];
    target.attributes.insert("state".into(), json!("down"));
    target
        .attributes
        .insert("state_conflict".into(), json!(true));
    assert!(!bridge_resolution_confirmed(channel, &state, now_ms));

    state.sources.get_mut(source_id).unwrap().items[0]
        .attributes
        .insert("state_conflict".into(), json!(false));
    state.sources.get_mut(source_id).unwrap().reported_health = HealthState::Degraded;
    assert!(!bridge_resolution_confirmed(channel, &state, now_ms));

    let source = state.sources.get_mut(source_id).unwrap();
    source.reported_health = HealthState::Healthy;
    source.items.clear();
    assert!(!bridge_resolution_confirmed(channel, &state, now_ms));
}

#[test]
fn predictive_bridge_signal_remains_active_without_resolution_coverage() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([
            ("fl511.bridge.brickell".into(), "bridge.brickell".into()),
            ("aisstream.bridge.brickell".into(), "bridge.brickell".into()),
        ]),
        ..PersistedRuntimeState::default()
    };
    let mut fl511 = healthy_source_state(
        "bridge.brickell",
        bridge_item("253", "Brickell Avenue Bridge", "target", "unknown"),
        now_ms,
    );
    fl511.reported_health = HealthState::Degraded;
    state.sources.insert("fl511.bridge.brickell".into(), fl511);
    state.sources.insert(
        "aisstream.bridge.brickell".into(),
        healthy_source_state("bridge.brickell", ais_bridge_item(), now_ms),
    );
    let mut decision = clear_decision();
    decision.state = BridgeStateDto::Likely;
    decision.state_label = "Likely opening".into();
    decision.availability = AvailabilityDto::Fresh;

    let channels = channel_snapshots(&preferences, &state, &decision, now_ms);
    let bridge = channels
        .iter()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    assert!(bridge.active, "AIS may still raise a positive warning");
    assert!(
        !bridge.coverage_complete,
        "AIS cannot establish restoration"
    );
}

#[test]
fn passed_known_opener_and_confirmed_close_remove_the_live_signal() {
    let now_ms = "2026-08-15T03:10:00Z"
        .parse::<Timestamp>()
        .unwrap()
        .as_millisecond();
    let make_ais = |id: &str, at: i64, movement, route_intersects, eta| BridgeEvidence {
        observation_id: ObservationId::from(id),
        source_id: SourceId::from("aisstream.bridge.brickell"),
        observed_at: TimestampMillis(at),
        expires_at: None,
        availability: AvailabilityStatus::Live,
        reliability: Confidence::from_basis_points(8_500),
        fact: BridgeObservation::AisTrack {
            mmsi: Some("367705830".into()),
            vessel_name: Some("PEPIN".into()),
            movement,
            route_intersects,
            schedule_exempt: false,
            eta,
            opening_propensity: Some(Confidence::from_basis_points(8_333)),
        },
    };
    let predictor = BridgePredictor::default();
    let inbound = make_ais(
        "pepin-inbound",
        now_ms,
        VesselMovement::Approaching,
        true,
        Some(EtaRangeMinutes::new(7, 14)),
    );
    let likely = predictor
        .evaluate(TimestampMillis(now_ms), &[inbound], None)
        .unwrap();
    assert_eq!(likely.state, BridgeState::Likely);

    let later_ms = now_ms + 10 * 60_000;
    let passed = make_ais(
        "pepin-passed",
        later_ms,
        VesselMovement::Diverging,
        false,
        None,
    );
    let closed = BridgeEvidence {
        observation_id: ObservationId::from("controller-closed"),
        source_id: SourceId::from("fl511.bridge.brickell"),
        observed_at: TimestampMillis(later_ms),
        expires_at: None,
        availability: AvailabilityStatus::Live,
        reliability: Confidence::from_basis_points(9_900),
        fact: BridgeObservation::Controller {
            state: BridgeControllerState::Closed,
        },
    };
    let cleared = predictor
        .evaluate(TimestampMillis(later_ms), &[passed, closed], Some(&likely))
        .unwrap();
    let decision = decision_snapshot(&cleared, "America/New_York").unwrap();

    let preferences = AppPreferences::default();
    let mut ais_item = ais_bridge_item();
    ais_item
        .attributes
        .insert("movement".into(), json!("diverging"));
    ais_item
        .attributes
        .insert("route_intersects".into(), json!(false));
    ais_item.attributes.remove("eta_min_minutes");
    ais_item.attributes.remove("eta_max_minutes");
    let state = PersistedRuntimeState {
        active_sources: BTreeMap::from([
            ("fl511.bridge.brickell".into(), "bridge.brickell".into()),
            ("aisstream.bridge.brickell".into(), "bridge.brickell".into()),
        ]),
        sources: BTreeMap::from([
            (
                "fl511.bridge.brickell".into(),
                healthy_source_state(
                    "bridge.brickell",
                    bridge_item("253", "Brickell Avenue Bridge", "target", "down"),
                    later_ms,
                ),
            ),
            (
                "aisstream.bridge.brickell".into(),
                healthy_source_state("bridge.brickell", ais_item, later_ms),
            ),
        ]),
        ..PersistedRuntimeState::default()
    };
    let channels = channel_snapshots(&preferences, &state, &decision, later_ms);
    let bridge = channels
        .iter()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();

    assert_eq!(decision.state, BridgeStateDto::Clear);
    assert!(
        !bridge.active,
        "Live and e-ink membership consume this flag"
    );
}

#[test]
fn long_range_eta_is_informational_and_cannot_activate_bridge_channel() {
    let now_ms = "2026-08-15T14:00:00Z"
        .parse::<Timestamp>()
        .unwrap()
        .as_millisecond();
    let transit = BridgeEvidence {
        observation_id: ObservationId::from("long-range-transit"),
        source_id: SourceId::from("bbpilots.bridge.brickell"),
        observed_at: TimestampMillis(now_ms),
        expires_at: None,
        availability: AvailabilityStatus::Live,
        reliability: Confidence::CERTAIN,
        fact: BridgeObservation::ScheduledTransit {
            vessel: "Test tow".into(),
            exempt: true,
            eta: Some(EtaRangeMinutes::new(90, 90)),
        },
    };
    let prediction = BridgePredictor::default()
        .evaluate(TimestampMillis(now_ms), &[transit], None)
        .unwrap();
    let decision = decision_snapshot(&prediction, "America/New_York").unwrap();

    assert_eq!(prediction.predictive_state, BridgeState::Likely);
    assert_eq!(decision.state, BridgeStateDto::Clear);
    assert_eq!(decision.eta_min, Some(90));
    assert_eq!(decision.state_label, "Road open");
    assert_eq!(decision.action, "Alerts begin at T-30.");
    assert_eq!(
        channel_urgency(ChannelKindDto::Bridge, None, &decision),
        UrgencyDto::Routine
    );
}

#[tokio::test]
async fn live_ais_connection_loss_fails_closed_beside_healthy_fl511() {
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let store = Store::in_memory().await.unwrap();
    let mut configured_preferences = AppPreferences::default();
    configured_preferences.ais.enabled = true;
    configured_preferences.ais.api_key_configured = true;
    store
        .set_json(
            PREFERENCES_KEY,
            &configured_preferences,
            "2026-08-14T16:20:00Z",
        )
        .await
        .unwrap();
    let engine = RuntimeEngine::initialize(
        store,
        RuntimeConfig::default(),
        Arc::new(Fl511AndAisFactory {
            fl511: Arc::new(DownFl511Collector),
            ais: Arc::new(AisConnectionLossCollector {
                calls: AtomicUsize::new(0),
            }),
        }),
        clock.clone(),
    )
    .await
    .unwrap();

    let first = engine.refresh_all().await.unwrap();
    assert_eq!(first.succeeded, 2);
    let first_snapshot = engine.get_snapshot().await.unwrap();
    let first_ais_status = engine.get_aisstream_status().await.unwrap();
    assert_eq!(
        first_ais_status.connection_state,
        AisConnectionStateDto::Live
    );
    assert_eq!(first_ais_status.fresh_vessel_count, 1);
    let first_bridge = first_snapshot
        .channels
        .iter()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    assert!(first_bridge.coverage_complete);
    assert!(first_snapshot.evidence.iter().any(|item| {
        item.source_label == "AISStream" && item.availability == AvailabilityDto::Fresh
    }));
    let first_success =
        engine.state.lock().await.sources["aisstream.bridge.brickell"].last_success_ms;

    clock.advance(10_000);
    let second = engine.refresh_all().await.unwrap();
    assert_eq!(second.failed, 1);
    let second_snapshot = engine.get_snapshot().await.unwrap();
    let second_bridge = second_snapshot
        .channels
        .iter()
        .find(|channel| channel.id == "bridge.brickell")
        .unwrap();
    assert!(!second_bridge.coverage_complete);
    let ais = second_snapshot
        .evidence
        .iter()
        .find(|item| item.source_label == "AISStream")
        .expect("cached AIS row remains inspectable");
    assert_eq!(ais.availability, AvailabilityDto::Offline);
    assert_eq!(ais.contribution_bps, Some(0));
    let status = engine.get_aisstream_status().await.unwrap();
    assert_eq!(status.connection_state, AisConnectionStateDto::Disconnected);
    assert_eq!(status.fresh_vessel_count, 0);
    assert!(status.last_position_at.is_some());
    assert_eq!(
        engine.state.lock().await.sources["aisstream.bridge.brickell"].last_success_ms,
        first_success,
        "a lost socket must not refresh source success"
    );
}

#[tokio::test]
async fn ais_secret_can_be_set_replaced_and_cleared_without_restart() {
    let store = Store::in_memory().await.unwrap();
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let factory = Arc::new(
        CredentialFreeCollectorFactory::new("BrickellStatus fixture (+https://example.invalid)")
            .unwrap(),
    );
    let engine = RuntimeEngine::initialize(store.clone(), RuntimeConfig::default(), factory, clock)
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.ais.enabled = true;
    preferences.ais.api_key_configured = true;
    engine.save_preferences(preferences).await.unwrap();
    assert!(
        !engine.get_preferences().await.ais.api_key_configured,
        "a serialized client flag cannot impersonate a host secret"
    );
    assert!(
        !engine
            .state
            .lock()
            .await
            .active_sources
            .contains_key("aisstream.bridge.brickell")
    );

    engine
        .set_aisstream_key(Some("first-fixture-aisstream-key".into()))
        .await
        .unwrap();
    assert!(engine.get_preferences().await.ais.api_key_configured);
    assert!(engine.get_preferences().await.ais.enabled);
    let armed = engine.get_aisstream_status().await.unwrap();
    assert_eq!(armed.connection_state, AisConnectionStateDto::Armed);
    assert!(armed.source_registered);
    assert_eq!(armed.fresh_vessel_count, 0);
    assert!(
        engine
            .state
            .lock()
            .await
            .active_sources
            .contains_key("aisstream.bridge.brickell")
    );

    engine
        .set_aisstream_key(Some("replacement-fixture-aisstream-key".into()))
        .await
        .unwrap();
    assert!(engine.get_preferences().await.ais.api_key_configured);
    assert!(engine.get_preferences().await.ais.enabled);
    assert!(
        !engine
            .state
            .lock()
            .await
            .sources
            .contains_key("aisstream.bridge.brickell")
    );

    engine.set_aisstream_key(None).await.unwrap();
    assert!(!engine.get_preferences().await.ais.api_key_configured);
    assert!(!engine.get_preferences().await.ais.enabled);
    let needs_key = engine.get_aisstream_status().await.unwrap();
    assert_eq!(needs_key.connection_state, AisConnectionStateDto::NeedsKey);
    assert!(!needs_key.source_registered);
    assert!(
        !engine
            .state
            .lock()
            .await
            .active_sources
            .contains_key("aisstream.bridge.brickell")
    );
    let persisted = store
        .get_json::<AppPreferences>(PREFERENCES_KEY)
        .await
        .unwrap()
        .unwrap();
    assert!(!persisted.ais.api_key_configured);
}

#[tokio::test]
async fn failed_ais_secret_transaction_restores_factory_and_published_state() {
    let store = Store::in_memory().await.unwrap();
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let factory = Arc::new(
        CredentialFreeCollectorFactory::new("BrickellStatus fixture (+https://example.invalid)")
            .unwrap(),
    );
    let engine = RuntimeEngine::initialize(
        store.clone(),
        RuntimeConfig::default(),
        factory.clone(),
        clock,
    )
    .await
    .unwrap();
    let mut enabled = engine.get_preferences().await;
    enabled.ais.enabled = true;
    engine.save_preferences(enabled).await.unwrap();
    let old_preferences = engine.get_preferences().await;
    let old_snapshot = engine.get_snapshot().await.unwrap();
    install_live_state_write_failure(&store).await;

    let result = engine
        .set_aisstream_key(Some("must-not-survive-storage-failure".into()))
        .await;

    assert!(matches!(result, Err(RuntimeError::Storage(_))));
    assert!(!factory.aisstream_key_configured().unwrap());
    assert_eq!(engine.get_preferences().await, old_preferences);
    assert_eq!(engine.get_snapshot().await.unwrap(), old_snapshot);
    let status = engine.get_aisstream_status().await.unwrap();
    assert_eq!(status.connection_state, AisConnectionStateDto::NeedsKey);
    assert!(!status.source_registered);
    assert_eq!(
        store
            .get_json::<AppPreferences>(PREFERENCES_KEY)
            .await
            .unwrap(),
        Some(old_preferences)
    );
}

#[tokio::test]
async fn moving_the_watched_span_retires_cached_positions_immediately() {
    let now_ms = 1_786_741_200_000;
    let store = Store::in_memory().await.unwrap();
    let clock = Arc::new(FixedClock(AtomicI64::new(now_ms)));
    let factory = Arc::new(
        CredentialFreeCollectorFactory::new("BrickellStatus fixture (+https://example.invalid)")
            .unwrap(),
    );
    let engine = RuntimeEngine::initialize(store, RuntimeConfig::default(), factory, clock)
        .await
        .unwrap();
    let mut preferences = engine.get_preferences().await;
    preferences.ais.enabled = true;
    engine.save_preferences(preferences).await.unwrap();
    engine
        .set_aisstream_key(Some("fixture-aisstream-key".into()))
        .await
        .unwrap();
    let source_id = "aisstream.bridge.brickell".to_owned();
    let mut cached = healthy_source_state("bridge.brickell", ais_bridge_item(), now_ms);
    cached.fail_closed_on_error = true;
    engine
        .state
        .lock()
        .await
        .sources
        .insert(source_id.clone(), cached);
    assert_eq!(
        engine
            .get_aisstream_status()
            .await
            .unwrap()
            .connection_state,
        AisConnectionStateDto::Live
    );

    // Moving the span moves the subscription, so every position cached against
    // the old one describes water this channel is no longer watching. The
    // radius used to be the thing a reader could change here; it is fixed now,
    // and the target is what remains.
    let mut moved = engine.get_preferences().await;
    let bridge = moved
        .profile
        .channels
        .iter_mut()
        .find(|channel| channel.kind == ChannelKindDto::Bridge)
        .unwrap();
    bridge.scope.insert("latitude".into(), json!(25.7712));
    engine.save_preferences(moved).await.unwrap();
    let resized_status = engine.get_aisstream_status().await.unwrap();
    assert_eq!(
        resized_status.connection_state,
        AisConnectionStateDto::Armed
    );
    assert_eq!(resized_status.fresh_vessel_count, 0);
    assert!(!engine.state.lock().await.sources.contains_key(&source_id));

    let mut recached = healthy_source_state("bridge.brickell", ais_bridge_item(), now_ms);
    recached.fail_closed_on_error = true;
    engine
        .state
        .lock()
        .await
        .sources
        .insert(source_id.clone(), recached);
    // Removing the key is how the source is turned off; there is no separate
    // switch to leave in the wrong position while a good key sits beside it.
    engine.set_aisstream_key(None).await.unwrap();
    let disabled_status = engine.get_aisstream_status().await.unwrap();
    assert_eq!(
        disabled_status.connection_state,
        AisConnectionStateDto::NeedsKey
    );
    assert!(!disabled_status.source_registered);
    assert!(!engine.state.lock().await.sources.contains_key(&source_id));
}

#[tokio::test]
async fn failed_persistence_keeps_new_backoff_and_source_state_in_memory() {
    let store = Store::in_memory().await.unwrap();
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let collector = Arc::new(SequenceCollector {
        calls: AtomicUsize::new(0),
        fail_after_first: false,
    });
    let engine = RuntimeEngine::initialize(
        store.clone(),
        RuntimeConfig::default(),
        Arc::new(StaticFactory { collector }),
        clock,
    )
    .await
    .unwrap();
    install_live_state_write_failure(&store).await;

    let result = engine.refresh_all().await;
    assert!(matches!(result, Err(RuntimeError::Storage(_))));
    assert!(
        store
            .get_json::<PersistedRuntimeState>(LIVE_STATE_KEY)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        engine.get_snapshot().await.unwrap().decision.state,
        BridgeStateDto::Open
    );
    let published = engine.state.lock().await;
    assert_eq!(published.sources.len(), 1);
    assert_eq!(published.last_cycle_ms, Some(1_786_741_200_000));
}

#[tokio::test]
async fn cached_data_moves_from_delayed_to_stale_after_failure() {
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let collector = Arc::new(SequenceCollector {
        calls: AtomicUsize::new(0),
        fail_after_first: true,
    });
    let engine = engine_with(collector, clock.clone()).await;
    engine.refresh_all().await.unwrap();
    clock.advance(30_000);
    engine.refresh_all().await.unwrap();
    let delayed = engine.get_snapshot().await.unwrap();
    assert_eq!(delayed.channels[0].availability, AvailabilityDto::Delayed);
    clock.advance(3 * 60_000);
    let stale = engine.get_snapshot().await.unwrap();
    assert_eq!(stale.channels[0].availability, AvailabilityDto::Stale);
    assert_eq!(stale.evidence[0].state, EvidenceStateDto::Stale);
}

#[test]
fn a_healthy_source_stays_fresh_until_its_stale_deadline() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let hurricane = preferences
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::Hurricane)
        .unwrap();
    let mut source = SourceState::empty(&hurricane.id);
    source.reported_health = HealthState::Healthy;
    source.last_success_ms = Some(now_ms - 5 * 60 * 60 * 1_000);

    assert_eq!(
        source_availability(&source, hurricane, now_ms).0,
        AvailabilityDto::Fresh
    );
    source.last_success_ms = Some(now_ms - 361 * 60 * 1_000);
    assert_eq!(
        source_availability(&source, hurricane, now_ms).0,
        AvailabilityDto::Stale
    );
}

#[tokio::test]
async fn collector_minimum_interval_limits_background_polling_but_not_manual_refresh() {
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let collector = Arc::new(SequenceCollector {
        calls: AtomicUsize::new(0),
        fail_after_first: false,
    });
    let engine = RuntimeEngine::initialize(
        Store::in_memory().await.unwrap(),
        RuntimeConfig::default(),
        Arc::new(CadencedFactory {
            collector: collector.clone(),
            minimum_interval: Duration::from_secs(300),
        }),
        clock.clone(),
    )
    .await
    .unwrap();

    assert_eq!(engine.refresh_due().await.unwrap().attempted, 1);
    clock.advance(60_000);
    let scheduled = engine.refresh_due().await.unwrap();
    assert_eq!(scheduled.attempted, 0);
    assert_eq!(scheduled.skipped_backoff, 1);
    assert_eq!(collector.calls.load(Ordering::SeqCst), 1);

    assert_eq!(engine.refresh_all().await.unwrap().attempted, 1);
    assert_eq!(collector.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn forecast_history_keeps_one_latest_material_sample_per_minute() {
    let now_ms = "2026-08-14T19:10:00Z"
        .parse::<Timestamp>()
        .unwrap()
        .as_millisecond();
    let clock = Arc::new(FixedClock(AtomicI64::new(now_ms)));
    let engine = engine_with(Arc::new(DownFl511Collector), clock).await;
    let mut state = PersistedRuntimeState::default();
    let clear = engine
        .predictor
        .evaluate(TimestampMillis(now_ms), &[], None)
        .unwrap();
    engine
        .record_forecast_sample(&clear, &mut state)
        .await
        .unwrap();

    let observed_at = TimestampMillis(now_ms + 15_000);
    let ais = BridgeEvidence {
        observation_id: ObservationId::from("ais-strong"),
        source_id: SourceId::from("aisstream.bridge.brickell"),
        observed_at,
        expires_at: None,
        availability: AvailabilityStatus::Live,
        reliability: Confidence::CERTAIN,
        fact: BridgeObservation::AisTrack {
            mmsi: Some("367000001".into()),
            vessel_name: Some("Test Vessel".into()),
            movement: VesselMovement::Approaching,
            route_intersects: true,
            schedule_exempt: false,
            eta: Some(EtaRangeMinutes::new(8, 12)),
            opening_propensity: Some(Confidence::CERTAIN),
        },
    };
    let likely = engine
        .predictor
        .evaluate(observed_at, std::slice::from_ref(&ais), Some(&clear))
        .unwrap();
    engine
        .record_forecast_sample(&likely, &mut state)
        .await
        .unwrap();
    let next_minute = engine
        .predictor
        .evaluate(TimestampMillis(now_ms + 60_000), &[ais], Some(&likely))
        .unwrap();
    engine
        .record_forecast_sample(&next_minute, &mut state)
        .await
        .unwrap();

    let samples = engine
        .store
        .forecast_samples_since(FORECAST_TARGET_KEY, now_ms, 10)
        .await
        .unwrap();
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0].evaluated_at_ms, now_ms + 15_000);
    assert_eq!(samples[0].state, "likely");
    assert!(samples[0].predictive_score_bps < 10_000);
    assert!(!samples[0].contribution_bps_json.contains("controller"));
    assert_eq!(samples[1].minute_bucket_ms, now_ms + 60_000);
}

#[tokio::test]
async fn cached_bridge_items_do_not_advance_confirmation_time() {
    let now_ms = 1_786_741_200_000;
    let clock = Arc::new(FixedClock(AtomicI64::new(now_ms)));
    let engine = engine_with(Arc::new(DownFl511Collector), clock).await;
    let source_id = "fl511.bridge.brickell";
    let mut source = healthy_source_state(
        "bridge.brickell",
        bridge_item("253", "Brickell Avenue Bridge", "target", "up"),
        now_ms - 60_000,
    );
    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([(source_id.into(), "bridge.brickell".into())]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(source_id.into(), source.clone());

    engine.persist_refresh(&state, now_ms).await.unwrap();
    assert!(
        engine
            .store
            .list_bridge_state_intervals(source_id, "brickell")
            .await
            .unwrap()
            .is_empty()
    );

    source.last_success_ms = Some(now_ms);
    state.sources.insert(source_id.into(), source);
    engine.persist_refresh(&state, now_ms).await.unwrap();
    let intervals = engine
        .store
        .list_bridge_state_intervals(source_id, "brickell")
        .await
        .unwrap();
    assert_eq!(intervals.len(), 1);
    assert_eq!(intervals[0].last_confirmed_at_ms, now_ms);
}

#[tokio::test]
async fn every_live_track_refreshes_the_full_vessel_catalog() {
    let now_ms = 1_786_741_200_000;
    let clock = Arc::new(FixedClock(AtomicI64::new(now_ms)));
    let engine = engine_with(Arc::new(DownFl511Collector), clock).await;
    let source_id = "aisstream.bridge.brickell";
    let mut source = SourceState::empty("bridge.brickell");
    source.cursor.metadata.insert(
        AIS_VESSEL_CATALOG_CURSOR_KEY.into(),
        json!([{
            "mmsi": "367000001",
            "observedAt": "2026-08-14T21:00:00Z",
            "positionObservedAt": "2026-08-14T21:00:00Z",
            "vesselName": "MIAMI STAR",
            "vesselClass": "yacht",
            "callSign": "WDF1234",
            "imoNumber": 9876543,
            "destination": "MIAMI RIVER",
            "lengthMeters": 31.4,
            "beamMeters": 7.1,
            "draughtMeters": 2.4,
            "latitude": 25.7698,
            "longitude": -80.1902,
            "speedKnots": 7.2,
            "courseDegrees": 265.0,
            "posture": "underway",
            "branch": "river",
            "sMeters": 51.0,
            "offsetMeters": -2.0
        }, {
            "mmsi": "368000002",
            "observedAt": "2026-08-14T21:00:00Z",
            "positionObservedAt": "2026-08-14T21:00:00Z",
            "vesselName": "BAY RUNNER",
            "vesselClass": "passenger",
            "latitude": 25.7702,
            "longitude": -80.1801,
            "speedKnots": 9.1,
            "courseDegrees": 270.0,
            "posture": "underway",
            "branch": "north_approach",
            "sMeters": -820.0,
            "offsetMeters": 8.0
        }])
        .to_string(),
    );
    source.cursor.metadata.insert(
        AIS_VESSEL_TRACKS_CURSOR_KEY.into(),
        json!([{
            "mmsi": "367000001",
            "vesselName": "MIAMI STAR",
            "movement": "approaching",
            "routeIntersects": true,
            "speedKnots": 7.2,
            "courseDegrees": 265.0,
            "observedAt": "2026-08-14T21:00:00Z",
            "vesselClass": "yacht",
            "callSign": "WDF1234",
            "imoNumber": 9876543,
            "destination": "MIAMI RIVER",
            "lengthMeters": 31.4,
            "beamMeters": 7.1,
            "draughtMeters": 2.4,
            "points": [{
                "latitude": 25.7698,
                "longitude": -80.1902,
                "observedAt": "2026-08-14T20:59:30Z",
                "speedKnots": 3.8,
                "courseDegrees": 271.0,
                "branch": "river",
                "sMeters": 42.0,
                "offsetMeters": -3.0
            }]
        }])
        .to_string(),
    );
    let state = PersistedRuntimeState {
        sources: BTreeMap::from([(source_id.into(), source)]),
        active_sources: BTreeMap::from([(source_id.into(), "bridge.brickell".into())]),
        ..PersistedRuntimeState::default()
    };

    engine.record_ais_track_fixes(&state).await.unwrap();
    let vessels = engine.store.list_ais_ledger(10).await.unwrap();
    let vessel = vessels
        .iter()
        .find(|vessel| vessel.mmsi == "367000001")
        .expect("live hull entered catalog");
    assert_eq!(vessel.name.as_deref(), Some("MIAMI STAR"));
    assert_eq!(vessel.call_sign.as_deref(), Some("WDF1234"));
    assert_eq!(vessel.imo_number, Some(9_876_543));
    assert_eq!(vessel.destination.as_deref(), Some("MIAMI RIVER"));
    assert_eq!(vessel.beam_meters, Some(7.1));
    assert!(
        vessels.iter().any(|vessel| vessel.mmsi == "368000002"),
        "the compact catalog must retain hulls outside the rich-history cap"
    );
    let fixes = engine.store.track_fixes_since(0).await.unwrap();
    let fix = fixes
        .iter()
        .find(|fix| fix.mmsi == "367000001")
        .expect("point entered track history");
    assert_eq!(fix.speed_knots, Some(3.8));
    assert_eq!(fix.course_degrees, Some(271.0));
    assert_eq!(fix.branch.as_deref(), Some("river"));
    assert_eq!(fix.s_meters, Some(42.0));
    assert_eq!(fix.offset_meters, Some(-3.0));
    let breadth_fix = fixes
        .iter()
        .find(|fix| fix.mmsi == "368000002")
        .expect("catalog latest fix entered broad movement history");
    assert_eq!(breadth_fix.speed_knots, Some(9.1));
    assert_eq!(breadth_fix.branch.as_deref(), Some("north_approach"));
    assert_eq!(breadth_fix.s_meters, Some(-820.0));
}

struct PanicCollector;

#[async_trait]
impl Collector for PanicCollector {
    fn name(&self) -> &'static str {
        "must-not-run"
    }

    async fn collect(&self, _context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        panic!("collector was not expected to run")
    }
}

#[tokio::test]
async fn preferences_persist_across_runtime_instances() {
    let store = Store::in_memory().await.unwrap();
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let factory = Arc::new(StaticFactory {
        collector: Arc::new(PanicCollector),
    });
    let first = RuntimeEngine::initialize(
        store.clone(),
        RuntimeConfig::default(),
        factory.clone(),
        clock.clone(),
    )
    .await
    .unwrap();
    let mut preferences = first.get_preferences().await;
    preferences.profile.name = "My bridge desk".into();
    preferences.profile.channels[4].enabled = false;
    preferences.profile.channels[6].enabled = true;
    first.save_preferences(preferences).await.unwrap();
    let second = RuntimeEngine::initialize(store, RuntimeConfig::default(), factory, clock)
        .await
        .unwrap();
    assert_eq!(
        second.get_preferences().await.profile.name,
        "My bridge desk"
    );
    assert!(!second.get_preferences().await.profile.channels[4].enabled);
    assert!(second.get_preferences().await.profile.channels[6].enabled);
}

#[tokio::test]
async fn failed_preference_transaction_keeps_durable_and_published_pair_unchanged() {
    let store = Store::in_memory().await.unwrap();
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let factory = Arc::new(StaticFactory {
        collector: Arc::new(PanicCollector),
    });
    let engine = RuntimeEngine::initialize(store.clone(), RuntimeConfig::default(), factory, clock)
        .await
        .unwrap();
    let old_preferences = engine.get_preferences().await;
    let old_snapshot = engine.get_snapshot().await.unwrap();
    let mut replacement = old_preferences.clone();
    replacement.profile.name = "Must roll back".into();
    install_live_state_write_failure(&store).await;

    let result = engine.save_preferences(replacement).await;
    assert!(matches!(result, Err(RuntimeError::Storage(_))));
    assert_eq!(engine.get_preferences().await, old_preferences);
    assert_eq!(engine.get_snapshot().await.unwrap(), old_snapshot);
    assert_eq!(
        store
            .get_json::<AppPreferences>(PREFERENCES_KEY)
            .await
            .unwrap(),
        Some(old_preferences)
    );
    assert!(
        store
            .get_json::<PersistedRuntimeState>(LIVE_STATE_KEY)
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn backoff_is_bounded() {
    let config = RuntimeConfig::default();
    assert_eq!(
        config.user_agent,
        "BrickellStatus/0.1 (+https://github.com/cmiami/BrickellStatus)"
    );
    assert_eq!(backoff_for(&config, 1), config.backoff_initial);
    assert_eq!(backoff_for(&config, 100), config.backoff_max);
}

#[test]
fn personal_rain_rule_activates_when_threshold_crosses_in_lead_window() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let item = weather_hourly_item(now_ms, 70.0, 10.0);
    let activation =
        evaluate_weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms);

    assert_eq!(activation.state, PersonalWeatherState::RainHeadsUp);
    assert!(activation.summary.starts_with("Personal rain heads-up"));
    assert!(weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms).1);
}

#[test]
fn personal_rain_rule_does_not_activate_below_threshold() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let item = weather_hourly_item(now_ms, 59.0, 10.0);
    let activation =
        evaluate_weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms);

    assert_eq!(activation.state, PersonalWeatherState::Normal);
    assert!(!activation.summary.contains("heads-up"));
    assert!(!weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms).1);
}

#[test]
fn personal_rain_rule_is_suppressed_for_stale_data() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let item = weather_hourly_item(now_ms, 90.0, 10.0);
    let activation =
        evaluate_weather_activation(&channel, &[&item], AvailabilityDto::Stale, now_ms);

    assert_eq!(activation.state, PersonalWeatherState::Stale);
    assert!(activation.summary.contains("personal rules suppressed"));
    assert!(!weather_activation(&channel, &[&item], AvailabilityDto::Stale, now_ms).1);
}

#[test]
fn personal_rain_rule_respects_disabled_setting() {
    let now_ms = 1_786_741_200_000;
    let mut channel = AppPreferences::default().profile.channels[1].clone();
    channel
        .scope
        .insert("rainAlertEnabled".into(), json!(false));
    let item = weather_hourly_item(now_ms, 90.0, 10.0);
    let activation =
        evaluate_weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms);

    assert_eq!(activation.state, PersonalWeatherState::Normal);
    assert!(!weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms).1);
}

#[test]
fn rain_probability_requires_percent_units_and_a_bounded_value() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let valid = weather_hourly_item(now_ms, 90.0, 10.0);
    assert!(weather_activation(&channel, &[&valid], AvailabilityDto::Fresh, now_ms).1);

    let mut unknown_unit = valid.clone();
    unknown_unit.attributes.insert(
        "units".into(),
        json!({"precipitation_probability": "ratio", "wind_gusts_10m": "km/h"}),
    );
    assert!(!weather_activation(&channel, &[&unknown_unit], AvailabilityDto::Fresh, now_ms).1);

    let out_of_range = weather_hourly_item(now_ms, 140.0, 10.0);
    assert!(!weather_activation(&channel, &[&out_of_range], AvailabilityDto::Fresh, now_ms).1);
}

#[test]
fn personal_wind_rule_respects_disabled_setting() {
    let now_ms = 1_786_741_200_000;
    let mut channel = AppPreferences::default().profile.channels[1].clone();
    channel
        .scope
        .insert("windAlertEnabled".into(), json!(false));
    let item = weather_hourly_item(now_ms, 10.0, 120.0);
    let activation =
        evaluate_weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms);

    assert_eq!(activation.state, PersonalWeatherState::Normal);
    assert!(!weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms).1);
}

#[test]
fn forecast_wind_rule_respects_hour_bucket_horizon_and_unit_allowlist() {
    let now_ms = 1_786_741_200_000;
    let mut channel = AppPreferences::default().profile.channels[1].clone();
    channel
        .scope
        .insert("rainAlertEnabled".into(), json!(false));
    channel.scope.insert("rainLeadMinutes".into(), json!(90));

    let valid = weather_hourly_item(now_ms, 10.0, 120.0);
    let valid_activation =
        evaluate_weather_activation(&channel, &[&valid], AvailabilityDto::Fresh, now_ms);
    assert_eq!(valid_activation.state, PersonalWeatherState::WindHeadsUp);
    assert!(valid_activation.summary.contains("in 30 min"));
    let signal = channel_signal(
        ChannelKindDto::Weather,
        &channel,
        &[&valid],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert!(signal.detail.contains("Gusts 75 mph in 30 min"));
    let metric_signal = channel_signal(
        ChannelKindDto::Weather,
        &channel,
        &[&valid],
        true,
        now_ms,
        UnitSystem::Metric,
    )
    .unwrap();
    assert!(metric_signal.detail.contains("Gusts 120 km/h in 30 min"));

    let mut beyond_horizon = valid.clone();
    beyond_horizon.starts_at =
        Some(chrono::DateTime::from_timestamp_millis(now_ms + 91 * 60_000).unwrap());
    let activation =
        evaluate_weather_activation(&channel, &[&beyond_horizon], AvailabilityDto::Fresh, now_ms);
    assert_eq!(activation.state, PersonalWeatherState::Normal);

    let mut old_bucket = valid.clone();
    old_bucket.starts_at =
        Some(chrono::DateTime::from_timestamp_millis(now_ms - 6 * 60_000).unwrap());
    let activation =
        evaluate_weather_activation(&channel, &[&old_bucket], AvailabilityDto::Fresh, now_ms);
    assert_eq!(activation.state, PersonalWeatherState::WindHeadsUp);

    let mut unknown_unit = valid;
    unknown_unit
        .attributes
        .insert("units".into(), json!({"wind_gusts_10m": "kn"}));
    let activation =
        evaluate_weather_activation(&channel, &[&unknown_unit], AvailabilityDto::Fresh, now_ms);
    assert_eq!(activation.state, PersonalWeatherState::Normal);
}

#[test]
fn current_wind_rule_requires_a_current_bounded_timestamp() {
    let now_ms = 1_786_741_200_000;
    let mut channel = AppPreferences::default().profile.channels[1].clone();
    channel
        .scope
        .insert("rainAlertEnabled".into(), json!(false));
    let mut item = weather_hourly_item(now_ms, 10.0, 120.0);
    item.kind = ItemKind::WeatherCurrent;
    item.starts_at = None;
    item.observed_at = Some(chrono::DateTime::from_timestamp_millis(now_ms).unwrap());
    assert!(weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms).1);

    item.observed_at = None;
    assert!(!weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms).1);
    item.observed_at =
        Some(chrono::DateTime::from_timestamp_millis(now_ms + 5 * 60_000 + 1).unwrap());
    assert!(!weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms).1);
    item.observed_at = Some(
        chrono::DateTime::from_timestamp_millis(
            now_ms - i64::from(channel.max_age_minutes) * 60_000 - 1,
        )
        .unwrap(),
    );
    assert!(!weather_activation(&channel, &[&item], AvailabilityDto::Fresh, now_ms).1);
}

#[test]
fn stale_area_weather_cannot_activate_beside_a_fresh_area() {
    let now_ms = 1_786_741_200_000;
    let mut preferences = AppPreferences::default();
    let mut second_area = preferences.areas[0].clone();
    second_area.id = "area.boston".into();
    second_area.label = "Boston, Massachusetts".into();
    second_area.latitude = 42.3601;
    second_area.longitude = -71.0589;
    preferences.areas.push(second_area);
    let channel = &mut preferences.profile.channels[1];
    channel
        .scope
        .insert("areaIds".into(), json!(["area.miami", "area.boston"]));

    let mut fresh_item = weather_hourly_item(now_ms, 10.0, 10.0);
    fresh_item.id = "weather:fresh-area".into();
    fresh_item
        .attributes
        .insert("area_label".into(), json!("Miami, Florida"));
    let mut stale_item = weather_hourly_item(now_ms, 95.0, 10.0);
    stale_item.id = "weather:stale-area".into();
    stale_item
        .attributes
        .insert("area_label".into(), json!("Boston, Massachusetts"));

    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([
            ("weather.area.fresh".into(), "weather.miami".into()),
            ("weather.area.stale".into(), "weather.miami".into()),
        ]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(
        "weather.area.fresh".into(),
        healthy_source_state("weather.miami", fresh_item, now_ms),
    );
    state.sources.insert(
        "weather.area.stale".into(),
        healthy_source_state("weather.miami", stale_item, now_ms - 16 * 60_000),
    );

    let channels = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let weather = channels
        .iter()
        .find(|channel| channel.id == "weather.miami")
        .unwrap();

    assert_eq!(weather.availability, AvailabilityDto::Delayed);
    assert_eq!(weather.age_seconds, 16 * 60);
    assert!(!weather.coverage_complete);
    assert!(!weather.active, "stale 95% rain must not activate");
    assert!(weather.signal.is_none());
    assert!(!weather.summary.contains("95%"));
    assert!(
        weather
            .summary
            .contains("partial coverage (1/2 sources usable)")
    );
    assert!(weather.summary.contains("stale/offline items suppressed"));
}

#[test]
fn stale_feed_item_cannot_activate_beside_a_fresh_feed() {
    let now_ms = 1_786_741_200_000;
    let mut preferences = AppPreferences::default();
    let channel = &mut preferences.profile.channels[4];
    channel.scope.insert(
        "feeds".into(),
        json!([
            "https://fresh.example/feed",
            "https://offline.example/feed",
            "https://stale.example/feed"
        ]),
    );
    // Stated here rather than inherited: the shipped default no longer filters
    // by topic, and this test is about staleness, not about what the default
    // happens to be.
    channel
        .scope
        .insert("topics".into(), json!(["Miami", "transportation"]));

    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([
            ("rss.feed.fresh".into(), "news.local".into()),
            ("rss.feed.offline".into(), "news.local".into()),
            ("rss.feed.stale".into(), "news.local".into()),
        ]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(
        "rss.feed.fresh".into(),
        healthy_source_state(
            "news.local",
            news_item("news:fresh", "Neighborhood arts calendar", now_ms),
            now_ms,
        ),
    );
    state
        .sources
        .get_mut("rss.feed.fresh")
        .unwrap()
        .items
        .push(news_item(
            "news:old-on-fresh-source",
            "Breaking Miami transportation archive",
            now_ms - 181 * 60_000,
        ));
    // The cached item itself has a recent publication time; its collector
    // cache is nevertheless stale and must remain non-actionable.
    state.sources.insert(
        "rss.feed.stale".into(),
        healthy_source_state(
            "news.local",
            news_item(
                "news:stale",
                "Breaking Miami transportation closure",
                now_ms,
            ),
            now_ms - 181 * 60_000,
        ),
    );
    let mut offline = healthy_source_state(
        "news.local",
        news_item(
            "news:offline",
            "Breaking Miami transportation emergency",
            now_ms,
        ),
        now_ms,
    );
    offline.last_success_ms = None;
    state.sources.insert("rss.feed.offline".into(), offline);

    let channels = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let news = channels
        .iter()
        .find(|channel| channel.id == "news.local")
        .unwrap();

    assert_eq!(news.availability, AvailabilityDto::Delayed);
    assert_eq!(news.age_seconds, 181 * 60);
    assert!(!news.coverage_complete);
    assert!(
        !news.active,
        "stale or offline matching feed items must not activate"
    );
    assert!(news.signal.is_none());
    assert!(news.summary.contains("Nothing new"));
    assert!(
        news.summary
            .contains("partial coverage (1/3 sources usable)")
    );
    assert!(news.summary.contains("stale/offline items suppressed"));
}

#[test]
fn partial_source_loss_cannot_look_like_a_trustworthy_resolution() {
    let now_ms = 1_786_741_200_000;
    let mut preferences = AppPreferences::default();
    preferences.profile.channels[4].scope.insert(
        "feeds".into(),
        json!(["https://match.example/feed", "https://quiet.example/feed"]),
    );
    // Stated here rather than inherited: the shipped default no longer filters
    // by topic, and this test needs one source to match and one to stay quiet.
    preferences.profile.channels[4]
        .scope
        .insert("topics".into(), json!(["Miami", "transportation"]));
    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([
            ("rss.match".into(), "news.local".into()),
            ("rss.quiet".into(), "news.local".into()),
        ]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(
        "rss.match".into(),
        healthy_source_state(
            "news.local",
            news_item("news:match", "Miami transportation closure", now_ms),
            now_ms,
        ),
    );
    state.sources.insert(
        "rss.quiet".into(),
        healthy_source_state(
            "news.local",
            news_item("news:quiet", "Neighborhood arts calendar", now_ms),
            now_ms,
        ),
    );

    let before = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let before = before
        .iter()
        .find(|channel| channel.id == "news.local")
        .unwrap();
    assert_eq!(before.availability, AvailabilityDto::Fresh);
    assert!(before.coverage_complete);
    assert!(before.active);
    assert!(before.signal.is_some());

    // A delayed source remains usable, so complete delayed coverage is
    // distinguishable from partial source loss.
    state.sources.get_mut("rss.quiet").unwrap().last_error = Some("fixture delay".into());
    let delayed = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let delayed = delayed
        .iter()
        .find(|channel| channel.id == "news.local")
        .unwrap();
    assert_eq!(delayed.availability, AvailabilityDto::Delayed);
    assert!(delayed.coverage_complete);
    assert!(delayed.active);

    // Losing the only matching source must not emit an inactive snapshot
    // that a consumer could mistake for a trustworthy all-clear.
    state.sources.get_mut("rss.match").unwrap().last_success_ms = None;
    let after = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let after = after
        .iter()
        .find(|channel| channel.id == "news.local")
        .unwrap();
    assert_eq!(after.availability, AvailabilityDto::Delayed);
    assert!(!after.coverage_complete);
    assert!(!after.active);
    assert!(after.signal.is_none());
    assert!(
        after
            .summary
            .contains("partial coverage (1/2 sources usable)")
    );
}

#[test]
fn news_item_age_uses_the_entry_timestamp_and_fails_closed() {
    let now_ms = 1_786_741_200_000;
    let channel = &AppPreferences::default().profile.channels[4];
    let at_limit = news_item(
        "news:at-limit",
        "Miami transportation update",
        now_ms - i64::from(channel.max_age_minutes) * 60_000,
    );
    let too_old = news_item(
        "news:too-old",
        "Miami transportation update",
        now_ms - i64::from(channel.max_age_minutes) * 60_000 - 1,
    );
    let near_future = news_item(
        "news:clock-skew",
        "Miami transportation update",
        now_ms + 5 * 60_000,
    );
    let far_future = news_item(
        "news:future",
        "Miami transportation update",
        now_ms + 5 * 60_000 + 1,
    );
    let mut missing_time = news_item("news:no-time", "Miami transportation update", now_ms);
    missing_time.observed_at = None;

    assert!(news_item_matches_scope(&at_limit, channel, now_ms));
    assert!(news_item_matches_scope(&near_future, channel, now_ms));
    assert!(!news_item_matches_scope(&too_old, channel, now_ms));
    assert!(!news_item_matches_scope(&far_future, channel, now_ms));
    assert!(!news_item_matches_scope(&missing_time, channel, now_ms));
}

#[test]
fn a_cyclone_activates_only_when_it_comes_within_range_of_a_saved_place() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let channel = preferences.profile.channels[3].clone();
    let atlantic = tropical_item("al092026");
    let pacific = tropical_item("ep052026");
    let coverage = ChannelCoverage {
        availability: AvailabilityDto::Fresh,
        age_seconds: 0,
        total_sources: 1,
        usable_sources: 1,
        fresh_sources: 1,
    };

    let (pacific_summary, pacific_active) = channel_summary(
        ChannelKindDto::Hurricane,
        &channel,
        &[&pacific],
        &clear_decision(),
        &coverage,
        now_ms,
        &preferences.areas,
        UnitSystem::Imperial,
    );
    assert!(!pacific_active);
    assert!(pacific_summary.contains("No active Atlantic cyclone"));

    // A storm in the basin but nowhere near a saved place is context, not an
    // alert. This replaces a switch that read "every Atlantic cyclone" and
    // defaulted off, which is why the channel could never activate at all.
    let distant = tropical_item_at("al032026", 15.0, -45.0);
    let (distant_summary, distant_active) = channel_summary(
        ChannelKindDto::Hurricane,
        &channel,
        &[&distant],
        &clear_decision(),
        &coverage,
        now_ms,
        &preferences.areas,
        UnitSystem::Imperial,
    );
    assert!(!distant_active);
    assert!(
        distant_summary.contains("none within range"),
        "{distant_summary}"
    );

    let (near_summary, near_active) = channel_summary(
        ChannelKindDto::Hurricane,
        &channel,
        &[&atlantic],
        &clear_decision(),
        &coverage,
        now_ms,
        &preferences.areas,
        UnitSystem::Imperial,
    );
    assert!(near_active);
    assert!(near_summary.contains("within range"), "{near_summary}");

    // A storm with no position cannot be placed, and an unplaceable storm is
    // not evidence of a nearby one.
    let mut unplaced = tropical_item("al042026");
    unplaced.location = None;
    assert!(
        !channel_summary(
            ChannelKindDto::Hurricane,
            &channel,
            &[&unplaced],
            &clear_decision(),
            &coverage,
            now_ms,
            &preferences.areas,
            UnitSystem::Imperial,
        )
        .1
    );
}

#[test]
fn material_key_changes_for_same_count_alert_replacements() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let decision = clear_decision();
    let key = |kind, channel: &ChannelPreference, item: &CollectorItem| {
        channel_material_key(kind, channel, &[item], &decision, true, now_ms)
    };

    let official_channel = &preferences.profile.channels[2];
    let official_a = official_alert_item("nws:alert-a", "Alert", "Actual", Some(now_ms + 60_000));
    let official_b = official_alert_item("nws:alert-b", "Alert", "Actual", Some(now_ms + 60_000));
    assert_ne!(
        key(ChannelKindDto::Official, official_channel, &official_a),
        key(ChannelKindDto::Official, official_channel, &official_b)
    );

    let news_channel = &preferences.profile.channels[4];
    let news_a = news_item("news:a", "Miami transportation update", now_ms);
    let news_b = news_item("news:b", "Miami transportation update", now_ms);
    assert_ne!(
        key(ChannelKindDto::News, news_channel, &news_a),
        key(ChannelKindDto::News, news_channel, &news_b)
    );

    let tropical_channel = &preferences.profile.channels[3];
    let storm_a = tropical_item("al012026");
    let storm_b = tropical_item("al022026");
    assert_ne!(
        key(ChannelKindDto::Hurricane, tropical_channel, &storm_a),
        key(ChannelKindDto::Hurricane, tropical_channel, &storm_b)
    );

    let mut earthquake_channel = preferences.profile.channels[5].clone();
    earthquake_channel.enabled = true;
    let earthquake_a = earthquake_item("usgs:a", now_ms);
    let earthquake_b = earthquake_item("usgs:b", now_ms);
    assert_ne!(
        key(
            ChannelKindDto::Earthquake,
            &earthquake_channel,
            &earthquake_a
        ),
        key(
            ChannelKindDto::Earthquake,
            &earthquake_channel,
            &earthquake_b
        )
    );
}

#[test]
fn material_key_is_order_independent_for_the_same_item_set() {
    let now_ms = 1_786_741_200_000;
    let channel = &AppPreferences::default().profile.channels[4];
    let decision = clear_decision();
    let first = news_item("news:a", "Miami transportation update", now_ms);
    let second = news_item("news:b", "Miami transportation update", now_ms);

    let forward = channel_material_key(
        ChannelKindDto::News,
        channel,
        &[&first, &second],
        &decision,
        true,
        now_ms,
    );
    let reverse = channel_material_key(
        ChannelKindDto::News,
        channel,
        &[&second, &first],
        &decision,
        true,
        now_ms,
    );

    assert_eq!(forward, reverse);
}

#[test]
fn official_signal_exposes_bounded_provider_content_and_replacement_identity() {
    let now_ms = 1_786_741_200_000;
    let expires_ms = now_ms + 30 * 60_000;
    let preferences = AppPreferences::default();
    let mut first = official_alert_item("nws:alert-a", "Alert", "Actual", Some(expires_ms));
    first.title = "Flash Flood Warning".into();
    first.summary = Some("Flash flooding is occurring near downtown Miami.".into());
    first.attributes.insert(
        "instruction".into(),
        json!("Move to higher ground now. Do not enter flooded roads."),
    );
    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([("nws.area.miami".into(), "official.miami".into())]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(
        "nws.area.miami".into(),
        healthy_source_state("official.miami", first, now_ms),
    );

    let first_snapshot = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let first_snapshot = first_snapshot
        .iter()
        .find(|channel| channel.id == "official.miami")
        .unwrap();
    let first_key = first_snapshot.material_key.clone();
    let first_summary = first_snapshot.summary.clone();
    let signal = first_snapshot.signal.as_ref().unwrap();
    assert_eq!(signal.headline, "Flash Flood Warning");
    assert_eq!(
        signal.detail,
        "Flash flooding is occurring near downtown Miami."
    );
    assert_eq!(signal.action, "This alert is in force now.");
    assert_eq!(signal.severity.as_deref(), Some("Severe"));
    assert_eq!(
        signal.expires_at.as_deref(),
        Some(iso_timestamp(expires_ms).unwrap().as_str())
    );
    let json = serde_json::to_value(first_snapshot).unwrap();
    assert_eq!(json["coverageComplete"], true);
    assert_eq!(
        json["signal"]["expiresAt"],
        iso_timestamp(expires_ms).unwrap()
    );

    let mut replacement = official_alert_item("nws:alert-b", "Update", "Actual", Some(expires_ms));
    replacement.title = "Tornado Warning".into();
    replacement.summary = Some("A confirmed tornado is moving northeast.".into());
    replacement.attributes.insert(
        "instruction".into(),
        json!("Take shelter in an interior room immediately."),
    );
    state.sources.get_mut("nws.area.miami").unwrap().items = vec![replacement];

    let replacement_snapshot = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let replacement_snapshot = replacement_snapshot
        .iter()
        .find(|channel| channel.id == "official.miami")
        .unwrap();
    assert_eq!(replacement_snapshot.summary, first_summary);
    assert_ne!(replacement_snapshot.material_key, first_key);
    let replacement_signal = replacement_snapshot.signal.as_ref().unwrap();
    assert_eq!(replacement_signal.headline, "Tornado Warning");
    assert_eq!(replacement_signal.action, "This alert is in force now.");
    assert_eq!(replacement_signal.severity.as_deref(), Some("Severe"));
}

#[test]
fn news_signal_exposes_publisher_content_and_replacement_identity() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let mut first = news_item(
        "news:first",
        "Breaking Miami transportation closure",
        now_ms,
    );
    first.summary = Some("The downtown ramp closes at 6 PM for emergency repairs.".into());
    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([("rss.local".into(), "news.local".into())]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(
        "rss.local".into(),
        healthy_source_state("news.local", first, now_ms),
    );

    let first_snapshot = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let first_snapshot = first_snapshot
        .iter()
        .find(|channel| channel.id == "news.local")
        .unwrap();
    let first_key = first_snapshot.material_key.clone();
    let first_summary = first_snapshot.summary.clone();
    let signal = first_snapshot.signal.as_ref().unwrap();
    assert_eq!(signal.headline, "Breaking Miami transportation closure");
    assert_eq!(
        signal.detail,
        "The downtown ramp closes at 6 PM for emergency repairs."
    );
    // The action line names the publisher that filed the item. It used to be a
    // fixed sentence on every card, which the panel cut to "OPEN THE S>" and
    // which told the reader nothing either way.
    assert_eq!(signal.action, "Fixture feed");
    assert_eq!(signal.severity.as_deref(), Some("Breaking"));

    let mut replacement = news_item(
        "news:replacement",
        "Miami transportation service restored",
        now_ms,
    );
    replacement.summary = Some("Metrorail service has resumed after an earlier delay.".into());
    state.sources.get_mut("rss.local").unwrap().items = vec![replacement];

    let replacement_snapshot = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let replacement_snapshot = replacement_snapshot
        .iter()
        .find(|channel| channel.id == "news.local")
        .unwrap();
    assert_eq!(replacement_snapshot.summary, first_summary);
    assert_ne!(replacement_snapshot.material_key, first_key);
    let replacement_signal = replacement_snapshot.signal.as_ref().unwrap();
    assert_eq!(
        replacement_signal.headline,
        "Miami transportation service restored"
    );
    assert_eq!(replacement_signal.severity.as_deref(), Some("Routine"));
}

#[test]
fn authored_notices_publish_their_automatic_expiration_without_a_provider_end() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let observed_ms = now_ms - 15 * 60_000;

    let news_channel = preferences
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::News)
        .unwrap();
    let news = news_item("news:expiry", "Miami transportation update", observed_ms);
    let news_signal = channel_signal(
        ChannelKindDto::News,
        news_channel,
        &[&news],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(
        news_signal.expires_at.as_deref(),
        Some(
            iso_timestamp(observed_ms + i64::from(news_channel.max_age_minutes) * 60_000)
                .unwrap()
                .as_str()
        )
    );

    let sports_channel = preferences
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::Sports)
        .unwrap();
    let sports = news_item("sports:expiry", "Miami team roster update", observed_ms);
    let sports_signal = channel_signal(
        ChannelKindDto::Sports,
        sports_channel,
        &[&sports],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(
        sports_signal.expires_at.as_deref(),
        Some(
            iso_timestamp(observed_ms + i64::from(sports_channel.max_age_minutes) * 60_000)
                .unwrap()
                .as_str()
        )
    );

    let earthquake_channel = preferences
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::Earthquake)
        .unwrap();
    let earthquake = earthquake_item("usgs:expiry", observed_ms);
    let earthquake_signal = channel_signal(
        ChannelKindDto::Earthquake,
        earthquake_channel,
        &[&earthquake],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(
        earthquake_signal.expires_at.as_deref(),
        Some(
            iso_timestamp(observed_ms + 1_440 * 60_000)
                .unwrap()
                .as_str()
        )
    );

    for (kind, mut item) in [
        (ChannelKindDto::Hurricane, tropical_item("al01")),
        (ChannelKindDto::Markets, market_quote_item(6.5, None)),
    ] {
        item.observed_at = chrono::DateTime::from_timestamp_millis(observed_ms);
        let channel = preferences
            .profile
            .channels
            .iter()
            .find(|channel| channel.kind == kind)
            .unwrap();
        let signal =
            channel_signal(kind, channel, &[&item], true, now_ms, UnitSystem::Imperial).unwrap();
        assert_eq!(
            signal.expires_at.as_deref(),
            Some(
                iso_timestamp(observed_ms + i64::from(channel.max_age_minutes) * 60_000)
                    .unwrap()
                    .as_str()
            )
        );
    }

    let mut provider_bounded = news;
    provider_bounded.ends_at = chrono::DateTime::from_timestamp_millis(now_ms + 10 * 60_000);
    let provider_bounded = channel_signal(
        ChannelKindDto::News,
        news_channel,
        &[&provider_bounded],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(
        provider_bounded.expires_at.as_deref(),
        Some(iso_timestamp(now_ms + 10 * 60_000).unwrap().as_str()),
        "a provider end remains authoritative"
    );
}

#[test]
fn every_current_item_becomes_a_notice_regardless_of_the_legacy_slide_cap() {
    let now_ms = 1_786_741_200_000;
    let mut preferences = AppPreferences::default();
    let news = preferences
        .profile
        .channels
        .iter_mut()
        .find(|channel| channel.id == "news.local")
        .unwrap();
    // Stored profiles may still carry this retired control. It must no longer
    // decide how many current items survive into the slide set.
    news.max_items = 1;

    let routine = news_item("news:routine", "Miami transportation update", now_ms);
    let breaking = news_item(
        "news:breaking",
        "Breaking Miami transportation closure",
        now_ms,
    );
    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([("rss.local".into(), "news.local".into())]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(
        "rss.local".into(),
        healthy_source_state("news.local", routine, now_ms),
    );
    state
        .sources
        .get_mut("rss.local")
        .unwrap()
        .items
        .push(breaking);

    let snapshots = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let news = snapshots
        .iter()
        .find(|channel| channel.id == "news.local")
        .unwrap();

    assert_eq!(news.notices.len(), 2);
    assert_eq!(
        news.notices[0].signal.headline,
        "Breaking Miami transportation closure"
    );
    assert_eq!(news.notices[0].priority.urgency, UrgencyDto::HeadsUp);
    assert_eq!(news.notices[1].priority.urgency, UrgencyDto::Routine);
    assert_eq!(news.signal.as_ref(), Some(&news.notices[0].signal));
    assert_ne!(news.notices[0].key, news.notices[1].key);
    assert!(
        news.notices
            .iter()
            .all(|notice| { notice.source_url.as_deref() == Some("https://example.com/feed.xml") })
    );
}

#[test]
fn syndicated_signal_uses_new_context_instead_of_repeating_the_headline() {
    let now_ms = 1_786_741_200_000;
    let channel = &AppPreferences::default().profile.channels[4];
    let title = "The Data Center Backlash Bursts Into the Midterms";
    let mut clustered = news_item("news:cluster", title, now_ms - 18 * 60_000);
    clustered.source.name = "The New York Times".into();
    clustered.source.url =
        Some(Url::parse("https://news.google.com/rss/articles/cluster-id").unwrap());
    clustered.summary = Some(format!(
        "{title} The New York Times See the moment politicians turned against data centers"
    ));

    let signal = channel_signal(
        ChannelKindDto::News,
        channel,
        &[&clustered],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(signal.detail, "Published 18 min ago");

    let mut no_synopsis = news_item("news:no-summary", title, now_ms - 18 * 60_000);
    no_synopsis.summary = Some(title.into());
    no_synopsis
        .attributes
        .insert("authors".into(), json!(["Signals Desk"]));
    let signal = channel_signal(
        ChannelKindDto::News,
        channel,
        &[&no_synopsis],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(signal.detail, "By Signals Desk · 18 min ago");
}

#[test]
fn signal_text_removes_controls_and_is_character_bounded() {
    let hostile = format!("\u{202e}\n{}\u{0007}", "x".repeat(300));
    let bounded = bounded_signal_text(&hostile, 160);
    assert!(bounded.chars().count() <= 160);
    assert!(!bounded.chars().any(char::is_control));
    assert!(!bounded.contains('\u{202e}'));
    assert!(bounded.ends_with('…'));
}

#[test]
fn official_alerts_require_current_actual_alert_or_update_records() {
    let now_ms = 1_786_741_200_000;
    let channel = &AppPreferences::default().profile.channels[2];
    let future = now_ms + 30 * 60_000;

    let alert = official_alert_item("nws:alert", "Alert", "Actual", Some(future));
    let update = official_alert_item("nws:update", "Update", "actual", Some(future));
    let cancelled = official_alert_item("nws:cancel", "Cancel", "Actual", Some(future));
    let expired = official_alert_item("nws:expired", "Alert", "Actual", Some(now_ms));
    let missing_expiry = official_alert_item("nws:no-expiry", "Alert", "Actual", None);
    let exercise = official_alert_item("nws:exercise", "Alert", "Exercise", Some(future));
    let unknown_type = official_alert_item("nws:unknown", "Unknown", "Actual", Some(future));

    assert!(official_alert_matches_scope(&alert, channel, now_ms));
    assert!(official_alert_matches_scope(&update, channel, now_ms));
    assert!(!official_alert_matches_scope(&cancelled, channel, now_ms));
    assert!(!official_alert_matches_scope(&expired, channel, now_ms));
    assert!(!official_alert_matches_scope(
        &missing_expiry,
        channel,
        now_ms
    ));
    assert!(!official_alert_matches_scope(&exercise, channel, now_ms));
    assert!(!official_alert_matches_scope(
        &unknown_type,
        channel,
        now_ms
    ));
}

#[test]
fn expired_official_alert_does_not_activate_from_a_fresh_source() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let mut state = PersistedRuntimeState {
        active_sources: BTreeMap::from([("nws.area.miami".into(), "official.miami".into())]),
        ..PersistedRuntimeState::default()
    };
    state.sources.insert(
        "nws.area.miami".into(),
        healthy_source_state(
            "official.miami",
            official_alert_item("nws:expired", "Alert", "Actual", Some(now_ms - 1)),
            now_ms,
        ),
    );

    let channels = channel_snapshots(&preferences, &state, &clear_decision(), now_ms);
    let official = channels
        .iter()
        .find(|channel| channel.id == "official.miami")
        .unwrap();

    assert_eq!(official.availability, AvailabilityDto::Fresh);
    assert!(official.coverage_complete);
    assert!(!official.active);
    assert!(official.signal.is_none());
    assert!(official.summary.contains("No alerts in force"));
}

#[test]
fn earthquakes_require_current_non_deleted_event_timestamps() {
    let now_ms = 1_786_741_200_000;
    let channel = &AppPreferences::default().profile.channels[5];
    let valid = earthquake_item("usgs:valid", now_ms);
    assert!(earthquake_matches_scope(&valid, channel, now_ms));

    let mut missing_time = valid.clone();
    missing_time.observed_at = None;
    assert!(!earthquake_matches_scope(&missing_time, channel, now_ms));

    let mut future = valid.clone();
    future.observed_at =
        Some(chrono::DateTime::from_timestamp_millis(now_ms + 5 * 60_000 + 1).unwrap());
    assert!(!earthquake_matches_scope(&future, channel, now_ms));

    let mut deleted = valid;
    deleted.attributes.insert("status".into(), json!("deleted"));
    assert!(!earthquake_matches_scope(&deleted, channel, now_ms));

    let mut missing_status = earthquake_item("usgs:missing-status", now_ms);
    missing_status.attributes.remove("status");
    assert!(!earthquake_matches_scope(&missing_status, channel, now_ms));

    let mut unknown_status = earthquake_item("usgs:unknown-status", now_ms);
    unknown_status
        .attributes
        .insert("status".into(), json!("superseded"));
    assert!(!earthquake_matches_scope(&unknown_status, channel, now_ms));
}

#[test]
fn market_move_rule_activates_with_price_session_and_delay_semantics() {
    let mut channel = AppPreferences::default().profile.channels[6].clone();
    channel.enabled = true;
    let item = market_quote_item(6.46, Some(15));

    let (summary, active) = market_activation(&channel, &[&item], AvailabilityDto::Delayed);

    assert!(active);
    assert!(summary.contains("AMD 172.40 USD +6.46% OPEN"));
    assert!(summary.contains("move ≥5.0%"));
    assert!(summary.contains("provider reports 15 min delay"));
}

#[test]
fn market_move_rule_requires_symbols_and_suppresses_stale_quotes() {
    let mut channel = AppPreferences::default().profile.channels[6].clone();
    channel.enabled = true;
    let item = market_quote_item(9.0, None);

    channel.scope.insert("symbols".into(), json!([]));
    let (missing_symbols, active) = market_activation(&channel, &[&item], AvailabilityDto::Fresh);
    assert!(!active);
    assert!(missing_symbols.contains("Add at least one market symbol"));

    channel.scope.insert("symbols".into(), json!(["AMD"]));
    let (stale, active) = market_activation(&channel, &[&item], AvailabilityDto::Stale);
    assert!(!active);
    assert!(stale.contains("move rule suppressed"));
}

#[test]
fn area_changes_invalidate_every_selected_area_channel() {
    let old = AppPreferences::default();
    let mut new = old.clone();
    new.areas[0].enabled = false;

    let changed = changed_channel_ids(&old, &new);
    assert!(changed.contains("weather.miami"));
    assert!(changed.contains("official.miami"));
    assert!(changed.contains("hurricane.atlantic"));
    assert!(!changed.contains("bridge.brickell"));
}

#[test]
fn snapshot_area_context_names_enabled_selected_areas() {
    let mut preferences = AppPreferences::default();
    let mut boston = preferences.areas[0].clone();
    boston.id = "area.boston".into();
    boston.label = "Boston, Massachusetts".into();
    boston.latitude = 42.3601;
    boston.longitude = -71.0589;
    preferences.areas.push(boston);
    let channel = &mut preferences.profile.channels[1];
    channel
        .scope
        .insert("areaIds".into(), json!(["area.miami", "area.boston"]));

    assert_eq!(
        area_context_label(channel, &preferences.areas).as_deref(),
        Some("Miami, Florida / Boston, Massachusetts")
    );
    preferences.areas[1].weather_enabled = false;
    assert_eq!(
        area_context_label(channel, &preferences.areas).as_deref(),
        Some("Miami, Florida")
    );
}

/// Emits a fixed FL511 bridge set, so a test can stage an acquisition fault.
struct StagedBridgeCollector {
    items: Vec<CollectorItem>,
}

#[async_trait]
impl Collector for StagedBridgeCollector {
    fn name(&self) -> &'static str {
        "fixture-staged-bridges"
    }

    async fn collect(&self, _context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        Ok(CollectorBatch {
            source: self.name().into(),
            items: self.items.clone(),
            health: CollectorHealth::healthy(),
            cursor: CollectorCursor::default(),
            not_modified: false,
        })
    }
}

async fn intervals_after(items: Vec<CollectorItem>, bridge_key: &str) -> Vec<String> {
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let engine = engine_with(Arc::new(StagedBridgeCollector { items }), clock).await;
    engine.refresh_all().await.unwrap();
    engine
        .store
        .list_bridge_state_intervals("fl511.bridge.brickell", bridge_key)
        .await
        .unwrap()
        .into_iter()
        .map(|interval| interval.state)
        .collect()
}

#[tokio::test]
async fn a_single_unresolved_bridge_is_still_recorded() {
    // One bridge FL511 cannot resolve is durable evidence about that span, so
    // it must survive: suppressing it would hide a genuinely broken selector.
    let states = intervals_after(
        vec![
            bridge_item("brickell", "Brickell Avenue Bridge", "target", "up"),
            bridge_item("sw_2_ave", "SW 2 Ave Bridge", "upstream", "unknown"),
            bridge_item("sw_1_st", "SW 1 St Bridge", "upstream", "down"),
        ],
        "sw_2_ave",
    )
    .await;
    assert_eq!(states, vec!["unknown".to_string()]);
}

#[tokio::test]
async fn correlated_unknown_readings_are_not_recorded_as_observations() {
    // Two bridges cannot both become "unknown" because of anything on the
    // river; that is a failed fetch. Recording it splits a real interval in two
    // and inflates the opening count for the affected spans.
    let states = intervals_after(
        vec![
            bridge_item("brickell", "Brickell Avenue Bridge", "target", "up"),
            bridge_item("sw_2_ave", "SW 2 Ave Bridge", "upstream", "unknown"),
            bridge_item("sw_1_st", "SW 1 St Bridge", "upstream", "unknown"),
        ],
        "sw_2_ave",
    )
    .await;
    assert!(states.is_empty(), "expected no interval, got {states:?}");
}

#[tokio::test]
async fn a_resolved_bridge_is_still_recorded_during_a_correlated_fault() {
    // The fault is per-reading, not per-pass: bridges that did resolve are
    // still trustworthy and must not be dropped alongside the bad ones.
    let states = intervals_after(
        vec![
            bridge_item("brickell", "Brickell Avenue Bridge", "target", "up"),
            bridge_item("sw_2_ave", "SW 2 Ave Bridge", "upstream", "unknown"),
            bridge_item("sw_1_st", "SW 1 St Bridge", "upstream", "unknown"),
        ],
        "brickell",
    )
    .await;
    assert_eq!(states, vec!["up".to_string()]);
}

#[tokio::test]
async fn every_recorded_interval_carries_the_engine_session() {
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let engine = engine_with(
        Arc::new(StagedBridgeCollector {
            items: vec![bridge_item(
                "brickell",
                "Brickell Avenue Bridge",
                "target",
                "up",
            )],
        }),
        clock,
    )
    .await;
    engine.refresh_all().await.unwrap();
    let intervals = engine
        .store
        .list_bridge_state_intervals("fl511.bridge.brickell", "brickell")
        .await
        .unwrap();
    assert_eq!(intervals.len(), 1);
    assert!(
        intervals[0]
            .session_id
            .as_deref()
            .is_some_and(|id| !id.is_empty()),
        "an interval with no session cannot be told apart from a restart artifact"
    );
}

#[tokio::test]
async fn a_scheduled_vessel_movement_never_appears_as_a_bridge_reading() {
    // The bridge surfaces exist to report the state of the bascule. A ship's
    // timetable belongs in the prediction, not in the list of readings for the
    // bridge itself, where it buries the one thing those surfaces are for.
    let clock = Arc::new(FixedClock(AtomicI64::new(1_786_741_200_000)));
    let mut movement = bridge_item("brickell", "PEPIN EXPRESS", "target", "up");
    movement.kind = ItemKind::VesselMovement;
    movement.attributes.insert("river".into(), json!(true));
    movement
        .attributes
        .insert("vessel".into(), json!("PEPIN EXPRESS"));
    movement.attributes.insert("tug".into(), json!("MRT"));
    movement
        .attributes
        .insert("bridge_eta_at".into(), json!("2026-08-13T22:30:00+00:00"));

    let engine = engine_with(
        Arc::new(StagedBridgeCollector {
            items: vec![
                bridge_item("brickell", "Brickell Avenue Bridge", "target", "down"),
                movement,
            ],
        }),
        clock,
    )
    .await;
    engine.refresh_all().await.unwrap();
    let snapshot = engine.get_snapshot().await.unwrap();

    assert!(
        snapshot
            .evidence
            .iter()
            .all(|strip| !strip.title.contains("PEPIN")),
        "a booked transit must not be listed as a bridge reading: {:?}",
        snapshot
            .evidence
            .iter()
            .map(|strip| strip.title.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        snapshot
            .evidence
            .iter()
            .any(|strip| strip.source_label == "Bridge status reporting"),
        "the bascule reading itself must still be shown"
    );
}

#[test]
fn a_slow_source_is_not_stale_merely_for_keeping_its_own_schedule() {
    // The pilots' board is collected every ten minutes while the bridge channel
    // tolerates two. Judging it against the channel budget alone marks it stale
    // for eight minutes out of every ten and reports a permanent fault that is
    // really a schedule.
    let now_ms = 1_786_741_200_000;
    let mut channel = AppPreferences::default()
        .profile
        .channels
        .into_iter()
        .find(|channel| channel.kind == ChannelKindDto::Bridge)
        .expect("the default profile has a bridge channel");
    channel.max_age_minutes = 2;

    let mut slow = SourceState::empty(&channel.id);
    slow.reported_health = HealthState::Healthy;
    slow.last_success_ms = Some(now_ms - 5 * 60 * 1_000);
    slow.poll_interval_ms = Some(10 * 60 * 1_000);
    assert_eq!(
        source_availability(&slow, &channel, now_ms).0,
        AvailabilityDto::Fresh,
        "a five-minute-old reading from a ten-minute feed is on schedule"
    );

    // A fast source gets no such latitude.
    let mut fast = SourceState::empty(&channel.id);
    fast.reported_health = HealthState::Healthy;
    fast.last_success_ms = Some(now_ms - 5 * 60 * 1_000);
    fast.poll_interval_ms = Some(15 * 1_000);
    assert_eq!(
        source_availability(&fast, &channel, now_ms).0,
        AvailabilityDto::Stale,
        "a five-minute-old reading from a fifteen-second feed is genuinely late"
    );

    // And a slow source that has missed well past its own cadence still trips.
    let mut overdue = slow.clone();
    overdue.last_success_ms = Some(now_ms - 30 * 60 * 1_000);
    assert_eq!(
        source_availability(&overdue, &channel, now_ms).0,
        AvailabilityDto::Stale
    );
}

/// The band is the shared identity behind both dedupe paths, so its boundaries
/// are behaviour rather than an implementation detail: this is the test that
/// fails if "62% at 40 minutes" and "95% at 5 minutes" ever collapse together
/// again, or if a refresh that moves a number by a point starts re-alerting.
#[test]
fn the_weather_band_moves_with_the_forecast_and_not_with_its_noise() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let band = |probability: f64, gust_kmh: f64, starts_in_minutes: i64| {
        let mut item = weather_hourly_item(now_ms, probability, gust_kmh);
        item.starts_at = Some(
            chrono::DateTime::from_timestamp_millis(now_ms + starts_in_minutes * 60_000).unwrap(),
        );
        weather_signal(&item, &channel, now_ms, UnitSystem::Imperial).band
    };

    // A forecast refresh that nudges the probability is the same material.
    assert_eq!(band(62.0, 10.0, 40), band(66.0, 10.0, 44));
    // Nearer and likelier is not.
    assert_ne!(band(62.0, 10.0, 40), band(95.0, 10.0, 5));
    // Either one alone is enough.
    assert_ne!(band(62.0, 10.0, 40), band(95.0, 10.0, 40));
    assert_ne!(band(62.0, 10.0, 40), band(62.0, 10.0, 5));
    // Below the activation threshold there is no band to dedupe on.
    assert_eq!(band(10.0, 10.0, 40), None);
    // The rule that produced it is named, so a future amount rule cannot be
    // mistaken for this one.
    assert!(
        band(62.0, 10.0, 40)
            .unwrap()
            .starts_with("rain-probability:")
    );
}

/// Units are a display concern. Banding on the canonical value keeps a
/// preference change from looking like new weather.
#[test]
fn switching_unit_systems_does_not_change_the_weather_band() {
    let now_ms = 1_786_741_200_000;
    let mut channel = AppPreferences::default().profile.channels[1].clone();
    channel.scope.insert("windGustMph".into(), json!(30.0));
    let item = weather_hourly_item(now_ms, 90.0, 80.0);
    assert_eq!(
        weather_signal(&item, &channel, now_ms, UnitSystem::Imperial).band,
        weather_signal(&item, &channel, now_ms, UnitSystem::Metric).band
    );
}

fn weather_minutely_item(now_ms: i64, millimetres: f64, starts_in_minutes: i64) -> CollectorItem {
    CollectorItem {
        id: format!("open-meteo:minutely-15:{starts_in_minutes}"),
        kind: ItemKind::WeatherMinutely,
        title: "15-minute forecast".into(),
        summary: Some(format!("{millimetres} mm in 15 minutes")),
        observed_at: None,
        starts_at: Some(
            chrono::DateTime::from_timestamp_millis(now_ms + starts_in_minutes * 60_000).unwrap(),
        ),
        ends_at: None,
        location: Some(Location::point(25.7617, -80.1918)),
        source: SourceLink {
            name: "Open-Meteo".into(),
            url: Some(Url::parse("https://open-meteo.com/").unwrap()),
        },
        attributes: BTreeMap::from([
            ("precipitation".into(), json!(millimetres)),
            ("rain".into(), json!(millimetres)),
            (
                "units".into(),
                json!({"precipitation": "mm", "rain": "mm", "showers": "mm"}),
            ),
        ]),
    }
}

fn weather_current_item(now_ms: i64, millimetres: f64, observed_minutes_ago: i64) -> CollectorItem {
    CollectorItem {
        id: format!("open-meteo:current:{observed_minutes_ago}"),
        kind: ItemKind::WeatherCurrent,
        title: "Current weather".into(),
        summary: Some(format!("{millimetres} mm in the current period")),
        observed_at: Some(
            chrono::DateTime::from_timestamp_millis(now_ms - observed_minutes_ago * 60_000)
                .unwrap(),
        ),
        starts_at: None,
        ends_at: None,
        location: Some(Location::point(25.7617, -80.1918)),
        source: SourceLink {
            name: "Open-Meteo".into(),
            url: Some(Url::parse("https://open-meteo.com/").unwrap()),
        },
        attributes: BTreeMap::from([
            ("precipitation".into(), json!(millimetres)),
            ("rain".into(), json!(millimetres)),
            (
                "units".into(),
                json!({"precipitation": "mm", "rain": "mm", "showers": "mm"}),
            ),
        ]),
    }
}

/// An hourly bucket answers "some time in the next hour". A 15-minute bin
/// answers "in eight minutes". Only the second is worth interrupting someone
/// for, so when both are available the bin has to be the one that speaks.
#[test]
fn a_measured_quarter_hour_speaks_over_an_hourly_probability() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let hourly = weather_hourly_item(now_ms, 90.0, 10.0);
    let minutely = weather_minutely_item(now_ms, 0.6, 8);

    let (summary, active) = weather_activation(
        &channel,
        &[&hourly, &minutely],
        AvailabilityDto::Fresh,
        now_ms,
    );
    assert!(active);
    assert!(summary.contains("mm forecast"), "{summary}");
    assert!(!summary.contains("chance"), "{summary}");
    assert!(summary.contains("beginning in 8 min"), "{summary}");
}

/// The amount rule reads only its own bin, so there is no path by which a
/// coarser bucket or a bin beyond the window can be reported as a 15-minute
/// answer. Without a qualifying bin it falls back to probability and says so.
#[test]
fn the_amount_rule_fails_closed_outside_its_window_and_units() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let dry_hourly = weather_hourly_item(now_ms, 10.0, 10.0);

    let beyond_window = weather_minutely_item(now_ms, 5.0, 45);
    assert!(!weather_activation(&channel, &[&beyond_window], AvailabilityDto::Fresh, now_ms).1);

    let already_ended = weather_minutely_item(now_ms, 5.0, -30);
    assert!(!weather_activation(&channel, &[&already_ended], AvailabilityDto::Fresh, now_ms).1);

    let mut wrong_units = weather_minutely_item(now_ms, 5.0, 8);
    wrong_units
        .attributes
        .insert("units".into(), json!({"precipitation": "inch"}));
    assert!(!weather_activation(&channel, &[&wrong_units], AvailabilityDto::Fresh, now_ms).1);

    // Below the amount floor: drizzle nobody would notice.
    let drizzle = weather_minutely_item(now_ms, 0.01, 8);
    assert!(!weather_activation(&channel, &[&drizzle], AvailabilityDto::Fresh, now_ms).1);

    // And with no usable bin at all, the hourly rule still applies on its own.
    let wet_hourly = weather_hourly_item(now_ms, 90.0, 10.0);
    let (summary, active) = weather_activation(
        &channel,
        &[&wet_hourly, &drizzle, &dry_hourly],
        AvailabilityDto::Fresh,
        now_ms,
    );
    assert!(active);
    assert!(summary.contains("chance"), "{summary}");
}

/// A forecast bin already in progress remains a forecast. It may rank at zero
/// lead, but neither its headline nor severity may claim an observation.
#[test]
fn a_current_quarter_hour_forecast_does_not_claim_rain_is_observed() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let falling = weather_minutely_item(now_ms, 1.4, 0);
    let signal = weather_signal(&falling, &channel, now_ms, UnitSystem::Imperial);

    assert_eq!(signal.imminence_minutes, Some(0));
    assert_eq!(signal.band.as_deref(), Some("rain-amount:0-5:moderate"));
    assert_eq!(signal.headline, "Rain expected soon");
    assert_eq!(signal.severity, "Imminent");
    assert!(
        signal
            .detail
            .contains("forecast in the current 15-minute period"),
        "{}",
        signal.detail
    );
    assert!(!signal.detail.contains("observed"), "{}", signal.detail);

    // Heavier rain in the same bin is a different band, so it re-alerts.
    let heavier = weather_minutely_item(now_ms, 4.0, 0);
    assert_ne!(
        signal.band,
        weather_signal(&heavier, &channel, now_ms, UnitSystem::Imperial).band
    );
}

/// A strong hourly chance whose bucket is about to begin earns the imminence
/// ordering bonus, while its copy honestly continues to describe an hour.
#[test]
fn high_confidence_near_hourly_rain_is_imminent_without_claiming_an_onset() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let mut near = weather_hourly_item(now_ms, 80.0, 10.0);
    near.starts_at = Some(chrono::DateTime::from_timestamp_millis(now_ms + 15 * 60_000).unwrap());
    let near_signal = channel_signal(
        ChannelKindDto::Weather,
        &channel,
        &[&near],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();

    assert_eq!(near_signal.imminence_minutes, Some(15));
    assert_eq!(near_signal.headline, "Rain likely soon");
    assert_eq!(near_signal.severity.as_deref(), Some("Imminent"));
    assert_eq!(
        near_signal.detail,
        "80% chance of rain in the hour beginning in 15 min."
    );
    assert_eq!(
        near_signal.expires_at.as_deref(),
        Some(iso_timestamp(now_ms + 75 * 60_000).unwrap().as_str())
    );

    // The actionable hourly bucket must not be hidden by a narrower forecast
    // that does not begin until after the 15-minute decision window.
    let farther_amount = weather_minutely_item(now_ms, 0.6, 20);
    let selected = channel_signal(
        ChannelKindDto::Weather,
        &channel,
        &[&farther_amount, &near],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(selected.headline, "Rain likely soon");
    assert_eq!(selected.imminence_minutes, Some(15));

    let mut lower_confidence = near.clone();
    lower_confidence
        .attributes
        .insert("precipitation_probability".into(), json!(79.0));
    let lower_confidence =
        weather_signal(&lower_confidence, &channel, now_ms, UnitSystem::Imperial);
    assert_eq!(lower_confidence.imminence_minutes, None);

    let mut later = near.clone();
    later.starts_at = Some(chrono::DateTime::from_timestamp_millis(now_ms + 16 * 60_000).unwrap());
    let later_signal = channel_signal(
        ChannelKindDto::Weather,
        &channel,
        &[&later],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(later_signal.imminence_minutes, None);

    let mut just_over_boundary = near.clone();
    just_over_boundary.starts_at =
        Some(chrono::DateTime::from_timestamp_millis(now_ms + 15 * 60_000 + 1).unwrap());
    assert_eq!(
        weather_signal(&just_over_boundary, &channel, now_ms, UnitSystem::Imperial,)
            .imminence_minutes,
        None
    );
    assert!(
        channel_priority(
            ChannelKindDto::Weather,
            Some(&near_signal),
            &clear_decision(),
            false,
        )
        .score
            > channel_priority(
                ChannelKindDto::Weather,
                Some(&later_signal),
                &clear_decision(),
                false,
            )
            .score
    );
}

/// Current precipitation remains actionable even if every future period is
/// dry, and expires at the end of the current provider interval.
#[test]
fn observed_current_precipitation_keeps_rain_active_until_its_bin_ends() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let current = weather_current_item(now_ms, 0.4, 5);
    let dry_minutely = weather_minutely_item(now_ms, 0.0, 8);
    let dry_hourly = weather_hourly_item(now_ms, 10.0, 10.0);

    let (summary, active) = weather_activation(
        &channel,
        &[&current, &dry_minutely, &dry_hourly],
        AvailabilityDto::Fresh,
        now_ms,
    );
    assert!(active);
    assert!(summary.contains("observed in the current 15-minute period"));

    let signal = channel_signal(
        ChannelKindDto::Weather,
        &channel,
        &[&current, &dry_minutely, &dry_hourly],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert_eq!(signal.headline, "Rain is falling now");
    assert_eq!(signal.severity.as_deref(), Some("Falling now"));
    assert_eq!(signal.imminence_minutes, Some(0));
    assert_eq!(
        signal.expires_at.as_deref(),
        Some(iso_timestamp(now_ms + 10 * 60_000).unwrap().as_str())
    );

    assert!(
        !weather_activation(
            &channel,
            &[&current],
            AvailabilityDto::Fresh,
            now_ms + 10 * 60_000,
        )
        .1
    );
    assert!(
        !weather_activation(
            &channel,
            &[&current],
            AvailabilityDto::Fresh,
            now_ms + 10 * 60_000 + 1,
        )
        .1
    );
}

#[test]
fn overlapping_weather_bins_compose_one_current_notice() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let current = weather_current_item(now_ms, 0.4, 5);
    let minutely = weather_minutely_item(now_ms, 0.8, 8);
    let hourly = weather_hourly_item(now_ms, 90.0, 10.0);

    let notices = channel_notices(
        ChannelKindDto::Weather,
        &channel,
        &[&current, &minutely, &hourly],
        now_ms,
        UnitSystem::Imperial,
        &clear_decision(),
        false,
    );

    assert_eq!(notices.len(), 1, "forecast bins are evidence, not slides");
    assert_eq!(notices[0].signal.headline, "Rain is falling now");
}

#[test]
fn separate_rain_and_wind_rows_compose_one_imminent_weather_notice() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let current_rain = weather_current_item(now_ms, 0.4, 5);
    let mut near_wind = weather_hourly_item(now_ms, 10.0, 65.0);
    near_wind.id = "open-meteo:hourly:near-wind".into();
    near_wind.starts_at =
        Some(chrono::DateTime::from_timestamp_millis(now_ms + 5 * 60_000).unwrap());
    let mut farther_stronger_wind = weather_hourly_item(now_ms, 10.0, 95.0);
    farther_stronger_wind.id = "open-meteo:hourly:farther-wind".into();
    farther_stronger_wind.starts_at =
        Some(chrono::DateTime::from_timestamp_millis(now_ms + 40 * 60_000).unwrap());

    let notices = channel_notices(
        ChannelKindDto::Weather,
        &channel,
        &[&farther_stronger_wind, &current_rain, &near_wind],
        now_ms,
        UnitSystem::Imperial,
        &clear_decision(),
        false,
    );

    assert_eq!(notices.len(), 1, "weather rows compose one live state");
    let notice = &notices[0];
    assert_eq!(notice.signal.headline, "Rain now; strong gusts expected");
    assert!(notice.signal.detail.contains("observed"));
    assert!(notice.signal.detail.contains("Gusts 40 mph in 5 min"));
    assert!(!notice.signal.detail.contains("59 mph in 40 min"));
    assert_eq!(notice.signal.imminence_minutes, Some(0));
    assert_eq!(notice.priority.imminence_minutes, Some(0));
    assert!(
        notice
            .signal
            .band
            .as_deref()
            .is_some_and(|band| band.contains("rain-amount") && band.contains("wind-gust"))
    );
    assert_eq!(
        notice.signal.expires_at.as_deref(),
        Some(iso_timestamp(now_ms + 10 * 60_000).unwrap().as_str()),
        "combined copy expires when its first supporting fact does"
    );
}

#[test]
fn wind_only_notice_carries_imminence_into_its_priority() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let mut wind = weather_hourly_item(now_ms, 10.0, 80.0);
    wind.starts_at = Some(chrono::DateTime::from_timestamp_millis(now_ms + 12 * 60_000).unwrap());

    let notices = channel_notices(
        ChannelKindDto::Weather,
        &channel,
        &[&wind],
        now_ms,
        UnitSystem::Imperial,
        &clear_decision(),
        false,
    );

    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].signal.headline, "Strong gusts expected");
    assert_eq!(notices[0].signal.imminence_minutes, Some(12));
    assert_eq!(notices[0].priority.imminence_minutes, Some(12));
    assert_eq!(notices[0].signal.severity.as_deref(), Some("Imminent"));
}

#[test]
fn quarter_hour_weather_signal_expires_at_the_end_of_its_forecast_bin() {
    let now_ms = 1_786_741_200_000;
    let channel = AppPreferences::default().profile.channels[1].clone();
    let minutely = weather_minutely_item(now_ms, 0.6, 8);
    let signal = channel_signal(
        ChannelKindDto::Weather,
        &channel,
        &[&minutely],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();

    assert_eq!(signal.headline, "Rain expected soon");
    assert_eq!(
        signal.expires_at.as_deref(),
        Some(iso_timestamp(now_ms + 23 * 60_000).unwrap().as_str())
    );
}

/// The ordering the redesign exists to produce, through the real wiring rather
/// than through the scoring function alone: a channel that can say when it
/// matters outranks one that says "some time in the next half hour", even when
/// the second is the app's own anchor.
#[test]
fn imminent_rain_outranks_a_distant_bridge_prediction_end_to_end() {
    let now_ms = 1_786_741_200_000;
    let weather_channel = AppPreferences::default().profile.channels[1].clone();
    let rain = weather_signal(
        &weather_minutely_item(now_ms, 0.6, 8),
        &weather_channel,
        now_ms,
        UnitSystem::Imperial,
    );
    assert_eq!(rain.imminence_minutes, Some(8));

    let decision = |state: BridgeStateDto, eta_min: Option<u16>| DecisionSnapshot {
        channel_id: "bridge.brickell".into(),
        subject: "Brickell Avenue Bridge".into(),
        state,
        state_label: String::new(),
        meaning: String::new(),
        action: String::new(),
        eta_min,
        eta_max: eta_min.map(|minutes| minutes + 3),
        confidence_bps: Some(7_000),
        confidence_label: None,
        confidence_basis: None,
        next_legal_slot: None,
        opening_allowed_now: true,
        availability: AvailabilityDto::Fresh,
        source_age_seconds: 0,
    };
    let rain_signal = ChannelSignalDto {
        headline: "Rain".into(),
        detail: rain.detail,
        action: String::new(),
        severity: Some("Heads-up".into()),
        expires_at: None,
        band: rain.band,
        imminence_minutes: rain.imminence_minutes,
        series: Vec::new(),
        previous_close: None,
    };

    let rain_score = channel_priority(
        ChannelKindDto::Weather,
        Some(&rain_signal),
        &decision(BridgeStateDto::Clear, None),
        false,
    )
    .score;
    let distant_bridge = channel_priority(
        ChannelKindDto::Bridge,
        None,
        &decision(BridgeStateDto::Likely, Some(35)),
        true,
    )
    .score;
    let imminent_bridge = channel_priority(
        ChannelKindDto::Bridge,
        None,
        &decision(BridgeStateDto::Likely, Some(4)),
        true,
    )
    .score;
    let open_bridge = channel_priority(
        ChannelKindDto::Bridge,
        None,
        &decision(BridgeStateDto::Open, None),
        true,
    )
    .score;

    assert!(
        rain_score > distant_bridge,
        "rain in 8 min ({rain_score}) must outrank a bridge predicted at T-35 ({distant_bridge})"
    );
    // And the converse, so this can never be read as "the bridge always loses".
    assert!(
        imminent_bridge > rain_score,
        "a bridge at T-4 ({imminent_bridge}) must outrank rain in 8 min ({rain_score})"
    );
    assert!(
        open_bridge > rain_score,
        "a raised span ({open_bridge}) outranks every forecast ({rain_score})"
    );
}

/// Preferences are the reader's, not the session's. This opens a real file,
/// enables a channel, drops the engine, and opens it again — the same thing the
/// app does when it restarts.
#[tokio::test]
async fn enabling_a_channel_survives_a_restart() {
    let file = std::env::temp_dir().join(format!(
        "brickellstatus-persist-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&file);

    {
        let store = Store::open(&file).await.unwrap();
        let engine = RuntimeEngine::new(store, RuntimeConfig::default())
            .await
            .unwrap();
        let mut preferences = engine.get_preferences().await;
        let markets = preferences
            .profile
            .channels
            .iter_mut()
            .find(|channel| channel.kind == ChannelKindDto::Markets)
            .unwrap();
        assert!(!markets.enabled, "markets ships off");
        markets.enabled = true;
        engine.save_preferences(preferences).await.unwrap();
    }

    let store = Store::open(&file).await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let reloaded = engine.get_preferences().await;
    let markets = reloaded
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::Markets)
        .unwrap();
    let enabled = markets.enabled;
    let _ = std::fs::remove_file(&file);
    assert!(enabled, "the channel must still be on after a restart");
}

/// A channel added in a later release has to reach a profile that already
/// exists. Seeding only on a fresh install meant an upgrading reader never saw
/// it — the stored channel list was taken as the whole truth at load.
#[tokio::test]
async fn a_channel_shipped_after_this_install_still_arrives() {
    let file = std::env::temp_dir().join(format!(
        "brickellstatus-adopt-{}-{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&file);

    // A profile from before Sports existed, with one channel deliberately
    // edited so the adoption cannot be a wholesale reset to defaults.
    {
        let store = Store::open(&file).await.unwrap();
        let engine = RuntimeEngine::new(store, RuntimeConfig::default())
            .await
            .unwrap();
        let mut preferences = engine.get_preferences().await;
        preferences
            .profile
            .channels
            .retain(|channel| channel.kind != ChannelKindDto::Sports);
        let news = preferences
            .profile
            .channels
            .iter_mut()
            .find(|channel| channel.kind == ChannelKindDto::News)
            .unwrap();
        news.title = "My own headlines".into();
        news.scope
            .insert("feeds".into(), json!(["https://example.com/mine.xml"]));
        engine.save_preferences(preferences).await.unwrap();
    }

    let store = Store::open(&file).await.unwrap();
    let engine = RuntimeEngine::new(store, RuntimeConfig::default())
        .await
        .unwrap();
    let reloaded = engine.get_preferences().await;
    let _ = std::fs::remove_file(&file);

    let sports = reloaded
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::Sports)
        .expect("the sports channel must arrive on an existing profile");
    assert!(!sports.enabled, "an adopted channel must not start itself");

    let news = reloaded
        .profile
        .channels
        .iter()
        .find(|channel| channel.kind == ChannelKindDto::News)
        .unwrap();
    assert_eq!(
        news.title, "My own headlines",
        "edits must survive adoption"
    );
    assert_eq!(news.scope["feeds"], json!(["https://example.com/mine.xml"]));
}

/// The reader is told what is true about their stock, never which company the
/// app asked. A source that has not answered yet is not a broken source, and
/// naming one is something the reader can do nothing with.
#[test]
fn market_copy_never_names_the_feed_or_calls_a_slow_start_a_fault() {
    let mut channel = AppPreferences::default().profile.channels[6].clone();
    channel.enabled = true;

    // A source failing every poll must not read as one that merely started a
    // moment ago; the reader has to be able to tell those apart.
    let (pending, active) = market_activation(&channel, &[], AvailabilityDto::Offline);
    assert!(!active);
    assert_eq!(pending, "Quotes unavailable");

    let (empty, _) = market_activation(&channel, &[], AvailabilityDto::Fresh);
    assert_eq!(empty, "No quote available");

    let quote = market_quote_item(6.5, None);
    let (live, live_active) = market_activation(&channel, &[&quote], AvailabilityDto::Fresh);
    assert!(live_active);
    for copy in [&pending, &empty, &live] {
        let lowered = copy.to_ascii_lowercase();
        assert!(!lowered.contains("yahoo"), "{copy}");
        assert!(!lowered.contains("chart"), "{copy}");
    }
}

/// The chart reaches the surfaces that draw it. Without this the console and
/// the panel both fall back to a number and the feature is invisible.
#[test]
fn a_market_signal_carries_the_session_shape() {
    let now_ms: i64 = 1_786_741_200_000;
    let mut channel = AppPreferences::default().profile.channels[6].clone();
    channel.enabled = true;
    let mut quote = market_quote_item(6.5, None);
    quote
        .attributes
        .insert("series".into(), json!([511.0, 512.5, 511.5, 514.39]));

    let signal = channel_signal(
        ChannelKindDto::Markets,
        &channel,
        &[&quote],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .expect("an active market channel produces a signal");
    assert_eq!(signal.series, vec![511.0, 512.5, 511.5, 514.39]);
    // And a quote without one is simply undrawn rather than a broken card.
    let bare = market_quote_item(6.5, None);
    let plain = channel_signal(
        ChannelKindDto::Markets,
        &channel,
        &[&bare],
        true,
        now_ms,
        UnitSystem::Imperial,
    )
    .unwrap();
    assert!(plain.series.is_empty());
}

/// Card copy describes the situation, never the rule that surfaced it.
///
/// Every one of these lines used to read like "the publisher item matches the
/// configured topics and freshness window" -- a sentence about this app's own
/// plumbing, printed on a display where somebody is trying to find out what is
/// happening outside.
#[test]
fn no_channel_ever_prints_the_rule_that_produced_it() {
    const PLUMBING: [&str; 8] = [
        "configured",
        "threshold",
        "matches",
        "freshness window",
        "this channel",
        "source reports",
        "product",
        "crosses",
    ];
    let now_ms: i64 = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let cases: Vec<(ChannelKindDto, usize, CollectorItem)> = vec![
        (
            ChannelKindDto::Weather,
            1,
            weather_minutely_item(now_ms, 0.6, 8),
        ),
        (
            ChannelKindDto::Official,
            2,
            official_alert_item("nws:a", "Alert", "Actual", Some(now_ms + 60_000)),
        ),
        (ChannelKindDto::Hurricane, 3, tropical_item("al012026")),
        (
            ChannelKindDto::News,
            4,
            news_item("news:a", "Miami transportation update", now_ms),
        ),
        (
            ChannelKindDto::Earthquake,
            5,
            earthquake_item("usgs:a", now_ms),
        ),
        (ChannelKindDto::Markets, 6, market_quote_item(6.5, None)),
    ];
    for (kind, index, item) in cases {
        let mut channel = preferences.profile.channels[index].clone();
        channel.enabled = true;
        let Some(signal) =
            channel_signal(kind, &channel, &[&item], true, now_ms, UnitSystem::Imperial)
        else {
            continue;
        };
        let action = signal.action.to_ascii_lowercase();
        for word in PLUMBING {
            assert!(
                !action.contains(word),
                "{kind:?} prints its own rule at the reader: {:?}",
                signal.action
            );
        }
    }
}

#[test]
fn one_alert_reaching_two_area_collectors_publishes_one_notice() {
    let now_ms = 1_786_741_200_000;
    let preferences = AppPreferences::default();
    let channel = &preferences.profile.channels[2];

    // An Official channel registers one NWS collector per watched area, and an
    // alert is issued for a zone rather than for the point a collector polls.
    // Two nearby areas therefore return the identical alert, and the channel
    // flattens its collectors into a single item list before notices are cut.
    // Every notice identity is derived from the item id, so the second copy
    // arrived carrying a key the first had already used -- and the console
    // keys its alert rail on exactly that, where a repeat is fatal rather than
    // untidy: the whole live page stopped rendering and left the loading
    // skeleton up with nothing on screen to explain it.
    let alert = official_alert_item(
        "nws:FLC086-coastal-flood",
        "Alert",
        "Actual",
        Some(now_ms + 60 * 60_000),
    );
    let same_alert_from_second_area = alert.clone();

    let notices = channel_notices(
        ChannelKindDto::Official,
        channel,
        &[&alert, &same_alert_from_second_area],
        now_ms,
        UnitSystem::Imperial,
        &clear_decision(),
        false,
    );

    assert_eq!(notices.len(), 1, "one alert reported twice is one event");

    let mut keys = notices
        .iter()
        .map(|notice| notice.key.as_str())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    let published = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), published, "notice keys must stay unique");
}
