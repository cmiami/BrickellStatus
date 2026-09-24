//! Shadow opening model: scored every forecast minute, never drives an alert.
//!
//! A logistic model over rows the app already stores, as they existed at the
//! forecast minute: recent AIS fixes, crossing outcomes resolved at least 20
//! minutes earlier, bridge lifts, pilots'-board revisions still on the board,
//! and the legal schedule. It predicts a Brickell lift within 30 minutes.
//!
//! This mirrors `scripts/opening_shadow.py` line for line; the training script
//! `scripts/fit_opening_shadow.py` uses that module, and
//! `fixtures/opening_shadow_parity.json` pins both to identical answers. On
//! Sep 13 to 23 2026, unseen by fitting, it raised 134 false alerts where the
//! live model raised 281, and warned for 114 of 196 openings where the live
//! model warned for 137. That is a quieter model that misses more, not a
//! strict improvement, so it runs in shadow until its own record decides.

use std::collections::{BTreeMap, BTreeSet};

use brickellstatus_model::{BridgeOperatingMode, TimestampMillis};
use brickellstatus_policy::BrickellSchedule;
use serde::Deserialize;

pub(crate) const MINUTE_MS: i64 = 60_000;
const KNOT_METERS_PER_SECOND: f64 = 0.5144;
const FIX_WINDOW_MS: i64 = 7 * MINUTE_MS;
const PREVIOUS_FIX_MIN_GAP_MS: i64 = 45_000;
const PREVIOUS_FIX_LOOKBACK_MS: i64 = 10 * MINUTE_MS;
/// Oldest fix the scorer reads: a latest fix plus its motion baseline.
pub(crate) const FIX_LOOKBACK_MS: i64 = FIX_WINDOW_MS + PREVIOUS_FIX_LOOKBACK_MS;
pub(crate) const HISTORY_BUFFER_MS: i64 = 20 * MINUTE_MS;
pub(crate) const UPSTREAM_LOOKBACK_MS: i64 = 30 * MINUTE_MS;
pub(crate) const BOARD_GRACE_MS: i64 = 10 * MINUTE_MS;
const CLOSING_METERS: f64 = 20.0;
const MIN_UNDERWAY_KNOTS: f64 = 0.8;
const COMMITTED_APPROACH_METERS: f64 = 1_600.0;
const FAR_APPROACH_MINUTES: f64 = 40.0;
const WAITING_METERS: f64 = 500.0;
const KNOWN_OPENER_PROPENSITY: f64 = 0.6;

pub(crate) const FEATURES: [&str; 21] = [
    "ais_closing",
    "ais_known",
    "ais_sail",
    "ais_eta30",
    "ais_out",
    "ais_in",
    "ais_wait",
    "ais_far_known",
    "ais_far_unknown",
    "up_any15",
    "up_two20",
    "up_sw2ave12",
    "up_sw1st12",
    "up_ordered20",
    "bbp_down",
    "bbp_up",
    "sched_weekday",
    "blackout",
    "slot_soon",
    "daytime",
    "recent_open30",
];

fn upstream_rank(key: &str) -> Option<u8> {
    match key {
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

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ShadowFix {
    pub mmsi: String,
    pub at: i64,
    pub sog: Option<f64>,
    pub branch: Option<String>,
    pub s: Option<f64>,
    pub posture: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ShadowLift {
    pub key: String,
    pub relation: String,
    pub at: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ShadowBooking {
    pub direction: String,
    pub scheduled_at: i64,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct ShadowSchedule {
    pub mode: String,
    pub local_hour: u8,
    pub seconds_to_slot: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ShadowInputs {
    pub now_ms: i64,
    pub fixes: Vec<ShadowFix>,
    /// Opened and fits-under crossings resolved before the history buffer.
    pub history: BTreeMap<String, (i64, i64)>,
    pub classes: BTreeMap<String, String>,
    pub up_starts: Vec<ShadowLift>,
    pub board: Vec<ShadowBooking>,
    pub schedule: ShadowSchedule,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ShadowWeights {
    pub features: Vec<String>,
    pub coefficients: Vec<f64>,
    pub intercept: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ShadowModel {
    pub version: String,
    #[serde(flatten)]
    pub weights: ShadowWeights,
    pub enter: f64,
    pub exit: f64,
    pub board_windows: BTreeMap<String, (i64, i64)>,
}

impl ShadowModel {
    /// The shipped model, checked once: every named feature must exist here.
    pub(crate) fn shipped() -> Option<&'static ShadowModel> {
        static MODEL: std::sync::OnceLock<Option<ShadowModel>> = std::sync::OnceLock::new();
        MODEL
            .get_or_init(|| {
                let model: ShadowModel =
                    serde_json::from_str(include_str!("../models/opening_shadow_v1.json")).ok()?;
                let valid = model.weights.features.len() == model.weights.coefficients.len()
                    && model
                        .weights
                        .features
                        .iter()
                        .all(|name| FEATURES.contains(&name.as_str()));
                valid.then_some(model)
            })
            .as_ref()
    }

    pub(crate) fn probability(&self, inputs: &ShadowInputs) -> f64 {
        probability(&features(inputs, &self.board_windows), &self.weights)
    }
}

/// Legal mode, local hour, and seconds to the next `:00` or `:30`.
pub(crate) fn schedule_context(schedule: &BrickellSchedule, now_ms: i64) -> Option<ShadowSchedule> {
    let status = schedule.evaluate(TimestampMillis(now_ms)).ok()?;
    let into_half_hour =
        (now_ms.div_euclid(1_000) + i64::from(status.local_time.offset_seconds)).rem_euclid(1_800);
    Some(ShadowSchedule {
        mode: match status.mode {
            BridgeOperatingMode::OnSignal => "on_signal",
            BridgeOperatingMode::Scheduled => "scheduled",
            BridgeOperatingMode::Blackout => "blackout",
        }
        .into(),
        local_hour: status.local_time.hour,
        seconds_to_slot: if into_half_hour == 0 {
            0
        } else {
            1_800 - into_half_hour
        },
    })
}

/// The model's inputs at `inputs.now_ms`, keyed by [`FEATURES`].
pub(crate) fn features(
    inputs: &ShadowInputs,
    board_windows: &BTreeMap<String, (i64, i64)>,
) -> BTreeMap<&'static str, f64> {
    let now = inputs.now_ms;
    let mut out = FEATURES
        .iter()
        .map(|name| (*name, 0.0))
        .collect::<BTreeMap<_, _>>();
    let mut set = |name: &'static str| {
        out.insert(name, 1.0);
    };

    let mut by_hull = BTreeMap::<&str, Vec<&ShadowFix>>::new();
    for fix in inputs.fixes.iter().filter(|fix| fix.at < now) {
        by_hull.entry(fix.mmsi.as_str()).or_default().push(fix);
    }
    let mut best_eta: Option<f64> = None;
    for (mmsi, fixes) in &mut by_hull {
        fixes.sort_by_key(|fix| fix.at);
        let Some(latest) = fixes.last().copied() else {
            continue;
        };
        if latest.at < now - FIX_WINDOW_MS {
            continue;
        }
        let posture = latest.posture.as_deref();
        if posture == Some("waiting") && latest.s.is_some_and(|s| s.abs() <= WAITING_METERS) {
            set("ais_wait");
        }
        let (Some(sog), Some(s)) = (latest.sog, latest.s) else {
            continue;
        };
        if posture != Some("underway") || sog <= MIN_UNDERWAY_KNOTS {
            continue;
        }
        let previous = fixes
            .iter()
            .rev()
            .find(|fix| {
                fix.at >= latest.at - PREVIOUS_FIX_LOOKBACK_MS
                    && fix.at <= latest.at - PREVIOUS_FIX_MIN_GAP_MS
                    && fix.s.is_some()
            })
            .and_then(|fix| fix.s);
        let Some(previous) = previous else {
            continue;
        };
        if s.abs() >= previous.abs() - CLOSING_METERS {
            continue;
        }
        let eta = s.abs() / (sog * KNOT_METERS_PER_SECOND) / 60.0;
        let (opened, fits_under) = inputs.history.get(*mmsi).copied().unwrap_or((0, 0));
        let known = opened + fits_under > 0
            && (opened as f64 + 1.0) / ((opened + fits_under) as f64 + 2.0)
                >= KNOWN_OPENER_PROPENSITY;
        if !(latest.branch.as_deref() == Some("river") || s.abs() <= COMMITTED_APPROACH_METERS) {
            if eta <= FAR_APPROACH_MINUTES {
                set(if known {
                    "ais_far_known"
                } else {
                    "ais_far_unknown"
                });
            }
            continue;
        }
        set("ais_closing");
        if s > 0.0 {
            set("ais_out");
        } else if s < 0.0 {
            set("ais_in");
        }
        if known {
            set("ais_known");
        }
        if inputs.classes.get(*mmsi).map(String::as_str) == Some("sailing") {
            set("ais_sail");
        }
        best_eta = Some(best_eta.map_or(eta, |best| best.min(eta)));
    }
    if best_eta.is_some_and(|eta| eta <= 30.0) {
        set("ais_eta30");
    }

    let upstream = inputs
        .up_starts
        .iter()
        .filter(|lift| {
            lift.relation == "upstream"
                && upstream_rank(&lift.key).is_some()
                && lift.at >= now - UPSTREAM_LOOKBACK_MS
                && lift.at <= now
        })
        .collect::<Vec<_>>();
    let lifted = |minutes: i64| {
        upstream
            .iter()
            .filter(|lift| lift.at >= now - minutes * MINUTE_MS)
            .map(|lift| lift.key.as_str())
            .collect::<BTreeSet<_>>()
    };
    if !lifted(15).is_empty() {
        set("up_any15");
    }
    if lifted(20).len() >= 2 {
        set("up_two20");
    }
    if lifted(12).contains("sw_2_ave") {
        set("up_sw2ave12");
    }
    if lifted(12).contains("sw_1_st") {
        set("up_sw1st12");
    }
    let mut run = upstream
        .iter()
        .filter(|lift| lift.at >= now - 20 * MINUTE_MS)
        .filter_map(|lift| upstream_rank(&lift.key).map(|rank| (lift.at, rank)))
        .collect::<Vec<_>>();
    run.sort_unstable();
    if run.len() >= 2 && run.windows(2).all(|pair| pair[1].1 < pair[0].1) {
        set("up_ordered20");
    }
    if inputs
        .up_starts
        .iter()
        .any(|lift| lift.relation == "target" && lift.at >= now - 30 * MINUTE_MS && lift.at < now)
    {
        set("recent_open30");
    }

    for booking in &inputs.board {
        if !(booking.first_seen <= now && now <= booking.last_seen + BOARD_GRACE_MS) {
            continue;
        }
        let Some(&(low, high)) = board_windows.get(&booking.direction) else {
            continue;
        };
        if booking.scheduled_at + low * MINUTE_MS <= now
            && now <= booking.scheduled_at + high * MINUTE_MS
        {
            set(if booking.direction == "downriver" {
                "bbp_down"
            } else {
                "bbp_up"
            });
        }
    }

    let schedule = &inputs.schedule;
    if schedule.mode == "scheduled" {
        set("sched_weekday");
    }
    if schedule.mode == "blackout" {
        set("blackout");
    }
    let daytime = (7..22).contains(&schedule.local_hour);
    if daytime {
        set("daytime");
        if schedule.seconds_to_slot <= 12 * 60 {
            set("slot_soon");
        }
    }
    out
}

pub(crate) fn probability(values: &BTreeMap<&'static str, f64>, weights: &ShadowWeights) -> f64 {
    let z = weights.intercept
        + weights
            .features
            .iter()
            .zip(&weights.coefficients)
            .map(|(name, coefficient)| {
                coefficient * values.get(name.as_str()).copied().unwrap_or(0.0)
            })
            .sum::<f64>();
    1.0 / (1.0 + (-z.clamp(-30.0, 30.0)).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Case {
        inputs: ShadowInputs,
        expected_features: BTreeMap<String, f64>,
        expected_probability: f64,
    }

    #[derive(Deserialize)]
    struct ScheduleCase {
        now_ms: i64,
        mode: String,
        local_hour: u8,
        seconds_to_slot: i64,
    }

    #[derive(Deserialize)]
    struct Fixture {
        board_windows: BTreeMap<String, (i64, i64)>,
        model: ShadowWeights,
        cases: Vec<Case>,
        schedule_cases: Vec<ScheduleCase>,
    }

    fn fixture() -> Fixture {
        serde_json::from_str(include_str!("../fixtures/opening_shadow_parity.json"))
            .expect("parity fixture parses")
    }

    #[test]
    fn features_and_probability_match_the_python_training_definitions() {
        let fixture = fixture();
        for case in &fixture.cases {
            let values = features(&case.inputs, &fixture.board_windows);
            let values = values
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(values, case.expected_features);
            let probability = probability(
                &features(&case.inputs, &fixture.board_windows),
                &fixture.model,
            );
            assert!(
                (probability - case.expected_probability).abs() < 1e-10,
                "{probability} vs {}",
                case.expected_probability
            );
        }
    }

    #[test]
    fn schedule_context_matches_the_python_training_definitions() {
        let schedule = BrickellSchedule::new().unwrap();
        for case in fixture().schedule_cases {
            assert_eq!(
                schedule_context(&schedule, case.now_ms),
                Some(ShadowSchedule {
                    mode: case.mode,
                    local_hour: case.local_hour,
                    seconds_to_slot: case.seconds_to_slot,
                }),
                "at {}",
                case.now_ms
            );
        }
    }

    #[test]
    fn the_shipped_model_loads_and_names_only_known_features() {
        let model = ShadowModel::shipped().expect("shipped shadow model is valid");
        assert_eq!(model.weights.features.len(), FEATURES.len());
        assert!(model.exit < model.enter);
        assert!(model.board_windows.contains_key("downriver"));
    }
}
