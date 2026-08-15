//! Bridge prediction state and its orthogonal urgency and source-health axes.
#![allow(missing_docs)]

use crate::TimestampMillis;
use serde::{Deserialize, Serialize};

/// Bridge prediction and confirmation states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeState {
    #[default]
    Clear,
    Watch,
    Likely,
    Open,
}

/// How strongly a prediction should compete for the user's attention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    #[default]
    Passive,
    Notice,
    TimeSensitive,
    Critical,
}

/// Current health of the predictive source set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Availability {
    pub status: AvailabilityStatus,
    pub checked_at: TimestampMillis,
    pub last_success_at: Option<TimestampMillis>,
    pub detail: Option<String>,
}

impl Availability {
    pub fn live(at: TimestampMillis) -> Self {
        Self {
            status: AvailabilityStatus::Live,
            checked_at: at,
            last_success_at: Some(at),
            detail: None,
        }
    }
}

/// Coarse source-health state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityStatus {
    #[default]
    Live,
    Degraded,
    Stale,
    Offline,
    Disabled,
}
