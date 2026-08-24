use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use brickellstatus_collectors::{
    AisStreamApiKey, AisStreamCollector, AisStreamConfig, AisStreamSubscription, BbPilotsCollector,
    BbPilotsConfig, BridgeRelation, CollectContext, Collector, CollectorBatch, CollectorError,
    Fl511BridgeCollector, Fl511Config, NhcCurrentStormsCollector, NhcRssCollector,
    NwsAlertsCollector, OpenMeteoCollector, SyndicationCollector, SyndicationConfig,
    UsgsEarthquakesCollector, UsgsWindow, YahooChartCollector, YahooChartConfig,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    AisStreamKeyChange, AlertArea, AlertAreaSource, AppPreferences, ChannelKindDto,
    ChannelPreference, CollectorFactory, CollectorRegistration, RuntimeError,
};

/// Bridge status is the one signal worth polling hard: an opening lasts minutes
/// and a stale answer is worse than none. This matches the engine tick, so FL511
/// is collected on every pass.
const BRIDGE_POLL_INTERVAL: Duration = Duration::from_secs(15);

/// The pilots' board is a planning document republished on the order of
/// minutes; the movements it lists are hours out.
const BBPILOTS_POLL_INTERVAL: Duration = Duration::from_secs(600);

/// Floor for every other source. The engine tick is set by the fastest consumer
/// (FL511), so each remaining collector declares its own rate explicitly --
/// otherwise lowering the tick for bridge status would silently quadruple the
/// request volume this app sends to NWS, USGS, NHC, Open-Meteo, and arbitrary
/// user RSS feeds, none of which change that quickly.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct CredentialFreeCollectorFactory {
    user_agent: String,
    aisstream: Arc<Mutex<AisStreamRegistry>>,
}

#[derive(Default)]
struct AisStreamRegistry {
    key: Option<AisStreamApiKey>,
    collectors: BTreeMap<String, AisCollectorEntry>,
}

struct AisCollectorEntry {
    subscription: AisStreamSubscription,
    collector: Arc<AisStreamCollector>,
}

impl fmt::Debug for CredentialFreeCollectorFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialFreeCollectorFactory")
            .field("user_agent", &self.user_agent)
            .field("aisstream", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl CredentialFreeCollectorFactory {
    pub fn new(user_agent: impl Into<String>) -> Result<Self, RuntimeError> {
        let user_agent = user_agent.into();
        if user_agent.trim().is_empty() {
            return Err(RuntimeError::Configuration(
                "a descriptive User-Agent is required for public collectors".into(),
            ));
        }
        Ok(Self {
            user_agent,
            aisstream: Arc::new(Mutex::new(AisStreamRegistry::default())),
        })
    }

    /// Injects a host-owned API key without placing it in serializable
    /// preferences. Passing `None` keeps AIS disabled at the factory boundary.
    pub fn with_aisstream_key(self, key: Option<String>) -> Result<Self, RuntimeError> {
        let key = key.map(AisStreamApiKey::new).transpose()?;
        self.replace_aisstream_key(key)?;
        Ok(self)
    }

    pub fn with_aisstream_api_key(
        self,
        key: Option<AisStreamApiKey>,
    ) -> Result<Self, RuntimeError> {
        self.replace_aisstream_key(key)?;
        Ok(self)
    }

    pub fn aisstream_key_configured(&self) -> Result<bool, RuntimeError> {
        self.aisstream
            .lock()
            .map(|registry| registry.key.is_some())
            .map_err(|_| RuntimeError::Configuration("AIS secret registry is unavailable".into()))
    }

    fn replace_aisstream_key(&self, key: Option<AisStreamApiKey>) -> Result<(), RuntimeError> {
        self.begin_aisstream_api_key_change(key)?.commit();
        Ok(())
    }

    fn begin_aisstream_api_key_change(
        &self,
        key: Option<AisStreamApiKey>,
    ) -> Result<AisStreamKeyChange, RuntimeError> {
        let (previous_key, previous_collectors) = {
            let mut registry = self.aisstream.lock().map_err(|_| {
                RuntimeError::Configuration("AIS secret registry is unavailable".into())
            })?;
            let previous_key = std::mem::replace(&mut registry.key, key);
            let previous_collectors = std::mem::take(&mut registry.collectors);
            (previous_key, previous_collectors)
        };
        let registry = Arc::clone(&self.aisstream);
        Ok(AisStreamKeyChange::new(move |committed| {
            if committed {
                for entry in previous_collectors.values() {
                    entry.collector.cancel();
                }
                return;
            }

            // Rollback itself must be best-effort infallible: recover a
            // poisoned registry rather than leaving a newly staged secret or
            // collector session installed after a storage failure.
            let mut current = registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for entry in current.collectors.values() {
                entry.collector.cancel();
            }
            current.key = previous_key;
            current.collectors = previous_collectors;
        }))
    }

    fn ais_collector(
        &self,
        id: &str,
        subscription: AisStreamSubscription,
    ) -> Result<Arc<AisStreamCollector>, RuntimeError> {
        let mut registry = self.aisstream.lock().map_err(|_| {
            RuntimeError::Configuration("AIS secret registry is unavailable".into())
        })?;
        let key = registry.key.clone().ok_or_else(|| {
            RuntimeError::Configuration("AISStream secret is not configured".into())
        })?;
        if let Some(existing) = registry.collectors.get(id)
            && existing.subscription == subscription
        {
            return Ok(Arc::clone(&existing.collector));
        }
        let collector = Arc::new(AisStreamCollector::new(AisStreamConfig::new(
            key,
            subscription.clone(),
        )));
        if let Some(previous) = registry.collectors.insert(
            id.to_owned(),
            AisCollectorEntry {
                subscription,
                collector: Arc::clone(&collector),
            },
        ) {
            previous.collector.cancel();
        }
        Ok(collector)
    }

    fn retain_ais_collectors(&self, active: &BTreeSet<String>) -> Result<(), RuntimeError> {
        let mut registry = self.aisstream.lock().map_err(|_| {
            RuntimeError::Configuration("AIS collector registry is unavailable".into())
        })?;
        registry.collectors.retain(|id, entry| {
            let retain = active.contains(id);
            if !retain {
                entry.collector.cancel();
            }
            retain
        });
        Ok(())
    }
}

impl CollectorFactory for CredentialFreeCollectorFactory {
    fn build(
        &self,
        preferences: &AppPreferences,
    ) -> Result<Vec<CollectorRegistration>, RuntimeError> {
        let mut registrations = Vec::new();
        let mut active_ais_collectors = BTreeSet::new();
        for channel in preferences
            .profile
            .channels
            .iter()
            .filter(|channel| channel.enabled)
        {
            if channel.kind == ChannelKindDto::Bridge {
                let (latitude, longitude) = point_from_scope(&channel.scope, &channel.id)?;
                let bridge_label = channel
                    .scope
                    .get("bridge")
                    .and_then(Value::as_str)
                    .unwrap_or("Brickell Avenue Bridge")
                    .trim()
                    .to_owned();
                if bridge_label.is_empty() {
                    return Err(RuntimeError::Configuration(format!(
                        "{}.scope.bridge cannot be empty",
                        channel.id
                    )));
                }

                // Bridge status reporting and upstream progression are always
                // on. They were scope switches, but neither expressed an
                // intent a reader could hold — turning one off only made the
                // forecast worse, silently. A stored `false` from before this
                // is ignored rather than honoured, so nobody is left with a
                // permanently degraded estimate and no control to undo it.
                {
                    let mut config = Fl511Config::brickell_and_upstream();
                    let target = config
                        .bridges
                        .iter_mut()
                        .find(|selector| selector.relation == BridgeRelation::Target)
                        .expect("the built-in FL511 config always contains a target");
                    target.latitude = latitude;
                    target.longitude = longitude;
                    target.name_contains = bridge_label.clone();
                    target.search_radius_meters = scope_number(
                        &channel.scope,
                        "radiusMeters",
                        250.0,
                        25.0..=10_000.0,
                        &channel.id,
                    )?;
                    registrations.push(
                        CollectorRegistration::new(
                            format!("fl511.{}", channel.id),
                            &channel.id,
                            Arc::new(Fl511BridgeCollector::new(config)?),
                        )
                        .with_minimum_interval(BRIDGE_POLL_INTERVAL),
                    );
                }

                // The pilots' board is the forward-looking half of bridge
                // prediction: FL511 confirms an opening that already happened,
                // while a scheduled Miami River transit implies one to come.
                if scope_bool(&channel.scope, "useBbPilots", true, &channel.id)? {
                    registrations.push(
                        CollectorRegistration::new(
                            format!("bbpilots.{}", channel.id),
                            &channel.id,
                            Arc::new(BbPilotsCollector::new(BbPilotsConfig::default())?),
                        )
                        .with_minimum_interval(BBPILOTS_POLL_INTERVAL),
                    );
                }

                // The key is the whole gate. A separate stored `enabled` flag
                // could sit at `false` beside a perfectly good key, which is a
                // state nothing on screen explains and the reader can no longer
                // reach a switch to fix.
                if self.aisstream_key_configured()? {
                    let id = format!("aisstream.{}", channel.id);
                    // Fixed, and not a setting. For the bridge this app is
                    // about, `for_bridge` replaces the square with the charted
                    // corridor anyway, so the radius only ever described the
                    // fallback — and nobody choosing a number here could know
                    // that. It stays as the bound for any other target.
                    const WATCH_RADIUS_KM: f64 = 12.0;
                    let subscription = AisStreamSubscription::for_bridge(
                        bridge_label,
                        latitude,
                        longitude,
                        WATCH_RADIUS_KM,
                    )?;
                    let collector = self.ais_collector(&id, subscription)?;
                    active_ais_collectors.insert(id.clone());
                    registrations.push(
                        CollectorRegistration::new(id, &channel.id, collector)
                            .fail_closed_on_error(),
                    );
                }
            } else if channel.kind == ChannelKindDto::Weather {
                for area in selected_areas(preferences, channel, AreaCapability::Weather) {
                    let collector =
                        Arc::new(OpenMeteoCollector::new(area.latitude, area.longitude, 24)?);
                    registrations.push(
                        CollectorRegistration::new(
                            format!("open_meteo.{}.{}", channel.id, short_hash(&area.id)),
                            &channel.id,
                            Arc::new(AreaContextCollector::single(collector, area)),
                        )
                        .with_minimum_interval(DEFAULT_POLL_INTERVAL),
                    );
                }
            } else if channel.kind == ChannelKindDto::Official {
                for area in selected_areas(preferences, channel, AreaCapability::Official)
                    .into_iter()
                    .filter(|area| supports_nws(area))
                {
                    let collector = Arc::new(NwsAlertsCollector::new(
                        area.latitude,
                        area.longitude,
                        self.user_agent.clone(),
                    )?);
                    registrations.push(
                        CollectorRegistration::new(
                            format!("nws.{}.{}", channel.id, short_hash(&area.id)),
                            &channel.id,
                            Arc::new(AreaContextCollector::single(collector, area)),
                        )
                        .with_minimum_interval(DEFAULT_POLL_INTERVAL),
                    );
                }
            } else if channel.kind == ChannelKindDto::Hurricane {
                let areas = selected_areas(preferences, channel, AreaCapability::Tropical);
                if areas.is_empty() {
                    continue;
                }
                registrations.push(
                    CollectorRegistration::new(
                        format!("nhc.current_storms.{}", channel.id),
                        &channel.id,
                        Arc::new(AreaContextCollector::multiple(
                            Arc::new(NhcCurrentStormsCollector::new()),
                            &areas,
                        )),
                    )
                    .with_minimum_interval(DEFAULT_POLL_INTERVAL),
                );
                registrations.push(
                    CollectorRegistration::new(
                        format!("nhc.atlantic_rss.{}", channel.id),
                        &channel.id,
                        Arc::new(AreaContextCollector::multiple(
                            Arc::new(NhcRssCollector::atlantic()?),
                            &areas,
                        )),
                    )
                    .with_minimum_interval(DEFAULT_POLL_INTERVAL),
                );
            } else if matches!(channel.kind, ChannelKindDto::News | ChannelKindDto::Sports) {
                // One unusable feed used to abort the whole build, which failed
                // `save_preferences` outright: a single mistyped character in
                // one feed blocked every other setting on the page from being
                // saved. Skip the bad entry, keep the good ones polling, and
                // let the editor show which one is wrong.
                // The registration id is derived from the feed, so a repeated
                // feed would otherwise collide with itself.
                let mut seen = BTreeSet::new();
                for feed in string_array(channel.scope.get("feeds")) {
                    if feed.trim().is_empty() || !seen.insert(feed) {
                        continue;
                    }
                    let Ok(url) = Url::parse(feed) else {
                        tracing::warn!(
                            channel = %channel.id,
                            feed = %redacted_feed(feed),
                            "skipping a feed that is not a URL",
                        );
                        continue;
                    };
                    let mut config = SyndicationConfig::new(url);
                    config.max_items = crate::preferences::AUTOMATIC_ITEM_LIMIT;
                    config.user_agent = self.user_agent.clone();
                    let collector = match SyndicationCollector::new(config) {
                        Ok(collector) => collector,
                        Err(error) => {
                            tracing::warn!(
                                channel = %channel.id,
                                feed = %redacted_feed(feed),
                                %error,
                                "skipping a feed the fetcher will not accept",
                            );
                            continue;
                        }
                    };
                    registrations.push(
                        CollectorRegistration::new(
                            format!("rss.{}.{}", short_hash(&channel.id), short_hash(feed)),
                            &channel.id,
                            Arc::new(collector),
                        )
                        .with_minimum_interval(DEFAULT_POLL_INTERVAL),
                    );
                }
            } else if channel.kind == ChannelKindDto::Earthquake {
                let window = channel
                    .scope
                    .get("feed")
                    .and_then(Value::as_str)
                    .map(window_from_text)
                    .transpose()?
                    .unwrap_or(UsgsWindow::Hour);
                registrations.push(
                    CollectorRegistration::new(
                        format!("usgs.significant.{}", channel.id),
                        &channel.id,
                        Arc::new(UsgsEarthquakesCollector::new(window)),
                    )
                    .with_minimum_interval(DEFAULT_POLL_INTERVAL),
                );
            } else if channel.kind == ChannelKindDto::Markets {
                let poll_seconds = scope_number(
                    &channel.scope,
                    "pollSeconds",
                    300.0,
                    60.0..=3_600.0,
                    &channel.id,
                )?
                .round() as u64;
                for symbol in string_array(channel.scope.get("symbols")) {
                    let config = YahooChartConfig::new(
                        symbol,
                        market_label(symbol),
                        self.user_agent.clone(),
                    )?;
                    registrations.push(
                        CollectorRegistration::new(
                            format!("yahoo_chart.{}.{}", channel.id, short_hash(&config.symbol)),
                            &channel.id,
                            Arc::new(YahooChartCollector::new(config)?),
                        )
                        .with_minimum_interval(Duration::from_secs(poll_seconds)),
                    );
                }
            }
        }
        self.retain_ais_collectors(&active_ais_collectors)?;
        Ok(registrations)
    }

    fn cancel(&self) {
        if let Ok(registry) = self.aisstream.lock() {
            for entry in registry.collectors.values() {
                entry.collector.cancel();
            }
        }
    }

    fn set_aisstream_key(&self, key: Option<String>) -> Result<(), RuntimeError> {
        let key = key.map(AisStreamApiKey::new).transpose()?;
        self.replace_aisstream_key(key)
    }

    fn begin_aisstream_key_change(
        &self,
        key: Option<String>,
    ) -> Result<AisStreamKeyChange, RuntimeError> {
        let key = key.map(AisStreamApiKey::new).transpose()?;
        self.begin_aisstream_api_key_change(key)
    }

    fn aisstream_key_configured(&self) -> Result<Option<bool>, RuntimeError> {
        CredentialFreeCollectorFactory::aisstream_key_configured(self).map(Some)
    }
}

#[derive(Clone, Copy)]
enum AreaCapability {
    Weather,
    Official,
    Tropical,
}

fn selected_areas<'a>(
    preferences: &'a AppPreferences,
    channel: &ChannelPreference,
    capability: AreaCapability,
) -> Vec<&'a AlertArea> {
    let explicitly_selected = channel.scope.get("areaIds").and_then(Value::as_array);
    preferences
        .areas
        .iter()
        .filter(|area| area.enabled)
        .filter(|area| match explicitly_selected {
            Some(ids) => ids.iter().any(|id| id.as_str() == Some(area.id.as_str())),
            None => true,
        })
        .filter(|area| match capability {
            AreaCapability::Weather => area.weather_enabled,
            AreaCapability::Official => area.official_alerts_enabled,
            AreaCapability::Tropical => area.tropical_context_enabled,
        })
        .collect()
}

fn supports_nws(area: &AlertArea) -> bool {
    area.country_code
        .as_deref()
        .is_some_and(|country| country.eq_ignore_ascii_case("US"))
        || (area.country_code.is_none()
            && matches!(
                area.source,
                AlertAreaSource::Device | AlertAreaSource::Manual
            ))
}

#[derive(Clone)]
struct AreaTag {
    id: String,
    label: String,
    latitude: f64,
    longitude: f64,
    time_zone: String,
}

impl From<&AlertArea> for AreaTag {
    fn from(area: &AlertArea) -> Self {
        Self {
            id: area.id.clone(),
            label: area.label.clone(),
            latitude: area.latitude,
            longitude: area.longitude,
            time_zone: area.time_zone.clone(),
        }
    }
}

struct AreaContextCollector {
    inner: Arc<dyn Collector>,
    areas: Vec<AreaTag>,
    context_key: String,
}

impl AreaContextCollector {
    fn single(inner: Arc<dyn Collector>, area: &AlertArea) -> Self {
        Self::from_areas(inner, std::slice::from_ref(&area))
    }

    fn multiple(inner: Arc<dyn Collector>, areas: &[&AlertArea]) -> Self {
        Self::from_areas(inner, areas)
    }

    fn from_areas(inner: Arc<dyn Collector>, areas: &[&AlertArea]) -> Self {
        let areas = areas
            .iter()
            .map(|area| AreaTag::from(*area))
            .collect::<Vec<_>>();
        let context_key = short_hash(
            &areas
                .iter()
                .map(|area| area.id.as_str())
                .collect::<Vec<_>>()
                .join("\0"),
        );
        Self {
            inner,
            areas,
            context_key,
        }
    }
}

#[async_trait]
impl Collector for AreaContextCollector {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        let mut batch = self.inner.collect(context).await?;
        let area_ids = self
            .areas
            .iter()
            .map(|area| area.id.as_str())
            .collect::<Vec<_>>();
        let area_labels = self
            .areas
            .iter()
            .map(|area| area.label.as_str())
            .collect::<Vec<_>>();
        // Coordinates travel with the item, not just names. A rule that has to
        // answer "is this storm near me" cannot do it from a list of labels,
        // and threading the whole preference set into every matcher to find out
        // would be a much wider seam than this.
        let area_points = self
            .areas
            .iter()
            .map(|area| json!({"lat": area.latitude, "lon": area.longitude}))
            .collect::<Vec<_>>();
        for item in &mut batch.items {
            item.id = format!("area:{}:{}", self.context_key, item.id);
            item.attributes.insert("area_ids".into(), json!(area_ids));
            item.attributes
                .insert("area_labels".into(), json!(area_labels));
            item.attributes
                .insert("area_points".into(), json!(area_points));
            if let [area] = self.areas.as_slice() {
                item.attributes.insert("area_id".into(), json!(area.id));
                item.attributes
                    .insert("area_label".into(), json!(area.label));
                item.attributes
                    .insert("area_latitude".into(), json!(area.latitude));
                item.attributes
                    .insert("area_longitude".into(), json!(area.longitude));
                item.attributes
                    .insert("area_time_zone".into(), json!(area.time_zone));
                if let Some(location) = &mut item.location
                    && location.name.is_none()
                {
                    location.name = Some(area.label.clone());
                }
            }
        }
        Ok(batch)
    }
}

fn scope_bool(
    scope: &std::collections::BTreeMap<String, Value>,
    key: &str,
    default: bool,
    channel_id: &str,
) -> Result<bool, RuntimeError> {
    match scope.get(key) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(RuntimeError::Configuration(format!(
            "{channel_id}.scope.{key} must be boolean"
        ))),
    }
}

fn scope_number(
    scope: &std::collections::BTreeMap<String, Value>,
    key: &str,
    default: f64,
    range: std::ops::RangeInclusive<f64>,
    channel_id: &str,
) -> Result<f64, RuntimeError> {
    let value = scope.get(key).map_or(Some(default), Value::as_f64);
    match value.filter(|value| value.is_finite() && range.contains(value)) {
        Some(value) => Ok(value),
        None => Err(RuntimeError::Configuration(format!(
            "{channel_id}.scope.{key} is outside the supported range"
        ))),
    }
}

fn point_from_scope(
    scope: &std::collections::BTreeMap<String, Value>,
    channel_id: &str,
) -> Result<(f64, f64), RuntimeError> {
    if let (Some(latitude), Some(longitude)) = (
        scope.get("latitude").and_then(Value::as_f64),
        scope.get("longitude").and_then(Value::as_f64),
    ) {
        return validate_point(latitude, longitude, channel_id);
    }
    if let Some(point) = scope.get("point").and_then(Value::as_str) {
        let Some((latitude, longitude)) = point.split_once(',') else {
            return Err(RuntimeError::Configuration(format!(
                "{channel_id}.scope.point must be latitude,longitude"
            )));
        };
        let latitude = latitude.trim().parse::<f64>().map_err(|error| {
            RuntimeError::Configuration(format!(
                "{channel_id}.scope.point has an invalid latitude: {error}"
            ))
        })?;
        let longitude = longitude.trim().parse::<f64>().map_err(|error| {
            RuntimeError::Configuration(format!(
                "{channel_id}.scope.point has an invalid longitude: {error}"
            ))
        })?;
        return validate_point(latitude, longitude, channel_id);
    }
    Err(RuntimeError::Configuration(format!(
        "{channel_id} requires latitude/longitude or point in its scope"
    )))
}

fn validate_point(
    latitude: f64,
    longitude: f64,
    channel_id: &str,
) -> Result<(f64, f64), RuntimeError> {
    if latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
    {
        Ok((latitude, longitude))
    } else {
        Err(RuntimeError::Configuration(format!(
            "{channel_id} contains invalid coordinates {latitude},{longitude}"
        )))
    }
}

fn string_array(value: Option<&Value>) -> impl Iterator<Item = &str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn window_from_text(value: &str) -> Result<UsgsWindow, RuntimeError> {
    match value {
        "significant_hour" => Ok(UsgsWindow::Hour),
        "significant_day" => Ok(UsgsWindow::Day),
        "4.5_day" => Ok(UsgsWindow::Magnitude45Day),
        _ => Err(RuntimeError::Configuration(format!(
            "unsupported USGS feed {value:?}"
        ))),
    }
}

fn market_label(symbol: &str) -> &str {
    match symbol.trim().to_ascii_uppercase().as_str() {
        "^GSPC" => "S&P 500",
        "^IXIC" => "NASDAQ",
        "^DJI" => "DOW",
        "^RUT" => "RUSSELL 2000",
        "^VIX" => "VIX",
        "CL=F" => "WTI",
        "BZ=F" => "BRENT",
        _ => symbol,
    }
}

/// A rejected feed string reduced to something safe to write to a log.
///
/// Whatever the user pasted is still in this string, and a URL we refused to
/// accept is exactly the kind that might carry a credential. Log the host when
/// it parses and a short prefix when it does not.
fn redacted_feed(feed: &str) -> String {
    match Url::parse(feed) {
        Ok(url) => url.host_str().unwrap_or("unknown host").to_owned(),
        Err(_) => feed.chars().take(24).collect(),
    }
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use brickellstatus_collectors::{
        CollectorHealth, CollectorItem, HealthState, ItemKind, SourceLink,
    };
    use serde_json::json;

    use super::*;

    fn collector_ids(preferences: &AppPreferences) -> Vec<String> {
        CredentialFreeCollectorFactory::new("TenderStatus fixture (test@example.invalid)")
            .unwrap()
            .build(preferences)
            .unwrap()
            .into_iter()
            .map(|registration| registration.id)
            .collect()
    }

    #[test]
    fn magnitude_45_day_feed_maps_to_the_usgs_window() {
        assert_eq!(
            window_from_text("4.5_day").unwrap(),
            UsgsWindow::Magnitude45Day
        );
        assert!(window_from_text("4.5_week").is_err());
    }

    /// The engine tick is 15s so FL511 can be polled at 15s. A collector with a
    /// zero `minimum_interval` runs on *every* tick, so any registration that
    /// forgets its floor silently inherits bridge-status cadence and starts
    /// hammering a public API four times faster than it used to. Only the AIS
    /// stream may do that, because its `collect` drains an already-open
    /// websocket buffer instead of issuing a request.
    #[test]
    fn every_polled_collector_declares_a_rate_slower_than_the_engine_tick() {
        let preferences = AppPreferences::default();
        let registrations = CredentialFreeCollectorFactory::new(
            "BrickellStatus fixture (+https://example.invalid)",
        )
        .unwrap()
        .build(&preferences)
        .unwrap();
        assert!(!registrations.is_empty());

        for registration in &registrations {
            if registration.id.starts_with("aisstream.") {
                continue;
            }
            assert!(
                !registration.minimum_interval.is_zero(),
                "{} has no minimum_interval and would poll on every 15s tick",
                registration.id
            );
            assert!(
                registration.minimum_interval >= BRIDGE_POLL_INTERVAL,
                "{} polls faster than bridge status",
                registration.id
            );
            if !registration.id.starts_with("fl511.") {
                assert!(
                    registration.minimum_interval >= DEFAULT_POLL_INTERVAL,
                    "{} polls faster than the default floor",
                    registration.id
                );
            }
        }
    }

    #[test]
    fn bridge_channel_registers_fl511_fast_and_the_pilots_board_slow() {
        let registrations = CredentialFreeCollectorFactory::new(
            "BrickellStatus fixture (+https://example.invalid)",
        )
        .unwrap()
        .build(&AppPreferences::default())
        .unwrap();
        let interval = |prefix: &str| {
            registrations
                .iter()
                .find(|registration| registration.id.starts_with(prefix))
                .unwrap_or_else(|| panic!("no registration for {prefix}"))
                .minimum_interval
        };
        assert_eq!(interval("fl511."), Duration::from_secs(15));
        assert_eq!(interval("bbpilots."), Duration::from_secs(600));
    }

    #[test]
    fn the_pilots_board_can_be_switched_off_without_disabling_fl511() {
        let mut preferences = AppPreferences::default();
        let bridge = preferences
            .profile
            .channels
            .iter_mut()
            .find(|channel| channel.kind == ChannelKindDto::Bridge)
            .expect("the default profile has a bridge channel");
        bridge.scope.insert("useBbPilots".into(), json!(false));
        let ids = collector_ids(&preferences);
        assert!(!ids.iter().any(|id| id.starts_with("bbpilots.")));
        assert!(ids.iter().any(|id| id.starts_with("fl511.")));
    }

    fn second_area() -> AlertArea {
        AlertArea {
            id: "area.boston".into(),
            label: "Boston, Massachusetts".into(),
            latitude: 42.3601,
            longitude: -71.0589,
            time_zone: "America/New_York".into(),
            country_code: Some("US".into()),
            admin_area: Some("Massachusetts".into()),
            source: AlertAreaSource::Search,
            enabled: true,
            weather_enabled: true,
            official_alerts_enabled: true,
            tropical_context_enabled: true,
        }
    }

    fn select_second_area(preferences: &mut AppPreferences, channel_indexes: &[usize]) {
        preferences.areas.push(second_area());
        for &index in channel_indexes {
            preferences.profile.channels[index]
                .scope
                .insert("areaIds".into(), json!(["area.miami", "area.boston"]));
        }
    }

    #[test]
    fn one_unusable_feed_cannot_stop_the_others_from_polling() {
        // The whole build used to abort on the first bad URL, and because
        // `save_preferences` calls this, a single mistyped character blocked
        // saving every unrelated setting on the page.
        let mut preferences = AppPreferences::default();
        preferences.profile.channels[4].scope.insert(
            "feeds".into(),
            json!([
                "https://good-one.example/feed",
                "http://insecure.example/feed",
                "not a url at all",
                "https://127.0.0.1/feed",
                "",
                "https://good-two.example/feed",
                "https://good-two.example/feed",
            ]),
        );

        let ids = collector_ids(&preferences);
        let feeds = ids
            .iter()
            .filter(|id| id.starts_with("rss."))
            .filter(|id| !id.contains("hurricane"))
            .count();
        // Two usable feeds registered; the repeat, the blank, the non-URL, the
        // plain-http one, and the loopback one are all skipped.
        assert_eq!(feeds, 2);
    }

    #[test]
    fn defaults_build_only_the_enabled_credential_free_sources() {
        let ids = collector_ids(&AppPreferences::default());
        // Five seeded news feeds, one current-day earthquake feed, and the
        // sports channel ships disabled so it registers nothing at all.
        assert_eq!(ids.len(), 12);
        assert!(ids.iter().any(|id| id == "fl511.bridge.brickell"));
        assert!(ids.iter().any(|id| id == "bbpilots.bridge.brickell"));
        assert!(
            ids.iter()
                .any(|id| id.starts_with("open_meteo.weather.miami."))
        );
        assert!(ids.iter().any(|id| id.starts_with("nws.official.miami.")));
        assert!(
            ids.iter()
                .any(|id| id == "nhc.current_storms.hurricane.atlantic")
        );
        assert!(
            ids.iter()
                .any(|id| id == "nhc.atlantic_rss.hurricane.atlantic")
        );
        assert_eq!(ids.iter().filter(|id| id.starts_with("rss.")).count(), 5);
        assert!(ids.iter().any(|id| id.starts_with("usgs.")));
        assert!(!ids.iter().any(|id| id.starts_with("aisstream.")));
    }

    #[test]
    fn ais_requires_both_enablement_and_an_actual_host_secret() {
        let mut preferences = AppPreferences::default();
        preferences.ais.enabled = true;
        preferences.ais.api_key_configured = true;

        let without_secret = collector_ids(&preferences);
        assert!(!without_secret.iter().any(|id| id.starts_with("aisstream.")));

        let factory = CredentialFreeCollectorFactory::new(
            "BrickellStatus fixture (+https://example.invalid)",
        )
        .unwrap()
        .with_aisstream_key(Some("fixture-aisstream-secret".into()))
        .unwrap();
        let first = factory.build(&preferences).unwrap();
        let second = factory.build(&preferences).unwrap();
        let first_ais = first
            .iter()
            .find(|registration| registration.id == "aisstream.bridge.brickell")
            .expect("enabled host-backed AIS collector");
        let second_ais = second
            .iter()
            .find(|registration| registration.id == "aisstream.bridge.brickell")
            .expect("cached AIS collector");
        assert!(Arc::ptr_eq(&first_ais.collector, &second_ais.collector));

        factory.set_aisstream_key(None).unwrap();
        assert!(!factory.aisstream_key_configured().unwrap());
        assert!(
            !factory
                .build(&preferences)
                .unwrap()
                .iter()
                .any(|registration| registration.id.starts_with("aisstream."))
        );

        factory
            .set_aisstream_key(Some("replacement-fixture-secret".into()))
            .unwrap();
        assert!(factory.aisstream_key_configured().unwrap());
        assert!(
            factory
                .build(&preferences)
                .unwrap()
                .iter()
                .any(|registration| registration.id == "aisstream.bridge.brickell")
        );
    }

    #[test]
    fn a_stored_bridge_reporting_switch_no_longer_parks_the_source() {
        // These were switches once. A profile written back then can still be
        // carrying `false`, and honouring it now would leave that reader with
        // a permanently worse forecast and nothing on screen to undo it.
        let mut preferences = AppPreferences::default();
        preferences.ais.enabled = true;
        preferences.profile.channels[0]
            .scope
            .insert("useFl511".into(), json!(false));
        preferences.profile.channels[0]
            .scope
            .insert("useUpstream".into(), json!(false));
        let ids = CredentialFreeCollectorFactory::new(
            "BrickellStatus fixture (+https://example.invalid)",
        )
        .unwrap()
        .with_aisstream_key(Some("fixture-aisstream-secret".into()))
        .unwrap()
        .build(&preferences)
        .unwrap()
        .into_iter()
        .map(|registration| registration.id)
        .collect::<Vec<_>>();
        assert!(ids.iter().any(|id| id == "aisstream.bridge.brickell"));
        assert!(
            ids.iter().any(|id| id.starts_with("fl511.")),
            "bridge status reporting runs whatever an old profile says"
        );
    }

    #[test]
    fn source_enablement_and_configured_rss_feeds_come_from_scope() {
        let mut preferences = AppPreferences::default();
        preferences.profile.channels[3].enabled = false;
        preferences.profile.channels[4]
            .scope
            .insert("feeds".into(), json!(["https://example.com/local.xml"]));
        preferences.profile.channels[5].enabled = true;

        let ids = collector_ids(&preferences);
        assert!(!ids.iter().any(|id| id.starts_with("nhc.")));
        assert!(ids.iter().any(|id| id.starts_with("rss.")));
        assert!(
            ids.iter()
                .any(|id| id == "usgs.significant.earthquake.significant")
        );
    }

    #[test]
    fn yahoo_market_collectors_follow_the_channel_enablement() {
        let mut preferences = AppPreferences::default();
        preferences.profile.channels[6].enabled = true;
        preferences.profile.channels[6]
            .scope
            .insert("symbols".into(), json!(["AMD", "^GSPC", "BTC-USD"]));
        let ids = collector_ids(&preferences);
        assert_eq!(
            ids.iter()
                .filter(|id| id.starts_with("yahoo_chart.markets.watchlist."))
                .count(),
            3
        );
        let registrations = CredentialFreeCollectorFactory::new(
            "BrickellStatus fixture (+https://example.invalid)",
        )
        .unwrap()
        .build(&preferences)
        .unwrap();
        assert!(
            registrations
                .iter()
                .filter(|registration| registration.id.starts_with("yahoo_chart."))
                .all(|registration| registration.minimum_interval == Duration::from_secs(300))
        );

        preferences.profile.channels[6].enabled = false;
        assert!(
            !collector_ids(&preferences)
                .iter()
                .any(|id| id.starts_with("yahoo_chart."))
        );
    }

    #[test]
    fn multiple_areas_expand_point_collectors_without_duplicating_nhc() {
        let mut preferences = AppPreferences::default();
        select_second_area(&mut preferences, &[1, 2, 3]);

        let ids = collector_ids(&preferences);
        assert_eq!(
            ids.iter()
                .filter(|id| id.starts_with("open_meteo."))
                .count(),
            2
        );
        assert_eq!(ids.iter().filter(|id| id.starts_with("nws.")).count(), 2);
        assert_eq!(ids.iter().filter(|id| id.starts_with("nhc.")).count(), 2);
    }

    #[test]
    fn disabled_areas_and_per_area_alert_types_do_not_schedule_collectors() {
        let mut preferences = AppPreferences::default();
        let mut boston = second_area();
        boston.weather_enabled = false;
        boston.official_alerts_enabled = false;
        boston.tropical_context_enabled = false;
        preferences.areas.push(boston);
        for index in [1, 2, 3] {
            preferences.profile.channels[index]
                .scope
                .insert("areaIds".into(), json!(["area.miami", "area.boston"]));
        }

        let ids = collector_ids(&preferences);
        assert_eq!(
            ids.iter()
                .filter(|id| id.starts_with("open_meteo."))
                .count(),
            1
        );
        assert_eq!(ids.iter().filter(|id| id.starts_with("nws.")).count(), 1);
        assert_eq!(ids.iter().filter(|id| id.starts_with("nhc.")).count(), 2);

        preferences.areas[0].enabled = false;
        preferences.areas[1].enabled = false;
        let ids = collector_ids(&preferences);
        assert!(!ids.iter().any(|id| {
            id.starts_with("open_meteo.") || id.starts_with("nws.") || id.starts_with("nhc.")
        }));
    }

    #[test]
    fn disabled_channels_schedule_no_collectors() {
        let mut preferences = AppPreferences::default();
        preferences.profile.channels[4]
            .scope
            .insert("feeds".into(), json!(["https://example.com/local.xml"]));
        preferences.profile.channels[5].enabled = true;
        for channel in &mut preferences.profile.channels {
            channel.enabled = false;
        }

        assert!(collector_ids(&preferences).is_empty());
    }

    #[test]
    fn nws_is_not_scheduled_for_a_known_non_us_area() {
        let mut preferences = AppPreferences::default();
        let mut toronto = second_area();
        toronto.id = "area.toronto".into();
        toronto.label = "Toronto, Ontario".into();
        toronto.country_code = Some("CA".into());
        toronto.admin_area = Some("Ontario".into());
        preferences.areas.push(toronto);
        preferences.profile.channels[2]
            .scope
            .insert("areaIds".into(), json!(["area.miami", "area.toronto"]));

        let ids = collector_ids(&preferences);
        assert_eq!(ids.iter().filter(|id| id.starts_with("nws.")).count(), 1);
    }

    struct FixtureCollector;

    #[async_trait]
    impl Collector for FixtureCollector {
        fn name(&self) -> &'static str {
            "fixture"
        }

        async fn collect(
            &self,
            _context: &CollectContext,
        ) -> Result<CollectorBatch, CollectorError> {
            Ok(CollectorBatch {
                source: "fixture".into(),
                items: vec![CollectorItem {
                    id: "hour:1".into(),
                    kind: ItemKind::WeatherHourly,
                    title: "Fixture forecast".into(),
                    summary: None,
                    observed_at: None,
                    starts_at: None,
                    ends_at: None,
                    location: Some(brickellstatus_collectors::Location::point(
                        42.3601, -71.0589,
                    )),
                    source: SourceLink {
                        name: "Fixture".into(),
                        url: None,
                    },
                    attributes: BTreeMap::new(),
                }],
                health: CollectorHealth {
                    state: HealthState::Healthy,
                    checked_at: chrono::DateTime::from_timestamp_millis(1_786_741_200_000).unwrap(),
                    message: None,
                },
                cursor: Default::default(),
                not_modified: false,
            })
        }
    }

    #[tokio::test]
    async fn area_context_is_retained_on_normalized_collector_items() {
        let area = second_area();
        let collector = AreaContextCollector::single(Arc::new(FixtureCollector), &area);
        let batch = collector.collect(&CollectContext::default()).await.unwrap();
        let item = &batch.items[0];
        assert_eq!(item.attributes["area_id"], "area.boston");
        assert_eq!(item.attributes["area_label"], "Boston, Massachusetts");
        assert_eq!(item.attributes["area_ids"], json!(["area.boston"]));
        assert!(item.id.starts_with("area:"));
        assert_eq!(
            item.location.as_ref().unwrap().name.as_deref(),
            Some("Boston, Massachusetts")
        );
    }
}
