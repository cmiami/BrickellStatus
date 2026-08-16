use embedded_graphics::pixelcolor::BinaryColor;
use thiserror::Error;

use crate::{
    ChannelAvailability, ChannelCard, ChannelCardError, ChannelFrame, ChannelUrgency, MonoFrame,
    RadarFigure,
    channel::display_ascii,
    panel_grid::{self, CONTENT_RIGHT, MARGIN_LEFT},
    panel_rail::{self, CONTENT_WIDTH},
    render_primitives::{
        LABEL_GLYPH_WIDTH, STRONG_GLYPH_WIDTH, fill, fit, label, large, line, outline, strong,
        text_width, wrap,
    },
};

/// The card's vertical grid, top to bottom. Each row is derived from the one
/// above it, so moving a band moves what follows rather than colliding with it,
/// and a drifted band is visible here rather than buried in a call.
const HEADER_HEIGHT: u32 = 15;
const TITLE_BASELINE: i32 = HEADER_HEIGHT as i32 + 2;
const TITLE_RULE_Y: i32 = TITLE_BASELINE + 11;
const HEADLINE_TOP: i32 = TITLE_RULE_Y + 2;
const HEADLINE_HEIGHT: u32 = 42;
/// Leading for the display face in the headline band, and for the emphatic face
/// when a figure has taken the width.
const HEADLINE_LEADING: i32 = 20;
const FIGURE_HEADLINE_LEADING: i32 = 14;
const DETAIL_BASELINE: i32 = HEADLINE_TOP + HEADLINE_HEIGHT as i32 + 3;
const DETAIL_RULE_Y: i32 = DETAIL_BASELINE + 11;
const ACTION_TOP: i32 = DETAIL_RULE_Y + 3;
const ACTION_HEIGHT: u32 = 16;

/// Left edge of the radar figure, and therefore the right edge of the headline
/// when one is present.
const RADAR_FIGURE_X: i32 = 132;
const RADAR_FIGURE_Y: i32 = HEADLINE_TOP;

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

/// Identity strip: which channel, and how loudly it is speaking.
///
/// Urgency sits here rather than beside the title because the title row now
/// carries the subject alone. The card used to print that subject twice — once
/// here and again in the tape along the bottom — on a panel with roughly six
/// usable rows.
fn draw_header(frame: &mut MonoFrame, card: &ChannelCard) {
    fill(frame, 0, 0, CONTENT_WIDTH, HEADER_HEIGHT, BinaryColor::On);
    let urgency = card.urgency.label();
    let right_x = CONTENT_RIGHT - text_width(urgency, LABEL_GLYPH_WIDTH);
    let channel_width = characters_between(MARGIN_LEFT, right_x);
    label(
        frame,
        MARGIN_LEFT,
        2,
        &fit(card.channel.label(), channel_width),
        BinaryColor::Off,
    );
    label(frame, right_x, 2, urgency, BinaryColor::Off);
}

/// The subject, with the whole row to itself.
fn draw_title(frame: &mut MonoFrame, card: &ChannelCard) {
    label(
        frame,
        MARGIN_LEFT,
        TITLE_BASELINE,
        &fit(&card.title, characters_between(MARGIN_LEFT, CONTENT_RIGHT)),
        BinaryColor::On,
    );
    line(
        frame,
        MARGIN_LEFT,
        TITLE_RULE_Y,
        CONTENT_RIGHT,
        TITLE_RULE_Y,
        BinaryColor::On,
    );
}

/// Characters of the label face that fit between two x positions.
fn characters_between(left: i32, right: i32) -> usize {
    usize::try_from(((right - left - 4).max(LABEL_GLYPH_WIDTH)) / LABEL_GLYPH_WIDTH).unwrap_or(1)
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
        fill(
            frame,
            0,
            HEADLINE_TOP,
            width,
            HEADLINE_HEIGHT,
            BinaryColor::On,
        );
    }

    let ink = if critical {
        BinaryColor::Off
    } else {
        BinaryColor::On
    };

    // A figure takes most of the width, and the headline has to survive losing
    // it. At the display face that left twelve characters a line, which cut the
    // message to make room for the picture corroborating it — "Heavy rain in 12
    // minutes" reached the panel as "IN 12 MINUT>". The emphatic face is two
    // pixels narrower and four shorter, which buys fifteen characters across
    // and a third line down: forty-five against twenty-four, for type that is
    // still bold and still legible at distance.
    if has_radar {
        let lines = wrap(
            &card.headline,
            FIGURE_HEADLINE_COLUMNS,
            FIGURE_HEADLINE_LINES,
        );
        for (index, value) in lines.iter().enumerate() {
            strong(
                frame,
                MARGIN_LEFT,
                HEADLINE_TOP + 1 + i32::try_from(index).unwrap_or(0) * FIGURE_HEADLINE_LEADING,
                value,
                ink,
            );
        }
        return;
    }

    let lines = wrap(&card.headline, 22, 2);
    let first_y = if lines.len() == 1 {
        HEADLINE_TOP + 10
    } else {
        HEADLINE_TOP + 1
    };
    for (index, value) in lines.iter().enumerate() {
        large(
            frame,
            MARGIN_LEFT,
            first_y + i32::try_from(index).unwrap_or(0) * HEADLINE_LEADING,
            value,
            ink,
        );
    }
}

/// Headline budget on a card carrying a figure, derived from the gap the figure
/// leaves and the face that fills it.
const FIGURE_HEADLINE_COLUMNS: usize =
    ((RADAR_FIGURE_X - MARGIN_LEFT - 4) / STRONG_GLYPH_WIDTH) as usize;
/// Three rows of the emphatic face fit the headline band exactly.
const FIGURE_HEADLINE_LINES: usize = 3;

fn draw_detail(frame: &mut MonoFrame, card: &ChannelCard) {
    label(
        frame,
        DETAIL_TEXT_X,
        DETAIL_BASELINE,
        &fit(&card.detail, DETAIL_CHARACTERS),
        BinaryColor::On,
    );
    line(
        frame,
        MARGIN_LEFT,
        DETAIL_RULE_Y,
        CONTENT_RIGHT,
        DETAIL_RULE_Y,
        BinaryColor::On,
    );
}

/// The action line, and the panel's emphasis ration.
///
/// Inversion is the only emphasis a one-bit panel has, and exactly one content
/// band may spend it: which one is itself the signal. Below critical the action
/// takes it, because the useful thing is what to do. At critical the headline
/// takes it instead, because the useful thing is what is happening — and the
/// action falls back to a heavy rule rather than competing. A critical card used
/// to invert its header, its headline, its action and its tape, four bands out
/// of six, which left the most urgent card on the panel with the least internal
/// hierarchy.
fn draw_action(frame: &mut MonoFrame, card: &ChannelCard) {
    let headline_owns_emphasis = card.urgency == ChannelUrgency::Critical;
    let inverse = card.urgency.is_interrupting() && !headline_owns_emphasis;
    if inverse {
        fill(
            frame,
            MARGIN_LEFT,
            ACTION_TOP,
            ACTION_WIDTH,
            ACTION_HEIGHT,
            BinaryColor::On,
        );
    } else {
        outline(
            frame,
            MARGIN_LEFT,
            ACTION_TOP,
            ACTION_WIDTH,
            ACTION_HEIGHT,
            if headline_owns_emphasis { 2 } else { 1 },
        );
        if card.urgency == ChannelUrgency::Advisory {
            // One solid registration edge, which is the system's own mark for a
            // state. The two hairlines this replaces read at panel scale as a
            // stray artefact or a pause glyph rather than as advisory.
            fill(
                frame,
                MARGIN_LEFT,
                ACTION_TOP,
                3,
                ACTION_HEIGHT,
                BinaryColor::On,
            );
        }
    }
    strong(
        frame,
        ACTION_TEXT_X,
        ACTION_TOP + 1,
        &fit(&card.action, ACTION_CHARACTERS),
        if inverse {
            BinaryColor::Off
        } else {
            BinaryColor::On
        },
    );
}

/// The detail line runs the full width, so its budget comes straight from the
/// gap rather than from `characters_between`, which reserves a trailing column
/// for a right-aligned value this row does not have.
const DETAIL_TEXT_X: i32 = MARGIN_LEFT + 1;
const DETAIL_CHARACTERS: usize = ((CONTENT_RIGHT - DETAIL_TEXT_X) / LABEL_GLYPH_WIDTH) as usize;

/// Width of the action box, from the left margin to the content edge.
const ACTION_WIDTH: u32 = (CONTENT_RIGHT - MARGIN_LEFT) as u32;
/// Left edge of the action line, clear of the advisory registration rule.
const ACTION_TEXT_X: i32 = MARGIN_LEFT + 9;
/// Characters the action line fits, derived rather than guessed: a wider face
/// buys legible letterforms and costs budget, and the box must win that trade
/// by truncating rather than by printing over its own border.
const ACTION_CHARACTERS: usize = ((CONTENT_RIGHT - ACTION_TEXT_X) / STRONG_GLYPH_WIDTH) as usize;

/// Bottom tape: whether this reading can be trusted, and how old it is.
///
/// The tape owns freshness outright. It used to repeat the title on the left
/// while the header carried the availability word and the rail carried a
/// one-letter code for the same thing — the subject printed twice, the freshness
/// three times, on a panel where every row is contested.
///
/// The feed that produced the reading is deliberately absent. It was printed
/// here on every non-bridge card long after the bridge frame had been stripped
/// of it, which is how a source name kept reappearing on surfaces it had been
/// removed from. `ChannelSource::name` is still carried on the card so Settings
/// can name it; nothing draws it.
fn draw_source_tape(frame: &mut MonoFrame, card: &ChannelCard) {
    panel_grid::draw_tape(frame, card.availability.label(), &source_age(card));
}

/// How old the reading is, in the fewest characters that stay honest.
fn source_age(card: &ChannelCard) -> String {
    match (card.availability, card.source.age_label()) {
        // Nothing has ever been collected, so there is no age to report and a
        // dash would imply a reading that briefly existed.
        (ChannelAvailability::Unavailable, _) => String::new(),
        (_, Some(age)) => age,
        (_, None) => "NO READING".into(),
    }
}

/// The shared rail. The availability letter it used to carry is gone: the tape
/// says the same thing in a word, and a lone `X` explained nothing.
fn draw_rail(frame: &mut MonoFrame, card: &ChannelCard) {
    panel_rail::draw_rail(
        frame,
        &display_ascii(card.channel.code()),
        urgency_slot(card.urgency),
        URGENCY_SLOTS,
        card.urgency.is_interrupting(),
    );
}

/// Rungs on the card's ladder, one per urgency, loudest at the top.
const URGENCY_SLOTS: usize = 4;

const fn urgency_slot(urgency: ChannelUrgency) -> usize {
    match urgency {
        ChannelUrgency::Critical => 0,
        ChannelUrgency::Urgent => 1,
        ChannelUrgency::Advisory => 2,
        ChannelUrgency::Routine => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_primitives::TRUNCATION_MARK;
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

    /// The grid has to stay a grid. Every band is derived from the one above
    /// it, and the point of that is that a change which would overlap two rows
    /// fails here rather than shipping as text printed over a rule.
    #[test]
    fn no_band_overlaps_the_one_below_it() {
        let rows: [(&str, i32, i32); 5] = [
            ("header", 0, HEADER_HEIGHT as i32),
            ("title", TITLE_BASELINE, TITLE_RULE_Y),
            (
                "headline",
                HEADLINE_TOP,
                HEADLINE_TOP + HEADLINE_HEIGHT as i32,
            ),
            ("detail", DETAIL_BASELINE, DETAIL_RULE_Y),
            ("action", ACTION_TOP, ACTION_TOP + ACTION_HEIGHT as i32),
        ];
        for pair in rows.windows(2) {
            let (name, _, ends) = pair[0];
            let (next, starts, _) = pair[1];
            assert!(
                ends <= starts,
                "{name} ends at {ends} but {next} starts at {starts}"
            );
        }
        let (_, _, action_ends) = rows[4];
        assert!(
            action_ends <= panel_grid::TAPE_TOP,
            "the action box runs into the tape at {}",
            panel_grid::TAPE_TOP
        );
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
            vec!["TRACK SHIFTED TWENTY", "FOUR MILES WEST OF MI>"]
        );
    }

    /// Copy that was cut has to look cut. The mark used to be a full stop, so
    /// "UNTIL 6:15 PM FOR DOWNTOWN MIAMI AND." read as a finished sentence and
    /// silently dropped the rest of a life-safety warning.
    #[test]
    fn truncated_copy_cannot_be_mistaken_for_a_finished_sentence() {
        let cut = fit("MOVE TO HIGHER GROUND IMMEDIATELY AND DO NOT DRIVE", 26);
        assert!(
            !cut.ends_with('.'),
            "{cut:?} ends in punctuation that reads as the end of the sentence"
        );
        assert!(cut.ends_with(TRUNCATION_MARK));
        assert_eq!(cut.chars().count(), 26);
    }

    /// ...and the mark sits against the last word rather than after a gap.
    #[test]
    fn the_truncation_mark_does_not_float_off_the_last_word() {
        assert_eq!(fit("MOVE TO HIGHER GROUND", 9), "MOVE TO>");
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
