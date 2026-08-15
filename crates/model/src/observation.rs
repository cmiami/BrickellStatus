//! Normalized evidence consumed by the bridge predictor.
#![allow(missing_docs)]

use crate::{
    Availability, ChannelId, Confidence, EtaRangeMinutes, ObservationId, SourceId, TimestampMillis,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub id: ObservationId,
    pub channel_id: ChannelId,
    pub source_id: SourceId,
    pub observed_at: TimestampMillis,
    pub received_at: TimestampMillis,
    pub expires_at: Option<TimestampMillis>,
    pub availability: Availability,
    pub data: BridgeObservation,
}

impl Observation {
    pub fn is_expired_at(&self, now: TimestampMillis) -> bool {
        self.expires_at.is_some_and(|expires| expires < now)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BridgeObservation {
    AisTrack {
        mmsi: Option<String>,
        vessel_name: Option<String>,
        movement: VesselMovement,
        route_intersects: bool,
        eta: Option<EtaRangeMinutes>,
        opening_propensity: Option<Confidence>,
    },
    OutboundProgress {
        bridge: String,
        stage: OutboundProgressStage,
        eta: Option<EtaRangeMinutes>,
    },
    Controller {
        state: BridgeControllerState,
    },
    /// A river transit booked on the pilots' dispatch board.
    ///
    /// Unlike the other variants this is scheduled rather than observed: it
    /// says a vessel is expected, not that one has been seen.
    ScheduledTransit {
        vessel: String,
        /// Whether the movement is exempt from the bridge's ordinary schedule.
        ///
        /// 33 CFR 117.261 excuses public vessels, tugs with tows, and vessels
        /// in distress from the Brickell blackout periods. Every RIVER movement
        /// the pilots publish is a tug-assisted commercial tow, so the blackout
        /// does not bind it: the bridge opens for cargo when it would refuse a
        /// yacht.
        exempt: bool,
        eta: Option<EtaRangeMinutes>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeOperatingMode {
    OnSignal,
    Scheduled,
    Blackout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VesselMovement {
    Approaching,
    Diverging,
    Stationary,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundProgressStage {
    High,
    VeryHigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeControllerState {
    Closed,
    Open,
    Unknown,
}
