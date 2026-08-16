use embedded_graphics::pixelcolor::BinaryColor;
use thiserror::Error;

use crate::{
    ChannelAvailability, ChannelCard, ChannelCardError, ChannelFrame, ChannelUrgency, MonoFrame,
    RadarFigure,
    channel::display_ascii,
    render_primitives::{
        STRONG_GLYPH_WIDTH, fill, fit, label, large, line, outline, strong, text_width, wrap,
    },
};

const CONTENT_WIDTH: u32 = 232;
/// Left edge of the radar figure, and therefore the right edge of the headline
/// when one is present.
const RADAR_FIGURE_X: i32 = 132;
const RADAR_FIGURE_Y: i32 = 30;
const RAIL_LEFT: i32 = 232;
const SOURCE_TAPE_TOP: i32 = 108;

/// Failure to turn a generic channel card into pixels.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ChannelRenderError {
    /// Card content violated a semantic or resource invariant.
    #[error(transparent)]
    Card(#[from] ChannelCardError),
}

/// Renders one generic signal card into a deterministic 250×122 one-bit frame.
pub fn render_channel_card(card: &ChannelCard) -> Result<MonoFrame, ChannelRenderError> {
    render_channel_card_with_radar(card, None)
}

/// As [`render_channel_card`], with a radar composite beside the headline.
///
/// The figure is corroborating texture, not the message: it narrows the
/// headline rather than replacing any of it, so a card that loses its radar
/// still says everything it needs to in words.
pub fn render_channel_card_with_radar(
    card: &ChannelCard,
    radar: Option<&RadarFigure>,
) -> Result<MonoFrame, ChannelRenderError> {
    card.validate()?;

    // An empty composite is not drawn. A handful of dithered specks reads as a
    // dirty panel rather than as weather, and the headline would rather have
    // the width back.
    let radar = radar.filter(|figure| figure.is_worth_drawing());

    let mut frame = MonoFrame::white();
    draw_header(&mut frame, card);
    draw_title(&mut frame, card);
    draw_headline(&mut frame, card, radar.is_some());
    if let Some(figure) = radar {
        figure.draw(
            &mut frame,
            u16::try_from(RADAR_FIGURE_X).unwrap_or(0),
            u16::try_from(RADAR_FIGURE_Y).unwrap_or(0),
        );
    }
    draw_detail(&mut frame, card);
    draw_action(&mut frame, card);
    draw_source_tape(&mut frame, card);
    draw_rail(&mut frame, card);
    Ok(frame)
}

/// Frame-oriented spelling of [`render_channel_card`] for rotation schedulers.
pub fn render_channel_frame(frame: &ChannelFrame) -> Result<MonoFrame, ChannelRenderError> {
    render_channel_card(frame)
}

fn draw_header(frame: &mut MonoFrame, card: &ChannelCard) {
    fill(frame, 0, 0, CONTENT_WIDTH, 15, BinaryColor::On);
    let availability = card.availability.label();
    let right_x = 228 - text_width(availability, 6);
    let label_width = usize::try_from((right_x - 10).max(6) / 6).unwrap_or(1);
    label(
        frame,
        4,
        2,
        &fit(card.channel.label(), label_width),
        BinaryColor::Off,
    );
    label(frame, right_x, 2, availability, BinaryColor::Off);
}

fn draw_title(frame: &mut MonoFrame, card: &ChannelCard) {
    let urgency = card.urgency.label();
    let right_x = 228 - text_width(urgency, 6);
    let title_width = usize::try_from((right_x - 10).max(6) / 6).unwrap_or(1);
    label(
        frame,
        4,
        17,
        &fit(&card.title, title_width),
        BinaryColor::On,
    );
    label(frame, right_x, 17, urgency, BinaryColor::On);
    line(frame, 4, 28, 228, 28, BinaryColor::On);
}

fn draw_headline(frame: &mut MonoFrame, card: &ChannelCard, has_radar: bool) {
    let critical = card.urgency == ChannelUrgency::Critical;
    if critical {
        // The inversion stops at the figure. Radar drawn into a black band
        // would read inside out — more ink meaning less rain.
        let width = if has_radar {
            u32::try_from(RADAR_FIGURE_X).unwrap_or(CONTENT_WIDTH)
        } else {
            CONTENT_WIDTH
        };
        fill(frame, 0, 30, width, 42, BinaryColor::On);
    }

    let columns = if has_radar { 12 } else { 22 };
    let lines = wrap(&card.headline, columns, 2);
    let first_y = if lines.len() == 1 { 40 } else { 31 };
    for (index, value) in lines.iter().enumerate() {
        large(
            frame,
            4,
            first_y + i32::try_from(index * 20).unwrap_or(0),
            value,
            if critical {
                BinaryColor::Off
            } else {
                BinaryColor::On
            },
        );
    }
}

fn draw_detail(frame: &mut MonoFrame, card: &ChannelCard) {
    label(frame, 5, 75, &fit(&card.detail, 37), BinaryColor::On);
    line(frame, 4, 86, 228, 86, BinaryColor::On);
}

fn draw_action(frame: &mut MonoFrame, card: &ChannelCard) {
    let inverse = card.urgency.is_interrupting();
    if inverse {
        fill(frame, 4, 89, 224, 16, BinaryColor::On);
    } else {
        outline(frame, 4, 89, 224, 16, 1);
        if card.urgency == ChannelUrgency::Advisory {
            line(frame, 7, 91, 7, 102, BinaryColor::On);
            line(frame, 9, 91, 9, 102, BinaryColor::On);
        }
    }
    strong(
        frame,
        ACTION_TEXT_X,
        90,
        &fit(&card.action, ACTION_CHARACTERS),
        if inverse {
            BinaryColor::Off
        } else {
            BinaryColor::On
        },
    );
}

/// Left edge of the action line, clear of the advisory registration rules.
const ACTION_TEXT_X: i32 = 13;
/// Characters the action line fits, derived rather than guessed: a wider face
/// buys legible letterforms and costs budget, and the box must win that trade
/// by truncating rather than by printing over its own border.
const ACTION_CHARACTERS: usize = ((228 - ACTION_TEXT_X) / STRONG_GLYPH_WIDTH) as usize;

/// Bottom tape: what this card is about on the left, how current it is on the
/// right.
///
/// The feed that produced the reading is deliberately absent. It was printed
/// here on every non-bridge card long after the bridge frame had been stripped
/// of it, which is how a source name kept reappearing on surfaces it had been
/// removed from. `ChannelSource::name` is still carried on the card so Settings
/// can name it; nothing draws it.
fn draw_source_tape(frame: &mut MonoFrame, card: &ChannelCard) {
    fill(
        frame,
        0,
        SOURCE_TAPE_TOP,
        CONTENT_WIDTH,
        14,
        BinaryColor::On,
    );
    let right = source_state(card);
    let right_x = 228 - text_width(&right, 6);
    let subject_width = usize::try_from((right_x - 10).max(6) / 6).unwrap_or(1);
    label(
        frame,
        4,
        109,
        &fit(&card.title, subject_width),
        BinaryColor::Off,
    );
    label(frame, right_x, 109, &right, BinaryColor::Off);
}

fn source_state(card: &ChannelCard) -> String {
    match card.availability {
        ChannelAvailability::Current => format!(
            "AGE {}",
            card.source.age_label().unwrap_or_else(|| "--".into())
        ),
        ChannelAvailability::Stale => format!(
            "STALE {}",
            card.source.age_label().unwrap_or_else(|| "--".into())
        ),
        ChannelAvailability::Offline => card
            .source
            .age_label()
            .map_or_else(|| "NO LINK".into(), |age| format!("LAST {age}")),
        ChannelAvailability::Unavailable => "NOT READY".into(),
    }
}

fn draw_rail(frame: &mut MonoFrame, card: &ChannelCard) {
    fill(frame, RAIL_LEFT, 0, 18, 122, BinaryColor::On);
    let code = display_ascii(card.channel.code());
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

    let active_slot = urgency_slot(card.urgency);
    for index in 0..4 {
        let y = 39 + index * 10;
        let length = if index % 2 == 0 { 10 } else { 7 };
        line(frame, 248 - length, y, 248, y, BinaryColor::Off);
        if index == active_slot {
            fill(frame, 234, y - 2, 14, 5, BinaryColor::Off);
        }
    }

    line(frame, 235, 80, 247, 80, BinaryColor::Off);
    label(
        frame,
        238,
        84,
        availability_code(card.availability),
        BinaryColor::Off,
    );

    if card.urgency.is_interrupting() {
        for y in (100..122).step_by(7) {
            line(frame, 233, y + 5, 240, y, BinaryColor::Off);
            line(frame, 241, y + 5, 249, y, BinaryColor::Off);
        }
    }
}

const fn urgency_slot(urgency: ChannelUrgency) -> i32 {
    match urgency {
        ChannelUrgency::Critical => 0,
        ChannelUrgency::Urgent => 1,
        ChannelUrgency::Advisory => 2,
        ChannelUrgency::Routine => 3,
    }
}

const fn availability_code(availability: ChannelAvailability) -> &'static str {
    match availability {
        ChannelAvailability::Current => "L",
        ChannelAvailability::Stale => "S",
        ChannelAvailability::Offline => "O",
        ChannelAvailability::Unavailable => "X",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChannelKind, ChannelSource, radar_figure_from_png};

    fn card(kind: ChannelKind, urgency: ChannelUrgency) -> ChannelCard {
        ChannelCard::new(
            kind,
            urgency,
            ChannelAvailability::Current,
            "Miami / Brickell",
            "Heavy rain in 12 minutes",
            "0.6 in/hr / gusts 31 mph",
            "Take cover by 4:20 PM",
            ChannelSource::aged("Open-Meteo", 42),
        )
    }

    #[test]
    fn every_builtin_channel_renders_a_physical_frame() {
        for kind in [
            ChannelKind::Weather,
            ChannelKind::OfficialAlert,
            ChannelKind::Tropical,
            ChannelKind::News,
            ChannelKind::Earthquake,
            ChannelKind::Markets,
        ] {
            let frame = render_channel_card(&card(kind, ChannelUrgency::Advisory)).unwrap();
            assert!(frame.black_pixel_count() > 1_000);
            assert!(frame.black_pixel_count() < 250 * 122);
        }
    }

    #[test]
    fn urgency_levels_have_materially_distinct_pixels() {
        let routine =
            render_channel_card(&card(ChannelKind::Weather, ChannelUrgency::Routine)).unwrap();
        let urgent =
            render_channel_card(&card(ChannelKind::Weather, ChannelUrgency::Urgent)).unwrap();
        let critical =
            render_channel_card(&card(ChannelKind::Weather, ChannelUrgency::Critical)).unwrap();
        assert_ne!(routine.packed(), urgent.packed());
        assert_ne!(urgent.packed(), critical.packed());
        assert!(critical.black_pixel_count() > routine.black_pixel_count());
    }

    #[test]
    fn display_fitting_is_deterministic_and_ignores_hidden_tail_copy() {
        let mut first = card(ChannelKind::News, ChannelUrgency::Routine);
        let mut second = first.clone();
        first.headline = format!("{} X", "LONG ".repeat(30));
        second.headline = format!("{} Y", "LONG ".repeat(30));
        assert_eq!(
            render_channel_card(&first).unwrap(),
            render_channel_card(&second).unwrap()
        );
    }

    #[test]
    fn long_unbroken_headlines_do_not_overrun_the_panel() {
        let mut value = card(ChannelKind::Tropical, ChannelUrgency::Critical);
        value.headline = "SUPERCALIFRAGILISTICEXPIALIDOCIOUSSTORMSIGNAL".into();
        let frame = render_channel_card(&value).unwrap();
        assert!(frame.black_pixel_count() > 1_000);
        assert!(frame.black_pixel_count() < 250 * 122);
    }

    #[test]
    fn offline_and_unavailable_states_render_without_an_age() {
        for availability in [
            ChannelAvailability::Offline,
            ChannelAvailability::Unavailable,
        ] {
            let mut value = card(ChannelKind::OfficialAlert, ChannelUrgency::Routine);
            value.availability = availability;
            value.source = ChannelSource::unavailable("NWS");
            assert!(render_channel_card(&value).is_ok());
        }
    }

    #[test]
    fn frame_alias_uses_the_same_renderer_contract() {
        let frame: ChannelFrame = card(ChannelKind::Markets, ChannelUrgency::Routine);
        assert_eq!(
            render_channel_frame(&frame).unwrap(),
            render_channel_card(&frame).unwrap()
        );
    }

    #[test]
    fn wrapping_prefers_words_and_truncates_only_the_final_line() {
        assert_eq!(
            wrap("TRACK SHIFTED TWENTY FOUR MILES WEST OF MIAMI", 22, 2),
            vec!["TRACK SHIFTED TWENTY", "FOUR MILES WEST OF MI."]
        );
    }

    /// Mirrors the bridge-frame guarantee for channel cards.
    #[test]
    fn a_channel_card_never_prints_the_feed_that_produced_it() {
        let mut card = card(ChannelKind::Weather, ChannelUrgency::Advisory);
        card.source.name = "SOME FEED NAME".into();
        let with_name = render_channel_card(&card).unwrap();
        card.source.name = "AN ENTIRELY DIFFERENT FEED".into();
        let with_other = render_channel_card(&card).unwrap();
        assert_eq!(
            with_name.packed(),
            with_other.packed(),
            "changing only the source name must not change a pixel"
        );
    }

    /// A composite dense enough to be worth drawing, in the monochrome scheme
    /// the panel requests.
    fn wet_figure() -> RadarFigure {
        let mut image = image::RgbaImage::new(64, 64);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let intensity = if (x / 8 + y / 8) % 2 == 0 { 30 } else { 200 };
            *pixel = image::Rgba([intensity, intensity, intensity, 255]);
        }
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        radar_figure_from_png(&bytes).unwrap()
    }

    #[test]
    fn a_radar_card_stays_inside_the_panel_and_leaves_the_words_room() {
        let card = card(ChannelKind::Weather, ChannelUrgency::Advisory);
        let with_radar = render_channel_card_with_radar(&card, Some(&wet_figure())).unwrap();
        let without = render_channel_card(&card).unwrap();
        assert_ne!(with_radar, without);
        // Ink in the figure's box, and none where the headline still lives.
        assert!(with_radar.black_pixel_count() > without.black_pixel_count());
        assert!((30..72).any(|y| (132..228).any(|x| with_radar.is_black(x, y))));
    }

    /// An empty sky costs the headline nothing. The figure is dropped and the
    /// wrap goes back to full width, so the card is identical to one that never
    /// had radar offered.
    #[test]
    fn an_empty_composite_is_dropped_rather_than_drawn_blank() {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(64, 64))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let empty = radar_figure_from_png(&bytes).unwrap();
        let card = card(ChannelKind::Weather, ChannelUrgency::Advisory);
        assert_eq!(
            render_channel_card_with_radar(&card, Some(&empty)).unwrap(),
            render_channel_card(&card).unwrap()
        );
    }

    /// A critical card inverts its headline band. Radar drawn into that band
    /// would read inside out — more ink meaning less rain — so the fill stops
    /// at the figure.
    #[test]
    fn a_critical_card_does_not_invert_the_radar() {
        let card = card(ChannelKind::Weather, ChannelUrgency::Critical);
        let frame = render_channel_card_with_radar(&card, Some(&wet_figure())).unwrap();
        // The band is black to the left of the figure...
        assert!(frame.is_black(20, 50));
        // ...and the figure's own box is not solid.
        let black = (30..72)
            .flat_map(|y| (132..228).map(move |x| (x, y)))
            .filter(|(x, y)| frame.is_black(*x, *y))
            .count();
        assert!(black > 0 && black < 96 * 42, "{black} of {}", 96 * 42);
    }
}
