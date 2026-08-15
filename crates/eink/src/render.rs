use embedded_graphics::pixelcolor::BinaryColor;
use thiserror::Error;

use crate::{
    ConfidenceBand, LiveSnapshot, MonoFrame, SnapshotState, SpanStatus,
    channel::display_ascii,
    model::SnapshotError,
    render_primitives::{fill, fit, huge, label, line, strong, text_width},
};

const CONTENT_WIDTH: u32 = 232;
const RAIL_LEFT: i32 = 232;

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
    draw_title(&mut frame, snapshot);
    draw_state_band(&mut frame, snapshot);
    draw_timing(&mut frame, snapshot);
    let spans = spans_to_draw(snapshot);
    if !spans.is_empty() {
        draw_spans(&mut frame, &spans);
    }
    draw_rail(&mut frame, snapshot, config);
    Ok(frame)
}

/// Thin identity strip. The source that produced the reading is deliberately
/// absent: a driver deciding whether to turn does not care whether the answer
/// came from a bridge controller or a vessel feed, only whether it is current.
fn draw_title(frame: &mut MonoFrame, snapshot: &LiveSnapshot) {
    label(frame, 4, 1, &fit(&snapshot.channel, 26), BinaryColor::On);
    if snapshot.freshness.is_stale() {
        let mark = format!("STALE {}", snapshot.freshness.age_label());
        let x = 228 - text_width(&mark, 6);
        label(frame, x, 1, &mark, BinaryColor::On);
    }
    line(frame, 4, 12, 228, 12, BinaryColor::On);
}

/// The state, as large and as loud as a one-bit panel allows.
///
/// Anything that has already happened or is about to gets the whole band
/// inverted, because a glance from across a room resolves a black rectangle
/// long before it resolves a word.
fn draw_state_band(frame: &mut MonoFrame, snapshot: &LiveSnapshot) {
    let alerting = matches!(snapshot.state, SnapshotState::Open | SnapshotState::Likely);
    if alerting {
        fill(frame, 0, 15, CONTENT_WIDTH, 46, BinaryColor::On);
    }
    let ink = if alerting {
        BinaryColor::Off
    } else {
        BinaryColor::On
    };

    // The word carries the state and the drawing confirms it. Two encodings of
    // the same fact, because one of them reads at a distance and the other
    // reads at a glance.
    let word = fit(snapshot.state.label(), 13);
    huge(frame, 5, 26, &word, ink);
    draw_bascule(frame, 150, 20, snapshot.state == SnapshotState::Open, ink);

    if !alerting {
        line(frame, 4, 60, 228, 60, BinaryColor::On);
    }
}

/// The countdown, which is the whole reason a prediction beats a camera.
fn draw_timing(frame: &mut MonoFrame, snapshot: &LiveSnapshot) {
    let predictive = matches!(snapshot.state, SnapshotState::Watch | SnapshotState::Likely);
    let headline = match (predictive, snapshot.eta) {
        // A range is the honest form: the river runs between three and six
        // knots and the vessel class is rarely known.
        (true, Some(eta)) if eta.earliest_minutes == eta.latest_minutes => {
            format!("T-{} MIN", eta.latest_minutes)
        }
        // "T-6-9 MIN" reads as one number with a stray dash; spelling the range
        // out costs three characters and removes the ambiguity.
        (true, Some(eta)) => format!("T-{} TO {} MIN", eta.earliest_minutes, eta.latest_minutes),
        (true, None) => "OPENING EXPECTED".into(),
        _ => snapshot.state.road_meaning().to_owned(),
    };
    let word = fit(&headline, 23);
    let x = ((CONTENT_WIDTH as i32 - text_width(&word, 10)) / 2).max(2);
    huge(frame, x, 66, &word, BinaryColor::On);

    if let Some(confidence) = snapshot.confidence_percent.filter(|_| predictive) {
        let band = ConfidenceBand::from_percent(confidence).label();
        let note = format!("{confidence}% {band}");
        let x = ((CONTENT_WIDTH as i32 - text_width(&note, 6)) / 2).max(2);
        label(frame, x, 88, &note, BinaryColor::On);
    }
}

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
    let top = 105;
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

    #[test]
    fn a_predictive_state_shows_a_countdown_rather_than_a_road_note() {
        // The countdown is the whole reason a prediction beats a camera, so it
        // has to be what the panel spends its largest type on.
        let mut likely = snapshot(SnapshotState::Likely);
        likely.eta = Some(EtaRange::new(6, 9));
        let mut without = likely.clone();
        without.eta = None;

        let with_eta = render_snapshot(&likely, &RenderConfig::default()).unwrap();
        let no_eta = render_snapshot(&without, &RenderConfig::default()).unwrap();
        assert_ne!(with_eta.packed(), no_eta.packed());
    }

    #[test]
    fn the_panel_never_shows_which_source_produced_the_reading() {
        // A driver deciding whether to turn does not care whether the answer
        // came from a bridge controller or a vessel feed. Changing only the
        // source label must not change a single pixel.
        let mut one = snapshot(SnapshotState::Clear);
        one.confidence_percent = None;
        one.freshness = Freshness::new("AIS + FL511", 40, 180);
        let mut other = one.clone();
        other.freshness = Freshness::new("SOMETHING ELSE ENTIRELY", 40, 180);

        let one = render_snapshot(&one, &RenderConfig::default()).unwrap();
        let other = render_snapshot(&other, &RenderConfig::default()).unwrap();
        assert_eq!(one.packed(), other.packed());
    }

    #[test]
    fn an_alerting_state_inverts_the_whole_band() {
        // A black rectangle resolves from across a room long before a word
        // does, so open and likely get the loudest treatment the panel has.
        let mut clear = snapshot(SnapshotState::Clear);
        clear.confidence_percent = None;
        let open =
            render_snapshot(&snapshot(SnapshotState::Open), &RenderConfig::default()).unwrap();
        let clear = render_snapshot(&clear, &RenderConfig::default()).unwrap();
        assert!(open.black_pixel_count() > clear.black_pixel_count() + 3_000);
    }
}
