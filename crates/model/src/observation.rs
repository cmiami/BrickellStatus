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
