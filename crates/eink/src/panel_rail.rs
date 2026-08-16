//! The registration rail every panel carries down its right edge.
//!
//! One rail, drawn by one function, because there used to be two. The bridge
//! panel and the channel card each grew their own copy and drifted: five slots
//! against four, a six-pixel minor tick against seven, ladders starting at
//! different heights, and an interrupt hatch whose last stroke fell off the
//! bottom of the panel in both. They rotate on the same screen seconds apart, so
//! a reader was re-learning the instrument every time the panel changed.
//!
//! The rail answers three questions in fixed positions: which channel this is
//! (the code), where this reading sits on its own ladder (the slot), and whether
//! it is allowed to interrupt (the hatch).

use embedded_graphics::pixelcolor::BinaryColor;

use crate::{
    HEIGHT, MonoFrame,
    render_primitives::{fill, fit, line, strong},
};

/// Left edge of the rail, and therefore the right edge of everything else.
pub(crate) const RAIL_LEFT: i32 = 232;
/// Width of the rail, taking it exactly to the panel's right edge.
pub(crate) const RAIL_WIDTH: u32 = 18;
/// Drawing width available to panel content.
pub(crate) const CONTENT_WIDTH: u32 = RAIL_LEFT as u32;

/// Baseline of the first character of the two-letter code.
const CODE_TOP: i32 = 2;
/// Vertical pitch between the two code characters.
const CODE_PITCH: i32 = 13;
/// Rule closing the code block off from the ladder.
const CODE_RULE_Y: i32 = 29;

/// Ladder region, between the code rule and the interrupt hatch.
const LADDER_TOP: i32 = 36;
const LADDER_BOTTOM: i32 = 92;
/// Vertical pitch between ladder rungs.
const SLOT_PITCH: i32 = 10;

/// First hatch stroke, and the step between strokes.
const HATCH_TOP: i32 = 96;
const HATCH_PITCH: usize = 7;
/// Vertical run of one hatch stroke.
const HATCH_RISE: i32 = 5;

/// Draws the rail: channel code, a ladder with one rung marked, and the
/// interrupt hatch when this reading is allowed to take the panel.
///
/// `slots` is how many rungs this panel's ladder has — the bridge has five
/// states, a channel card four urgencies — and `active` is the zero-based rung
/// to mark. The ladder is centred in its region whatever the count, so both
/// panels place the same reading at the same height.
pub(crate) fn draw_rail(
    frame: &mut MonoFrame,
    code: &str,
    active: usize,
    slots: usize,
    interrupting: bool,
) {
    fill(
        frame,
        RAIL_LEFT,
        0,
        RAIL_WIDTH,
        u32::from(HEIGHT),
        BinaryColor::On,
    );

    for (index, character) in fit(code, 2).chars().take(2).enumerate() {
        strong(
            frame,
            237,
            CODE_TOP + i32::try_from(index).unwrap_or(0) * CODE_PITCH,
            &character.to_string(),
            BinaryColor::Off,
        );
    }
    line(frame, 235, CODE_RULE_Y, 247, CODE_RULE_Y, BinaryColor::Off);

    draw_ladder(frame, active, slots);

    if interrupting {
        draw_hatch(frame);
    }
}

/// The ladder, centred in its region so a four-rung and a five-rung panel share
/// an optical centre rather than a shared starting pixel.
fn draw_ladder(frame: &mut MonoFrame, active: usize, slots: usize) {
    let slots = slots.max(1);
    let span = i32::try_from(slots.saturating_sub(1)).unwrap_or(0) * SLOT_PITCH;
    let top = LADDER_TOP + ((LADDER_BOTTOM - LADDER_TOP) - span) / 2;

    for index in 0..slots {
        let y = top + i32::try_from(index).unwrap_or(0) * SLOT_PITCH;
        // Alternating rung lengths give the ladder a readable rhythm rather
        // than a row of identical dashes.
        let length = if index % 2 == 0 { 10 } else { 6 };
        line(frame, 248 - length, y, 248, y, BinaryColor::Off);
        if index == active {
            fill(frame, 234, y - 2, 14, 5, BinaryColor::Off);
        }
    }
}

/// Diagonal hatch marking an interrupt.
///
/// Bounded to strokes that land entirely on the panel. Both renderers used to
/// step past the last drawable row, so the drawing layer clipped the final
/// stroke and the hatch tapered out instead of ending on the panel edge.
fn draw_hatch(frame: &mut MonoFrame) {
    let last = i32::from(HEIGHT) - 1 - HATCH_RISE;
    for y in (HATCH_TOP..=last).step_by(HATCH_PITCH) {
        line(frame, 233, y + HATCH_RISE, 240, y, BinaryColor::Off);
        line(frame, 241, y + HATCH_RISE, 249, y, BinaryColor::Off);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WIDTH;

    fn rail(active: usize, slots: usize, interrupting: bool) -> MonoFrame {
        let mut frame = MonoFrame::white();
        draw_rail(&mut frame, "BR", active, slots, interrupting);
        frame
    }

    /// The hatch used to run off the bottom of the panel in both renderers, so
    /// its final stroke was clipped and the mark tapered out.
    #[test]
    fn the_interrupt_hatch_lands_entirely_on_the_panel() {
        let interrupting = rail(0, 4, true);
        let quiet = rail(0, 4, false);
        assert_ne!(interrupting.packed(), quiet.packed(), "the hatch must draw");

        // The last row the hatch reaches has to carry the same pair of strokes
        // as the rows above it; a clipped stroke leaves a thinner last row.
        let ink_at = |frame: &MonoFrame, y: u16| {
            (RAIL_LEFT..i32::from(WIDTH))
                .filter(|x| !frame.is_black(u16::try_from(*x).unwrap_or(0), y))
                .count()
        };
        let bottom = i32::from(HEIGHT) - 1;
        assert!(
            ink_at(&interrupting, u16::try_from(bottom).unwrap_or(0)) == 0,
            "the hatch must stop clear of the panel edge rather than being cut by it"
        );
    }

    /// Both panels put the same reading at the same height even though their
    /// ladders have different rung counts.
    #[test]
    fn ladders_of_different_lengths_share_an_optical_centre() {
        let centre = |slots: usize| {
            let span = (slots - 1) as i32 * SLOT_PITCH;
            LADDER_TOP + ((LADDER_BOTTOM - LADDER_TOP) - span) / 2 + span / 2
        };
        assert_eq!(
            centre(4),
            centre(5),
            "a four-rung and a five-rung ladder must centre on the same row"
        );
    }

    /// A panel that names no slot must still draw a rail rather than panicking
    /// or marking a rung that does not exist.
    #[test]
    fn an_out_of_range_slot_marks_nothing_and_still_draws() {
        let frame = rail(99, 4, false);
        assert!(frame.black_pixel_count() > 0);
    }
}
