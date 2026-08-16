//! Turning a runtime snapshot into something the panel can draw.
//!
//! This is the step between "what is true" and "what appears on a 250x122
//! screen": which words, which urgency, which two upstream spans, and the
//! local clock time beside them.
//!
//! It lives in its own crate because two deployments need it and they must not
//! disagree. The desktop app renders locally; the hosted service renders in a
//! Cloudflare Worker. A second implementation of this projection would drift
//! from the first the moment either changed, and the drift would be invisible
//! -- both would produce a plausible frame, just not the same one.
//!
//! Nothing here reaches a network, a disk, or a clock it was not handed.

use bridgestatus_contract::{
    AppPreferences, AppSnapshot, AvailabilityDto, BridgeStateDto, ChannelKindDto, ChannelSnapshot,
    InterruptPreset, UrgencyDto,
};
use bridgestatus_eink::{
    ChannelAvailability, ChannelCard, ChannelKind, ChannelSource, ChannelUrgency, EtaRange,
    Evidence, Freshness, LiveSnapshot, MonoFrame, RenderConfig, SnapshotState, render_snapshot,
};
use jiff::{Timestamp, tz::TimeZone};
use std::collections::BTreeMap;

pub fn channel_card(
    channel: &ChannelSnapshot,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
) -> ChannelCard {
    let kind = match channel.kind {
        ChannelKindDto::Weather => ChannelKind::Weather,
        ChannelKindDto::Official => ChannelKind::OfficialAlert,
        ChannelKindDto::Hurricane => ChannelKind::Tropical,
        ChannelKindDto::News => ChannelKind::News,
        ChannelKindDto::Earthquake => ChannelKind::Earthquake,
        ChannelKindDto::Markets => ChannelKind::Markets,
        ChannelKindDto::System => ChannelKind::Custom {
            label: "SYSTEM".into(),
            code: "SY".into(),
        },
        ChannelKindDto::Bridge => ChannelKind::Custom {
            label: "BRIDGE".into(),
            code: "BR".into(),
        },
    };
    let urgency = if interrupt_allows(channel, preferences, snapshot) {
        match channel.priority.urgency {
            UrgencyDto::Routine => ChannelUrgency::Routine,
            UrgencyDto::HeadsUp => ChannelUrgency::Advisory,
            UrgencyDto::Action => ChannelUrgency::Urgent,
            UrgencyDto::Emergency => ChannelUrgency::Critical,
        }
    } else {
        ChannelUrgency::Routine
    };
    let availability = match channel.availability {
        AvailabilityDto::Fresh | AvailabilityDto::Delayed => ChannelAvailability::Current,
        AvailabilityDto::Stale => ChannelAvailability::Stale,
        AvailabilityDto::Offline => ChannelAvailability::Offline,
    };
    let source = if matches!(
        availability,
        ChannelAvailability::Current | ChannelAvailability::Stale
    ) {
        ChannelSource::aged(bounded_text(&channel.source_label, 96), channel.age_seconds)
    } else {
        ChannelSource::unavailable(bounded_text(&channel.source_label, 96))
    };
    let signal = channel.signal.as_ref();
    let headline = if channel.active {
        signal.map_or(channel.summary.as_str(), |signal| signal.headline.as_str())
    } else {
        channel.summary.as_str()
    };
    let detail = if channel.active {
        signal.map_or("ACTIVE SIGNAL", |signal| signal.detail.as_str())
    } else {
        "MONITORING · NO MATERIAL ALERT"
    };
    let action = if channel.active {
        signal.map_or("ACTIVE SIGNAL", |signal| signal.action.as_str())
    } else {
        "NO MATERIAL CHANGE"
    };
    ChannelCard::new(
        kind,
        urgency,
        availability,
        bounded_text(&channel.title, 96),
        bounded_text(headline, 160),
        bounded_text(detail, 240),
        bounded_text(action, 160),
        source,
    )
}

pub fn bounded_text(value: &str, maximum: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let value = if normalized.is_empty() {
        "NO CURRENT DETAIL".to_owned()
    } else {
        normalized
    };
    value.chars().take(maximum).collect()
}

pub fn display_snapshot(snapshot: &AppSnapshot) -> LiveSnapshot {
    let state = match snapshot.decision.state {
        BridgeStateDto::Clear => SnapshotState::Clear,
        BridgeStateDto::Possible => SnapshotState::Watch,
        BridgeStateDto::Likely => SnapshotState::Likely,
        BridgeStateDto::Open => SnapshotState::Open,
    };
    let source = snapshot
        .evidence
        .iter()
        .find(|item| item.state == bridgestatus_contract::EvidenceStateDto::Live)
        .map(|item| item.source_label.clone())
        .unwrap_or_else(|| "Tender's Log".into());
    let mut output = LiveSnapshot::brickell(
        state,
        Freshness::new(source, snapshot.decision.source_age_seconds, 300),
    );
    output.channel = snapshot.decision.subject.to_ascii_uppercase();
    output.road_meaning = snapshot.decision.meaning.to_ascii_uppercase();
    output.eta = snapshot
        .decision
        .eta_min
        .map(|minimum| EtaRange::new(minimum, snapshot.decision.eta_max.unwrap_or(minimum)));
    if state.is_predictive() {
        output.confidence_percent = snapshot.decision.confidence_bps.map(bps_to_percent);
    }
    output.evidence = snapshot
        .evidence
        .iter()
        .filter(|item| item.state == bridgestatus_contract::EvidenceStateDto::Live)
        .take(3)
        .map(|item| {
            Evidence::new(
                item.title.to_ascii_uppercase(),
                item.source_label.to_ascii_uppercase(),
            )
        })
        .collect();
    output.spans = upstream_spans(
        &snapshot.bridge_intervals,
        TimeZone::get(&snapshot.local_time_zone).ok().as_ref(),
    );
    output
}

/// Condenses a bridge name into the two or three characters the E213 has room
/// for beside a clock time. Falls back to the leading alphanumerics of the key
/// so an unrecognized upstream span still appears rather than vanishing.
pub fn span_code(bridge_key: &str, bridge_name: &str) -> String {
    match bridge_key {
        "sw_2_ave" => "2AV".into(),
        "sw_1_st" => "1ST".into(),
        "w_flagler" => "FLG".into(),
        "nw_5_st" => "5ST".into(),
        "nw_12_ave" => "12A".into(),
        "nw_17_ave" => "17A".into(),
        "nw_22_ave" => "22A".into(),
        "nw_27_ave" => "27A".into(),
        _ => {
            let source = if bridge_key.is_empty() {
                bridge_name
            } else {
                bridge_key
            };
            let code: String = source
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .take(3)
                .collect();
            code.to_ascii_uppercase()
        }
    }
}

/// Current state of each upstream span, newest observation per bridge.
///
/// Only an interval that has not ended describes the present. A completed one
/// is history, and reporting it as still up would tell a driver the river is
/// blocked when it is not.
pub fn upstream_spans(
    intervals: &[bridgestatus_contract::BridgeStateIntervalDto],
    zone: Option<&TimeZone>,
) -> Vec<bridgestatus_eink::SpanStatus> {
    let mut latest: BTreeMap<&str, &bridgestatus_contract::BridgeStateIntervalDto> =
        BTreeMap::new();
    for interval in intervals {
        if interval.relation != bridgestatus_contract::BridgeRelationDto::Upstream {
            continue;
        }
        let winner = match latest.get(interval.bridge_key.as_str()) {
            None => true,
            // An in-progress interval always beats a completed one, however
            // recent the completed one is; otherwise the later start wins.
            Some(held) => match (held.ended_at.is_none(), interval.ended_at.is_none()) {
                (false, true) => true,
                (true, false) => false,
                _ => interval.started_at > held.started_at,
            },
        };
        if winner {
            latest.insert(interval.bridge_key.as_str(), interval);
        }
    }

    let mut ordered = latest.values().copied().collect::<Vec<_>>();
    ordered.sort_by_key(|interval| interval.river_order);
    ordered
        .into_iter()
        .map(|interval| {
            let open = interval.ended_at.is_none()
                && interval.state == bridgestatus_contract::ObservedBridgeStateDto::Up;
            let mut span = bridgestatus_eink::SpanStatus::new(
                span_code(&interval.bridge_key, &interval.bridge_name),
                open,
            );
            if open {
                span.opened_at = zone.and_then(|zone| local_clock(&interval.started_at, zone));
            }
            span
        })
        .collect()
}

/// Formats an RFC 3339 instant as a bare local `HH:MM`, which is all the E213
/// has room for and all a reader needs to line two openings up.
pub fn local_clock(instant: &str, zone: &TimeZone) -> Option<String> {
    let timestamp: Timestamp = instant.parse().ok()?;
    let zoned = timestamp.to_zoned(zone.clone());
    Some(format!("{:02}:{:02}", zoned.hour(), zoned.minute()))
}

/// Renders the current runtime snapshot through the same E213 path used by the
/// desktop application.
pub fn render_live_bridge_frame(
    snapshot: &AppSnapshot,
) -> Result<MonoFrame, bridgestatus_eink::RenderError> {
    render_snapshot(&display_snapshot(snapshot), &RenderConfig::default())
}

pub fn bps_to_percent(bps: u16) -> u8 {
    ((bps.saturating_add(50) / 100).min(100)) as u8
}

/// Whether this channel is currently allowed to interrupt.
///
/// Lives beside the projection because it decides the urgency a card is drawn
/// at, and a card drawn at the wrong urgency is a design decision made by
/// accident.
pub fn interrupt_allows(
    channel: &ChannelSnapshot,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
) -> bool {
    if !channel.enabled || !channel.active {
        return false;
    }
    match channel.interrupt_preset {
        InterruptPreset::Off | InterruptPreset::Custom => false,
        InterruptPreset::Meaningful => channel.kind != ChannelKindDto::System,
        InterruptPreset::Recommended => match channel.kind {
            ChannelKindDto::Bridge => matches!(
                snapshot.decision.state,
                BridgeStateDto::Likely | BridgeStateDto::Open
            ),
            ChannelKindDto::News => preferences
                .profile
                .channels
                .iter()
                .find(|preference| preference.id == channel.id)
                .and_then(|preference| preference.scope.get("breakingOnly"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            ChannelKindDto::System => false,
            ChannelKindDto::Weather
            | ChannelKindDto::Official
            | ChannelKindDto::Hurricane
            | ChannelKindDto::Earthquake
            | ChannelKindDto::Markets => true,
        },
        InterruptPreset::ConfirmedOnly => match channel.kind {
            ChannelKindDto::Bridge => snapshot.decision.state == BridgeStateDto::Open,
            ChannelKindDto::Official | ChannelKindDto::Hurricane | ChannelKindDto::Earthquake => {
                true
            }
            ChannelKindDto::Weather
            | ChannelKindDto::News
            | ChannelKindDto::Markets
            | ChannelKindDto::System => false,
        },
    }
}
