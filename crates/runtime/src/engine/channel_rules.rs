fn channel_snapshots(
    preferences: &AppPreferences,
    state: &PersistedRuntimeState,
    decision: &DecisionSnapshot,
    now_ms: i64,
) -> Vec<ChannelSnapshot> {
    preferences
        .profile
        .channels
        .iter()
        .map(|channel| {
            let sources = channel_source_views(channel, state, now_ms);
            let coverage = channel_availability(channel, &sources);
            // Cached items inherit the health of the collector that produced
            // them. Keeping that association here prevents a healthy sibling
            // collector from making stale or offline source data actionable.
            let items = sources
                .iter()
                .filter(|source| source.is_usable())
                .filter_map(|source| source.state)
                .flat_map(|source| source.items.iter())
                .collect::<Vec<_>>();
            let kind = channel.kind;
            let (summary, active) = channel_summary(
                kind,
                channel,
                &items,
                decision,
                &coverage,
                now_ms,
                &preferences.areas,
                preferences.unit_system,
            );
            let material_key =
                channel_material_key(kind, channel, &items, decision, active, now_ms);
            let signal = channel_signal(
                kind,
                channel,
                &items,
                active,
                now_ms,
                preferences.unit_system,
            );
            let coverage_complete = coverage.total_sources > 0
                && coverage.usable_sources == coverage.total_sources
                && (kind != ChannelKindDto::Bridge
                    || bridge_resolution_confirmed(channel, state, now_ms));
            ChannelSnapshot {
                id: channel.id.clone(),
                kind,
                title: channel.title.clone(),
                source_label: source_label(kind, channel).into(),
                availability: coverage.availability,
                age_seconds: coverage.age_seconds,
                coverage_complete,
                summary,
                material_key,
                signal,
                enabled: channel.enabled,
                active,
                presence: channel.presence,
                interrupt_preset: channel.interrupt_preset,
                destinations: channel.destinations.clone(),
            }
        })
        .collect()
}

/// Returns true only when the current FL511 target observation explicitly
/// confirms the road is restored. Predictive sources such as AIS remain
/// actionable without this proof, but they cannot establish an all-clear.
///
/// A degraded FL511 batch is deliberately insufficient even when it carries a
/// cached `down` value: degraded health can mean a missing selector, an
/// unrecognized target state, or a disagreement between the tooltip and layer.
fn bridge_resolution_confirmed(
    channel: &ChannelPreference,
    state: &PersistedRuntimeState,
    now_ms: i64,
) -> bool {
    let mut saw_current_target = false;

    for (source_id, channel_id) in &state.active_sources {
        if channel_id != &channel.id || !source_id.starts_with("fl511.") {
            continue;
        }
        let Some(source) = state.sources.get(source_id) else {
            return false;
        };
        if source_availability(source, channel, now_ms).0 != AvailabilityDto::Fresh {
            return false;
        }

        for item in source.items.iter().filter(|item| {
            let observed_ms = item.observed_at.as_ref().map_or_else(
                || source.last_success_ms.unwrap_or(now_ms),
                |time| time.timestamp_millis(),
            );
            item.kind == ItemKind::Bridge
                && item.attributes.get("relation").and_then(Value::as_str) == Some("target")
                && bridge_item_is_current(item, channel, observed_ms, now_ms)
        }) {
            saw_current_target = true;
            if item.attributes.get("state").and_then(Value::as_str) != Some("down")
                || item
                    .attributes
                    .get("state_conflict")
                    .and_then(Value::as_bool)
                    != Some(false)
            {
                return false;
            }
        }
    }

    saw_current_target
}

fn channel_material_key(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    usable_items: &[&CollectorItem],
    decision: &DecisionSnapshot,
    active: bool,
    now_ms: i64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"channel-material\0");
    hasher.update(channel.id.as_bytes());
    hasher.update([0]);
    if active {
        hasher.update(b"active");
    } else {
        hasher.update(b"inactive");
    }

    if kind == ChannelKindDto::Bridge {
        hasher.update([0]);
        hasher.update(format!("{:?}", decision.state).as_bytes());
    }

    if active {
        let mut material = matching_channel_items(kind, channel, usable_items, now_ms)
            .into_iter()
            .map(|item| {
                serde_json::to_vec(item)
                    .unwrap_or_else(|_| format!("{}\0{}", item.id, item.title).into_bytes())
            })
            .collect::<Vec<_>>();
        material.sort();
        material.dedup();
        for item in material {
            hasher.update([0]);
            hasher.update(item);
        }
    }

    format!("material:{}", hex_digest(&hasher.finalize()))
}

fn channel_signal(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    usable_items: &[&CollectorItem],
    active: bool,
    now_ms: i64,
    unit_system: UnitSystem,
) -> Option<ChannelSignalDto> {
    if !active || matches!(kind, ChannelKindDto::Bridge | ChannelKindDto::System) {
        return None;
    }
    let item = matching_channel_items(kind, channel, usable_items, now_ms)
        .into_iter()
        .next()?;
    let expires_at = item
        .ends_at
        .as_ref()
        .and_then(|value| iso_timestamp(value.timestamp_millis()).ok());

    let (detail, action, severity): (String, String, Option<String>) = match kind {
        ChannelKindDto::Weather => (
            weather_signal_detail(item, channel, now_ms, unit_system),
            "Forecast conditions cross the configured weather thresholds.".into(),
            Some("Heads-up".into()),
        ),
        ChannelKindDto::Official => (
            signal_text(
                item.summary.as_deref(),
                "An active official alert matches this channel.",
                360,
            ),
            "The official source reports this alert as active.".into(),
            bounded_optional_signal_text(
                item.attributes.get("severity").and_then(Value::as_str),
                40,
            ),
        ),
        ChannelKindDto::Hurricane => (
            signal_text(
                item.summary.as_deref(),
                "An active Atlantic cyclone is listed by the National Hurricane Center.",
                360,
            ),
            "The current NHC cyclone product matches this channel; local impact is not inferred."
                .into(),
            bounded_optional_signal_text(
                item.attributes
                    .get("classification")
                    .and_then(Value::as_str),
                40,
            ),
        ),
        ChannelKindDto::News => (
            signal_text(
                item.summary.as_deref(),
                "A current publisher item matches this channel's topic rules.",
                360,
            ),
            "The publisher item matches the configured topics and freshness window.".into(),
            Some(if news_item_is_breaking(item) {
                "Breaking".into()
            } else {
                "Routine".into()
            }),
        ),
        ChannelKindDto::Earthquake => (
            signal_text(
                item.summary.as_deref(),
                "A recent earthquake meets the configured magnitude and age rules.",
                360,
            ),
            "The USGS event crosses the configured magnitude and age thresholds.".into(),
            item.attributes
                .get("magnitude")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .map(|value| format!("Magnitude {value:.1}")),
        ),
        ChannelKindDto::Markets => {
            let quote = market_quote_view(item)?;
            let currency = if quote.currency.is_empty() {
                String::new()
            } else {
                format!(" {}", quote.currency)
            };
            (
                bounded_signal_text(
                    &format!(
                        "{} {:.2}{currency} {:+.2}% · {}",
                        quote.label, quote.price, quote.change_percent, quote.session
                    ),
                    360,
                ),
                "The market move crosses the configured change threshold.".into(),
                Some("Material move".into()),
            )
        }
        ChannelKindDto::Bridge | ChannelKindDto::System => return None,
    };

    Some(ChannelSignalDto {
        headline: signal_text(Some(&item.title), &channel.title, 160),
        detail,
        action: bounded_signal_text(&action, 240),
        severity,
        expires_at,
    })
}

fn matching_channel_items<'a>(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    usable_items: &[&'a CollectorItem],
    now_ms: i64,
) -> Vec<&'a CollectorItem> {
    let mut matching = usable_items
        .iter()
        .copied()
        .filter(|item| channel_material_item_matches(kind, channel, item, now_ms))
        .collect::<Vec<_>>();
    matching.sort_by(|left, right| {
        signal_priority(kind, channel, right, now_ms)
            .total_cmp(&signal_priority(kind, channel, left, now_ms))
            .then_with(|| item_time_ms(right).cmp(&item_time_ms(left)))
            .then_with(|| left.id.cmp(&right.id))
    });
    matching.truncate(channel.max_items);
    matching
}

fn signal_priority(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    item: &CollectorItem,
    now_ms: i64,
) -> f64 {
    match kind {
        ChannelKindDto::Official => match item
            .attributes
            .get("severity")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("extreme") => 4.0,
            Some("severe") => 3.0,
            Some("moderate") => 2.0,
            Some("minor") => 1.0,
            _ => 0.0,
        },
        ChannelKindDto::Weather => {
            weather_item_activation_score(item, channel, now_ms).unwrap_or_default()
        }
        ChannelKindDto::Hurricane => item
            .attributes
            .get("intensity_knots")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or_default(),
        ChannelKindDto::News => {
            if news_item_is_breaking(item) {
                1.0
            } else {
                0.0
            }
        }
        ChannelKindDto::Earthquake => item
            .attributes
            .get("magnitude")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or_default(),
        ChannelKindDto::Markets => market_quote_view(item)
            .map(|quote| quote.change_percent.abs())
            .unwrap_or_default(),
        ChannelKindDto::Bridge | ChannelKindDto::System => 0.0,
    }
}

fn item_time_ms(item: &CollectorItem) -> i64 {
    item.observed_at
        .as_ref()
        .or(item.starts_at.as_ref())
        .map_or(i64::MIN, |value| value.timestamp_millis())
}

fn bounded_optional_signal_text(value: Option<&str>, max_chars: usize) -> Option<String> {
    value
        .map(|value| bounded_signal_text(value, max_chars))
        .filter(|value| !value.is_empty())
}

fn signal_text(value: Option<&str>, fallback: &str, max_chars: usize) -> String {
    value
        .map(|value| bounded_signal_text(value, max_chars))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| bounded_signal_text(fallback, max_chars))
}

fn bounded_signal_text(value: &str, max_chars: usize) -> String {
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }
    let mut bounded = cleaned
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    bounded.push('…');
    bounded
}

fn channel_material_item_matches(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    item: &CollectorItem,
    now_ms: i64,
) -> bool {
    match kind {
        ChannelKindDto::Bridge => item.kind == ItemKind::Bridge,
        ChannelKindDto::Weather => weather_item_activation_score(item, channel, now_ms).is_some(),
        ChannelKindDto::Official => official_alert_matches_scope(item, channel, now_ms),
        ChannelKindDto::Hurricane => {
            scope_boolean(channel, "allAtlanticSystems", false) && atlantic_cyclone(item)
        }
        ChannelKindDto::News => news_item_matches_scope(item, channel, now_ms),
        ChannelKindDto::Earthquake => earthquake_matches_scope(item, channel, now_ms),
        ChannelKindDto::Markets => market_quote_view(item).is_some_and(|quote| {
            quote.change_percent.abs() >= scope_f64(channel, "movePercent", 5.0)
        }),
        ChannelKindDto::System => false,
    }
}

fn news_item_is_breaking(item: &CollectorItem) -> bool {
    if item.title.to_ascii_lowercase().contains("breaking")
        || item
            .summary
            .as_deref()
            .is_some_and(|summary| summary.to_ascii_lowercase().contains("breaking"))
    {
        return true;
    }
    item.attributes
        .get("categories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|category| category.to_ascii_lowercase().contains("breaking"))
}

/// Returns a deterministic ranking score only when this particular forecast
/// item crosses at least one enabled personal weather rule. This mirrors the
/// channel activation gates so unrelated forecast rows cannot become the
/// delivered signal or its material identity.
#[derive(Clone, Copy, Debug)]
struct RainActivationFact {
    lead_minutes: i64,
    probability: f64,
    threshold: f64,
}

#[derive(Clone, Copy, Debug)]
struct WindActivationFact {
    lead_minutes: i64,
    mph: f64,
    threshold: f64,
}

fn rain_activation_fact(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> Option<RainActivationFact> {
    if !scope_boolean(channel, "rainAlertEnabled", false) || item.kind != ItemKind::WeatherHourly {
        return None;
    }
    let threshold = scope_f64(channel, "rainProbabilityThreshold", 60.0);
    if !threshold.is_finite() || !(0.0..=100.0).contains(&threshold) {
        return None;
    }
    let lead_limit = scope_f64(channel, "rainLeadMinutes", 90.0)
        .round()
        .clamp(0.0, 1_440.0) as i64;
    let starts_ms = item.starts_at.as_ref()?.timestamp_millis();
    // Hourly precipitation describes the complete bucket, so the current
    // bucket remains useful through the end of its hour.
    if starts_ms.saturating_add(60 * 60 * 1_000) < now_ms {
        return None;
    }
    let lead_minutes = starts_ms.saturating_sub(now_ms).max(0) / 60_000;
    if lead_minutes > lead_limit {
        return None;
    }
    let probability = item
        .attributes
        .get("precipitation_probability")?
        .as_f64()
        .filter(|value| value.is_finite() && (0.0..=100.0).contains(value))?;
    if item
        .attributes
        .get("units")?
        .as_object()?
        .get("precipitation_probability")?
        .as_str()?
        .trim()
        != "%"
    {
        return None;
    }
    (probability >= threshold).then_some(RainActivationFact {
        lead_minutes,
        probability,
        threshold,
    })
}

fn wind_activation_fact(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> Option<WindActivationFact> {
    if !scope_boolean(channel, "windAlertEnabled", true) {
        return None;
    }
    let threshold = channel
        .scope
        .get("windGustMph")?
        .as_f64()
        .filter(|value| value.is_finite())?;
    const MAX_CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;
    let lead_minutes = match item.kind {
        ItemKind::WeatherCurrent => {
            let observed_ms = item.observed_at.as_ref()?.timestamp_millis();
            let maximum_age_ms = i64::from(channel.max_age_minutes).saturating_mul(60 * 1_000);
            if observed_ms > now_ms.saturating_add(MAX_CLOCK_SKEW_MS)
                || now_ms.saturating_sub(observed_ms).max(0) > maximum_age_ms
            {
                return None;
            }
            0
        }
        ItemKind::WeatherHourly => {
            let starts_ms = item.starts_at.as_ref()?.timestamp_millis();
            if starts_ms.saturating_add(60 * 60 * 1_000) < now_ms {
                return None;
            }
            let lead_minutes = starts_ms.saturating_sub(now_ms).max(0) / 60_000;
            // The bundled channel shares its configured forecast lead window
            // across precipitation and forecast gust evaluation. A distinct
            // windLeadMinutes scope key is also honored for future clients.
            let lead_limit = scope_f64(
                channel,
                "windLeadMinutes",
                scope_f64(channel, "rainLeadMinutes", 90.0),
            )
            .round()
            .clamp(0.0, 1_440.0) as i64;
            if lead_minutes > lead_limit {
                return None;
            }
            lead_minutes
        }
        _ => return None,
    };
    let gust = item
        .attributes
        .get("wind_gusts_10m")?
        .as_f64()
        .filter(|value| value.is_finite())?;
    let unit = item
        .attributes
        .get("units")?
        .as_object()?
        .get("wind_gusts_10m")?
        .as_str()?
        .trim();
    let mph = if unit.eq_ignore_ascii_case("mph") {
        gust
    } else if unit.eq_ignore_ascii_case("km/h") {
        gust / 1.609_344
    } else {
        return None;
    };
    (mph.is_finite() && mph >= threshold).then_some(WindActivationFact {
        lead_minutes,
        mph,
        threshold,
    })
}

fn weather_item_activation_score(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> Option<f64> {
    let rain = rain_activation_fact(item, channel, now_ms)
        .map(|fact| 20_000.0 - fact.lead_minutes as f64 + fact.probability / 1_000.0);
    let wind = wind_activation_fact(item, channel, now_ms).map(|fact| 10_000.0 + fact.mph);
    match (rain, wind) {
        (Some(rain), Some(wind)) => Some(rain.max(wind)),
        (Some(score), None) | (None, Some(score)) => Some(score),
        (None, None) => None,
    }
}

fn weather_signal_detail(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
    unit_system: UnitSystem,
) -> String {
    let rain = rain_activation_fact(item, channel, now_ms).map(|fact| {
        format!(
            "Rain {:.0}% in {} min (threshold {:.0}%)",
            fact.probability, fact.lead_minutes, fact.threshold
        )
    });
    let wind = wind_activation_fact(item, channel, now_ms).map(|fact| {
        let timing = if fact.lead_minutes == 0 {
            "now".into()
        } else {
            format!("in {} min", fact.lead_minutes)
        };
        let (gust, label) = display_wind_speed(fact.mph, unit_system);
        let (threshold, _) = display_wind_speed(fact.threshold, unit_system);
        format!("Gusts {gust:.0} {label} {timing} (threshold {threshold:.0} {label})")
    });
    let detail = match (rain, wind) {
        (Some(rain), Some(wind)) => format!("{rain} · {wind}"),
        (Some(detail), None) | (None, Some(detail)) => detail,
        (None, None) => "A configured personal weather threshold was crossed.".into(),
    };
    bounded_signal_text(&detail, 360)
}

#[derive(Clone, Copy, Debug)]
struct ChannelSourceView<'a> {
    state: Option<&'a SourceState>,
    availability: AvailabilityDto,
    age_seconds: u64,
}

impl ChannelSourceView<'_> {
    fn is_usable(self) -> bool {
        matches!(
            self.availability,
            AvailabilityDto::Fresh | AvailabilityDto::Delayed
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ChannelCoverage {
    availability: AvailabilityDto,
    age_seconds: u64,
    total_sources: usize,
    usable_sources: usize,
    fresh_sources: usize,
}

fn channel_source_views<'a>(
    channel: &ChannelPreference,
    state: &'a PersistedRuntimeState,
    now_ms: i64,
) -> Vec<ChannelSourceView<'a>> {
    state
        .active_sources
        .iter()
        .filter(|(_, channel_id)| *channel_id == &channel.id)
        .map(|(source_id, _)| {
            let source = state.sources.get(source_id);
            let (availability, age_seconds) = source
                .map_or((AvailabilityDto::Offline, 0), |source| {
                    source_availability(source, channel, now_ms)
                });
            ChannelSourceView {
                state: source,
                availability,
                age_seconds,
            }
        })
        .collect()
}

fn channel_availability(
    channel: &ChannelPreference,
    sources: &[ChannelSourceView<'_>],
) -> ChannelCoverage {
    if !channel.enabled || sources.is_empty() {
        return ChannelCoverage {
            availability: AvailabilityDto::Offline,
            age_seconds: 0,
            total_sources: sources.len(),
            usable_sources: 0,
            fresh_sources: 0,
        };
    }

    let total_sources = sources.len();
    let usable_sources = sources.iter().filter(|source| source.is_usable()).count();
    let fresh_sources = sources
        .iter()
        .filter(|source| source.availability == AvailabilityDto::Fresh)
        .count();
    // "Fresh" is a complete-coverage assertion: every configured collector
    // for the channel must be fresh. Partial but still usable coverage is
    // deliberately reported as delayed/degraded rather than best-of fresh.
    let availability = if fresh_sources == total_sources {
        AvailabilityDto::Fresh
    } else if usable_sources > 0 {
        AvailabilityDto::Delayed
    } else if sources
        .iter()
        .any(|source| source.availability == AvailabilityDto::Stale)
    {
        AvailabilityDto::Stale
    } else {
        AvailabilityDto::Offline
    };
    // The oldest contributing source is the honest channel-level age. Using
    // the youngest source here would hide degraded multi-source coverage.
    let age_seconds = sources
        .iter()
        .map(|source| source.age_seconds)
        .max()
        .unwrap_or_default();
    ChannelCoverage {
        availability,
        age_seconds,
        total_sources,
        usable_sources,
        fresh_sources,
    }
}

fn source_availability(
    source: &SourceState,
    channel: &ChannelPreference,
    now_ms: i64,
) -> (AvailabilityDto, u64) {
    let Some(last_success) = source.last_success_ms else {
        return (AvailabilityDto::Offline, 0);
    };
    let age_seconds = TimestampMillis(last_success).age_seconds_at(TimestampMillis(now_ms));
    if source.fail_closed_on_error
        && (source.last_error.is_some() || source.reported_health != HealthState::Healthy)
    {
        return (AvailabilityDto::Offline, age_seconds);
    }
    let stale_after = u64::from(channel.max_age_minutes) * 60;
    if age_seconds > stale_after {
        return (AvailabilityDto::Stale, age_seconds);
    }
    if source.last_error.is_some() || source.reported_health != HealthState::Healthy {
        (AvailabilityDto::Delayed, age_seconds)
    } else {
        (AvailabilityDto::Fresh, age_seconds)
    }
}

fn aisstream_status(
    preferences: &AppPreferences,
    state: &PersistedRuntimeState,
    now_ms: i64,
) -> Result<AisStreamStatusDto, RuntimeError> {
    let registered = state
        .active_sources
        .iter()
        .filter(|(source_id, _)| source_id.starts_with("aisstream."))
        .collect::<Vec<_>>();
    let source_registered = !registered.is_empty();
    let channels = preferences
        .profile
        .channels
        .iter()
        .map(|channel| (channel.id.as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    let mut availability_values = Vec::new();
    let mut last_success_ms = None;
    let mut last_position_ms = None;
    let mut fresh_vessel_count = 0usize;
    let mut last_error: Option<(i64, String)> = None;
    let mut attempted = false;

    for (source_id, channel_id) in &registered {
        let Some(source) = state.sources.get(*source_id) else {
            availability_values.push(AvailabilityDto::Offline);
            continue;
        };
        attempted |= source.last_attempt_ms.is_some();
        if let Some(success) = source.last_success_ms {
            last_success_ms =
                Some(last_success_ms.map_or(success, |current: i64| current.max(success)));
        }
        if let Some(error) = &source.last_error {
            let attempted_at = source.last_attempt_ms.unwrap_or(i64::MIN);
            if last_error
                .as_ref()
                .is_none_or(|(current, _)| attempted_at > *current)
            {
                last_error = Some((attempted_at, bounded_signal_text(error, 240)));
            }
        }
        let availability = channels
            .get(channel_id.as_str())
            .map_or(AvailabilityDto::Offline, |channel| {
                source_availability(source, channel, now_ms).0
            });
        availability_values.push(availability);
        if let Some(cursor_position) = source
            .cursor
            .metadata
            .get("last_position_at_ms")
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value <= now_ms.saturating_add(30_000))
            .filter(|value| iso_timestamp(*value).is_ok())
        {
            last_position_ms = Some(
                last_position_ms
                    .map_or(cursor_position, |current: i64| current.max(cursor_position)),
            );
        }
        for item in &source.items {
            if item.kind != ItemKind::Bridge
                || item.attributes.get("relation").and_then(Value::as_str) != Some("ais")
            {
                continue;
            }
            if let Some(observed) = item
                .observed_at
                .as_ref()
                .map(|value| value.timestamp_millis())
            {
                last_position_ms =
                    Some(last_position_ms.map_or(observed, |current: i64| current.max(observed)));
            }
        }
        if availability == AvailabilityDto::Fresh {
            let channel = channels.get(channel_id.as_str()).copied();
            let expiring_count = source
                .cursor
                .metadata
                .get("fresh_vessel_expirations_ms")
                .and_then(|value| serde_json::from_str::<Vec<i64>>(value).ok())
                .filter(|expirations| expirations.len() <= MAX_AIS_STATUS_VESSELS)
                .map(|expirations| {
                    expirations
                        .into_iter()
                        .filter(|expires_at| *expires_at >= now_ms)
                        .count()
                });
            fresh_vessel_count =
                fresh_vessel_count.saturating_add(expiring_count.unwrap_or_else(|| {
                    source
                        .items
                        .iter()
                        .filter(|item| {
                            if item.kind != ItemKind::Bridge
                                || item.attributes.get("relation").and_then(Value::as_str)
                                    != Some("ais")
                            {
                                return false;
                            }
                            let observed_ms = item.observed_at.as_ref().map_or_else(
                                || source.last_success_ms.unwrap_or(now_ms),
                                |time| time.timestamp_millis(),
                            );
                            channel.is_some_and(|channel| {
                                bridge_item_is_current(item, channel, observed_ms, now_ms)
                            })
                        })
                        .count()
                }));
        }
    }

    let availability = if !source_registered {
        AvailabilityDto::Offline
    } else if availability_values
        .iter()
        .all(|value| *value == AvailabilityDto::Fresh)
    {
        AvailabilityDto::Fresh
    } else if availability_values
        .iter()
        .any(|value| matches!(value, AvailabilityDto::Fresh | AvailabilityDto::Delayed))
    {
        AvailabilityDto::Delayed
    } else if availability_values.contains(&AvailabilityDto::Stale) {
        AvailabilityDto::Stale
    } else {
        AvailabilityDto::Offline
    };
    let last_error = last_error
        .map(|(_, error)| error)
        .filter(|error| !error.is_empty());
    let rejected = last_error
        .as_deref()
        .is_some_and(|error| error.to_ascii_lowercase().contains("rejected"));
    let starting = last_error
        .as_deref()
        .is_some_and(|error| error.to_ascii_lowercase().contains("starting"));
    let connection_state = if !preferences.ais.enabled {
        AisConnectionStateDto::Disabled
    } else if !preferences.ais.api_key_configured {
        AisConnectionStateDto::NeedsKey
    } else if source_registered && (!attempted || (starting && last_success_ms.is_none())) {
        AisConnectionStateDto::Armed
    } else if rejected {
        AisConnectionStateDto::Rejected
    } else if availability == AvailabilityDto::Fresh && fresh_vessel_count > 0 {
        AisConnectionStateDto::Live
    } else if availability == AvailabilityDto::Fresh {
        AisConnectionStateDto::Armed
    } else {
        AisConnectionStateDto::Disconnected
    };
    let detail = match connection_state {
        AisConnectionStateDto::Disabled => "AISStream is disabled.".into(),
        AisConnectionStateDto::NeedsKey => {
            "AISStream needs a key from the desktop secret store.".into()
        }
        AisConnectionStateDto::Armed if last_position_ms.is_none() && last_success_ms.is_some() => {
            "WebSocket connected; AISStream has not delivered a vessel position for this subscription."
                .into()
        }
        AisConnectionStateDto::Armed => {
            "WebSocket connected; no vessel position is currently fresh in the bridge area."
                .into()
        }
        AisConnectionStateDto::Live => format!(
            "Connected; {fresh_vessel_count} fresh vessel{} inside the configured bridge area.",
            if fresh_vessel_count == 1 { "" } else { "s" }
        ),
        AisConnectionStateDto::Rejected => {
            "AISStream rejected the subscription; verify or replace the saved key.".into()
        }
        AisConnectionStateDto::Disconnected => last_error.clone().unwrap_or_else(|| {
            if source_registered {
                "AISStream is not currently connected.".into()
            } else {
                "No enabled bridge channel has an AISStream source registered.".into()
            }
        }),
    };

    Ok(AisStreamStatusDto {
        enabled: preferences.ais.enabled,
        provider: preferences.ais.provider,
        api_key_configured: preferences.ais.api_key_configured,
        source_registered,
        connection_state,
        availability,
        radius_kilometers: preferences.ais.radius_kilometers,
        last_success_at: last_success_ms.map(iso_timestamp).transpose()?,
        last_position_at: last_position_ms.map(iso_timestamp).transpose()?,
        fresh_vessel_count,
        detail,
        last_error,
    })
}

const MAX_AIS_STATUS_VESSELS: usize = 512;
const MAX_MAP_VESSEL_TRACKS: usize = 64;
const MAX_MAP_TRACK_POINTS: usize = 121;
const MAP_TRACK_RETENTION_MS: i64 = 60 * 60 * 1_000;

fn source_health(
    preferences: &AppPreferences,
    state: &PersistedRuntimeState,
    now_ms: i64,
) -> Result<Vec<SourceHealth>, RuntimeError> {
    let channels = preferences
        .profile
        .channels
        .iter()
        .map(|channel| (channel.id.as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    state
        .active_sources
        .iter()
        .map(|(source_id, channel_id)| {
            let channel = channels.get(channel_id.as_str()).ok_or_else(|| {
                RuntimeError::Configuration(format!(
                    "source {source_id:?} references unknown channel {channel_id:?}"
                ))
            })?;
            let Some(source) = state.sources.get(source_id) else {
                return Ok(SourceHealth {
                    source_id: source_id.clone(),
                    channel_id: channel_id.clone(),
                    availability: AvailabilityDto::Offline,
                    detail: "No collector result has been recorded yet.".into(),
                    failure_count: 0,
                    last_attempt_at: None,
                    last_success_at: None,
                });
            };
            let (availability, _) = source_availability(source, channel, now_ms);
            let detail = match availability {
                AvailabilityDto::Fresh if source_id.starts_with("aisstream.") => {
                    let count = source
                        .cursor
                        .metadata
                        .get("fresh_vessel_count")
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(0);
                    if count == 0 {
                        "WebSocket connected; no current vessel position has been delivered for this subscription."
                            .into()
                    } else {
                        format!(
                            "WebSocket connected; {count} current vessel position{} received.",
                            if count == 1 { "" } else { "s" }
                        )
                    }
                }
                AvailabilityDto::Fresh => source
                    .health_message
                    .clone()
                    .unwrap_or_else(|| "Reporting within the accepted freshness window.".into()),
                AvailabilityDto::Delayed | AvailabilityDto::Stale | AvailabilityDto::Offline => {
                    source
                        .last_error
                        .clone()
                        .or_else(|| source.health_message.clone())
                        .unwrap_or_else(|| "No current observation is available.".into())
                }
            };
            Ok(SourceHealth {
                source_id: source_id.clone(),
                channel_id: channel_id.clone(),
                availability,
                detail,
                failure_count: source.failure_count,
                last_attempt_at: source.last_attempt_ms.map(iso_timestamp).transpose()?,
                last_success_at: source.last_success_ms.map(iso_timestamp).transpose()?,
            })
        })
        .collect()
}

fn vessel_tracks(state: &PersistedRuntimeState, now_ms: i64) -> Vec<VesselTrackSnapshot> {
    let cutoff = now_ms.saturating_sub(MAP_TRACK_RETENTION_MS);
    let latest = now_ms.saturating_add(30_000);
    let mut tracks = state
        .active_sources
        .keys()
        .filter(|source_id| source_id.starts_with("aisstream."))
        .filter_map(|source_id| state.sources.get(source_id))
        .filter_map(|source| source.cursor.metadata.get(AIS_VESSEL_TRACKS_CURSOR_KEY))
        .filter_map(|encoded| serde_json::from_str::<Vec<VesselTrackSnapshot>>(encoded).ok())
        .flatten()
        .filter_map(|mut track| {
            let mmsi_valid = track.mmsi.len() == 9
                && track
                    .mmsi
                    .bytes()
                    .all(|character| character.is_ascii_digit());
            let observed_ms = track
                .observed_at
                .parse::<Timestamp>()
                .ok()?
                .as_millisecond();
            if !mmsi_valid
                || observed_ms < cutoff
                || observed_ms > latest
                || !track.speed_knots.is_finite()
                || !(0.0..102.3).contains(&track.speed_knots)
                || !track.course_degrees.is_finite()
                || !(0.0..360.0).contains(&track.course_degrees)
            {
                return None;
            }
            track.points.retain(|point| {
                let timestamp = point
                    .observed_at
                    .parse::<Timestamp>()
                    .ok()
                    .map(|value| value.as_millisecond());
                timestamp.is_some_and(|value| (cutoff..=latest).contains(&value))
                    && valid_map_coordinate(point.latitude, point.longitude)
            });
            if track.points.is_empty() {
                return None;
            }
            if track.points.len() > MAX_MAP_TRACK_POINTS {
                track
                    .points
                    .drain(..track.points.len().saturating_sub(MAX_MAP_TRACK_POINTS));
            }
            Some((observed_ms, track))
        })
        .collect::<Vec<_>>();
    tracks.sort_by(|left, right| right.0.cmp(&left.0));
    tracks.truncate(MAX_MAP_VESSEL_TRACKS);
    tracks.into_iter().map(|(_, track)| track).collect()
}

fn valid_map_coordinate(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
}

fn channel_summary(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    items: &[&CollectorItem],
    decision: &DecisionSnapshot,
    coverage: &ChannelCoverage,
    now_ms: i64,
    areas: &[AlertArea],
    unit_system: UnitSystem,
) -> (String, bool) {
    if !channel.enabled {
        return ("Disabled".into(), false);
    }
    let availability = coverage.availability;
    let (summary, active) = match kind {
        ChannelKindDto::Bridge => {
            let eta = match (decision.eta_min, decision.eta_max) {
                (Some(minimum), Some(maximum)) => format!(" · {minimum}–{maximum} min"),
                _ => String::new(),
            };
            (
                format!("{}{}", decision.state_label, eta),
                matches!(
                    decision.state,
                    BridgeStateDto::Likely | BridgeStateDto::Open
                ),
            )
        }
        ChannelKindDto::Weather => {
            weather_activation_with_units(channel, items, availability, now_ms, unit_system)
        }
        ChannelKindDto::Official => {
            let count = items
                .iter()
                .filter(|item| official_alert_matches_scope(item, channel, now_ms))
                .count();
            if count == 0 {
                ("No active alert".into(), false)
            } else {
                (format!("{count} active official alert(s)"), true)
            }
        }
        ChannelKindDto::Hurricane => {
            let storms = items.iter().filter(|item| atlantic_cyclone(item)).count();
            if storms == 0 {
                (
                    "No active Atlantic cyclone in NHC CurrentStorms".into(),
                    false,
                )
            } else if !scope_boolean(channel, "allAtlanticSystems", false) {
                (
                    format!(
                        "{storms} active Atlantic system(s) · activation suppressed because local impact is not implemented"
                    ),
                    false,
                )
            } else {
                (format!("{storms} active Atlantic cyclone(s)"), true)
            }
        }
        ChannelKindDto::News => {
            let count = items
                .iter()
                .filter(|item| news_item_matches_scope(item, channel, now_ms))
                .count();
            if count == 0 {
                ("No current feed items match this channel".into(), false)
            } else {
                (format!("{count} recent item(s) in rotation"), true)
            }
        }
        ChannelKindDto::Earthquake => {
            let count = items
                .iter()
                .filter(|item| earthquake_matches_scope(item, channel, now_ms))
                .count()
                .min(channel.max_items);
            if count == 0 {
                ("No significant earthquake".into(), false)
            } else {
                (format!("{count} significant earthquake(s)"), true)
            }
        }
        ChannelKindDto::Markets => market_activation(channel, items, availability),
        ChannelKindDto::System => ("Runtime status".into(), false),
    };
    let summary = area_context_label(channel, areas)
        .map_or(summary.clone(), |area| format!("{area} · {summary}"));
    (
        channel_coverage_summary(summary, kind, coverage),
        active && coverage.usable_sources > 0,
    )
}

fn channel_coverage_summary(
    summary: String,
    kind: ChannelKindDto,
    coverage: &ChannelCoverage,
) -> String {
    if matches!(kind, ChannelKindDto::Bridge | ChannelKindDto::System)
        || coverage.availability == AvailabilityDto::Fresh
    {
        return summary;
    }
    if coverage.total_sources == 0 {
        return format!("{summary} · no collector configured");
    }
    if coverage.usable_sources == 0 {
        return match coverage.availability {
            AvailabilityDto::Stale => {
                format!("{summary} · all source data stale; activation suppressed")
            }
            AvailabilityDto::Offline => {
                format!("{summary} · all sources offline; activation suppressed")
            }
            AvailabilityDto::Fresh | AvailabilityDto::Delayed => summary,
        };
    }
    if coverage.usable_sources < coverage.total_sources {
        return format!(
            "{summary} · partial coverage ({}/{} sources usable); stale/offline items suppressed",
            coverage.usable_sources, coverage.total_sources
        );
    }
    if coverage.fresh_sources < coverage.total_sources {
        return format!("{summary} · source responses delayed");
    }
    summary
}

fn area_context_label(channel: &ChannelPreference, areas: &[AlertArea]) -> Option<String> {
    let selected = channel.scope.get("areaIds")?.as_array()?;
    let selected = selected
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let labels = areas
        .iter()
        .filter(|area| {
            area.enabled
                && selected.contains(area.id.as_str())
                && area_enabled_for_channel(area, channel)
        })
        .map(|area| area.label.as_str())
        .collect::<Vec<_>>();
    match labels.as_slice() {
        [] => Some("No enabled areas".into()),
        [label] => Some((*label).into()),
        _ => {
            let visible = labels
                .iter()
                .take(3)
                .copied()
                .collect::<Vec<_>>()
                .join(" / ");
            let remaining = labels.len().saturating_sub(3);
            if remaining == 0 {
                Some(visible)
            } else {
                Some(format!("{visible} +{remaining}"))
            }
        }
    }
}

fn area_enabled_for_channel(area: &AlertArea, channel: &ChannelPreference) -> bool {
    match channel.kind {
        ChannelKindDto::Weather => area.weather_enabled,
        ChannelKindDto::Official => {
            area.official_alerts_enabled
                && (area
                    .country_code
                    .as_deref()
                    .is_some_and(|country| country.eq_ignore_ascii_case("US"))
                    || (area.country_code.is_none()
                        && matches!(
                            area.source,
                            AlertAreaSource::Device | AlertAreaSource::Manual
                        )))
        }
        ChannelKindDto::Hurricane => area.tropical_context_enabled,
        _ => true,
    }
}

fn official_alert_matches_scope(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> bool {
    if item.kind != ItemKind::OfficialAlert {
        return false;
    }
    let Some(ends_at) = item.ends_at.as_ref() else {
        return false;
    };
    if ends_at.timestamp_millis() <= now_ms {
        return false;
    }
    // This channel is currently backed by NWS CAP data. Only actual Alert and
    // Update records are actionable; Cancel, Ack, Error, exercise, test, and
    // unknown records fail closed even if they remain in a cached batch.
    let actual = item
        .attributes
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status.trim().eq_ignore_ascii_case("actual"));
    let actionable_message = item
        .attributes
        .get("message_type")
        .and_then(Value::as_str)
        .is_some_and(|message_type| {
            matches!(
                message_type.trim().to_ascii_lowercase().as_str(),
                "alert" | "update"
            )
        });
    if !actual || !actionable_message {
        return false;
    }
    if !scope_boolean(channel, "includeStatements", false)
        && item.title.to_ascii_lowercase().contains("statement")
    {
        return false;
    }
    let severities = scope_strings(channel, "severity");
    if severities.is_empty() {
        return true;
    }
    item.attributes
        .get("severity")
        .and_then(Value::as_str)
        .is_some_and(|severity| {
            severities
                .iter()
                .any(|configured| configured.eq_ignore_ascii_case(severity))
        })
}

fn atlantic_cyclone(item: &CollectorItem) -> bool {
    item.kind == ItemKind::TropicalCyclone
        && item
            .id
            .strip_prefix("nhc:")
            .and_then(|id| id.get(..2))
            .is_some_and(|basin| basin.eq_ignore_ascii_case("al"))
}

fn news_item_matches_scope(item: &CollectorItem, channel: &ChannelPreference, now_ms: i64) -> bool {
    if item.kind != ItemKind::News {
        return false;
    }
    let Some(observed_at) = item.observed_at.as_ref() else {
        return false;
    };
    let observed_ms = observed_at.timestamp_millis();
    // Allow a small publisher clock skew, but reject scheduled/far-future
    // timestamps and entries older than the user's accepted item age. Source
    // freshness alone cannot make an old article current.
    const MAX_FUTURE_CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;
    if observed_ms > now_ms.saturating_add(MAX_FUTURE_CLOCK_SKEW_MS) {
        return false;
    }
    let maximum_age_ms = i64::from(channel.max_age_minutes).saturating_mul(60 * 1_000);
    if now_ms.saturating_sub(observed_ms).max(0) > maximum_age_ms {
        return false;
    }
    let mut searchable = item.title.to_ascii_lowercase();
    if let Some(summary) = &item.summary {
        searchable.push(' ');
        searchable.push_str(&summary.to_ascii_lowercase());
    }
    if let Some(categories) = item.attributes.get("categories").and_then(Value::as_array) {
        for category in categories.iter().filter_map(Value::as_str) {
            searchable.push(' ');
            searchable.push_str(&category.to_ascii_lowercase());
        }
    }
    if scope_strings(channel, "excludeTopics")
        .iter()
        .any(|topic| searchable.contains(&topic.to_ascii_lowercase()))
    {
        return false;
    }
    let topics = scope_strings(channel, "topics");
    if !topics.is_empty()
        && !topics
            .iter()
            .any(|topic| searchable.contains(&topic.to_ascii_lowercase()))
    {
        return false;
    }
    !scope_boolean(channel, "breakingOnly", false) || searchable.contains("breaking")
}

fn earthquake_matches_scope(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> bool {
    if item.kind != ItemKind::Earthquake {
        return false;
    }
    if !item
        .attributes
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| {
            matches!(
                status.trim().to_ascii_lowercase().as_str(),
                "automatic" | "reviewed"
            )
        })
    {
        return false;
    }
    let Some(observed_at) = item.observed_at.as_ref() else {
        return false;
    };
    let observed_ms = observed_at.timestamp_millis();
    const MAX_FUTURE_CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;
    if observed_ms > now_ms.saturating_add(MAX_FUTURE_CLOCK_SKEW_MS) {
        return false;
    }
    let minimum = scope_f64(channel, "minimumMagnitude", 0.0);
    if item
        .attributes
        .get("magnitude")
        .and_then(Value::as_f64)
        .is_none_or(|magnitude| magnitude < minimum)
    {
        return false;
    }
    let max_age_ms = scope_f64(channel, "eventAgeMinutes", f64::INFINITY) * 60_000.0;
    let age_ms = now_ms.saturating_sub(observed_ms).max(0);
    age_ms as f64 <= max_age_ms
}

struct MarketQuoteView<'a> {
    label: &'a str,
    price: f64,
    currency: &'a str,
    change_percent: f64,
    session: &'a str,
    provider_delay_minutes: Option<u64>,
    delay_reported: bool,
}

fn market_activation(
    channel: &ChannelPreference,
    items: &[&CollectorItem],
    availability: AvailabilityDto,
) -> (String, bool) {
    if scope_strings(channel, "symbols").is_empty() {
        return ("Add at least one market symbol".into(), false);
    }
    if availability == AvailabilityDto::Offline {
        return (
            "Yahoo chart unavailable · no usable quote received".into(),
            false,
        );
    }
    if availability == AvailabilityDto::Stale {
        return (
            format!(
                "Market quotes stale · older than {} minutes · move rule suppressed",
                channel.max_age_minutes
            ),
            false,
        );
    }

    let mut quotes = items
        .iter()
        .filter(|item| item.kind == ItemKind::MarketQuote)
        .filter_map(|item| market_quote_view(item))
        .collect::<Vec<_>>();
    quotes.sort_by(|left, right| {
        f64::total_cmp(&right.change_percent.abs(), &left.change_percent.abs())
    });
    if quotes.is_empty() {
        return (
            "Yahoo chart returned no complete price/previous-close pair".into(),
            false,
        );
    }

    let threshold = scope_f64(channel, "movePercent", 5.0);
    let crossed = quotes
        .iter()
        .any(|quote| quote.change_percent.abs() >= threshold);
    let quote_count = channel.max_items.clamp(1, 3);
    let quote_summary = quotes
        .iter()
        .take(quote_count)
        .map(|quote| {
            let currency = if quote.currency.is_empty() {
                String::new()
            } else {
                format!(" {}", quote.currency)
            };
            format!(
                "{} {:.2}{currency} {:+.2}% {}",
                quote.label, quote.price, quote.change_percent, quote.session
            )
        })
        .collect::<Vec<_>>()
        .join(" / ");
    let rule = if crossed {
        format!("move ≥{threshold:.1}%")
    } else {
        format!("inside {threshold:.1}% band")
    };
    let delay = quotes
        .iter()
        .filter_map(|quote| quote.provider_delay_minutes)
        .max()
        .map(|minutes| {
            if minutes == 0 {
                " · provider reports real time".into()
            } else {
                format!(" · provider reports {minutes} min delay")
            }
        })
        .or_else(|| {
            quotes
                .iter()
                .any(|quote| !quote.delay_reported)
                .then(|| " · provider delay not reported".into())
        })
        .unwrap_or_default();
    (format!("{quote_summary} · {rule}{delay}"), crossed)
}

fn market_quote_view(item: &CollectorItem) -> Option<MarketQuoteView<'_>> {
    let label = item
        .attributes
        .get("label")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(item.title.as_str());
    let price = item.attributes.get("price")?.as_f64()?;
    let change_percent = item.attributes.get("change_percent")?.as_f64()?;
    if !price.is_finite() || !change_percent.is_finite() {
        return None;
    }
    let delay_reported = item
        .attributes
        .get("delay_semantics")
        .and_then(Value::as_str)
        == Some("provider_reported");
    Some(MarketQuoteView {
        label,
        price,
        currency: item
            .attributes
            .get("currency")
            .and_then(Value::as_str)
            .unwrap_or(""),
        change_percent,
        session: item
            .attributes
            .get("session_label")
            .and_then(Value::as_str)
            .unwrap_or("SESSION N/A"),
        provider_delay_minutes: item
            .attributes
            .get("provider_delay_minutes")
            .and_then(Value::as_u64),
        delay_reported,
    })
}

fn scope_boolean(channel: &ChannelPreference, key: &str, default: bool) -> bool {
    channel
        .scope
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn scope_f64(channel: &ChannelPreference, key: &str, default: f64) -> f64 {
    channel
        .scope
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or(default)
}

fn scope_strings<'a>(channel: &'a ChannelPreference, key: &str) -> Vec<&'a str> {
    channel
        .scope
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PersonalWeatherState {
    Normal,
    RainHeadsUp,
    WindHeadsUp,
    RainAndWindHeadsUp,
    Stale,
    Offline,
    Disabled,
}

struct WeatherActivation {
    state: PersonalWeatherState,
    summary: String,
}

#[cfg(test)]
fn weather_activation(
    channel: &ChannelPreference,
    items: &[&CollectorItem],
    availability: AvailabilityDto,
    now_ms: i64,
) -> (String, bool) {
    weather_activation_with_units(channel, items, availability, now_ms, UnitSystem::Imperial)
}

fn weather_activation_with_units(
    channel: &ChannelPreference,
    items: &[&CollectorItem],
    availability: AvailabilityDto,
    now_ms: i64,
    unit_system: UnitSystem,
) -> (String, bool) {
    let activation =
        evaluate_weather_activation_with_units(channel, items, availability, now_ms, unit_system);
    let active = matches!(
        activation.state,
        PersonalWeatherState::RainHeadsUp
            | PersonalWeatherState::WindHeadsUp
            | PersonalWeatherState::RainAndWindHeadsUp
    );
    (activation.summary, active)
}

#[cfg(test)]
fn evaluate_weather_activation(
    channel: &ChannelPreference,
    items: &[&CollectorItem],
    availability: AvailabilityDto,
    now_ms: i64,
) -> WeatherActivation {
    evaluate_weather_activation_with_units(
        channel,
        items,
        availability,
        now_ms,
        UnitSystem::Imperial,
    )
}

fn evaluate_weather_activation_with_units(
    channel: &ChannelPreference,
    items: &[&CollectorItem],
    availability: AvailabilityDto,
    now_ms: i64,
    unit_system: UnitSystem,
) -> WeatherActivation {
    if !channel.enabled {
        return WeatherActivation {
            state: PersonalWeatherState::Disabled,
            summary: "Disabled".into(),
        };
    }
    if availability == AvailabilityDto::Offline {
        return WeatherActivation {
            state: PersonalWeatherState::Offline,
            summary: "Weather source offline · personal rules not evaluated".into(),
        };
    }
    if availability == AvailabilityDto::Stale {
        return WeatherActivation {
            state: PersonalWeatherState::Stale,
            summary: format!(
                "Weather data stale · older than {} minutes · personal rules suppressed",
                channel.max_age_minutes
            ),
        };
    }

    let rain = items
        .iter()
        .filter_map(|item| {
            rain_activation_fact(item, channel, now_ms).map(|fact| {
                (
                    fact,
                    item_area_label(item).unwrap_or_else(|| "Configured area".into()),
                )
            })
        })
        .min_by(|(left, _), (right, _)| {
            left.lead_minutes
                .cmp(&right.lead_minutes)
                .then_with(|| right.probability.total_cmp(&left.probability))
        });

    let wind = items
        .iter()
        .filter_map(|item| {
            wind_activation_fact(item, channel, now_ms).map(|fact| {
                (
                    fact,
                    item_area_label(item).unwrap_or_else(|| "Configured area".into()),
                )
            })
        })
        .max_by(|(left, _), (right, _)| left.mph.total_cmp(&right.mph));

    let delayed_suffix = if availability == AvailabilityDto::Delayed {
        " · source response delayed"
    } else {
        ""
    };
    match (rain, wind) {
        (Some((rain, rain_area)), Some((wind, wind_area))) => {
            let (gust, label) = display_wind_speed(wind.mph, unit_system);
            let (threshold, _) = display_wind_speed(wind.threshold, unit_system);
            WeatherActivation {
                state: PersonalWeatherState::RainAndWindHeadsUp,
                summary: format!(
                    "Personal weather heads-up · {rain_area}: rain {:.0}% within {} min (≥{:.0}%) · {wind_area}: gusts {gust:.0} {label} in {} min (≥{threshold:.0} {label}){delayed_suffix}",
                    rain.probability, rain.lead_minutes, rain.threshold, wind.lead_minutes,
                ),
            }
        }
        (Some((rain, area)), None) => WeatherActivation {
            state: PersonalWeatherState::RainHeadsUp,
            summary: format!(
                "Personal rain heads-up · {area}: {:.0}% within {} min (threshold {:.0}%){delayed_suffix}",
                rain.probability, rain.lead_minutes, rain.threshold,
            ),
        },
        (None, Some((wind, area))) => {
            let (gust, label) = display_wind_speed(wind.mph, unit_system);
            let (threshold, _) = display_wind_speed(wind.threshold, unit_system);
            WeatherActivation {
                state: PersonalWeatherState::WindHeadsUp,
                summary: format!(
                    "Personal wind heads-up · {area}: gusts {gust:.0} {label} in {} min (threshold {threshold:.0} {label}){delayed_suffix}",
                    wind.lead_minutes,
                ),
            }
        }
        (None, None) => WeatherActivation {
            state: PersonalWeatherState::Normal,
            summary: items
                .iter()
                .find(|item| item.kind == ItemKind::WeatherCurrent)
                .and_then(|item| current_weather_summary(item, unit_system))
                .unwrap_or_else(|| "No personal weather threshold crossed".into()),
        },
    }
}

fn display_wind_speed(mph: f64, unit_system: UnitSystem) -> (f64, &'static str) {
    match unit_system {
        UnitSystem::Imperial => (mph, "mph"),
        UnitSystem::Metric => (mph * 1.609_344, "km/h"),
    }
}

fn current_weather_summary(item: &CollectorItem, unit_system: UnitSystem) -> Option<String> {
    let temperature = item.attributes.get("temperature_2m")?.as_f64()?;
    let apparent = item
        .attributes
        .get("apparent_temperature")
        .and_then(Value::as_f64);
    let source_unit = item
        .attributes
        .get("units")
        .and_then(Value::as_object)
        .and_then(|units| units.get("temperature_2m"))
        .and_then(Value::as_str)?;
    let celsius = if source_unit == "°F" {
        (temperature - 32.0) / 1.8
    } else if source_unit == "°C" {
        temperature
    } else {
        return item.summary.clone();
    };
    let apparent_celsius = apparent.map(|value| {
        if source_unit == "°F" {
            (value - 32.0) / 1.8
        } else {
            value
        }
    });
    let (temperature, apparent, label) = match unit_system {
        UnitSystem::Imperial => (
            celsius * 1.8 + 32.0,
            apparent_celsius.map(|value| value * 1.8 + 32.0),
            "°F",
        ),
        UnitSystem::Metric => (celsius, apparent_celsius, "°C"),
    };
    Some(match apparent {
        Some(apparent) => format!("{temperature:.0}{label}, feels like {apparent:.0}{label}"),
        None => format!("{temperature:.0}{label}"),
    })
}

fn item_area_label(item: &CollectorItem) -> Option<String> {
    item.attributes
        .get("area_label")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub(crate) fn output_snapshots(preferences: &AppPreferences) -> Vec<OutputSnapshot> {
    let (epaper_state, epaper_detail) = match preferences.display.transport {
        DisplayTransport::Preview => (OutputStateDto::Ready, "Preview transport selected"),
        DisplayTransport::Auto => (
            OutputStateDto::Unconfigured,
            "Auto-detect awaits a desktop USB/BLE adapter",
        ),
        DisplayTransport::Usb => (
            OutputStateDto::Degraded,
            "USB configured; desktop device writer must report readiness",
        ),
        DisplayTransport::Ble => (
            OutputStateDto::Degraded,
            "Bluetooth configured; desktop device writer must report readiness",
        ),
    };
    let (whatsapp_state, whatsapp_detail) = if !preferences.whatsapp.enabled {
        (
            OutputStateDto::Unconfigured,
            "Optional · connect Meta Cloud API",
        )
    } else if preferences.whatsapp.consent == WhatsAppRecipientConsent::NotRecorded {
        (
            OutputStateDto::Unconfigured,
            "WhatsApp recipient opt-in consent has not been recorded",
        )
    } else if preferences.whatsapp.consent == WhatsAppRecipientConsent::Unsubscribed {
        (
            OutputStateDto::Unconfigured,
            "WhatsApp recipient is unsubscribed; delivery is suppressed",
        )
    } else if !whatsapp_consent_is_current(&preferences.whatsapp) {
        (
            OutputStateDto::Unconfigured,
            "WhatsApp opt-in is not bound to the current recipient",
        )
    } else if !preferences.whatsapp.token_configured {
        (
            OutputStateDto::Unconfigured,
            "WhatsApp token is not configured",
        )
    } else {
        (
            OutputStateDto::Ready,
            "Configuration is valid; live delivery health is reported by the host",
        )
    };
    vec![
        OutputSnapshot {
            id: DestinationIdDto::Epaper,
            title: "E-paper".into(),
            state: epaper_state,
            detail: epaper_detail.into(),
            last_accepted_at: None,
            delivery_state: None,
        },
        OutputSnapshot {
            id: DestinationIdDto::Whatsapp,
            title: "WhatsApp".into(),
            state: whatsapp_state,
            detail: whatsapp_detail.into(),
            last_accepted_at: None,
            delivery_state: (whatsapp_state != OutputStateDto::Ready)
                .then_some(DeliveryStateDto::Suppressed),
        },
        OutputSnapshot {
            id: DestinationIdDto::Desktop,
            title: "Desktop notice".into(),
            state: OutputStateDto::Ready,
            detail: "Best-effort OS notification submission; delivery confirmation is unavailable"
                .into(),
            last_accepted_at: None,
            delivery_state: None,
        },
    ]
}
