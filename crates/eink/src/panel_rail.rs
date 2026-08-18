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
//!
//! Its width is the same on every panel and its marks are placed from its own
//! left edge, so the larger board moves the rail outward rather than stretching
//! it. A ruler that grows with the sheet is not a ruler.

use embedded_graphics::pixelcolor::BinaryColor;

use crate::{
    MonoFrame,
    panel::PanelGrid,
    render_primitives::{fill, fit, line, strong},
};

/// Baseline of the first character of the two-letter code.
const CODE_TOP: i32 = 2;
/// Vertical pitch between the two code characters.
const CODE_PITCH: i32 = 13;
/// Rule closing the code block off from the ladder.
const CODE_RULE_Y: i32 = 29;

/// Ladder region, between the code rule and the interrupt hatch.
const LADDER_TOP: i32 = 36;
/// Distance from the bottom of the panel at which the ladder stops, leaving the
/// hatch its room. Measured from the edge rather than fixed at a row, so the
/// taller panel lengthens the ladder region instead of crowding the hatch.
const LADDER_BOTTOM_INSET: i32 = 30;
/// Vertical pitch between ladder rungs.
const SLOT_PITCH: i32 = 10;

/// First hatch stroke, likewise measured up from the bottom edge, and the step
/// between strokes.
const HATCH_TOP_INSET: i32 = 26;
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
    let grid = frame.grid();
    let rail = grid.rail_left();
    fill(
        frame,
        rail,
        0,
        crate::panel::RAIL_WIDTH,
        u32::from(grid.height),
        BinaryColor::On,
    );

    for (index, character) in fit(code, 2).chars().take(2).enumerate() {
        strong(
            frame,
            rail + 5,
            CODE_TOP + i32::try_from(index).unwrap_or(0) * CODE_PITCH,
            &character.to_string(),
            BinaryColor::Off,
        );
    }
    line(
        frame,
        rail + 3,
        CODE_RULE_Y,
        rail + 15,
        CODE_RULE_Y,
        BinaryColor::Off,
    );

    draw_ladder(frame, grid, active, slots);

    if interrupting {
        draw_hatch(frame, grid);
    }
}

/// The ladder, centred in its region so a four-rung and a five-rung panel share
/// an optical centre rather than a shared starting pixel.
fn draw_ladder(frame: &mut MonoFrame, grid: PanelGrid, active: usize, slots: usize) {
    let rail = grid.rail_left();
    let bottom = i32::from(grid.height) - LADDER_BOTTOM_INSET;
    let slots = slots.max(1);
    let span = i32::try_from(slots.saturating_sub(1)).unwrap_or(0) * SLOT_PITCH;
    let top = LADDER_TOP + ((bottom - LADDER_TOP) - span) / 2;

    for index in 0..slots {
        let y = top + i32::try_from(index).unwrap_or(0) * SLOT_PITCH;
        // Alternating rung lengths give the ladder a readable rhythm rather
        // than a row of identical dashes.
        let length = if index % 2 == 0 { 10 } else { 6 };
        line(frame, rail + 16 - length, y, rail + 16, y, BinaryColor::Off);
        if index == active {
            fill(frame, rail + 2, y - 2, 14, 5, BinaryColor::Off);
        }
    }
}

/// Diagonal hatch marking an interrupt.
///
/// Bounded to strokes that land entirely on the panel. Both renderers used to
/// step past the last drawable row, so the drawing layer clipped the final
/// stroke and the hatch tapered out instead of ending on the panel edge.
fn draw_hatch(frame: &mut MonoFrame, grid: PanelGrid) {
    let rail = grid.rail_left();
    let first = i32::from(grid.height) - HATCH_TOP_INSET;
    let last = i32::from(grid.height) - 1 - HATCH_RISE;
    for y in (first..=last).step_by(HATCH_PITCH) {
        line(
            frame,
            rail + 1,
            y + HATCH_RISE,
            rail + 8,
            y,
            BinaryColor::Off,
        );
        line(
            frame,
            rail + 9,
            y + HATCH_RISE,
            rail + 17,
            y,
            BinaryColor::Off,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PanelModel;

    fn rail(model: PanelModel, active: usize, slots: usize, interrupting: bool) -> MonoFrame {
        let mut frame = MonoFrame::white(model);
        draw_rail(&mut frame, "BR", active, slots, interrupting);
        frame
    }

    /// The hatch used to run off the bottom of the panel in both renderers, so
    /// its final stroke was clipped and the mark tapered out.
    #[test]
    fn the_interrupt_hatch_lands_entirely_on_the_panel() {
        for model in PanelModel::ALL {
            let interrupting = rail(model, 0, 4, true);
            let quiet = rail(model, 0, 4, false);
            assert_ne!(
                interrupting.packed(),
                quiet.packed(),
                "{model:?}: the hatch must draw"
            );

            let grid = model.grid();
            let ink_at = |frame: &MonoFrame, y: u16| {
                (grid.rail_left()..i32::from(grid.width))
                    .filter(|x| !frame.is_black(u16::try_from(*x).unwrap_or(0), y))
                    .count()
            };
            let bottom = u16::try_from(i32::from(grid.height) - 1).unwrap_or(0);
            assert!(
                ink_at(&interrupting, bottom) == 0,
                "{model:?}: the hatch must stop clear of the panel edge"
            );
        }
    }

    /// Both panels put the same reading at the same height even though their
    /// ladders have different rung counts.
    #[test]
    fn ladders_of_different_lengths_share_an_optical_centre() {
        for model in PanelModel::ALL {
            let grid = model.grid();
            let bottom = i32::from(grid.height) - LADDER_BOTTOM_INSET;
            let centre = |slots: usize| {
                let span = (slots - 1) as i32 * SLOT_PITCH;
                LADDER_TOP + ((bottom - LADDER_TOP) - span) / 2 + span / 2
            };
            assert_eq!(
                centre(4),
                centre(5),
                "{model:?}: a four-rung and a five-rung ladder must centre together"
            );
        }
    }

    /// A panel that names no slot must still draw a rail rather than panicking
    /// or marking a rung that does not exist.
    #[test]
    fn an_out_of_range_slot_marks_nothing_and_still_draws() {
        for model in PanelModel::ALL {
            assert!(rail(model, 99, 4, false).black_pixel_count() > 0);
        }
    }

    /// The rail is the same object on both panels: same width, same marks, sat
    /// against the right edge of whichever board it is on.
    #[test]
    fn the_rail_is_the_same_ruler_on_both_panels() {
        let small = rail(PanelModel::E213, 1, 5, true);
        let large = rail(PanelModel::E290, 1, 5, true);
        let sample = |frame: &MonoFrame, y: u16| {
            (0..crate::panel::RAIL_WIDTH as u16)
                .map(|offset| {
                    let x = u16::try_from(frame.grid().rail_left()).unwrap_or(0) + offset;
                    frame.is_black(x, y)
                })
                .collect::<Vec<_>>()
        };
        // Rows above the ladder region carry the code block, which is placed
        // from the top on both panels and must therefore be identical.
        for y in 0..CODE_RULE_Y as u16 {
            assert_eq!(sample(&small, y), sample(&large, y), "rail row {y}");
        }
    }
}
