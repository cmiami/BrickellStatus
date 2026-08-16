use core::convert::Infallible;

use embedded_graphics::{
    Drawable,
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_6X10, FONT_8X13_BOLD, FONT_10X20},
    },
    pixelcolor::BinaryColor,
    prelude::{Point, Primitive},
    primitives::{Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
    text::{Baseline, Text},
};

use crate::{MonoFrame, channel::display_ascii};

/// Mark left where copy was cut.
///
/// Not a full stop, which is what this used to append. On a life-safety card a
/// truncation the reader cannot see is a correctness problem: "UNTIL 6:15 PM FOR
/// DOWNTOWN MIAMI AND." reads as a finished sentence and quietly drops the rest
/// of the warning. A character that cannot end a sentence says "there was more"
/// without ever being mistaken for punctuation.
pub(crate) const TRUNCATION_MARK: char = '>';

pub(crate) fn fit(value: &str, characters: usize) -> String {
    let value = display_ascii(value);
    if value.chars().count() <= characters {
        return value;
    }
    let mut fitted: String = value.chars().take(characters.saturating_sub(1)).collect();
    // Trailing space before the mark reads as a gap rather than a cut.
    while fitted.ends_with(' ') {
        fitted.pop();
    }
    fitted.push(TRUNCATION_MARK);
    fitted
}

pub(crate) fn wrap(value: &str, characters: usize, maximum_lines: usize) -> Vec<String> {
    let mut remaining = display_ascii(value);
    let mut lines = Vec::with_capacity(maximum_lines);
    while !remaining.is_empty() && lines.len() < maximum_lines {
        if remaining.len() <= characters {
            lines.push(remaining);
            break;
        }
        if lines.len() + 1 == maximum_lines {
            lines.push(fit(&remaining, characters));
            break;
        }
        let split = remaining[..characters]
            .rfind(' ')
            .filter(|split| *split > 0)
            .unwrap_or(characters);
        lines.push(remaining[..split].trim().to_owned());
        remaining = remaining[split..].trim().to_owned();
    }
    lines
}

pub(crate) fn text_width(value: &str, glyph_width: i32) -> i32 {
    i32::try_from(value.len()).unwrap_or(i32::MAX / glyph_width) * glyph_width
}

pub(crate) fn label(frame: &mut MonoFrame, x: i32, y: i32, value: &str, color: BinaryColor) {
    text(frame, x, y, value, MonoTextStyle::new(&FONT_6X10, color));
}

/// Advance width of the [`label`] face, for callers sizing a string to a box.
pub(crate) const LABEL_GLYPH_WIDTH: i32 = 6;

/// The emphatic small face: rail codes and the action line.
///
/// 8x13 rather than the 7x13 bold it replaced, which rendered W as H at panel
/// scale with no overprint involved — the offline card advised `CHECK NETHORK`.
/// Same height, so the vertical grid is unchanged; one pixel wider, so callers
/// budget [`STRONG_GLYPH_WIDTH`] per character.
pub(crate) fn strong(frame: &mut MonoFrame, x: i32, y: i32, value: &str, color: BinaryColor) {
    text(
        frame,
        x,
        y,
        value,
        MonoTextStyle::new(&FONT_8X13_BOLD, color),
    );
}

/// Advance width of the [`strong`] face, for callers sizing a string to a box.
pub(crate) const STRONG_GLYPH_WIDTH: i32 = 8;

/// Draws the largest available font with a faux-bold stroke.
///
/// embedded-graphics tops out at a 10x20 mono face, which is not enough weight
/// to carry a status across a room. Overprinting thickens every stem, which on a
/// one-bit panel is the difference between text you read and a state you
/// recognise.
///
/// The overprint is vertical only, and that is not a detail. Smearing sideways
/// closes the one-pixel gaps *between* strokes, and in this face the gaps are
/// what distinguish W from H: the panel spent its largest type announcing
/// `WATCH` and drew `HATCH`. Smearing downward thickens the same stems without
/// touching the horizontal gaps, so the weight is bought and the letterforms
/// survive. Any change here has to be checked by rendering the state
/// vocabulary, not by reading the diff.
pub(crate) fn huge(frame: &mut MonoFrame, x: i32, y: i32, value: &str, color: BinaryColor) {
    for (offset_x, offset_y) in [(0, 0), (0, 1)] {
        large(frame, x + offset_x, y + offset_y, value, color);
    }
}

pub(crate) fn large(frame: &mut MonoFrame, x: i32, y: i32, value: &str, color: BinaryColor) {
    text(frame, x, y, value, MonoTextStyle::new(&FONT_10X20, color));
}

fn text(frame: &mut MonoFrame, x: i32, y: i32, value: &str, style: MonoTextStyle<'_, BinaryColor>) {
    infallible(Text::with_baseline(value, Point::new(x, y), style, Baseline::Top).draw(frame));
}

pub(crate) fn fill(
    frame: &mut MonoFrame,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: BinaryColor,
) {
    infallible(
        Rectangle::new(
            Point::new(x, y),
            embedded_graphics::geometry::Size::new(width, height),
        )
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(frame),
    );
}

pub(crate) fn outline(frame: &mut MonoFrame, x: i32, y: i32, width: u32, height: u32, stroke: u32) {
    let style = PrimitiveStyleBuilder::new()
        .stroke_color(BinaryColor::On)
        .stroke_width(stroke)
        .build();
    infallible(
        Rectangle::new(
            Point::new(x, y),
            embedded_graphics::geometry::Size::new(width, height),
        )
        .into_styled(style)
        .draw(frame),
    );
}

pub(crate) fn line(frame: &mut MonoFrame, x1: i32, y1: i32, x2: i32, y2: i32, color: BinaryColor) {
    infallible(
        Line::new(Point::new(x1, y1), Point::new(x2, y2))
            .into_styled(PrimitiveStyle::with_stroke(color, 1))
            .draw(frame),
    );
}

fn infallible<T>(result: Result<T, Infallible>) {
    match result {
        Ok(_) => {}
        Err(error) => match error {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ink laid down by one word, as a set of coordinates, so two words can be
    /// compared for how much of their form they actually share.
    fn glyph(
        draw: fn(&mut MonoFrame, i32, i32, &str, BinaryColor),
        value: &str,
    ) -> Vec<(u16, u16)> {
        let mut frame = MonoFrame::white();
        draw(&mut frame, 2, 2, value, BinaryColor::On);
        let mut set = Vec::new();
        for y in 0..crate::HEIGHT {
            for x in 0..crate::WIDTH {
                if frame.is_black(x, y) {
                    set.push((x, y));
                }
            }
        }
        set
    }

    /// Share of `a`'s ink that also appears in `b`. 1.0 means one letterform is
    /// wholly contained in the other, which for W and H means the reader has no
    /// way to tell them apart.
    fn overlap(a: &[(u16, u16)], b: &[(u16, u16)]) -> f32 {
        let common = a.iter().filter(|point| b.contains(point)).count();
        common as f32 / a.iter().len().max(1) as f32
    }

    /// The bug this exists to prevent: `WATCH` drawn as `HATCH`.
    ///
    /// A bolding pass that smears sideways fills the gap between W's inner
    /// strokes until every pixel of H lies inside W, leaving a reader no cue at
    /// all — and the state word is the largest, most consequential element the
    /// panel draws. Containment reaching 1.0 is the mechanically detectable
    /// form of this failure, and it is what the four-pass overprint produced.
    ///
    /// It is a floor, not a proof of legibility. A face can keep W and H
    /// formally distinct and still read alike at panel scale: the 7x13 bold this
    /// module used for `strong` measured a healthy 89% containment while
    /// visibly drawing `CHECK NETHORK`. Nothing here can replace rendering the
    /// vocabulary and looking at it; this only guarantees the glyphs never
    /// collapse into one another entirely.
    #[test]
    fn no_panel_face_lets_h_disappear_inside_w() {
        for (face, draw) in [
            (
                "state",
                huge as fn(&mut MonoFrame, i32, i32, &str, BinaryColor),
            ),
            ("emphatic", strong),
            ("label", label),
        ] {
            let shared = overlap(&glyph(draw, "H"), &glyph(draw, "W"));
            assert!(
                shared < 0.99,
                "in the {face} face H is {:.0}% contained in W, so the reader has \
                 no way to tell WATCH from HATCH",
                shared * 100.0
            );
        }
    }

    /// The action line has to fit inside its own box. A face change buys
    /// legibility with width, and the budget has to follow it — this is what
    /// stops a wider face from printing over its own border.
    #[test]
    fn the_strong_glyph_width_matches_the_face_it_describes() {
        let mut frame = MonoFrame::white();
        strong(&mut frame, 0, 0, "MM", BinaryColor::On);
        let rightmost = (0..crate::WIDTH)
            .rfind(|x| (0..crate::HEIGHT).any(|y| frame.is_black(*x, y)))
            .expect("two glyphs leave ink");
        assert!(
            i32::from(rightmost) < STRONG_GLYPH_WIDTH * 2,
            "two glyphs reached x{rightmost}, wider than the declared {STRONG_GLYPH_WIDTH}px advance"
        );
    }
}
