//! The grid both panel families are drawn on.
//!
//! Horizontal structure was systematised first — one rail, character budgets
//! derived from the face that draws them — and the vertical rhythm was left as
//! bare numbers scattered through two renderers: `17`, `28`, `75`, `86`, `89`,
//! `108`. That is the cause the panel's other defects were symptoms of. A band
//! could drift without anything making it visible in review, the two families
//! placed the same fact in different rows, and moving one band meant
//! re-deriving its neighbours by hand.
//!
//! Rows are named for what they carry, not for where they sit, and each is
//! derived from the one above it. Reading the constants top to bottom is
//! reading the panel top to bottom.
//!
//! The same argument now runs sideways. A second board arrived, and every
//! number tuned to the first one would have had to be tuned again — so
//! positions come from [`PanelGrid`], which measures from the panel's own
//! edges. The E213 numbers below are what those derivations produce for it.

use embedded_graphics::pixelcolor::BinaryColor;

use crate::{
    MonoFrame,
    panel::{MARGIN_LEFT, TAPE_HEIGHT},
    render_primitives::{LABEL_GLYPH_WIDTH, fill, fit, label, text_width},
};

/// Draws the bottom tape: how trustworthy on the left, how old on the right.
///
/// Shared because the two families used to disagree about where this fact
/// lives — the bridge panel announced staleness at top right and only when
/// stale, the channel card stated freshness at bottom left and always. They
/// rotate on the same screen seconds apart, so a reader was looking in two
/// places for one thing.
pub(crate) fn draw_tape(frame: &mut MonoFrame, state: &str, age: &str) {
    let grid = frame.grid();
    let top = grid.tape_top();
    // Baseline of the tape's text, optically centred in the band.
    let baseline = top + 1;
    fill(
        frame,
        0,
        top,
        grid.content_width(),
        TAPE_HEIGHT,
        BinaryColor::On,
    );
    let age = fit(age, 12);
    let right_x = grid.content_right() - text_width(&age, LABEL_GLYPH_WIDTH);
    let state_width =
        usize::try_from(((right_x - MARGIN_LEFT - 6).max(LABEL_GLYPH_WIDTH)) / LABEL_GLYPH_WIDTH)
            .unwrap_or(1);
    label(
        frame,
        MARGIN_LEFT,
        baseline,
        &fit(state, state_width),
        BinaryColor::Off,
    );
    if !age.is_empty() {
        label(frame, right_x, baseline, &age, BinaryColor::Off);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PanelModel;

    /// The tape sits on the panel's bottom edge, whatever the panel height is.
    #[test]
    fn the_tape_reaches_the_bottom_edge_exactly() {
        for model in PanelModel::ALL {
            let grid = model.grid();
            assert_eq!(grid.tape_top() + TAPE_HEIGHT as i32, i32::from(grid.height));
        }
    }

    /// Both halves have to land inside the content area, clear of the rail.
    ///
    /// Scanned within the tape's own width: the band stops at the rail, so
    /// anything past it is simply undrawn and would read as text to a check
    /// that looked for pale pixels across the whole row.
    #[test]
    fn a_long_age_still_leaves_the_state_word_room_and_clears_the_rail() {
        for model in PanelModel::ALL {
            let grid = model.grid();
            let mut frame = MonoFrame::white(model);
            draw_tape(&mut frame, "UNAVAILABLE", "NO READING");
            let band = u16::try_from(grid.content_width()).unwrap_or(grid.width);
            let rightmost_text = (0..band)
                .rfind(|x| {
                    (grid.tape_top()..i32::from(grid.height))
                        .any(|y| !frame.is_black(*x, u16::try_from(y).unwrap_or(0)))
                })
                .expect("the tape draws text");
            assert!(
                i32::from(rightmost_text) < grid.content_right(),
                "{model:?}: tape text reached x{rightmost_text}, past {}",
                grid.content_right()
            );
        }
    }

    /// A card with nothing to say about age still draws a coherent tape rather
    /// than a stray dash.
    #[test]
    fn an_absent_age_leaves_the_right_half_empty() {
        for model in PanelModel::ALL {
            let mut with = MonoFrame::white(model);
            draw_tape(&mut with, "UNAVAILABLE", "");
            let mut without = MonoFrame::white(model);
            draw_tape(&mut without, "UNAVAILABLE", "42S");
            assert_ne!(with.packed(), without.packed());
        }
    }

    /// The wider panel spends its width on the state word rather than on a
    /// wider gap between the two halves.
    #[test]
    fn the_wider_panel_gives_the_state_word_more_characters() {
        // Long enough to exhaust the smaller panel's budget; a string both
        // panels can already fit would prove nothing.
        const STATE: &str = "SOURCE UNAVAILABLE / COLLECTOR PARKED / NOTHING RETAINED";
        let mut small = MonoFrame::white(PanelModel::E213);
        draw_tape(&mut small, STATE, "15M");
        let mut large = MonoFrame::white(PanelModel::E290);
        draw_tape(&mut large, STATE, "15M");
        let ink = |frame: &MonoFrame| {
            let grid = frame.grid();
            (0..u16::try_from(grid.content_right()).unwrap_or(0))
                .filter(|x| {
                    (grid.tape_top()..i32::from(grid.height))
                        .any(|y| !frame.is_black(*x, u16::try_from(y).unwrap_or(0)))
                })
                .count()
        };
        assert!(
            ink(&large) > ink(&small),
            "the E290 tape must carry more of the sentence, not more air"
        );
    }
}
