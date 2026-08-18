//! Cross-format contract smoke tests.

use brickellstatus_model::{
    Availability, AvailabilityStatus, BridgeControllerState, BridgeObservation, ChannelId,
    Observation, ObservationId, SourceId, TimestampMillis,
};

#[test]
fn observation_round_trips_with_tagged_bridge_fact() {
    let now = TimestampMillis::new(1_700_000_000_000);
    let observation = Observation {
        id: ObservationId::from("obs-1"),
        channel_id: ChannelId::from("brickell"),
        source_id: SourceId::from("fl511"),
        observed_at: now,
        received_at: now,
        expires_at: None,
        availability: Availability::live(now),
        data: BridgeObservation::Controller {
            state: BridgeControllerState::Open,
        },
    };

    let json = serde_json::to_string(&observation).expect("serialize observation");
    assert!(json.contains("\"kind\":\"controller\""));
    assert_eq!(
        serde_json::from_str::<Observation>(&json).expect("deserialize observation"),
        observation
    );
}

#[test]
fn availability_is_independent_and_explicit() {
    let availability = Availability {
        status: AvailabilityStatus::Stale,
        checked_at: TimestampMillis::new(5_000),
        last_success_at: Some(TimestampMillis::new(1_000)),
        detail: Some("AIS receiver has not reported recently".into()),
    };

    assert_eq!(availability.status, AvailabilityStatus::Stale);
}
