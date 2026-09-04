//! Verify exact saved forecasts without starting the app or its collectors.
use brickellstatus_policy::{BridgePrediction, BridgePredictor, BridgeReplayInput};
use serde::Deserialize;
use std::io::{self, BufRead};

#[derive(Deserialize)]
struct Frame {
    input: BridgeReplayInput,
    expected: BridgePrediction,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut count = 0;
    for line in io::stdin().lock().lines() {
        let frame: Frame = serde_json::from_str(&line?)?;
        let actual = BridgePredictor::replay(&frame.input)?;
        if actual != frame.expected {
            return Err(format!(
                "evaluation {} differs (saved model {}, current {}); compare model versions before treating this as a regression",
                frame.input.at.0, frame.expected.model_version, actual.model_version,
            ).into());
        }
        count += 1;
    }
    if count == 0 {
        return Err("no saved evaluations supplied".into());
    }
    println!("Verified {count} exact forecast replays.");
    Ok(())
}
