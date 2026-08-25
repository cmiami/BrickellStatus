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
            BridgeStateDto::Clear => UrgencyDto::Routine,
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
        // An ordinary headline belongs in the current set but never interrupts.
        // A publisher-authored breaking label raises it one band; even then an
        // imminent bridge or rain event still carries the time advantage.
        ChannelKindDto::News => {
            if severity.as_deref() == Some("breaking") {
                UrgencyDto::HeadsUp
            } else {
                UrgencyDto::Routine
            }
        }
        // A trade, a draft pick, or a final score is worth a rotation slot and
        // nothing more. Sports never interrupts.
        ChannelKindDto::Sports => UrgencyDto::Routine,
        // Only a current precipitation observation proves rain is falling.
        // Forecast windows can still earn an imminence boost below, but must
        // not turn a likely hourly bucket into an observed condition.
        ChannelKindDto::Weather => {
            if severity.as_deref() == Some("falling now") {
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
/// Open-Meteo's `current` values are resolved from the same 15-minute model
/// intervals as its minutely feed. A non-zero current amount is useful through
/// the end of that interval, not merely until the next forecast response.
const CURRENT_WEATHER_BIN_MS: i64 = MINUTELY_BIN_MS;
const MAX_WEATHER_CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;
/// An hourly chance this strong, whose bucket is about to begin, is actionable
/// enough to take the same ordering path as a named 15-minute forecast. The
/// copy still describes the bucket rather than pretending this is an onset ETA.
const HIGH_CONFIDENCE_RAIN_PERCENT: f64 = 80.0;
const IMMINENT_WEATHER_WINDOW_MINUTES: i64 = 15;
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
    let confirmed = match kind {
        ChannelKindDto::Bridge => decision.state == BridgeStateDto::Open,
        ChannelKindDto::Weather => signal.and_then(|signal| signal.severity.as_deref())
            == Some("Falling now"),
        ChannelKindDto::Official
        | ChannelKindDto::Hurricane
        | ChannelKindDto::News
        | ChannelKindDto::Sports
        | ChannelKindDto::Earthquake
        | ChannelKindDto::Markets => signal.is_some(),
        ChannelKindDto::System => false,
    };
    let score = brickellstatus_policy::priority_score(brickellstatus_policy::PriorityInput {
        urgency: match urgency {
            UrgencyDto::Routine => brickellstatus_policy::Urgency::Routine,
            UrgencyDto::HeadsUp => brickellstatus_policy::Urgency::HeadsUp,
            UrgencyDto::Action => brickellstatus_policy::Urgency::Action,
            UrgencyDto::Emergency => brickellstatus_policy::Urgency::Emergency,
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
            let notices = if active {
                channel_notices(
                    kind,
                    channel,
                    &items,
                    now_ms,
                    preferences.unit_system,
                    decision,
                    channel.id == preferences.profile.home_channel_id,
                )
            } else {
                Vec::new()
            };
            let signal = notices.first().map(|notice| notice.signal.clone());
            let coverage_complete = coverage.total_sources > 0
                && coverage.usable_sources == coverage.total_sources
                && (kind != ChannelKindDto::Bridge
                    || bridge_resolution_confirmed(channel, state, now_ms));
            let priority = notices.first().map_or_else(
                || {
                    channel_priority(
                        kind,
                        signal.as_ref(),
                        decision,
                        channel.id == preferences.profile.home_channel_id,
                    )
                },
                |notice| notice.priority,
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
                notices,
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

fn channel_notices(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    usable_items: &[&CollectorItem],
    now_ms: i64,
    unit_system: UnitSystem,
    decision: &DecisionSnapshot,
    is_anchor: bool,
) -> Vec<ChannelNoticeDto> {
    if matches!(kind, ChannelKindDto::Bridge | ChannelKindDto::System) {
        return Vec::new();
    }
    if kind == ChannelKindDto::Weather {
        // Provider rows are evidence about one current weather state, not
        // separate authored events. Pick rain and wind independently so a
        // precise rain bin cannot hide a qualifying gust in another row, then
        // publish the composed state as exactly one notice.
        let (rain, wind) = best_weather_facts(channel, usable_items, now_ms);
        let Some(signal) = weather_channel_signal(rain, wind, unit_system) else {
            return Vec::new();
        };
        let priority = channel_priority(kind, Some(&signal), decision, is_anchor);
        return vec![ChannelNoticeDto {
            key: channel_weather_notice_key(channel),
            source_url: usable_items.iter().find_map(|item| item_source_url(item)),
            signal,
            priority,
        }];
    }

    let mut notices = matching_channel_items(kind, channel, usable_items, now_ms)
        .into_iter()
        .filter_map(|item| {
            let signal = channel_signal_from_item(kind, channel, item, now_ms, unit_system)?;
            let priority = channel_priority(kind, Some(&signal), decision, is_anchor);
            Some(ChannelNoticeDto {
                key: channel_notice_key(channel, item),
                source_url: item_source_url(item),
                signal,
                priority,
            })
        })
        .collect::<Vec<_>>();
    notices.sort_by(|left, right| {
        right
            .priority
            .score
            .cmp(&left.priority.score)
    });
    notices
}

fn item_source_url(item: &CollectorItem) -> Option<String> {
    item.source.url.as_ref().map(ToString::to_string)
}

#[cfg(test)]
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
    if kind == ChannelKindDto::Weather {
        let (rain, wind) = best_weather_facts(channel, usable_items, now_ms);
        return weather_channel_signal(rain, wind, unit_system);
    }
    let item = matching_channel_items(kind, channel, usable_items, now_ms)
        .into_iter()
        .next()?;
    channel_signal_from_item(kind, channel, item, now_ms, unit_system)
}

fn channel_notice_key(channel: &ChannelPreference, item: &CollectorItem) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"channel-notice\0");
    hasher.update(channel.id.as_bytes());
    hasher.update([0]);
    hasher.update(item.id.as_bytes());
    format!("notice:{}", hex_digest(&hasher.finalize()))
}

fn channel_weather_notice_key(channel: &ChannelPreference) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"channel-weather-notice\0");
    hasher.update(channel.id.as_bytes());
    format!("notice:{}", hex_digest(&hasher.finalize()))
}

fn channel_signal_from_item(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    item: &CollectorItem,
    now_ms: i64,
    unit_system: UnitSystem,
) -> Option<ChannelSignalDto> {
    if kind == ChannelKindDto::Weather {
        return weather_channel_signal(
            rain_activation_fact(item, channel, now_ms),
            wind_activation_fact(item, channel, now_ms),
            unit_system,
        );
    }

    let headline = signal_text(Some(&item.title), &channel.title, 160);
    let expires_at = item
        .ends_at
        .as_ref()
        .map(|value| value.timestamp_millis())
        .or_else(|| authored_item_expiration_ms(kind, channel, item))
        .and_then(|value| iso_timestamp(value).ok());

    // Set only by the kinds whose material is a measurement. The rest are
    // authored events and already have a stable identity of their own.
    let mut band = None;
    let imminence_minutes = None;
    let mut series = Vec::new();
    let mut previous_close = None;
    let (detail, action, severity): (String, String, Option<String>) = match kind {
        ChannelKindDto::Weather => unreachable!("weather returns before authored composition"),
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
        // This used to say "Headline only. Open the story for detail", which
        // told a non-interactive panel reader nothing. Keep the publisher as a
        // structured fact; the panel projection promotes it to the ruled row
        // and spends the lower field on the synopsis.
        ChannelKindDto::News => (
            syndicated_item_detail(item, now_ms),
            item_publisher(item),
            Some(if news_item_is_breaking(item) {
                "Breaking".into()
            } else {
                "Routine".into()
            }),
        ),
        ChannelKindDto::Sports => (
            syndicated_item_detail(item, now_ms),
            item_publisher(item),
            Some(if sports_item_is_transaction(item) {
                "Roster move".into()
            } else {
                "Routine".into()
            }),
        ),
        ChannelKindDto::Earthquake => (
            earthquake_local_time(item),
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
            previous_close = quote.previous_close;
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
        headline,
        detail,
        action: bounded_signal_text(&action, 240),
        severity,
        expires_at,
        band,
        imminence_minutes,
        series,
        previous_close,
    })
}

/// Authored items without a provider end time still need an absolute end for
/// clients that are holding a snapshot between runtime refreshes. These are the
/// same automatic age windows used by each kind's relevance matcher.
fn authored_item_expiration_ms(
    kind: ChannelKindDto,
    channel: &ChannelPreference,
    item: &CollectorItem,
) -> Option<i64> {
    let observed_ms = item.observed_at.as_ref()?.timestamp_millis();
    let minutes = match kind {
        ChannelKindDto::Hurricane
        | ChannelKindDto::News
        | ChannelKindDto::Sports
        | ChannelKindDto::Markets => f64::from(channel.max_age_minutes),
        ChannelKindDto::Earthquake => scope_f64(
            channel,
            "eventAgeMinutes",
            f64::from(channel.max_age_minutes),
        ),
        _ => return None,
    };
    if !minutes.is_finite() || minutes < 0.0 {
        return None;
    }
    let duration_ms = (minutes * 60_000.0).round().clamp(0.0, i64::MAX as f64) as i64;
    Some(observed_ms.saturating_add(duration_ms))
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
    matching.truncate(crate::preferences::AUTOMATIC_ITEM_LIMIT);
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
        // A completed transaction outranks a preview or a recap: it is the one
        // sports item that is news rather than commentary.
        ChannelKindDto::Sports => {
            if sports_item_is_transaction(item) {
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

/// When the quake happened, on the reader's own clock.
///
/// USGS titles already read "M 4.5 - 80 km W of El Aguilar, Argentina", and
/// `item.summary` is built from the same magnitude and place -- so a card made
/// of both said the same thing twice and spent its second line doing it. The
/// news and sports paths strip that repetition with `copy_without_leading`;
/// here there is nothing left once it is stripped, because the title is the
/// whole fact.
///
/// Time is what the title does not carry and what a reader checks next: a
/// tremor an hour ago and one three days ago are different news at the same
/// magnitude. It is shown in the machine's own zone rather than UTC, because
/// the panel is read by someone standing in front of it.
fn earthquake_local_time(item: &CollectorItem) -> String {
    local_clock_label(item_time_ms(item))
}

/// Renders an instant on the machine's own clock, or says it has none.
fn local_clock_label(milliseconds: i64) -> String {
    if milliseconds == i64::MIN {
        return "Time not reported".into();
    }
    let Ok(timestamp) = jiff::Timestamp::from_millisecond(milliseconds) else {
        return "Time not reported".into();
    };
    timestamp
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime("%b %-d, %-I:%M %p")
        .to_string()
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
        // Sports arrives over the same syndication collector as news, so it
        // reuses the same topic, age, and exclusion gates rather than growing a
        // second copy of them that could drift.
        ChannelKindDto::News | ChannelKindDto::Sports => {
            news_item_matches_scope(item, channel, now_ms)
        }
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

/// The publisher a syndicated item came from, for the card's action line.
///
/// `SourceLink.name` is resolved by the parser from the feed's own title, so a
/// catalog entry and a hand-typed URL both name themselves honestly. The
/// fallback only fires for a feed that states no title at all.
fn item_publisher(item: &CollectorItem) -> String {
    let name = item.source.name.trim();
    if name.is_empty() {
        "Syndicated feed".into()
    } else {
        name.to_owned()
    }
}

/// Supporting copy for a syndicated headline, with each fact stated once.
///
/// Google News descriptions open by repeating the complete headline, then
/// concatenate links to other reporting. That is not a synopsis and the panel
/// cannot open those links, so it is discarded rather than presented as useful
/// story detail. Direct feeds keep their real summary. When a publisher supplied
/// no synopsis at all, a byline and the story's publication age are the next
/// useful facts the item actually knows.
fn syndicated_item_detail(item: &CollectorItem, now_ms: i64) -> String {
    let title = bounded_signal_text(&item.title, 360);
    if let Some(summary) = item
        .summary
        .as_deref()
        .map(|summary| bounded_signal_text(summary, 360))
        .filter(|summary| !summary.is_empty())
    {
        if let Some(remainder) = copy_without_leading(&summary, &title) {
            let google_cluster = item
                .source
                .url
                .as_ref()
                .and_then(url::Url::host_str)
                .is_some_and(|host| host.eq_ignore_ascii_case("news.google.com"));
            if !google_cluster && !copy_key(remainder).is_empty() {
                return bounded_signal_text(remainder, 360);
            }
        } else if copy_key(&summary) != copy_key(&title) {
            return summary;
        }
    }

    let age = publication_age(item, now_ms);
    let authors = item
        .attributes
        .get("authors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|author| !author.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(", ");
    if authors.is_empty() {
        format!("Published {age}")
    } else {
        bounded_signal_text(&format!("By {authors} · {age}"), 360)
    }
}

/// Removes one repeated leading field plus punctuation used as a separator.
fn copy_without_leading<'a>(value: &'a str, repeated: &str) -> Option<&'a str> {
    let candidate = value.get(..repeated.len())?;
    candidate.eq_ignore_ascii_case(repeated).then(|| {
        value[repeated.len()..].trim_start_matches(|character: char| {
            character.is_whitespace()
                || matches!(character, ':' | '-' | '\u{2013}' | '\u{2014}' | '·' | '|')
        })
    })
}

/// Case- and punctuation-insensitive key used only to detect duplicate copy.
fn copy_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn publication_age(item: &CollectorItem, now_ms: i64) -> String {
    let age_seconds = item
        .observed_at
        .as_ref()
        .map_or(0, |observed| now_ms.saturating_sub(observed.timestamp_millis()).max(0) / 1_000);
    match age_seconds {
        0..=59 => "just now".into(),
        60..=3_599 => format!("{} min ago", age_seconds / 60),
        3_600..=86_399 => format!("{} hr ago", age_seconds / 3_600),
        _ => format!("{} days ago", age_seconds / 86_400),
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

/// Whether a sports item reports a roster move rather than commentary about
/// one.
///
/// Word-bounded on purpose. The news matcher next door tests raw substrings, so
/// its `transit` rule also fires on `transitional`; here `sign` would otherwise
/// match `design` and `assignment`, and a preview column would outrank the
/// trade it speculates about.
fn sports_item_is_transaction(item: &CollectorItem) -> bool {
    const TRANSACTION_WORDS: &[&str] = &[
        "acquire",
        "acquired",
        "acquires",
        "claim",
        "claimed",
        "draft",
        "drafted",
        "drafts",
        "extension",
        "release",
        "released",
        "signing",
        "signs",
        "trade",
        "traded",
        "trades",
        "waived",
        "waivers",
    ];

    let matches = |value: &str| {
        value
            .to_ascii_lowercase()
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|word| TRANSACTION_WORDS.contains(&word))
    };

    matches(&item.title)
        || item.summary.as_deref().is_some_and(matches)
        || item
            .attributes
            .get("categories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(matches)
}

/// Returns a deterministic ranking score only when this particular forecast
/// item crosses at least one enabled personal weather rule. This mirrors the
/// channel activation gates so unrelated forecast rows cannot become the
/// delivered signal or its material identity.
#[derive(Clone, Copy, Debug)]
struct RainActivationFact {
    lead_minutes: i64,
    measure: RainMeasure,
    evidence: RainEvidence,
    valid_until_ms: i64,
    supports_priority_imminence: bool,
}

/// What the provider actually knows about this rain period. This distinction
/// is part of the product truth: a current amount may say rain is falling, a
/// forecast amount names a quarter-hour, and a probability only names an hour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RainEvidence {
    CurrentObservation,
    QuarterHourForecast,
    HourlyForecast,
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
    Probability { percent: f64 },
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
            Self::Probability { percent } => probability_band(percent),
        }
    }

    /// Orders two facts with the same lead time. Heavier and likelier first.
    fn magnitude(self) -> f64 {
        match self {
            Self::Amount { millimetres } => millimetres,
            Self::Probability { percent } => percent,
        }
    }

    fn is_amount(self) -> bool {
        matches!(self, Self::Amount { .. })
    }
}

impl RainActivationFact {
    fn priority_imminence_minutes(self) -> Option<u16> {
        self.supports_priority_imminence
            .then_some(self.lead_minutes.clamp(0, i64::from(u16::MAX)) as u16)
    }

    fn headline(self) -> &'static str {
        match self.evidence {
            RainEvidence::CurrentObservation => "Rain is falling now",
            RainEvidence::QuarterHourForecast if self.lead_minutes <= 15 => "Rain expected soon",
            RainEvidence::QuarterHourForecast => "Rain expected later",
            RainEvidence::HourlyForecast if self.priority_imminence_minutes().is_some() => {
                "Rain likely soon"
            }
            RainEvidence::HourlyForecast => "Chance of rain",
        }
    }

    fn detail(self) -> String {
        match (self.evidence, self.measure) {
            (RainEvidence::CurrentObservation, RainMeasure::Amount { millimetres }) => {
                format!("{millimetres:.2} mm observed in the current 15-minute period.")
            }
            (RainEvidence::QuarterHourForecast, RainMeasure::Amount { millimetres })
                if self.lead_minutes == 0 =>
            {
                format!("{millimetres:.2} mm forecast in the current 15-minute period.")
            }
            (RainEvidence::QuarterHourForecast, RainMeasure::Amount { millimetres }) => format!(
                "{millimetres:.2} mm forecast for the 15-minute period beginning in {} min.",
                self.lead_minutes
            ),
            (RainEvidence::HourlyForecast, RainMeasure::Probability { percent })
                if self.lead_minutes == 0 =>
            {
                format!("{percent:.0}% chance of rain this hour.")
            }
            (RainEvidence::HourlyForecast, RainMeasure::Probability { percent }) => format!(
                "{percent:.0}% chance of rain in the hour beginning in {} min.",
                self.lead_minutes
            ),
            // Constructors keep these combinations impossible. Retain a
            // truthful fallback so malformed future data cannot invent copy.
            (_, RainMeasure::Amount { millimetres }) => {
                format!("{millimetres:.2} mm of precipitation reported.")
            }
            (_, RainMeasure::Probability { percent }) => {
                format!("{percent:.0}% chance of rain.")
            }
        }
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
    valid_until_ms: i64,
}

impl WindActivationFact {
    fn priority_imminence_minutes(self) -> u16 {
        self.lead_minutes.clamp(0, i64::from(u16::MAX)) as u16
    }
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
        ItemKind::WeatherCurrent => rain_current_fact(item, channel, now_ms),
        ItemKind::WeatherMinutely => rain_amount_fact(item, channel, now_ms),
        ItemKind::WeatherHourly => rain_probability_fact(item, channel, now_ms),
        _ => None,
    }
}

/// A non-zero current amount is direct evidence that the active weather bin is
/// wet. Keeping it alongside forecasts prevents a revised future bin from
/// clearing rain that the provider still reports in the current period.
fn rain_current_fact(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> Option<RainActivationFact> {
    let observed_ms = item.observed_at.as_ref()?.timestamp_millis();
    let valid_until_ms = observed_ms.saturating_add(CURRENT_WEATHER_BIN_MS);
    if observed_ms > now_ms.saturating_add(MAX_WEATHER_CLOCK_SKEW_MS) || valid_until_ms <= now_ms {
        return None;
    }
    let millimetres = qualifying_rain_amount(item, channel)?;
    Some(RainActivationFact {
        lead_minutes: 0,
        measure: RainMeasure::Amount { millimetres },
        evidence: RainEvidence::CurrentObservation,
        valid_until_ms,
        supports_priority_imminence: true,
    })
}

fn qualifying_rain_amount(item: &CollectorItem, channel: &ChannelPreference) -> Option<f64> {
    let threshold = scope_f64(channel, "rainAmountMm", DEFAULT_RAIN_AMOUNT_MM);
    if !threshold.is_finite() || threshold <= 0.0 {
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
    (millimetres >= threshold).then_some(millimetres)
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
    let window_minutes = scope_f64(channel, "rainWindowMinutes", DEFAULT_RAIN_WINDOW_MINUTES)
        .round()
        .clamp(0.0, 180.0) as i64;
    let starts_ms = item.starts_at.as_ref()?.timestamp_millis();
    let valid_until_ms = starts_ms.saturating_add(MINUTELY_BIN_MS);
    // A bin describes its own quarter-hour, so the one in progress stays useful
    // until it ends.
    if valid_until_ms <= now_ms {
        return None;
    }
    if starts_ms.saturating_sub(now_ms).max(0) / 60_000 > window_minutes {
        return None;
    }
    let millimetres = qualifying_rain_amount(item, channel)?;
    Some(RainActivationFact {
        // A forecast period already in progress has zero lead, but remains a
        // forecast rather than being promoted to an observation.
        lead_minutes: starts_ms.saturating_sub(now_ms).max(0) / 60_000,
        measure: RainMeasure::Amount { millimetres },
        evidence: RainEvidence::QuarterHourForecast,
        valid_until_ms,
        supports_priority_imminence: true,
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
    if starts_ms.saturating_add(60 * 60 * 1_000) <= now_ms {
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
        },
        evidence: RainEvidence::HourlyForecast,
        valid_until_ms: starts_ms.saturating_add(60 * 60 * 1_000),
        supports_priority_imminence: probability >= HIGH_CONFIDENCE_RAIN_PERCENT
            && starts_ms >= now_ms
            && starts_ms.saturating_sub(now_ms)
                <= IMMINENT_WEATHER_WINDOW_MINUTES.saturating_mul(60_000),
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
    let (lead_minutes, valid_until_ms) = match item.kind {
        ItemKind::WeatherCurrent => {
            let observed_ms = item.observed_at.as_ref()?.timestamp_millis();
            let maximum_age_ms = i64::from(channel.max_age_minutes).saturating_mul(60 * 1_000);
            if observed_ms > now_ms.saturating_add(MAX_WEATHER_CLOCK_SKEW_MS)
                || observed_ms.saturating_add(maximum_age_ms) <= now_ms
            {
                return None;
            }
            (0, observed_ms.saturating_add(maximum_age_ms))
        }
        ItemKind::WeatherHourly => {
            let starts_ms = item.starts_at.as_ref()?.timestamp_millis();
            if starts_ms.saturating_add(60 * 60 * 1_000) <= now_ms {
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
            (lead_minutes, starts_ms.saturating_add(60 * 60 * 1_000))
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
        valid_until_ms,
    })
}

fn weather_item_activation_score(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
) -> Option<f64> {
    let rain = rain_activation_fact(item, channel, now_ms).map(rain_activation_score);
    let wind = wind_activation_fact(item, channel, now_ms).map(wind_activation_score);
    match (rain, wind) {
        (Some(rain), Some(wind)) => Some(rain.max(wind)),
        (Some(score), None) | (None, Some(score)) => Some(score),
        (None, None) => None,
    }
}

fn wind_activation_score(fact: WindActivationFact) -> f64 {
    // A threshold-crossing gust that affects the reader sooner is the better
    // representative of the current channel state. Strength breaks equal-lead
    // ties without allowing a distant, slightly stronger bucket to hide it.
    10_000.0 - fact.lead_minutes as f64 + fact.mph / 1_000.0
}

fn best_weather_facts(
    channel: &ChannelPreference,
    items: &[&CollectorItem],
    now_ms: i64,
) -> (Option<RainActivationFact>, Option<WindActivationFact>) {
    let rain = items
        .iter()
        .filter_map(|item| rain_activation_fact(item, channel, now_ms))
        .max_by(|left, right| {
            rain_activation_score(*left).total_cmp(&rain_activation_score(*right))
        });
    let wind = items
        .iter()
        .filter_map(|item| wind_activation_fact(item, channel, now_ms))
        .max_by(|left, right| {
            wind_activation_score(*left).total_cmp(&wind_activation_score(*right))
        });
    (rain, wind)
}

fn rain_activation_score(fact: RainActivationFact) -> f64 {
    // Current evidence speaks first. Inside the 15-minute action window, a
    // named quarter-hour remains more precise than an hourly chance at the same
    // lead; either outranks a more distant amount forecast. Outside that window
    // the narrower amount rule remains the preferred source of copy.
    let rule_floor = match (fact.evidence, fact.priority_imminence_minutes()) {
        (RainEvidence::CurrentObservation, _) => 40_000.0,
        (RainEvidence::QuarterHourForecast, Some(minutes)) if minutes <= 15 => 36_000.0,
        (RainEvidence::HourlyForecast, Some(_)) => 35_000.0,
        (_, _) if fact.measure.is_amount() => 30_000.0,
        _ => 20_000.0,
    };
    rule_floor - fact.lead_minutes as f64 + fact.measure.magnitude() / 1_000.0
}

/// The weather signal's prose and its banded identity, derived together.
///
/// They are computed in one pass on purpose: a band that can disagree with the
/// sentence it summarizes is worse than no band, because dedupe would then
/// suppress a message whose text had genuinely changed.
struct WeatherSignal {
    headline: String,
    detail: String,
    action: String,
    severity: String,
    expires_at: Option<String>,
    band: Option<String>,
    /// A 15-minute amount names its forecast period. A high-confidence hourly
    /// chance may use the beginning of its bucket for ordering only when that
    /// bucket is within 15 minutes; its copy continues to call it an hour.
    imminence_minutes: Option<u16>,
}

#[cfg(test)]
fn weather_signal(
    item: &CollectorItem,
    channel: &ChannelPreference,
    now_ms: i64,
    unit_system: UnitSystem,
) -> WeatherSignal {
    let rain_fact = rain_activation_fact(item, channel, now_ms);
    let wind_fact = wind_activation_fact(item, channel, now_ms);
    weather_signal_from_facts(rain_fact, wind_fact, unit_system)
}

fn weather_channel_signal(
    rain_fact: Option<RainActivationFact>,
    wind_fact: Option<WindActivationFact>,
    unit_system: UnitSystem,
) -> Option<ChannelSignalDto> {
    if rain_fact.is_none() && wind_fact.is_none() {
        return None;
    }
    let weather = weather_signal_from_facts(rain_fact, wind_fact, unit_system);
    Some(ChannelSignalDto {
        headline: weather.headline,
        detail: weather.detail,
        action: bounded_signal_text(&weather.action, 240),
        severity: Some(weather.severity),
        expires_at: weather.expires_at,
        band: weather.band,
        imminence_minutes: weather.imminence_minutes,
        series: Vec::new(),
        previous_close: None,
    })
}

fn weather_signal_from_facts(
    rain_fact: Option<RainActivationFact>,
    wind_fact: Option<WindActivationFact>,
    unit_system: UnitSystem,
) -> WeatherSignal {
    let rain = rain_fact.map(RainActivationFact::detail);
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
        (None, None) => "Current weather conditions need attention.".into(),
    };

    let headline = match (rain_fact, wind_fact) {
        (Some(rain), Some(wind))
            if rain.evidence == RainEvidence::CurrentObservation && wind.lead_minutes == 0 =>
        {
            "Rain and strong gusts now"
        }
        (Some(rain), Some(_)) if rain.evidence == RainEvidence::CurrentObservation => {
            "Rain now; strong gusts expected"
        }
        (Some(_), Some(wind)) if wind.lead_minutes == 0 => {
            "Strong gusts now; rain expected"
        }
        (Some(_), Some(_)) => "Rain and strong gusts expected",
        (Some(rain), None) => rain.headline(),
        (None, Some(wind)) if wind.lead_minutes == 0 => "Strong gusts now",
        (None, Some(_)) => "Strong gusts expected",
        (None, None) => "Weather update",
    };
    let severity = match (rain_fact, wind_fact) {
        (Some(fact), _) if fact.evidence == RainEvidence::CurrentObservation => "Falling now",
        (Some(fact), _) if fact.priority_imminence_minutes().is_some() => "Imminent",
        (_, Some(fact)) if fact.lead_minutes <= IMMINENT_WEATHER_WINDOW_MINUTES => "Imminent",
        _ => "Heads-up",
    };
    let action = match (rain_fact.is_some(), wind_fact.is_some()) {
        (true, true) => "Plan for wet roads and secure loose outdoor items.",
        (true, false) => "Plan for wet roads and slower traffic.",
        (false, true) => "Use caution around exposed roads and walkways.",
        (false, false) => "Review current conditions before heading out.",
    };

    // A combined card is valid only while every fact printed on it is valid.
    // A later refresh may replace it with the surviving rain-only or wind-only
    // card, but a consumer holding this snapshot must never outlive its copy.
    let valid_until_ms = [
        rain_fact.map(|fact| fact.valid_until_ms),
        wind_fact.map(|fact| fact.valid_until_ms),
    ]
    .into_iter()
    .flatten()
    .min();

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
        headline: headline.into(),
        detail: bounded_signal_text(&detail, 360),
        action: action.into(),
        severity: severity.into(),
        expires_at: valid_until_ms.and_then(|value| iso_timestamp(value).ok()),
        band: (!parts.is_empty()).then(|| parts.join("+")),
        imminence_minutes: [
            rain_fact.and_then(RainActivationFact::priority_imminence_minutes),
            wind_fact.map(WindActivationFact::priority_imminence_minutes),
        ]
        .into_iter()
        .flatten()
        .min(),
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

/// Whether this source has told us no fresher answer can exist yet.
///
/// The argument above — that a budget cannot double as a staleness test for a
/// source deliberately collected less often than the budget — has a second case
/// the cadence term does not cover. A market that has closed is not a stale
/// source. Friday's closing price *is* the current price of AMD all weekend;
/// polling harder cannot produce a newer one, and there is no fault to report.
///
/// Left as it was, the markets channel called itself degraded from Friday's
/// close to Monday's open, about sixty-five hours, every single week — better
/// than a third of the time, on a channel that was working perfectly. The cost
/// is not the false label. It is that a reader could no longer tell a closed
/// market from a broken feed, which is the one thing the availability line is
/// there to say.
fn source_is_quiescent(source: &SourceState) -> bool {
    source.items.iter().any(|item| {
        item.attributes
            .get("session_label")
            .and_then(Value::as_str)
            .is_some_and(|session| session.eq_ignore_ascii_case("CLOSED"))
    })
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
    if age_seconds > stale_after && !source_is_quiescent(source) {
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
    let mut first_attempt_ms: Option<i64> = None;

    for (source_id, channel_id) in &registered {
        let Some(source) = state.sources.get(*source_id) else {
            availability_values.push(AvailabilityDto::Offline);
            continue;
        };
        attempted |= source.last_attempt_ms.is_some();
        if let Some(first) = source.first_attempt_ms {
            first_attempt_ms = Some(first_attempt_ms.map_or(first, |current: i64| current.min(first)));
        }
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
    // A websocket that opened a minute ago and has not been told about a vessel
    // yet is not broken; the river is often simply empty. Reporting a fault for
    // that trains the reader to ignore the one indicator that matters, so the
    // source is given time to settle before anything it says counts as a
    // failure. A rejected key is exempt: that is an answer, not a silence.
    const SETTLING_MS: i64 = 3 * 60 * 1_000;
    // Only before the source has ever delivered. Once it has worked, losing it
    // is a real fault and must fail closed immediately — the grace is for not
    // having started yet, not for having stopped.
    let settling = last_success_ms.is_none()
        && first_attempt_ms.is_some_and(|first| now_ms.saturating_sub(first) < SETTLING_MS);

    let connection_state = if !preferences.ais.api_key_configured {
        AisConnectionStateDto::NeedsKey
    } else if rejected {
        AisConnectionStateDto::Rejected
    } else if source_registered && (!attempted || (starting && last_success_ms.is_none())) {
        AisConnectionStateDto::Armed
    } else if availability == AvailabilityDto::Fresh && fresh_vessel_count > 0 {
        AisConnectionStateDto::Live
    } else if availability == AvailabilityDto::Fresh || (source_registered && settling) {
        AisConnectionStateDto::Armed
    } else {
        AisConnectionStateDto::Disconnected
    };
    let detail = match connection_state {
        AisConnectionStateDto::Disabled => "The vessel source is off.".into(),
        AisConnectionStateDto::NeedsKey => "Add a key to start the vessel source.".into(),
        AisConnectionStateDto::Armed if settling && last_success_ms.is_none() => {
            "Connecting.".into()
        }
        AisConnectionStateDto::Armed => "Connected. No vessel in range right now.".into(),
        AisConnectionStateDto::Live => format!(
            "Connected. {fresh_vessel_count} vessel{} in range.",
            if fresh_vessel_count == 1 { "" } else { "s" }
        ),
        AisConnectionStateDto::Rejected => "The key was refused. Replace it.".into(),
        AisConnectionStateDto::Disconnected => "Not connected.".into(),
    };

    Ok(AisStreamStatusDto {
        // A key is the whole decision. Storing one turns the source on and
        // clearing it turns the source off, so there is nothing else to ask.
        enabled: preferences.ais.api_key_configured,
        provider: preferences.ais.provider,
        api_key_configured: preferences.ais.api_key_configured,
        source_registered,
        connection_state,
        availability,

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
    // Live consumes the same bounded snapshot as Map. Keep every credible
    // Brickell passage ahead of unrelated port traffic before applying that
    // display bound, then preserve recency within each group.
    tracks.sort_by(|(left_ms, left), (right_ms, right)| {
        right
            .route_intersects
            .cmp(&left.route_intersects)
            .then_with(|| right_ms.cmp(left_ms))
    });
    tracks.truncate(MAX_MAP_VESSEL_TRACKS);
    let schedule = BrickellSchedule::new().ok();
    tracks
        .into_iter()
        .map(|(observed_ms, mut track)| {
            // The collector knows where a hull is; only the ledger knows
            // whether that hull has ever needed the span raised. Joining here
            // keeps the learned history on one side of the boundary and the
            // live positions on the other.
            track.opening_propensity = state.ais_propensities.get(&track.mmsi).copied();
            track.schedule_exempt = schedule_exempt(track.vessel_class.as_deref());
            let learned_opener = track
                .opening_propensity
                .is_some_and(|score| score >= KNOWN_OPENER_LIKELY_BPS);
            let known_opener_committed = learned_opener
                && track.movement == VesselMovementDto::Approaching
                && matches!(
                    track.posture.as_deref(),
                    Some("underway" | "waiting" | "holding")
                );
            if known_opener_committed {
                track.route_intersects = true;
                if track.eta_min_minutes.is_none()
                    && let Some(eta) = track
                        .s_meters
                        .map(f64::abs)
                        .and_then(|distance| {
                            known_opener_eta_from_motion(distance, track.speed_knots)
                        })
                {
                    track.eta_min_minutes = Some(eta.earliest);
                    track.eta_max_minutes = Some(eta.latest);
                }
            }
            if let Some(schedule) = schedule.as_ref() {
                annotate_predicted_opening(&mut track, schedule, now_ms);
            }
            let opening_evidence = learned_opener
                || track.vessel_class.as_deref() == Some("sailing");
            let committed_tight_approach = track.movement == VesselMovementDto::Approaching
                && track.route_intersects
                && observed_ms
                    >= now_ms.saturating_sub(
                        i64::try_from(AIS_TRACK_RETENTION_SECONDS)
                            .unwrap_or(i64::MAX)
                            .saturating_mul(1_000),
                    )
                && track
                    .eta_min_minutes
                    .is_some_and(|minutes| minutes <= KNOWN_OPENER_LIKELY_ETA_MINUTES);
            track.likely_to_open_brickell = opening_evidence
                && committed_tight_approach
                && track.predicted_opening_at.is_some();
            track
        })
        .collect()
}

/// Whether 33 CFR 117.261 lets this hull be passed outside the ordinary
/// schedule.
///
/// The regulation names public vessels, tugs with tows, and vessels in
/// distress. A hull broadcasting a bare tug type is working traffic on this
/// river rather than a pleasure craft, and is treated as exempt; that is an
/// inference from what the Miami River carries, not a quotation of the rule,
/// and it is deliberately narrow — cargo and passenger types are not included,
/// because a hull that cannot reach the span does not get an exemption for it.
fn schedule_exempt(vessel_class: Option<&str>) -> bool {
    matches!(vessel_class, Some("tug + tow" | "tug" | "pilot"))
}

/// Writes the opening this vessel could actually be passed through.
///
/// An ETA is when the hull arrives; it is not when it gets through. During the
/// hour/half-hour period an ordinary vessel reaching the span at 18:32 waits
/// for 19:00, and saying otherwise would be the single most misleading number
/// this surface could show. Exempt traffic is passed on arrival.
fn annotate_predicted_opening(
    track: &mut VesselTrackSnapshot,
    schedule: &BrickellSchedule,
    now_ms: i64,
) {
    let Some(eta_minutes) = track.eta_min_minutes else {
        return;
    };
    let arrival_ms = now_ms.saturating_add(i64::from(eta_minutes) * 60_000);
    let arrival = TimestampMillis(arrival_ms);
    let opening = if track.schedule_exempt {
        Some(arrival)
    } else {
        // `None` means the bridge is on signal then, so arrival is the answer.
        schedule
            .ordinary_opening_at_or_after(arrival)
            .ok()
            .flatten()
            .or(Some(arrival))
    };
    let Some(opening) = opening else { return };
    track.waits_for_slot = opening.0 > arrival_ms;
    track.predicted_opening_at = iso_timestamp(opening.0).ok();
}

/// The corridor this app models, always published.
///
/// An earlier version withheld it whenever no AIS source was running, on the
/// theory that drawing water implies the water is watched. That was wrong in
/// practice: with the AIS channel switched off the live surface rendered
/// nothing at all, which reads as a broken page rather than as a disabled
/// source. The geometry is a fixed description of the river the app reasons
/// about, so it is always sent, and `ais_live` carries the thing the earlier
/// guard was actually trying to say.
fn river_corridor(state: &PersistedRuntimeState) -> RiverCorridorDto {
    let ais_live = state
        .active_sources
        .keys()
        .any(|source_id| source_id.starts_with("aisstream."));
    RiverCorridorDto {
        bridge_latitude: BRIDGE_LATITUDE,
        bridge_longitude: BRIDGE_LONGITUDE,
        ais_live,
        branches: corridor_geometry()
            .into_iter()
            .map(|branch| RiverCorridorBranchDto {
                id: branch.id.into(),
                label: branch.label.into(),
                corridor_offset_meters: branch.corridor_offset_meters,
                centerline: branch
                    .centerline
                    .iter()
                    .map(|(latitude, longitude)| [*latitude, *longitude])
                    .collect(),
                stations: branch
                    .stations
                    .iter()
                    .map(|station| RiverStationDto {
                        label: station.label.into(),
                        kind: station.kind.as_str().into(),
                        bridge_key: station.bridge_key.map(Into::into),
                        latitude: station.latitude,
                        longitude: station.longitude,
                        s_meters: project(station.latitude, station.longitude).s_meters,
                    })
                    .collect(),
            })
            .collect(),
    }
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
                ("No alerts in force".into(), false)
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
                    "No active Atlantic cyclone".into(),
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
                ("Nothing new".into(), false)
            } else if count == 1 {
                ("1 current headline".into(), true)
            } else {
                (format!("{count} current headlines"), true)
            }
        }
        ChannelKindDto::Sports => {
            let matching = items
                .iter()
                .filter(|item| news_item_matches_scope(item, channel, now_ms))
                .collect::<Vec<_>>();
            let moves = matching
                .iter()
                .filter(|item| sports_item_is_transaction(item))
                .count();
            match (matching.len(), moves) {
                (0, _) => ("Nothing new".into(), false),
                (1, 0) => ("1 current sports item".into(), true),
                (total, 0) => (format!("{total} current sports items"), true),
                (1, 1) => ("1 current sports item · 1 roster move".into(), true),
                (total, moves) => (
                    format!(
                        "{total} current sports items · {moves} roster {}",
                        if moves == 1 { "move" } else { "moves" }
                    ),
                    true,
                ),
            }
        }
        ChannelKindDto::Earthquake => {
            let count = items
                .iter()
                .filter(|item| earthquake_matches_scope(item, channel, now_ms))
                .count()
                .min(crate::preferences::AUTOMATIC_ITEM_LIMIT);
            if count == 0 {
                ("No current earthquakes".into(), false)
            } else if count == 1 {
                ("1 current earthquake".into(), true)
            } else {
                (format!("{count} current earthquakes"), true)
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
    /// The level `change_percent` is measured against, so a surface plotting the
    /// series can draw the same reference the number quotes.
    previous_close: Option<f64>,
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
    let quote_count = 3;
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
        previous_close: item
            .attributes
            .get("previous_close")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0),
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

fn rain_status(fact: RainActivationFact) -> String {
    fact.detail().trim_end_matches('.').to_owned()
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
        .max_by(|(left, _), (right, _)| {
            rain_activation_score(*left).total_cmp(&rain_activation_score(*right))
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
        .max_by(|(left, _), (right, _)| {
            wind_activation_score(*left).total_cmp(&wind_activation_score(*right))
        });

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
                    "Personal weather heads-up · {rain_area}: {} · {wind_area}: gusts {gust:.0} {label} in {} min (≥{threshold:.0} {label}){delayed_suffix}",
                    rain_status(rain),
                    wind.lead_minutes,
                ),
            }
        }
        (Some((rain, area)), None) => WeatherActivation {
            state: PersonalWeatherState::RainHeadsUp,
            summary: format!(
                "Personal rain heads-up · {area}: {}{delayed_suffix}",
                rain_status(rain),
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

#[cfg(test)]
mod availability_tests {
    use super::*;
    use serde_json::json;

    fn market_source(session: &str, last_success_ms: i64) -> SourceState {
        serde_json::from_value(json!({
            "channel_id": "markets.watchlist",
            "items": [{
                "id": "yahoo-chart:AMD",
                "kind": "market_quote",
                "title": "AMD",
                "source": { "name": "Yahoo Finance", "url": null },
                "attributes": { "session_label": session },
            }],
            "reported_health": "healthy",
            "last_success_ms": last_success_ms,
            "failure_count": 0,
            "poll_interval_ms": 300_000,
        }))
        .expect("source state fixture")
    }

    fn markets_channel() -> ChannelPreference {
        let mut channel = AppPreferences::default().profile.channels[6].clone();
        channel.max_age_minutes = 20;
        channel
    }

    /// Friday's close is the price of AMD all weekend. Judging it against a
    /// twenty-minute budget marked a working channel degraded from Friday
    /// afternoon to Monday morning -- about sixty-five hours, every week.
    #[test]
    fn a_closed_market_is_current_rather_than_stale() {
        let now_ms = 1_786_900_000_000;
        let two_days = 48 * 60 * 60 * 1_000;
        let (availability, age) =
            source_availability(&market_source("CLOSED", now_ms - two_days), &markets_channel(), now_ms);
        assert_eq!(
            availability,
            AvailabilityDto::Fresh,
            "a closed market has no fresher answer to offer"
        );
        assert!(
            age > 100_000,
            "the age is still reported honestly, it just does not mean stale"
        );
    }

    /// ...and the moment it reopens, the ordinary budget applies again, so a
    /// feed that dies during trading hours is still caught.
    #[test]
    fn an_open_market_that_stops_updating_is_still_stale() {
        let now_ms = 1_786_900_000_000;
        let an_hour = 60 * 60 * 1_000;
        let (availability, _) =
            source_availability(&market_source("OPEN", now_ms - an_hour), &markets_channel(), now_ms);
        assert_eq!(availability, AvailabilityDto::Stale);
    }

    /// Quiescence excuses age, never a fault. A closed market whose collector
    /// is erroring is still broken.
    #[test]
    fn a_closed_market_with_a_failing_collector_is_still_reported() {
        let now_ms = 1_786_900_000_000;
        let mut source = market_source("CLOSED", now_ms - 48 * 60 * 60 * 1_000);
        source.last_error = Some("connection refused".into());
        let (availability, _) = source_availability(&source, &markets_channel(), now_ms);
        assert_eq!(
            availability,
            AvailabilityDto::Delayed,
            "an error still surfaces even while the market is shut"
        );
    }
    #[test]
    fn an_earthquake_reads_its_time_on_the_local_clock() {
        // jiff defers formatting, so an unsupported specifier only fails when
        // the value is rendered. Render it.
        let detail = local_clock_label(1_756_051_620_000);
        assert!(detail.contains(':'), "expected a clock time, got {detail:?}");
        assert!(
            !detail.contains('\u{b7}'),
            "the time line must not restate the title, got {detail:?}"
        );

        // An item carrying no time at all still has to say something honest
        // rather than render the epoch or an empty second line.
        assert_eq!(local_clock_label(i64::MIN), "Time not reported");
    }
}
