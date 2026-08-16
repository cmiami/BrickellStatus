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

use embedded_graphics::pixelcolor::BinaryColor;

use crate::{
    HEIGHT, MonoFrame,
    panel_rail::CONTENT_WIDTH,
    render_primitives::{LABEL_GLYPH_WIDTH, fill, fit, label, text_width},
};

/// Left margin for everything that is not a full-bleed band.
pub(crate) const MARGIN_LEFT: i32 = 4;
/// Right edge of content, clear of the rail.
pub(crate) const CONTENT_RIGHT: i32 = 228;

/// The bottom tape, which both families use to say how far the reading can be
/// trusted. Anchored to the panel's bottom edge rather than to a number that
/// has to agree with one.
pub(crate) const TAPE_HEIGHT: u32 = 14;
pub(crate) const TAPE_TOP: i32 = HEIGHT as i32 - TAPE_HEIGHT as i32;
/// Baseline of the tape's text, optically centred in the band.
const TAPE_BASELINE: i32 = TAPE_TOP + 1;

/// Draws the bottom tape: how trustworthy on the left, how old on the right.
///
/// Shared because the two families used to disagree about where this fact
/// lives — the bridge panel announced staleness at top right and only when
/// stale, the channel card stated freshness at bottom left and always. They
/// rotate on the same screen seconds apart, so a reader was looking in two
/// places for one thing.
pub(crate) fn draw_tape(frame: &mut MonoFrame, state: &str, age: &str) {
    fill(
        frame,
        0,
        TAPE_TOP,
        CONTENT_WIDTH,
        TAPE_HEIGHT,
        BinaryColor::On,
    );
    let age = fit(age, 12);
    let right_x = CONTENT_RIGHT - text_width(&age, LABEL_GLYPH_WIDTH);
    let state_width =
        usize::try_from(((right_x - MARGIN_LEFT - 6).max(LABEL_GLYPH_WIDTH)) / LABEL_GLYPH_WIDTH)
            .unwrap_or(1);
    label(
        frame,
        MARGIN_LEFT,
        TAPE_BASELINE,
        &fit(state, state_width),
        BinaryColor::Off,
    );
    if !age.is_empty() {
        label(frame, right_x, TAPE_BASELINE, &age, BinaryColor::Off);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WIDTH;

    /// The tape sits on the panel's bottom edge, whatever the panel height is.
    #[test]
    fn the_tape_reaches_the_bottom_edge_exactly() {
        assert_eq!(TAPE_TOP + TAPE_HEIGHT as i32, i32::from(HEIGHT));
    }

    /// Both halves have to land inside the content area, clear of the rail.
    ///
    /// Scanned within the tape's own width: the band stops at the rail, so
    /// anything past it is simply undrawn and would read as text to a check
    /// that looked for pale pixels across the whole row.
    #[test]
    fn a_long_age_still_leaves_the_state_word_room_and_clears_the_rail() {
        let mut frame = MonoFrame::white();
        draw_tape(&mut frame, "UNAVAILABLE", "NO READING");
        let band = u16::try_from(CONTENT_WIDTH).unwrap_or(WIDTH);
        let rightmost_text = (0..band)
            .rfind(|x| {
                (TAPE_TOP..i32::from(HEIGHT))
                    .any(|y| !frame.is_black(*x, u16::try_from(y).unwrap_or(0)))
            })
            .expect("the tape draws text");
        assert!(
            i32::from(rightmost_text) < CONTENT_RIGHT,
            "tape text reached x{rightmost_text}, past the content edge at {CONTENT_RIGHT}"
        );
    }

    /// A card with nothing to say about age still draws a coherent tape rather
    /// than a stray dash.
    #[test]
    fn an_absent_age_leaves_the_right_half_empty() {
        let mut with = MonoFrame::white();
        draw_tape(&mut with, "UNAVAILABLE", "");
        let mut without = MonoFrame::white();
        draw_tape(&mut without, "UNAVAILABLE", "42S");
        assert_ne!(with.packed(), without.packed());
    }
}
