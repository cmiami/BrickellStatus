use core::convert::Infallible;

use embedded_graphics::{
    Drawable,
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_6X10, FONT_7X13_BOLD, FONT_10X20},
    },
    pixelcolor::BinaryColor,
    prelude::{Point, Primitive},
    primitives::{Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
    text::{Baseline, Text},
};

use crate::{MonoFrame, channel::display_ascii};

pub(crate) fn fit(value: &str, characters: usize) -> String {
    let value = display_ascii(value);
    if value.chars().count() <= characters {
        return value;
    }
    value
        .chars()
        .take(characters.saturating_sub(1))
        .collect::<String>()
        + "."
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

pub(crate) fn strong(frame: &mut MonoFrame, x: i32, y: i32, value: &str, color: BinaryColor) {
    text(
        frame,
        x,
        y,
        value,
        MonoTextStyle::new(&FONT_7X13_BOLD, color),
    );
}

/// Draws the largest available font with a faux-bold stroke.
///
/// embedded-graphics tops out at a 10x20 mono face, which is not enough weight
/// to carry a status across a room. Overprinting at one-pixel offsets thickens
/// every stem, which on a one-bit panel is the difference between text you read
/// and a state you recognise.
pub(crate) fn huge(frame: &mut MonoFrame, x: i32, y: i32, value: &str, color: BinaryColor) {
    for (offset_x, offset_y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
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
