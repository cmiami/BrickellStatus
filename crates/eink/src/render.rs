use embedded_graphics::pixelcolor::BinaryColor;
use thiserror::Error;

use crate::{
    ConfidenceBand, LiveSnapshot, MonoFrame, SnapshotState,
    channel::display_ascii,
    model::SnapshotError,
    render_primitives::{fill, fit, label, large, line, outline, strong},
};

const CONTENT_WIDTH: u32 = 232;
const RAIL_LEFT: i32 = 232;
const TAPE_TOP: i32 = 108;

/// Small set of layout controls which remain stable across live snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderConfig {
    /// Two-character mark shown at the top of the evidence rail.
    pub channel_code: String,
    /// Maximum evidence items collapsed into the one-line evidence strip.
    pub maximum_evidence_items: usize,
    /// Whether the source tape includes the current source age.
    pub show_source_age: bool,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            channel_code: "BR".into(),
            maximum_evidence_items: 2,
            show_source_age: true,
        }
    }
}

/// Failure to turn a semantic snapshot into pixels.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RenderError {
    /// Snapshot violated a display-semantic invariant.
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    /// Channel code did not contain any visible ASCII text.
    #[error("render channel code cannot be empty")]
    EmptyChannelCode,
}

/// Renders a deterministic 250×122 one-bit status instrument.
pub fn render_snapshot(
    snapshot: &LiveSnapshot,
    config: &RenderConfig,
) -> Result<MonoFrame, RenderError> {
    snapshot.validate()?;
    if display_ascii(&config.channel_code).is_empty() {
        return Err(RenderError::EmptyChannelCode);
    }

    let mut frame = MonoFrame::white();
    draw_header(&mut frame, snapshot);
    draw_decision(&mut frame, snapshot);
    draw_evidence(&mut frame, snapshot, config.maximum_evidence_items);
    draw_source_tape(&mut frame, snapshot, config.show_source_age);
    draw_rail(&mut frame, snapshot, config);
    Ok(frame)
}

fn draw_header(frame: &mut MonoFrame, snapshot: &LiveSnapshot) {
    fill(frame, 0, 0, CONTENT_WIDTH, 15, BinaryColor::On);
    label(frame, 4, 2, &fit(&snapshot.channel, 31), BinaryColor::Off);
    let health = if snapshot.freshness.is_stale() {
        "STALE"
    } else {
        "LIVE"
    };
    let x = 228 - i32::try_from(health.len() * 6).unwrap_or(0);
    label(frame, x, 2, health, BinaryColor::Off);
}

fn draw_decision(frame: &mut MonoFrame, snapshot: &LiveSnapshot) {
    let hard_inverse = snapshot.state == SnapshotState::Open;
    if hard_inverse {
        fill(frame, 0, 16, CONTENT_WIDTH, 25, BinaryColor::On);
    }
    large(
        frame,
        4,
        18,
        &fit(snapshot.state.label(), 21),
        if hard_inverse {
            BinaryColor::Off
        } else {
            BinaryColor::On
        },
    );
    line(frame, 4, 41, 228, 41, BinaryColor::On);

    if let Some(confidence) = snapshot.confidence_percent {
        let eta = snapshot
            .eta
            .map(|range| format!("ETA {}", range.display()))
            .unwrap_or_else(|| "ETA PENDING".into());
        strong(frame, 4, 45, &fit(&eta, 21), BinaryColor::On);
        label(
            frame,
            4,
            60,
            &fit(&snapshot.road_meaning, 25),
            BinaryColor::On,
        );

        outline(frame, 165, 44, 63, 27, 1);
        large(frame, 168, 47, &format!("{confidence}%"), BinaryColor::On);
        let band = ConfidenceBand::from_percent(confidence).label();
        label(frame, 202, 56, band, BinaryColor::On);
    } else {
        let eta = if matches!(snapshot.state, SnapshotState::Watch | SnapshotState::Likely) {
            snapshot.eta
        } else {
            None
        };
        let primary = eta
            .map(|range| format!("ETA {}", range.display()))
            .unwrap_or_else(|| snapshot.road_meaning.clone());
        strong(frame, 4, 46, &fit(&primary, 31), BinaryColor::On);
        if eta.is_some() {
            label(
                frame,
                4,
                61,
                &fit(&snapshot.road_meaning, 37),
                BinaryColor::On,
            );
        }
    }
}

fn draw_evidence(frame: &mut MonoFrame, snapshot: &LiveSnapshot, maximum_items: usize) {
    let take = maximum_items.clamp(1, 2).min(snapshot.evidence.len());
    if take == 0 {
        strong(frame, 5, 77, "NO CURRENT EVIDENCE", BinaryColor::On);
    } else {
        for (index, item) in snapshot.evidence.iter().take(take).enumerate() {
            let value = display_ascii(&item.summary);
            if index == 0 {
                strong(frame, 5, 77, &fit(&value, 31), BinaryColor::On);
            } else {
                label(frame, 5, 92, &fit(&value, 37), BinaryColor::On);
            }
        }
        if snapshot.evidence.len() > take {
            label(
                frame,
                205,
                92,
                &format!("+{}", snapshot.evidence.len() - take),
                BinaryColor::On,
            );
        }
    }
    line(frame, 4, 105, 228, 105, BinaryColor::On);
}

fn draw_source_tape(frame: &mut MonoFrame, snapshot: &LiveSnapshot, show_age: bool) {
    fill(frame, 0, TAPE_TOP, CONTENT_WIDTH, 14, BinaryColor::On);
    let source = fit(&snapshot.freshness.source, 22);
    label(frame, 4, 109, &source, BinaryColor::Off);

    let right = if snapshot.freshness.is_stale() {
        format!("STALE {}", snapshot.freshness.age_label())
    } else if show_age {
        format!("AGE {}", snapshot.freshness.age_label())
    } else {
        "CURRENT".into()
    };
    let x = 228 - i32::try_from(right.len() * 6).unwrap_or(0);
    label(frame, x, 109, &right, BinaryColor::Off);
}

fn draw_rail(frame: &mut MonoFrame, snapshot: &LiveSnapshot, config: &RenderConfig) {
    fill(frame, RAIL_LEFT, 0, 18, 122, BinaryColor::On);
    let code = fit(&config.channel_code, 2);
    for (index, character) in code.chars().take(2).enumerate() {
        strong(
            frame,
            237,
            2 + i32::try_from(index * 13).unwrap_or(0),
            &character.to_string(),
            BinaryColor::Off,
        );
    }
    line(frame, 235, 29, 247, 29, BinaryColor::Off);

    let slot = state_slot(snapshot.state);
    for index in 0..5 {
        let y = 44 + index * 10;
        let length = if index % 2 == 0 { 10 } else { 6 };
        line(frame, 248 - length, y, 248, y, BinaryColor::Off);
        if index == slot {
            fill(frame, 234, y - 2, 14, 5, BinaryColor::Off);
        }
    }

    if snapshot.state.is_interrupting() {
        for y in (96..122).step_by(7) {
            line(frame, 233, y + 5, 240, y, BinaryColor::Off);
            line(frame, 241, y + 5, 249, y, BinaryColor::Off);
        }
    }
}

const fn state_slot(state: SnapshotState) -> i32 {
    match state {
        SnapshotState::Likely => 0,
        SnapshotState::Open => 1,
        SnapshotState::Watch => 2,
        SnapshotState::Clear => 3,
        SnapshotState::Offline => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EtaRange, Evidence, Freshness};

    fn snapshot(state: SnapshotState) -> LiveSnapshot {
        let mut snapshot = LiveSnapshot::brickell(state, Freshness::new("AIS + FL511", 75, 180));
        snapshot.eta = Some(EtaRange::new(6, 9));
        snapshot.evidence = vec![
            Evidence::new("outbound vessel", "AIS"),
            Evidence::new("upstream bridge", "FL511"),
        ];
        if state.is_predictive() {
            snapshot.confidence_percent = Some(82);
        }
        snapshot
    }

    #[test]
    fn every_state_renders_nonempty_physical_frame() {
        for state in [
            SnapshotState::Clear,
            SnapshotState::Watch,
            SnapshotState::Likely,
            SnapshotState::Open,
            SnapshotState::Offline,
        ] {
            let frame = render_snapshot(&snapshot(state), &RenderConfig::default()).unwrap();
            assert!(frame.black_pixel_count() > 1_000, "state {state:?}");
            assert!(frame.black_pixel_count() < 250 * 122, "state {state:?}");
        }
    }

    #[test]
    fn interrupt_treatment_is_materially_distinct() {
        let clear =
            render_snapshot(&snapshot(SnapshotState::Clear), &RenderConfig::default()).unwrap();
        let open =
            render_snapshot(&snapshot(SnapshotState::Open), &RenderConfig::default()).unwrap();
        assert_ne!(clear.packed(), open.packed());
        assert!(open.black_pixel_count() > clear.black_pixel_count());
    }
}
