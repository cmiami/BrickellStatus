/// How hard this channel argues for interrupting, right now.
///
/// This lived in the desktop delivery path and its result was never read by
/// anything that could act on it. It belongs here, beside the facts it is
/// derived from, and it is written onto the snapshot so no surface can quietly
/// decide its own ranking again.
fn channel_urgency(
    kind: ChannelKindDto,
    signal: Option<&ChannelSignalDto>,
    decision: &DecisionSnapshot,
) -> UrgencyDto {
    let severity = signal
        .and_then(|signal| signal.severity.as_deref())
        .map(str::to_ascii_lowercase);
    match kind {
        ChannelKindDto::Bridge => match decision.state {
            BridgeStateDto::Open => UrgencyDto::Emergency,
            BridgeStateDto::Likely => UrgencyDto::HeadsUp,
            BridgeStateDto::Clear | BridgeStateDto::Possible => UrgencyDto::Routine,
        },
        ChannelKindDto::Official => match severity.as_deref() {
            Some("extreme") => UrgencyDto::Emergency,
            Some("severe") => UrgencyDto::Action,
            _ => UrgencyDto::HeadsUp,
        },
        ChannelKindDto::Earthquake => {
            let magnitude = severity
                .as_deref()
                .and_then(|value| value.strip_prefix("magnitude "))
                .and_then(|value| value.trim().parse::<f64>().ok());
            match magnitude {
                Some(value) if value >= 7.0 => UrgencyDto::Emergency,
                Some(value) if value >= 6.0 => UrgencyDto::Action,
                _ => UrgencyDto::HeadsUp,
            }
        }
        // Breaking news was Action, which put it above imminent rain. It is
        // worth showing, not worth stepping in front of weather you are about
        // to walk into.
        ChannelKindDto::News => UrgencyDto::HeadsUp,
        // Rain already falling in this quarter-hour changes a decision you are
        // about to make; rain later today does not.
        ChannelKindDto::Weather => {
            if signal.and_then(|signal| signal.imminence_minutes) == Some(0) {
                UrgencyDto::Action
            } else {
                UrgencyDto::HeadsUp
            }
        }
        ChannelKindDto::Hurricane => UrgencyDto::HeadsUp,
        // A market move never changes whether you should leave the building.
        ChannelKindDto::Markets => UrgencyDto::Routine,
        ChannelKindDto::System => UrgencyDto::Routine,
    }
}

/// Within-kind ordering only. Never large enough to reorder two kinds.
fn channel_severity_rank(kind: ChannelKindDto, signal: Option<&ChannelSignalDto>) -> u8 {
    let severity = signal
        .and_then(|signal| signal.severity.as_deref())
        .map(str::to_ascii_lowercase);
    match kind {
        ChannelKindDto::Official => match severity.as_deref() {
            Some("extreme") => 9,
            Some("severe") => 7,
            Some("moderate") => 4,
            Some("minor") => 2,
            _ => 0,
        },
        ChannelKindDto::Earthquake => severity
            .as_deref()
            .and_then(|value| value.strip_prefix("magnitude "))
            .and_then(|value| value.trim().parse::<f64>().ok())
            .map_or(0, |magnitude| (magnitude.round().max(0.0) as u8).min(9)),
        _ => 0,
    }
}

/// Minutes until this channel's signal affects the reader.
///
/// The bridge answers from its own prediction. Other channels have no dated
/// forecast yet, so they report `None` and score no imminence: an event that
/// cannot say when it matters has not earned a position over one that can.
/// Millimetres in one 15-minute bin that count as rain worth mentioning.
/// Below this the provider is reporting drizzle the reader would not notice.
const DEFAULT_RAIN_AMOUNT_MM: f64 = 0.05;
/// How far ahead the amount rule looks. Half an hour is the horizon over which
/// a quarter-hour bin is still a useful answer rather than a guess.
const DEFAULT_RAIN_WINDOW_MINUTES: f64 = 30.0;
/// One 15-minute bin, in milliseconds.
const MINUTELY_BIN_MS: i64 = 15 * 60 * 1_000;
/// How near a tropical cyclone has to be before it is this reader's problem.
///
/// Tropical-storm-force wind fields rarely reach beyond about 500 km, so 700
/// leaves roughly a day of warning at typical forward speeds without turning
/// every storm in the basin into a notification.
const DEFAULT_HURRICANE_RADIUS_KM: f64 = 700.0;

fn channel_imminence_minutes(
    kind: ChannelKindDto,
    signal: Option<&ChannelSignalDto>,
    decision: &DecisionSnapshot,
) -> Option<u16> {
    match kind {
        // The bridge's ETA lives on the decision rather than the signal,
        // because it is the product of the whole predictor rather than of one
        // matched item.
        ChannelKindDto::Bridge => match decision.state {
            // Already blocking the road; nothing is sooner than now.
            BridgeStateDto::Open => Some(0),
            _ => decision.eta_min,
        },
        // Everything else says so itself, or says nothing.
        _ => signal.and_then(|signal| signal.imminence_minutes),
    }
}

fn channel_priority(
    kind: ChannelKindDto,
    signal: Option<&ChannelSignalDto>,
    decision: &DecisionSnapshot,
    is_anchor: bool,
) -> ChannelPriorityDto {
    let urgency = channel_urgency(kind, signal, decision);
    let imminence_minutes = channel_imminence_minutes(kind, signal, decision);
    let confirmed = matches!(kind, ChannelKindDto::Bridge) && decision.state == BridgeStateDto::Open;
    let score = bridgestatus_policy::priority_score(bridgestatus_policy::PriorityInput {
        urgency: match urgency {
            UrgencyDto::Routine => bridgestatus_policy::Urgency::Routine,
            UrgencyDto::HeadsUp => bridgestatus_policy::Urgency::HeadsUp,
            UrgencyDto::Action => bridgestatus_policy::Urgency::Action,
            UrgencyDto::Emergency => bridgestatus_policy::Urgency::Emergency,
        },
        imminence_minutes,
        confirmed,
        severity_rank: channel_severity_rank(kind, signal),
        is_anchor,
    });
    ChannelPriorityDto {
        score,
        urgency,
        imminence_minutes,
        confirmed,
    }
}

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
            let priority = channel_priority(
                kind,
                signal.as_ref(),
                decision,
                channel.id == preferences.profile.home_channel_id,
            );
            ChannelSnapshot {
                priority,
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

    // Set only by the kinds whose material is a measurement. The rest are
    // authored events and already have a stable identity of their own.
    let mut band = None;
    let mut imminence_minutes = None;
    let mut series = Vec::new();
    let (detail, action, severity): (String, String, Option<String>) = match kind {
        ChannelKindDto::Weather => {
            let weather = weather_signal(item, channel, now_ms, unit_system);
            band = weather.band;
            imminence_minutes = weather.imminence_minutes;
            (
                weather.detail,
                "Expect wet roads and slower traffic.".into(),
                Some(if imminence_minutes == Some(0) {
                    "Falling now".into()
                } else {
                    "Heads-up".into()
                }),
            )
        }
        ChannelKindDto::Official => (
            signal_text(
                item.summary.as_deref(),
                "An active official alert matches this channel.",
                360,
            ),
            "This alert is in force now.".into(),
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
            "Track and intensity only. Local impact is not forecast here."
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
            "Headline only. Open the story for detail.".into(),
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
            "Reported magnitude and location, not local shaking.".into(),
            item.attributes
                .get("magnitude")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .map(|value| format!("Magnitude {value:.1}")),
        ),
        ChannelKindDto::Markets => {
            let quote = market_quote_view(item)?;
            series = item
                .attributes
                .get("series")
                .and_then(Value::as_array)
                .map(|values| values.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default();
            band = Some(format!(
                "{}:{}",
                if quote.change_percent < 0.0 {
                    "down"
                } else {
                    "up"
                },
                move_band(quote.change_percent)
            ));
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
                "Change is against the previous close.".into(),
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
        band,
        imminence_minutes,
        series,
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
        ChannelKindDto::Hurricane => atlantic_cyclone(item) && cyclone_is_local(item, channel),
        ChannelKindDto::News => news_item_matches_scope(item, channel, now_ms),
        ChannelKindDto::Earthquake => earthquake_matches_scope(item, channel, now_ms),
        ChannelKindDto::Markets => market_quote_view(item).is_some_and(|quote| {
            quote.change_percent.abs() >= scope_f64(channel, "movePercent", 5.0)
        }),
        ChannelKindDto::System => false,
    }
}

/// Whether a cyclone is close enough to any of this channel's saved places to
/// be worth showing.
///
/// This replaces a switch that read "all Atlantic systems", defaulted off, and
/// therefore made the channel unable to activate at all — the honest admission
/// at the time was that local impact had not been implemented. Distance is a
/// crude proxy for impact and is named as one: it says the storm is near, not
/// that it will hit.
///
/// Fails closed. A storm with no position, or a channel with no saved place,
/// does not activate — an unplaceable storm is not evidence of a nearby one.
fn cyclone_is_local(item: &CollectorItem, channel: &ChannelPreference) -> bool {
    let (Some(latitude), Some(longitude)) = (
        item.location.as_ref().and_then(|location| location.latitude),
        item.location
            .as_ref()
            .and_then(|location| location.longitude),
    ) else {
        return false;
    };
    let radius_km = scope_f64(channel, "hurricaneRadiusKm", DEFAULT_HURRICANE_RADIUS_KM);
    if !radius_km.is_finite() || radius_km <= 0.0 {
        return false;
    }
    item.attributes
        .get("area_points")
        .and_then(Value::as_array)
        .is_some_and(|points| {
            points.iter().any(|point| {
                let (Some(area_latitude), Some(area_longitude)) = (
                    point.get("lat").and_then(Value::as_f64),
                    point.get("lon").and_then(Value::as_f64),
                ) else {
                    return false;
                };
                great_circle_km(latitude, longitude, area_latitude, area_longitude) <= radius_km
            })
        })
}

/// Great-circle distance in kilometres.
fn great_circle_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6_371.0;
    let (lat1, lat2) = (lat1.to_radians(), lat2.to_radians());
    let delta_lat = lat2 - lat1;
    let delta_lon = (lon2 - lon1).to_radians();
    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().clamp(0.0, 1.0).asin()
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
    measure: RainMeasure,
}

/// Which rule armed the rain heads-up, and the number that armed it.
///
/// The two are not interchangeable and must never be presented as if they
/// were: an hourly bucket can say "some time in the next hour", a 15-minute
/// bin can say "in eight minutes". Keeping the rule attached to the number is
/// what stops the second sentence being written from the first one's data.
#[derive(Clone, Copy, Debug)]
enum RainMeasure {
    /// Millimetres falling in one 15-minute bin. Unlike the probability rule
    /// there is no threshold to report: the floor is a "would anyone notice
    /// this" constant rather than something the reader chose.
    Amount { millimetres: f64 },
    /// Percent chance across an hourly bucket.
    Probability { percent: f64, threshold: f64 },
}

impl RainMeasure {
    /// Named in the band so an amount answer can never dedupe against a
    /// probability one that happened to land in the same bins.
    fn rule(self) -> &'static str {
        match self {
            Self::Amount { .. } => "rain-amount",
            Self::Probability { .. } => "rain-probability",
        }
    }

    fn band(self) -> &'static str {
        match self {
            Self::Amount { millimetres } => amount_band(millimetres),
            Self::Probability { percent, .. } => probability_band(percent),
        }
    }

    /// The parenthetical that tells the reader which question was answered.
    fn describe(self) -> String {
        match self {
            Self::Amount { millimetres } => format!("{millimetres:.2} mm expected"),
            Self::Probability {
                percent, threshold, ..
            } => format!("{percent:.0}% chance, threshold {threshold:.0}%"),
        }
    }

    /// Orders two facts with the same lead time. Heavier and likelier first.
    fn magnitude(self) -> f64 {
        match self {
            Self::Amount { millimetres } => millimetres,
            Self::Probability { percent, .. } => percent,
        }
    }

    fn is_amount(self) -> bool {
        matches!(self, Self::Amount { .. })
    }
}

/// Rain intensity bands, in millimetres per 15-minute bin. The boundaries are
/// roughly 0.2, 1 and 3 mm/h once scaled to the hour, which is the usual
/// light/moderate/heavy split.
fn amount_band(millimetres: f64) -> &'static str {
    match millimetres {
        amount if amount < 0.25 => "trace",
        amount if amount < 0.75 => "light",
        amount if amount < 2.5 => "moderate",
        _ => "heavy",
    }
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
    if !scope_boolean(channel, "rainAlertEnabled", false) {
        return None;
    }
    match item.kind {
        ItemKind::WeatherMinutely => rain_amount_fact(item, channel, now_ms),
        ItemKind::WeatherHourly => rain_probability_fact(item, channel, now_ms),
        _ => None,
    }
}

/// The preferred rule: a named quarter-hour with measured millimetres in it.
///
/// Deliberately narrow. It reads only its own bin, so a bin that does not reach
/// the window simply does not arm — there is no path by which a coarser bucket
/// can be reported as a 15-minute answer.
fn rain_amount_fact(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> Option<RainActivationFact> {
    let threshold = scope_f64(channel, "rainAmountMm", DEFAULT_RAIN_AMOUNT_MM);
    if !threshold.is_finite() || threshold <= 0.0 {
        return None;
    }
    let window_minutes = scope_f64(channel, "rainWindowMinutes", DEFAULT_RAIN_WINDOW_MINUTES)
        .round()
        .clamp(0.0, 180.0) as i64;
    let starts_ms = item.starts_at.as_ref()?.timestamp_millis();
    // A bin describes its own quarter-hour, so the one in progress stays useful
    // until it ends.
    if starts_ms.saturating_add(MINUTELY_BIN_MS) < now_ms {
        return None;
    }
    if starts_ms.saturating_sub(now_ms).max(0) / 60_000 > window_minutes {
        return None;
    }
    let millimetres = item
        .attributes
        .get("precipitation")?
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)?;
    if item
        .attributes
        .get("units")?
        .as_object()?
        .get("precipitation")?
        .as_str()?
        .trim()
        != "mm"
    {
        return None;
    }
    (millimetres >= threshold).then_some(RainActivationFact {
        // A bin already in progress is rain now, not rain later.
        lead_minutes: starts_ms.saturating_sub(now_ms).max(0) / 60_000,
        measure: RainMeasure::Amount { millimetres },
    })
}

fn rain_probability_fact(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> Option<RainActivationFact> {
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
        measure: RainMeasure::Probability {
            percent: probability,
            threshold,
        },
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
    let rain = rain_activation_fact(item, channel, now_ms).map(|fact| {
        // Amount sits a band above probability so a measured quarter-hour is
        // always the item chosen to speak, even when an hourly bucket claims a
        // shorter lead.
        let rule_floor = if fact.measure.is_amount() {
            30_000.0
        } else {
            20_000.0
        };
        rule_floor - fact.lead_minutes as f64 + fact.measure.magnitude() / 1_000.0
    });
    let wind = wind_activation_fact(item, channel, now_ms).map(|fact| 10_000.0 + fact.mph);
    match (rain, wind) {
        (Some(rain), Some(wind)) => Some(rain.max(wind)),
        (Some(score), None) | (None, Some(score)) => Some(score),
        (None, None) => None,
    }
}

/// The weather signal's prose and its banded identity, derived together.
///
/// They are computed in one pass on purpose: a band that can disagree with the
/// sentence it summarizes is worse than no band, because dedupe would then
/// suppress a message whose text had genuinely changed.
struct WeatherSignal {
    detail: String,
    band: Option<String>,
    /// Set only by the amount rule. An hourly probability describes a bucket,
    /// not a moment, so reporting it as an ETA would be a guess wearing a
    /// number — and it is the term that outranks every other channel.
    imminence_minutes: Option<u16>,
}

fn weather_signal(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
    unit_system: UnitSystem,
) -> WeatherSignal {
    let rain_fact = rain_activation_fact(item, channel, now_ms);
    let wind_fact = wind_activation_fact(item, channel, now_ms);
    let rain = rain_fact.map(|fact| {
        let timing = if fact.lead_minutes == 0 {
            "now".into()
        } else {
            format!("in {} min", fact.lead_minutes)
        };
        format!("Rain {timing} ({})", fact.measure.describe())
    });
    let wind = wind_fact.map(|fact| {
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

    // The rule is named in the band so a future amount-based rain rule can never
    // be mistaken for a probability one that happened to land in the same bins.
    let mut parts = Vec::new();
    if let Some(fact) = rain_fact {
        parts.push(format!(
            "{}:{}:{}",
            fact.measure.rule(),
            lead_band(fact.lead_minutes),
            fact.measure.band()
        ));
    }
    if let Some(fact) = wind_fact {
        // Banded on the canonical mph the fact carries, not on the displayed
        // value, so switching units never re-alerts.
        parts.push(format!(
            "wind-gust:{}:{}",
            lead_band(fact.lead_minutes),
            gust_band(fact.mph)
        ));
    }

    WeatherSignal {
        detail: bounded_signal_text(&detail, 360),
        band: (!parts.is_empty()).then(|| parts.join("+")),
        imminence_minutes: rain_fact
            .filter(|fact| fact.measure.is_amount())
            .map(|fact| fact.lead_minutes.clamp(0, i64::from(u16::MAX)) as u16),
    }
}

/// Lead-time bands. Crossing one changes what a person would do; moving inside
/// one does not.
fn lead_band(minutes: i64) -> &'static str {
    match minutes {
        ..=5 => "0-5",
        6..=15 => "6-15",
        16..=30 => "16-30",
        31..=60 => "31-60",
        _ => "60+",
    }
}

fn probability_band(percent: f64) -> &'static str {
    match percent.round() as i64 {
        ..=39 => "p0-39",
        40..=59 => "p40-59",
        60..=79 => "p60-79",
        80..=94 => "p80-94",
        _ => "p95+",
    }
}

/// Gust bands in mph. The top edge is 58 mph because that is the National
/// Weather Service's severe threshold, not a round number.
fn gust_band(mph: f64) -> &'static str {
    match mph.round() as i64 {
        ..=24 => "g0-24",
        25..=39 => "g25-39",
        40..=57 => "g40-57",
        _ => "g58+",
    }
}

/// Market bands, on absolute move. Direction is carried separately so a swing
/// through zero is always news.
fn move_band(change_percent: f64) -> &'static str {
    match change_percent.abs() {
        percent if percent < 1.0 => "0-1",
        percent if percent < 2.0 => "1-2",
        percent if percent < 5.0 => "2-5",
        percent if percent < 10.0 => "5-10",
        _ => "10+",
    }
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
    // The channel budget says how old an answer may be before it stops being
    // useful. It cannot also serve as the staleness test for a source that is
    // deliberately collected less often than that: the pilots' board is polled
    // every ten minutes, so a two-minute budget would mark it stale for eight
    // minutes out of every ten and report a permanent fault that is really a
    // schedule. Allow a source its own cadence on top of the budget.
    let budget = u64::from(channel.max_age_minutes) * 60;
    let cadence = source
        .poll_interval_ms
        .and_then(|ms| u64::try_from(ms / 1_000).ok())
        .unwrap_or(0);
    let stale_after = budget.saturating_add(cadence);
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
    tracks.sort_by_key(|(observed_ms, _)| std::cmp::Reverse(*observed_ms));
    tracks.truncate(MAX_MAP_VESSEL_TRACKS);
    tracks.into_iter().map(|(_, track)| track).collect()
}

fn valid_map_coordinate(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
}

// Every argument is an independent input to the summary, so bundling them into
// a parameter struct would only move the same list one level out.
#[allow(clippy::too_many_arguments)]
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
            let storms = items
                .iter()
                .filter(|item| atlantic_cyclone(item))
                .collect::<Vec<_>>();
            let near = storms
                .iter()
                .filter(|item| cyclone_is_local(item, channel))
                .count();
            match (storms.len(), near) {
                (0, _) => (
                    "No active Atlantic cyclone in NHC CurrentStorms".into(),
                    false,
                ),
                (total, 0) => (
                    format!("{total} active Atlantic system(s) · none within range of a saved place"),
                    false,
                ),
                (_, near) => (format!("{near} Atlantic cyclone(s) within range"), true),
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
        // Honest about the outcome without naming the provider. "Waiting for
        // the first quote" was tried here and was worse: it reads as benign, so
        // a market source failing every single poll looked like a channel that
        // had merely started a moment ago. The coverage suffix appended by the
        // caller supplies the detail.
        return ("Quotes unavailable".into(), false);
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
        return ("No quote available".into(), false);
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

/// "now" for a bin already in progress, "in N min" otherwise. The amount rule
/// can legitimately report zero lead; the probability rule effectively cannot.
fn rain_timing(fact: &RainActivationFact) -> String {
    if fact.lead_minutes == 0 {
        "now".into()
    } else {
        format!("within {} min", fact.lead_minutes)
    }
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
            // Amount first: an hourly bucket must never speak over a measured
            // quarter-hour, however short its nominal lead.
            right
                .measure
                .is_amount()
                .cmp(&left.measure.is_amount())
                .then_with(|| left.lead_minutes.cmp(&right.lead_minutes))
                .then_with(|| right.measure.magnitude().total_cmp(&left.measure.magnitude()))
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
                    "Personal weather heads-up · {rain_area}: rain {} ({}) · {wind_area}: gusts {gust:.0} {label} in {} min (≥{threshold:.0} {label}){delayed_suffix}",
                    rain_timing(&rain),
                    rain.measure.describe(),
                    wind.lead_minutes,
                ),
            }
        }
        (Some((rain, area)), None) => WeatherActivation {
            state: PersonalWeatherState::RainHeadsUp,
            summary: format!(
                "Personal rain heads-up · {area}: {} ({}){delayed_suffix}",
                rain_timing(&rain),
                rain.measure.describe(),
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
