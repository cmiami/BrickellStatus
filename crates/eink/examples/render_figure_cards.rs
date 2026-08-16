//! Renders the cards that carry a figure — markets and weather radar.
//!
//! These are the two panels whose whole point is the picture beside the words,
//! and the state sheets draw them without one because a fixture cannot fetch a
//! radar tile. Both are synthesised here from data shaped like the real thing.

use std::path::PathBuf;

use bridgestatus_eink::{
    ChannelAvailability, ChannelCard, ChannelKind, ChannelSource, ChannelUrgency, HEIGHT,
    MonoFrame, RADAR_FIGURE_HEIGHT, RADAR_FIGURE_WIDTH, RadarFigure, WIDTH, radar_figure_from_png,
    render_channel_card_with_radar, series_figure,
};
use image::{GrayImage, Luma};

const SCALE: u32 = 3;
const GUTTER: u32 = 10;
const COLUMNS: u32 = 2;

/// A radar composite shaped like weather: a band of rain crossing from the
/// north-west, heavier at its core, over an otherwise clear sky.
fn synthetic_radar_tile(offset_x: f32, offset_y: f32, intensity: f32) -> Vec<u8> {
    let side = 256u32;
    let mut image = image::RgbaImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let fx = x as f32 / side as f32 - offset_x;
            let fy = y as f32 / side as f32 - offset_y;
            // A band at 45 degrees, so the shape is directional and any aspect
            // distortion in the pipeline shows up as a change of angle.
            let diagonal = std::f32::consts::FRAC_1_SQRT_2;
            let along = fx * diagonal + fy * diagonal;
            let across = -fx * diagonal + fy * diagonal;
            let band = (-(across * across) / 0.010).exp();
            let core = (-(along * along + across * across) / 0.006).exp();
            let value = (band * 0.55 + core * 0.8) * intensity;
            let ink = (255.0 - value * 255.0).clamp(0.0, 255.0) as u8;
            let alpha = if value > 0.02 { 255 } else { 0 };
            image.put_pixel(x, y, image::Rgba([ink, ink, ink, alpha]));
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("synthetic tile encodes");
    bytes.into_inner()
}

/// An intraday session at `base`, shaped like the day the card describes.
///
/// The scale matters: the reference the figure plots is the previous close, and
/// a series in the hundreds paired with a close in the thousands widens the
/// plotted range until the price line flattens against an edge. Real quotes are
/// always the same instrument; a fixture has to be too.
fn intraday(shape: &str, base: f64) -> Vec<f64> {
    let points = 78;
    let swing = base * 0.03;
    (0..points)
        .map(|index| {
            let t = index as f64 / (points - 1) as f64;
            let wobble = ((t * 21.0).sin() * 0.35 + (t * 47.0).sin() * 0.15) * swing;
            match shape {
                // Climbed all morning, gave half of it back after lunch.
                "fade" => base + swing * (t * 2.4).sin() - swing * 0.65 * t + wobble,
                "rally" => base + swing * 0.8 * t + wobble,
                "slide" => base - swing * 0.9 * t + wobble,
                _ => base + wobble * 0.2,
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn card(
    kind: ChannelKind,
    urgency: ChannelUrgency,
    title: &str,
    headline: &str,
    detail: &str,
    action: &str,
    figure: Option<&RadarFigure>,
) -> MonoFrame {
    render_channel_card_with_radar(
        &ChannelCard::new(
            kind,
            urgency,
            ChannelAvailability::Current,
            title,
            headline,
            detail,
            action,
            ChannelSource::aged("Yahoo Finance", 42),
        ),
        figure,
    )
    .expect("fixture card is valid")
}

fn sheet(frames: &[(String, MonoFrame)], path: PathBuf) {
    let cell_w = u32::from(WIDTH) * SCALE;
    let cell_h = u32::from(HEIGHT) * SCALE;
    let rows = (frames.len() as u32).div_ceil(COLUMNS);
    let mut sheet = GrayImage::from_pixel(
        COLUMNS * cell_w + (COLUMNS + 1) * GUTTER,
        rows * cell_h + (rows + 1) * GUTTER,
        Luma([150]),
    );
    for (index, (name, frame)) in frames.iter().enumerate() {
        let index = index as u32;
        let ox = GUTTER + (index % COLUMNS) * (cell_w + GUTTER);
        let oy = GUTTER + (index / COLUMNS) * (cell_h + GUTTER);
        for y in 0..cell_h {
            for x in 0..cell_w {
                let black = frame.is_black((x / SCALE) as u16, (y / SCALE) as u16);
                sheet.put_pixel(ox + x, oy + y, Luma([if black { 0 } else { 255 }]));
            }
        }
        println!("  r{}c{}  {name}", index / COLUMNS, index % COLUMNS);
    }
    sheet.save(&path).expect("sheet is writable");
    println!("wrote {}", path.display());
}

/// The figure on its own, enlarged, so the pin and the dither can be judged.
fn figure_plate(figures: &[(String, RadarFigure)], path: PathBuf) {
    let scale = 6u32;
    let cell_w = u32::from(RADAR_FIGURE_WIDTH) * scale;
    let cell_h = u32::from(RADAR_FIGURE_HEIGHT) * scale;
    let mut plate = GrayImage::from_pixel(
        cell_w + 2 * GUTTER,
        figures.len() as u32 * (cell_h + GUTTER) + GUTTER,
        Luma([150]),
    );
    let mut frame = MonoFrame::white();
    for (index, (name, figure)) in figures.iter().enumerate() {
        frame = MonoFrame::white();
        figure.draw(&mut frame, 0, 0);
        let oy = GUTTER + index as u32 * (cell_h + GUTTER);
        for y in 0..cell_h {
            for x in 0..cell_w {
                let black = frame.is_black((x / scale) as u16, (y / scale) as u16);
                plate.put_pixel(GUTTER + x, oy + y, Luma([if black { 0 } else { 255 }]));
            }
        }
        println!("  figure {index}: {name}");
    }
    let _ = frame;
    plate.save(&path).expect("plate is writable");
    println!("wrote {}", path.display());
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("crates/eink/previews"), PathBuf::from);
    std::fs::create_dir_all(&out).expect("output directory is writable");

    let overhead = radar_figure_from_png(&synthetic_radar_tile(0.5, 0.5, 1.0)).unwrap();
    let approaching = radar_figure_from_png(&synthetic_radar_tile(0.30, 0.30, 0.9)).unwrap();
    let scattered = radar_figure_from_png(&synthetic_radar_tile(0.72, 0.34, 0.45)).unwrap();

    println!("RADAR FIGURES");
    figure_plate(
        &[
            ("cell overhead".into(), overhead.clone()),
            (
                "band approaching from the north-west".into(),
                approaching.clone(),
            ),
            ("scattered, off to the east".into(), scattered.clone()),
        ],
        out.join("figure-radar.png"),
    );

    println!("\nFIGURE-BEARING CARDS");
    sheet(
        &[
            (
                "weather / radar overhead".into(),
                card(
                    ChannelKind::Weather,
                    ChannelUrgency::Urgent,
                    "Miami / Brickell",
                    "Heavy rain in 12 minutes",
                    "0.6 in/hr / gusts 31 mph",
                    "Take cover by 4:20 PM",
                    Some(&overhead),
                ),
            ),
            (
                "weather / band approaching".into(),
                card(
                    ChannelKind::Weather,
                    ChannelUrgency::Advisory,
                    "Miami / Brickell",
                    "Rain likely tonight",
                    "0.2 in/hr after 9 PM",
                    "Umbrella if out late",
                    Some(&approaching),
                ),
            ),
            (
                "markets / faded after lunch".into(),
                card(
                    ChannelKind::Markets,
                    ChannelUrgency::Routine,
                    "Watchlist",
                    "AMD 469.60 -2.86%",
                    "Vol 19.2M / range 469-483",
                    "Material move",
                    series_figure(&intraday("fade", 476.0), Some(483.4)).as_ref(),
                ),
            ),
            (
                "markets / rallied all session".into(),
                card(
                    ChannelKind::Markets,
                    ChannelUrgency::Routine,
                    "US markets",
                    "S&P 500 7,753 +0.42%",
                    "Vol 2.09B / range 7743-7774",
                    "Material move",
                    series_figure(&intraday("rally", 7735.0), Some(7720.6)).as_ref(),
                ),
            ),
        ],
        out.join("figure-cards.png"),
    );
}
