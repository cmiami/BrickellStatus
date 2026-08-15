use embedded_graphics::pixelcolor::BinaryColor;
use thiserror::Error;

use crate::{
    ConfidenceBand, LiveSnapshot, MonoFrame, SnapshotState, SpanStatus,
    channel::display_ascii,
    model::SnapshotError,
    render_primitives::{fill, fit, label, large, line, outline, strong, text_width},
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
    // Upstream spans take the second evidence line. At 250x122 there is no
    // spare row, and knowing which river spans are up is worth more than a
    // second restatement of why -- the first evidence line already says that.
    let spans = spans_to_draw(snapshot);
    let evidence_lines = if spans.is_empty() {
        config.maximum_evidence_items
    } else {
        1
    };
    draw_evidence(&mut frame, snapshot, evidence_lines);
    if !spans.is_empty() {
        draw_spans(&mut frame, &spans);
    }
    draw_source_tape(&mut frame, snapshot, config.show_source_age);
    draw_rail(&mut frame, snapshot, config);
    Ok(frame)
}

/// Picks the two upstream spans worth the row.
///
/// The river carries eight bascules upstream of Brickell and the panel has room
/// for two, so the choice is the message. An open span is the news -- it is
/// evidence of a vessel under way right now -- and the nearest open one is the
/// most imminent. Falling back to the nearest spans keeps the row stable and
/// legible when nothing is moving. Callers supply spans in river order.
fn spans_to_draw(snapshot: &LiveSnapshot) -> Vec<&SpanStatus> {
    let mut chosen: Vec<&SpanStatus> = snapshot.spans.iter().filter(|span| span.open).collect();
    for span in &snapshot.spans {
        if chosen.len() >= 2 {
            break;
        }
        if !span.open {
            chosen.push(span);
        }
    }
    chosen.truncate(2);
    chosen
}

/// Double-leaf bascule glyph in a 78x24 box anchored at `x, y`.
///
/// Each leaf is hinged over its own pier and the free ends meet at midspan when
/// closed; opening lifts both away from the centre. That silhouette is the one
/// thing a driver recognizes at a glance, which is why the display spends
/// pixels on a picture rather than another word.
fn draw_bascule(frame: &mut MonoFrame, x: i32, y: i32, open: bool, color: BinaryColor) {
    let deck = y + 18;
    let water = y + 22;
    // A narrow channel keeps each leaf short enough to stand up steeply inside
    // the box. A shallow lift reads as a peaked roof rather than an opening.
    let left_pier = x + 16;
    let right_pier = x + 56;

    line(frame, x, water, x + 74, water, color);
    // Piers, drawn as short verticals so the hinge points read as fixed.
    line(frame, left_pier, deck, left_pier, water - 1, color);
    line(frame, right_pier, deck, right_pier, water - 1, color);
    // Fixed approach spans either side.
    line(frame, x, deck, left_pier, deck, color);
    line(frame, right_pier, deck, x + 74, deck, color);

    if open {
        // Near-vertical, as a bascule sits at full open, leaving a clear gap
        // between the tips for the vessel to pass.
        line(frame, left_pier, deck, left_pier + 8, deck - 17, color);
        line(frame, left_pier + 1, deck, left_pier + 9, deck - 17, color);
        line(frame, right_pier, deck, right_pier - 8, deck - 17, color);
        line(
            frame,
            right_pier - 1,
            deck,
            right_pier - 9,
            deck - 17,
            color,
        );
    } else {
        // Closed: one continuous deck across the channel, drawn two pixels
        // thick so it carries the same visual weight as the raised leaves.
        line(frame, left_pier, deck, right_pier, deck, color);
        line(frame, left_pier, deck - 1, right_pier, deck - 1, color);
    }
}

fn draw_spans(frame: &mut MonoFrame, spans: &[&SpanStatus]) {
    let top = 91;
    for (index, span) in spans.iter().enumerate() {
        let x = 5 + i32::try_from(index).unwrap_or(0) * 112;
        let code = fit(&span.code, 3);
        if span.open {
            // Invert the code only. Extending the fill under the detail text
            // would paint it black-on-black and silently erase the time.
            let width = u32::try_from(text_width(&code, 6) + 4).unwrap_or(22);
            fill(frame, x - 2, top - 1, width, 12, BinaryColor::On);
            label(frame, x, top, &code, BinaryColor::Off);
            let detail = span
                .opened_at
                .as_deref()
                .map(|at| format!("UP {at}"))
                .unwrap_or_else(|| "UP".into());
            label(
                frame,
                x + text_width(&code, 6) + 8,
                top,
                &fit(&detail, 11),
                BinaryColor::On,
            );
        } else {
            label(frame, x, top, &code, BinaryColor::On);
            label(
                frame,
                x + text_width(&code, 6) + 6,
                top,
                "DOWN",
                BinaryColor::On,
            );
        }
    }
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
    let ink = if hard_inverse {
        BinaryColor::Off
    } else {
        BinaryColor::On
    };
    // Capped at 13 characters so the state word cannot run under the bascule
    // glyph parked at x=150. The longest real label, BRIDGE OPEN, is 11.
    large(frame, 4, 18, &fit(snapshot.state.label(), 13), ink);
    draw_bascule(frame, 150, 16, snapshot.state == SnapshotState::Open, ink);
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
    use crate::{EtaRange, Evidence, Freshness, SpanStatus};

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

    #[test]
    fn the_bascule_glyph_distinguishes_an_open_span_from_a_closed_one() {
        // Same everything but the state, so any pixel difference in the glyph
        // region is the leaves and nothing else.
        let mut closed = snapshot(SnapshotState::Clear);
        closed.confidence_percent = None;
        let mut open = closed.clone();
        open.state = SnapshotState::Open;

        let closed_frame = render_snapshot(&closed, &RenderConfig::default()).unwrap();
        let open_frame = render_snapshot(&open, &RenderConfig::default()).unwrap();
        assert_ne!(closed_frame.packed(), open_frame.packed());
    }

    #[test]
    fn upstream_spans_render_and_take_the_second_evidence_line() {
        let mut without = snapshot(SnapshotState::Clear);
        without.confidence_percent = None;
        let mut with = without.clone();
        with.spans = vec![
            SpanStatus::new("2AV", true).opened_at("14:20"),
            SpanStatus::new("1ST", false),
        ];

        let plain = render_snapshot(&without, &RenderConfig::default()).unwrap();
        let spanned = render_snapshot(&with, &RenderConfig::default()).unwrap();
        assert_ne!(plain.packed(), spanned.packed());
        // The open span is drawn inverted, so it adds a filled block.
        assert!(spanned.black_pixel_count() > plain.black_pixel_count());
    }

    #[test]
    fn an_open_span_reads_differently_with_and_without_an_opening_time() {
        let mut base = snapshot(SnapshotState::Clear);
        base.confidence_percent = None;
        let mut timed = base.clone();
        timed.spans = vec![SpanStatus::new("2AV", true).opened_at("14:20")];
        let mut untimed = base.clone();
        untimed.spans = vec![SpanStatus::new("2AV", true)];

        let timed = render_snapshot(&timed, &RenderConfig::default()).unwrap();
        let untimed = render_snapshot(&untimed, &RenderConfig::default()).unwrap();
        assert_ne!(timed.packed(), untimed.packed());
    }

    #[test]
    fn at_most_two_spans_are_drawn() {
        // Eight bascules upstream, room for two. Extra closed spans must not
        // change the frame, and must not panic on the slice.
        let mut two = snapshot(SnapshotState::Clear);
        two.confidence_percent = None;
        two.spans = vec![SpanStatus::new("2AV", false), SpanStatus::new("1ST", false)];
        let mut many = two.clone();
        many.spans.push(SpanStatus::new("FLG", false));
        many.spans.push(SpanStatus::new("5ST", false));

        let two = render_snapshot(&two, &RenderConfig::default()).unwrap();
        let many = render_snapshot(&many, &RenderConfig::default()).unwrap();
        assert_eq!(two.packed(), many.packed());
    }

    #[test]
    fn an_open_span_displaces_a_nearer_closed_one() {
        // A span that is up is evidence of a vessel under way; a span that is
        // down is not news. With only two slots, showing two closed spans while
        // an open one exists further upriver hides the only useful reading.
        let mut base = snapshot(SnapshotState::Clear);
        base.confidence_percent = None;

        let mut nearest_two_closed = base.clone();
        nearest_two_closed.spans = vec![
            SpanStatus::new("2AV", false),
            SpanStatus::new("1ST", false),
            SpanStatus::new("FLG", false),
        ];
        let mut one_open_upriver = base.clone();
        one_open_upriver.spans = vec![
            SpanStatus::new("2AV", false),
            SpanStatus::new("1ST", false),
            SpanStatus::new("FLG", true).opened_at("10:00"),
        ];

        let closed = render_snapshot(&nearest_two_closed, &RenderConfig::default()).unwrap();
        let opened = render_snapshot(&one_open_upriver, &RenderConfig::default()).unwrap();
        assert_ne!(
            closed.packed(),
            opened.packed(),
            "an open upstream span must reach the panel"
        );
        assert!(opened.black_pixel_count() > closed.black_pixel_count());
    }

    #[test]
    fn a_blank_span_code_is_rejected_before_pixels_are_produced() {
        let mut invalid = snapshot(SnapshotState::Clear);
        invalid.confidence_percent = None;
        invalid.spans = vec![SpanStatus::new("  ", true)];
        assert!(render_snapshot(&invalid, &RenderConfig::default()).is_err());
    }
}
