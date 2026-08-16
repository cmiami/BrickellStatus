//! One priority score across every channel.
//!
//! The app watches a bridge, rain, official alerts, storms, news and markets.
//! Those compete for one panel and one person's attention, and the question that
//! orders them is not which category they belong to but **how soon they matter**.
//! A bridge predicted forty minutes out is less urgent than rain arriving in
//! five, and no table of per-kind weights expresses that; only a term over time
//! does.
//!
//! So the ordering is a property of the function rather than a hand-tuned list:
//! imminence is the single lever that reorders across kinds, and everything else
//! is a base offset or a tiebreak. That is what keeps the ranking explainable
//! when a new channel is added.
//!
//! This module deliberately takes primitives rather than a snapshot type. The
//! policy crate must not depend on the runtime's DTOs, and keeping the inputs
//! plain makes the whole ordering testable as a table.

use serde::{Deserialize, Serialize};

/// How hard an event argues for interrupting someone.
///
/// Mirrors the runtime's `UrgencyDto`; the runtime converts at its boundary so
/// this crate stays free of DTO types.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    /// Worth showing, never worth interrupting for.
    #[default]
    Routine,
    /// Changes a plan if you happen to look.
    HeadsUp,
    /// Changes a decision you are about to make.
    Action,
    /// Life safety, or the road is blocked right now.
    Emergency,
}

impl Urgency {
    const fn base(self) -> u16 {
        match self {
            Self::Routine => 100,
            Self::HeadsUp => 300,
            Self::Action => 500,
            Self::Emergency => 700,
        }
    }
}

/// Everything the score is computed from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PriorityInput {
    /// How hard this event argues for interrupting.
    pub urgency: Urgency,
    /// Minutes until the event affects the reader. `None` means the timing is
    /// unknown, which scores zero rather than guessing: an event that cannot say
    /// when it matters has not earned a position over one that can.
    pub imminence_minutes: Option<u16>,
    /// Observed rather than predicted. A raised span outranks a forecast one.
    pub confirmed: bool,
    /// Within-kind ordering only, 0..=9. Never large enough to reorder kinds.
    pub severity_rank: u8,
    /// The profile's home channel. Breaks ties toward the surface the app is
    /// built around without letting it outrank a more imminent event.
    pub is_anchor: bool,
}

/// Largest imminence contribution, at zero minutes out.
const IMMINENCE_MAX: u16 = 200;
/// Points shed per minute of delay. At 4, the term reaches zero at 50 minutes,
/// which is roughly the horizon past which nothing is worth interrupting for.
const IMMINENCE_DECAY_PER_MINUTE: u16 = 4;
const CONFIRMED_BONUS: u16 = 100;
const ANCHOR_BONUS: u16 = 5;
/// Ceiling on `severity_rank`, so a within-kind tiebreak can never cross a base
/// band and reorder two different kinds.
const SEVERITY_RANK_MAX: u8 = 9;

/// Ranks one event against every other, highest first.
pub fn priority_score(input: PriorityInput) -> u16 {
    let imminence = input.imminence_minutes.map_or(0, |minutes| {
        IMMINENCE_MAX.saturating_sub(minutes.saturating_mul(IMMINENCE_DECAY_PER_MINUTE))
    });
    input
        .urgency
        .base()
        .saturating_add(imminence)
        .saturating_add(if input.confirmed { CONFIRMED_BONUS } else { 0 })
        .saturating_add(if input.is_anchor { ANCHOR_BONUS } else { 0 })
        .saturating_add(u16::from(input.severity_rank.min(SEVERITY_RANK_MAX)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(urgency: Urgency, minutes: Option<u16>) -> PriorityInput {
        PriorityInput {
            urgency,
            imminence_minutes: minutes,
            ..PriorityInput::default()
        }
    }

    fn bridge_open() -> PriorityInput {
        PriorityInput {
            urgency: Urgency::Emergency,
            imminence_minutes: Some(0),
            confirmed: true,
            severity_rank: 0,
            is_anchor: true,
        }
    }

    fn bridge_likely(minutes: u16) -> PriorityInput {
        PriorityInput {
            urgency: Urgency::HeadsUp,
            imminence_minutes: Some(minutes),
            confirmed: false,
            severity_rank: 0,
            is_anchor: true,
        }
    }

    fn rain_in(minutes: u16) -> PriorityInput {
        PriorityInput {
            urgency: Urgency::HeadsUp,
            imminence_minutes: Some(minutes),
            confirmed: false,
            severity_rank: 2,
            is_anchor: false,
        }
    }

    #[test]
    fn soonest_impact_outranks_a_more_distant_one_of_the_same_kind() {
        assert!(priority_score(rain_in(8)) > priority_score(rain_in(40)));
        assert!(priority_score(bridge_likely(5)) > priority_score(bridge_likely(35)));
    }

    /// The ordering the whole redesign exists to produce.
    #[test]
    fn imminent_rain_outranks_a_distant_bridge_prediction() {
        assert!(
            priority_score(rain_in(8)) > priority_score(bridge_likely(35)),
            "rain in 8 min ({}) must outrank a bridge predicted at T-35 ({})",
            priority_score(rain_in(8)),
            priority_score(bridge_likely(35))
        );
    }

    /// The converse, so "soonest wins" cannot be read as "the bridge never wins".
    #[test]
    fn an_imminent_bridge_prediction_outranks_more_distant_rain() {
        assert!(priority_score(bridge_likely(5)) > priority_score(rain_in(40)));
    }

    #[test]
    fn a_confirmed_open_bridge_outranks_every_prediction() {
        let open = priority_score(bridge_open());
        for minutes in [0, 5, 8, 20, 35] {
            assert!(open > priority_score(rain_in(minutes)));
            assert!(open > priority_score(bridge_likely(minutes)));
        }
    }

    #[test]
    fn unknown_timing_never_outranks_a_dated_event() {
        // A channel that cannot say when it matters loses to one that can, at
        // the same urgency.
        assert!(
            priority_score(input(Urgency::HeadsUp, Some(45)))
                > priority_score(input(Urgency::HeadsUp, None))
        );
    }

    #[test]
    fn imminence_cannot_promote_routine_over_an_emergency() {
        assert!(
            priority_score(input(Urgency::Emergency, Some(50)))
                > priority_score(input(Urgency::Routine, Some(0)))
        );
    }

    #[test]
    fn a_severity_tiebreak_never_reorders_two_kinds() {
        let mild = PriorityInput {
            severity_rank: 0,
            ..input(Urgency::Action, Some(10))
        };
        let severe = PriorityInput {
            severity_rank: SEVERITY_RANK_MAX,
            ..input(Urgency::HeadsUp, Some(10))
        };
        assert!(priority_score(mild) > priority_score(severe));
    }

    #[test]
    fn the_anchor_bonus_breaks_ties_without_winning_arguments() {
        let anchored = PriorityInput {
            is_anchor: true,
            ..input(Urgency::HeadsUp, Some(20))
        };
        let plain = input(Urgency::HeadsUp, Some(20));
        assert!(priority_score(anchored) > priority_score(plain));
        // ...but one minute of imminence still beats being the anchor.
        assert!(priority_score(input(Urgency::HeadsUp, Some(18))) > priority_score(anchored));
    }

    #[test]
    fn the_imminence_term_saturates_rather_than_wrapping() {
        assert_eq!(
            priority_score(input(Urgency::Routine, Some(u16::MAX))),
            Urgency::Routine.base()
        );
    }
}
