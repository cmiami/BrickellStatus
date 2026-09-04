//! Cross-format contract smoke tests.

use brickellstatus_model::{
    Availability, BridgeControllerState, BridgeObservation, ChannelId, Observation, ObservationId,
    SourceId, TimestampMillis,
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
