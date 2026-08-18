//! Radar as a one-bit figure.
//!
//! The panel is 250×122 and has two colours. A radar composite at that size is
//! not a map — it is texture, corroborating the sentence beside it. The words
//! stay load-bearing; the figure answers "is that band on top of me or beside
//! me" at a glance and nothing more.
//!
//! Two decisions make the result legible rather than noise.
//!
//! The tile is requested in RainViewer's black-and-white scheme. The colour
//! schemes are not luminance ramps: in the common rainbow scheme light rain is
//! dark blue and heavy rain is pale yellow, so converting to grey inverts the
//! intensity ordering in the middle of the range. Only a monochrome source
//! makes "darker means heavier" true.
//!
//! Transparency is composited over white before anything else. Radar tiles are
//! mostly transparent, and a converter that reads the colour channels without
//! the alpha channel sees whatever the encoder left in the transparent pixels —
//! frequently black, which fills the panel with rain that is not there.

use image::{ImageReader, imageops::FilterType};

use crate::MonoFrame;

/// The figure's box on the panel, left as a constant so a caller cannot ask for
/// a size the layout has no room for. Radar is one thing that goes in it; a
/// price line is another.
pub const RADAR_FIGURE_WIDTH: u16 = 96;
/// Height of the figure in panel pixels.
pub const RADAR_FIGURE_HEIGHT: u16 = 42;

/// A radar figure, ready to stamp into a frame. One byte per pixel, `true`
/// meaning black, in row-major order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadarFigure {
    pixels: Vec<bool>,
    /// Whether this figure is centred on the reader and should say so.
    ///
    /// Kept apart from the pixels rather than stamped into them, because
    /// [`Self::is_worth_drawing`] measures echo and the mark is not echo. Baked
    /// in, it added enough ink to carry an empty composite over the threshold,
    /// and the panel drew a crosshair over a clear sky.
    mark_centre: bool,
}

/// Reasons a radar composite could not be turned into a panel figure.
#[derive(Debug, thiserror::Error)]
pub enum RadarError {
    /// The bytes are not an image this build can decode.
    #[error("radar tile could not be decoded: {0}")]
    Decode(String),
    /// The response was larger than a radar tile has any reason to be.
    #[error("radar tile is {0} bytes, larger than the {1} byte limit")]
    TooLarge(usize, usize),
}

/// A generous ceiling for one 256 px radar tile, which measures in single-digit
/// kilobytes in practice. Present because the bytes arrive over the network and
/// decoding allocates in proportion to what the header claims.
const MAX_TILE_BYTES: usize = 512 * 1024;

/// Fraction of the figure that must be black before it is worth drawing.
///
/// Below this the composite is empty, and a handful of dithered specks reads as
/// a dirty panel rather than as weather. Saying nothing is the honest output.
const MIN_INK_FRACTION: f32 = 0.01;

impl RadarFigure {
    /// Whether the figure carries enough echo to be worth the space.
    pub fn is_worth_drawing(&self) -> bool {
        let black = self.pixels.iter().filter(|pixel| **pixel).count();
        (black as f32) / (self.pixels.len() as f32) >= MIN_INK_FRACTION
    }

    /// Stamps the figure into a frame with its top-left corner at `x, y`.
    pub fn draw(&self, frame: &mut MonoFrame, x: u16, y: u16) {
        for row in 0..RADAR_FIGURE_HEIGHT {
            for column in 0..RADAR_FIGURE_WIDTH {
                let index =
                    usize::from(row) * usize::from(RADAR_FIGURE_WIDTH) + usize::from(column);
                if self.pixels[index] {
                    frame.set_black(x + column, y + row, true);
                }
            }
        }
        if self.mark_centre {
            draw_location_mark(frame, x, y);
        }
    }
}

/// Radius of the ring marking the reader's own position.
const PIN_RADIUS: i32 = 4;
/// How far the crosshair arms reach past the ring.
const PIN_ARM: i32 = 8;

/// Stamps "you are here" at the centre of a figure already drawn at `x, y`.
///
/// A radar composite without it is a texture with no anchor: the reader can see
/// that it is raining somewhere in frame and cannot tell whether the band is on
/// top of them or twenty miles up the coast, which is the entire question the
/// figure exists to answer. The tile is centred on the reader's coordinates, so
/// the centre of the box *is* their position — the panel simply never said so.
///
/// Drawn black inside a cleared halo. A black crosshair alone vanishes into a
/// heavy cell, which is precisely where the reader most needs to find it;
/// clearing one pixel around every stroke keeps it legible over rain without
/// erasing enough of the composite to matter.
fn draw_location_mark(frame: &mut MonoFrame, origin_x: u16, origin_y: u16) {
    let centre_x = i32::from(origin_x) + i32::from(RADAR_FIGURE_WIDTH) / 2;
    let centre_y = i32::from(origin_y) + i32::from(RADAR_FIGURE_HEIGHT) / 2;
    let mut mark = Vec::new();

    // Ring, by midpoint sampling. Small enough that a circle routine would cost
    // more than walking the box it sits in.
    for offset_y in -PIN_RADIUS..=PIN_RADIUS {
        for offset_x in -PIN_RADIUS..=PIN_RADIUS {
            let distance = offset_x * offset_x + offset_y * offset_y;
            if distance <= PIN_RADIUS * PIN_RADIUS && distance > (PIN_RADIUS - 1) * (PIN_RADIUS - 1)
            {
                mark.push((centre_x + offset_x, centre_y + offset_y));
            }
        }
    }
    // Crosshair arms, reaching past the ring so the mark reads as an instrument
    // rather than as a blob of rain.
    for offset in -PIN_ARM..=PIN_ARM {
        mark.push((centre_x + offset, centre_y));
        mark.push((centre_x, centre_y + offset));
    }

    let bounds = |x: i32, y: i32| {
        x >= i32::from(origin_x)
            && y >= i32::from(origin_y)
            && x < i32::from(origin_x) + i32::from(RADAR_FIGURE_WIDTH)
            && y < i32::from(origin_y) + i32::from(RADAR_FIGURE_HEIGHT)
    };
    for (x, y) in &mark {
        for halo_y in -1..=1 {
            for halo_x in -1..=1 {
                let (hx, hy) = (x + halo_x, y + halo_y);
                if bounds(hx, hy) {
                    frame.set_black(hx as u16, hy as u16, false);
                }
            }
        }
    }
    for (x, y) in mark {
        if bounds(x, y) {
            frame.set_black(x as u16, y as u16, true);
        }
    }
}

/// Converts one radar composite into a panel figure.
///
/// The source is expected to be square and centred on the reader's coordinates
/// — RainViewer serves such a tile directly, which is why there is no
/// projection arithmetic here. The centre crop keeps the reader at the middle
/// of the figure rather than letting a wide aspect ratio slide them off it.
pub fn radar_figure_from_png(bytes: &[u8]) -> Result<RadarFigure, RadarError> {
    if bytes.len() > MAX_TILE_BYTES {
        return Err(RadarError::TooLarge(bytes.len(), MAX_TILE_BYTES));
    }
    let decoded = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| RadarError::Decode(error.to_string()))?
        .decode()
        .map_err(|error| RadarError::Decode(error.to_string()))?
        .to_rgba8();

    // Over white first. Radar tiles are mostly transparent and the colour left
    // behind transparent pixels is not defined by anything.
    let mut luma = image::GrayImage::new(decoded.width(), decoded.height());
    for (x, y, pixel) in decoded.enumerate_pixels() {
        let [red, green, blue, alpha] = pixel.0;
        let alpha = f32::from(alpha) / 255.0;
        let over_white = |channel: u8| f32::from(channel) * alpha + 255.0 * (1.0 - alpha);
        // Rec. 601 luma, which is what the eye reads as brightness.
        let value = 0.299 * over_white(red) + 0.587 * over_white(green) + 0.114 * over_white(blue);
        luma.put_pixel(x, y, image::Luma([value.clamp(0.0, 255.0) as u8]));
    }

    // Centre-crop to the *figure's* aspect ratio before scaling, so the resize
    // is uniform in both axes and a band of rain keeps the shape the sky has.
    //
    // Cropping to a square instead — which this did — hands a 1:1 source to a
    // 96x42 box and squashes it by better than two to one. A circular cell came
    // out as a wide ellipse, and rain approaching from the north looked closer
    // than rain approaching from the west.
    let (crop_width, crop_height) = aspect_crop(
        luma.width(),
        luma.height(),
        u32::from(RADAR_FIGURE_WIDTH),
        u32::from(RADAR_FIGURE_HEIGHT),
    );
    let cropped = image::imageops::crop_imm(
        &luma,
        (luma.width() - crop_width) / 2,
        (luma.height() - crop_height) / 2,
        crop_width,
        crop_height,
    )
    .to_image();
    let scaled = image::imageops::resize(
        &cropped,
        u32::from(RADAR_FIGURE_WIDTH),
        u32::from(RADAR_FIGURE_HEIGHT),
        FilterType::Triangle,
    );

    Ok(RadarFigure {
        pixels: floyd_steinberg(&scaled),
        mark_centre: true,
    })
}

/// The largest centred rectangle of `target` aspect that fits inside the source.
fn aspect_crop(width: u32, height: u32, target_width: u32, target_height: u32) -> (u32, u32) {
    if width == 0 || height == 0 || target_width == 0 || target_height == 0 {
        return (width.max(1), height.max(1));
    }
    // Compare source and target aspect without floating point: a source wider
    // than the target loses width, a taller one loses height.
    if u64::from(width) * u64::from(target_height) > u64::from(height) * u64::from(target_width) {
        let cropped =
            (u64::from(height) * u64::from(target_width) / u64::from(target_height)) as u32;
        (cropped.clamp(1, width), height)
    } else {
        let cropped =
            (u64::from(width) * u64::from(target_height) / u64::from(target_width)) as u32;
        (width, cropped.clamp(1, height))
    }
}

/// Error-diffusion dithering.
///
/// A plain threshold would erase everything but the heaviest cell, because on
/// this scale most of a composite sits in the middle of the range. Diffusing
/// the error spends the panel's two colours on conveying *how much* rain rather
/// than merely whether any exists.
fn floyd_steinberg(image: &image::GrayImage) -> Vec<bool> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut values = image
        .pixels()
        .map(|pixel| f32::from(pixel.0[0]))
        .collect::<Vec<_>>();
    let mut pixels = vec![false; values.len()];

    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let old = values[index];
            let new = if old < 128.0 { 0.0 } else { 255.0 };
            pixels[index] = new == 0.0;
            let error = old - new;
            let mut diffuse = |dx: usize, dy: usize, weight: f32| {
                if dx < width && dy < height {
                    values[dy * width + dx] += error * weight;
                }
            };
            diffuse(x + 1, y, 7.0 / 16.0);
            if x > 0 {
                diffuse(x - 1, y + 1, 3.0 / 16.0);
            }
            diffuse(x, y + 1, 5.0 / 16.0);
            diffuse(x + 1, y + 1, 1.0 / 16.0);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PanelModel;

    fn png(build: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut image = image::RgbaImage::new(64, 64);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgba(build(x, y));
        }
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    /// The failure this guards against fills the panel solid: radar tiles are
    /// mostly transparent, and whatever colour sits behind those pixels is
    /// undefined. Without compositing, "no rain" renders as "all rain".
    #[test]
    fn a_fully_transparent_tile_produces_no_ink() {
        let figure = radar_figure_from_png(&png(|_, _| [0, 0, 0, 0])).unwrap();
        assert!(figure.pixels.iter().all(|pixel| !pixel));
        assert!(!figure.is_worth_drawing());
    }

    #[test]
    fn a_solid_echo_produces_a_black_figure() {
        let figure = radar_figure_from_png(&png(|_, _| [0, 0, 0, 255])).unwrap();
        assert!(figure.pixels.iter().all(|pixel| *pixel));
        assert!(figure.is_worth_drawing());
    }

    /// Darker must mean heavier across the whole range, which is why the tile
    /// is requested in RainViewer's monochrome scheme rather than converted
    /// from a rainbow one.
    #[test]
    fn heavier_echo_leaves_more_ink_than_lighter_echo() {
        let ink = |grey: u8| {
            radar_figure_from_png(&png(|_, _| [grey, grey, grey, 255]))
                .unwrap()
                .pixels
                .iter()
                .filter(|pixel| **pixel)
                .count()
        };
        assert!(ink(40) > ink(120));
        assert!(ink(120) > ink(200));
    }

    /// Half-transparent black is mid-grey over white, not black. Dithering has
    /// to spend both colours on it rather than rounding it away.
    #[test]
    fn partial_coverage_dithers_rather_than_collapsing_to_one_colour() {
        let figure = radar_figure_from_png(&png(|_, _| [0, 0, 0, 128])).unwrap();
        let black = figure.pixels.iter().filter(|pixel| **pixel).count();
        assert!(black > 0 && black < figure.pixels.len(), "{black} black");
    }

    /// A few specks read as a dirty panel, not as weather.
    #[test]
    fn a_nearly_empty_composite_is_not_worth_drawing() {
        let figure = radar_figure_from_png(&png(|x, y| {
            if x == 0 && y == 0 {
                [0, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            }
        }))
        .unwrap();
        assert!(!figure.is_worth_drawing());
    }

    #[test]
    fn the_figure_is_deterministic_and_fits_its_box() {
        let bytes = png(|x, y| [((x * 4) % 256) as u8, 0, 0, ((y * 4) % 256) as u8]);
        let figure = radar_figure_from_png(&bytes).unwrap();
        assert_eq!(figure, radar_figure_from_png(&bytes).unwrap());
        assert_eq!(
            figure.pixels.len(),
            usize::from(RADAR_FIGURE_WIDTH) * usize::from(RADAR_FIGURE_HEIGHT)
        );

        let mut frame = MonoFrame::white(PanelModel::E213);
        figure.draw(&mut frame, 132, 30);
        // Nothing outside the box, in either direction.
        assert!(!frame.is_black(131, 50));
        assert!(!frame.is_black(228, 50));
        assert!(!frame.is_black(180, 29));
        assert!(!frame.is_black(180, 72));
    }

    #[test]
    fn a_response_that_is_not_an_image_is_an_error_rather_than_a_panic() {
        assert!(radar_figure_from_png(b"<html>404</html>").is_err());
        assert!(radar_figure_from_png(&vec![0u8; MAX_TILE_BYTES + 1]).is_err());
    }
}

/// Draws a series as a line inside the standard figure box.
///
/// Scaled to its own range rather than to zero, for the same reason the console
/// sparkline is: a stock that moved from 511 to 514 is a flat line against a
/// zero axis and a legible one against its own day, and the day is the question
/// being asked. Returns `None` when there is no shape to draw — one point is
/// not a line, and a flat series is drawn as the flat line it is.
pub fn series_figure(values: &[f64], reference: Option<f64>) -> Option<RadarFigure> {
    let usable = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if usable.len() < 2 {
        return None;
    }
    let reference = reference.filter(|value| value.is_finite() && *value > 0.0);
    // The reference is part of the plotted range, not an overlay on it. Scaling
    // to the series alone and then drawing a previous close outside that range
    // would pin the rule to an edge and quietly claim the day never crossed it.
    let mut low = usable.iter().copied().fold(f64::INFINITY, f64::min);
    let mut high = usable.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if let Some(value) = reference {
        low = low.min(value);
        high = high.max(value);
    }
    // A series with no range has no shape. Drawing it against a one-unit span
    // would pin it to an edge and read as a crash or a moonshot; it belongs on
    // the mid-line, which is what a day that went nowhere looks like.
    let flat = (high - low).abs() < f64::EPSILON;
    let span = if flat { 1.0 } else { high - low };

    let width = usize::from(RADAR_FIGURE_WIDTH);
    let height = usize::from(RADAR_FIGURE_HEIGHT);
    let mut pixels = vec![false; width * height];
    let row_for = |value: f64| -> usize {
        let normalized = if flat {
            0.5
        } else {
            ((value - low) / span).clamp(0.0, 1.0)
        };
        let row = (1.0 - normalized) * (height as f64 - 1.0);
        (row.round() as usize).min(height - 1)
    };

    // The previous close, as a dashed rule across the figure.
    //
    // Without it the line has a shape and no meaning: a reader sees the price
    // wander and cannot see which side of the day's starting point it wandered
    // on, which is the one thing the change percent beside it is claiming. It
    // has to be the previous close specifically and not the series' own first
    // sample — that is the open, and a quote that gapped down overnight can
    // climb all session while still printing a loss, so the picture and the
    // number would contradict each other. Dashed, so the price line stays the
    // figure's subject and this stays its reference.
    if let Some(value) = reference {
        let reference_row = row_for(value);
        for column in (0..width).step_by(4) {
            pixels[reference_row * width + column] = true;
        }
    }

    let mut previous: Option<usize> = None;
    for column in 0..width {
        // Nearest sample rather than interpolation: the series is already a
        // downsample, and inventing intermediate prices would draw values that
        // never traded.
        let index = (column * (usable.len() - 1) + width / 2) / width.max(1);
        let row = row_for(usable[index.min(usable.len() - 1)]);
        // Join to the previous column so a steep move reads as one line rather
        // than as two disconnected dots.
        let (from, to) = match previous {
            Some(previous_row) if previous_row <= row => (previous_row, row),
            Some(previous_row) => (row, previous_row),
            None => (row, row),
        };
        for filled in from..=to {
            pixels[filled * width + column] = true;
        }
        previous = Some(row);
    }
    Some(RadarFigure {
        pixels,
        mark_centre: false,
    })
}

#[cfg(test)]
mod figure_tests {
    use super::*;
    use crate::PanelModel;

    /// A square source squashed into a 96x42 box turns a circular cell into a
    /// wide ellipse, so rain approaching from the north looked nearer than rain
    /// approaching from the west. The crop has to match the box's aspect, which
    /// makes the resize uniform in both axes.
    #[test]
    fn the_crop_makes_the_resize_uniform_in_both_axes() {
        let (width, height) = aspect_crop(
            256,
            256,
            u32::from(RADAR_FIGURE_WIDTH),
            u32::from(RADAR_FIGURE_HEIGHT),
        );
        let horizontal = f64::from(RADAR_FIGURE_WIDTH) / f64::from(width);
        let vertical = f64::from(RADAR_FIGURE_HEIGHT) / f64::from(height);
        assert!(
            (horizontal - vertical).abs() < 0.02,
            "scaled {horizontal:.3} across and {vertical:.3} down, which distorts the sky"
        );
    }

    /// ...and the crop never asks for more source than exists.
    #[test]
    fn the_crop_stays_inside_the_source() {
        for (width, height) in [(256, 256), (512, 128), (100, 900), (1, 1)] {
            let (cropped_width, cropped_height) = aspect_crop(
                width,
                height,
                u32::from(RADAR_FIGURE_WIDTH),
                u32::from(RADAR_FIGURE_HEIGHT),
            );
            assert!(
                cropped_width <= width && cropped_width > 0,
                "{width}x{height}"
            );
            assert!(
                cropped_height <= height && cropped_height > 0,
                "{width}x{height}"
            );
        }
    }

    /// The location mark is not echo. Counting it as ink carried an empty
    /// composite over the "worth drawing" threshold, and the panel drew a
    /// crosshair over a clear sky.
    #[test]
    fn the_location_mark_never_makes_an_empty_composite_look_like_weather() {
        let empty = RadarFigure {
            pixels: vec![false; usize::from(RADAR_FIGURE_WIDTH) * usize::from(RADAR_FIGURE_HEIGHT)],
            mark_centre: true,
        };
        assert!(
            !empty.is_worth_drawing(),
            "a marked but empty composite must still be dropped"
        );
    }

    /// A price line is centred on nothing, so it is never marked. Compared as
    /// behaviour rather than by probing a coordinate: the mark's exact geometry
    /// is free to change, but only a composite centred on the reader may carry
    /// one at all.
    #[test]
    fn only_a_composite_centred_on_the_reader_carries_the_mark() {
        let solid = vec![true; usize::from(RADAR_FIGURE_WIDTH) * usize::from(RADAR_FIGURE_HEIGHT)];
        let pale = |figure: &RadarFigure| {
            let mut frame = MonoFrame::white(PanelModel::E213);
            figure.draw(&mut frame, 0, 0);
            (0..RADAR_FIGURE_WIDTH)
                .flat_map(|x| (0..RADAR_FIGURE_HEIGHT).map(move |y| (x, y)))
                .filter(|(x, y)| !frame.is_black(*x, *y))
                .count()
        };

        let marked = RadarFigure {
            pixels: solid.clone(),
            mark_centre: true,
        };
        let plain = RadarFigure {
            pixels: solid,
            mark_centre: false,
        };
        assert_eq!(
            pale(&plain),
            0,
            "an unmarked figure draws only its own echo"
        );
        assert!(
            pale(&marked) > 20,
            "the mark has to cut a legible halo through a heavy cell"
        );

        assert!(
            !series_figure(&[1.0, 2.0, 3.0, 2.0], None)
                .expect("a four-point series plots")
                .mark_centre,
            "a price line is not a place"
        );
        assert!(
            radar_figure_from_png(&solid_tile())
                .expect("a solid tile decodes")
                .mark_centre,
            "a composite centred on the reader says so"
        );
    }

    fn solid_tile() -> Vec<u8> {
        let mut image = image::RgbaImage::new(64, 64);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([0, 0, 0, 255]);
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
}

#[cfg(test)]
mod series_tests {
    use super::*;
    use crate::PanelModel;

    fn rows_with_ink(figure: &RadarFigure) -> Vec<usize> {
        (0..usize::from(RADAR_FIGURE_HEIGHT))
            .filter(|row| {
                (0..usize::from(RADAR_FIGURE_WIDTH))
                    .any(|column| figure.pixels[row * usize::from(RADAR_FIGURE_WIDTH) + column])
            })
            .collect()
    }

    /// The line has to go the way the numbers went. Getting this backwards
    /// draws a rally as a sell-off.
    #[test]
    fn a_rising_series_ends_higher_on_the_panel_than_it_started() {
        let figure = series_figure(&[10.0, 11.0, 12.0, 13.0, 14.0], None).unwrap();
        let width = usize::from(RADAR_FIGURE_WIDTH);
        let first = (0..usize::from(RADAR_FIGURE_HEIGHT))
            .find(|row| figure.pixels[row * width])
            .unwrap();
        let last = (0..usize::from(RADAR_FIGURE_HEIGHT))
            .find(|row| figure.pixels[row * width + width - 1])
            .unwrap();
        // Row 0 is the top of the panel, so "higher" means a smaller row.
        assert!(last < first, "ends at row {last}, started at {first}");
    }

    #[test]
    fn a_falling_series_ends_lower() {
        let figure = series_figure(&[14.0, 13.0, 12.0, 11.0, 10.0], None).unwrap();
        let width = usize::from(RADAR_FIGURE_WIDTH);
        let first = (0..usize::from(RADAR_FIGURE_HEIGHT))
            .find(|row| figure.pixels[row * width])
            .unwrap();
        let last = (0..usize::from(RADAR_FIGURE_HEIGHT))
            .find(|row| figure.pixels[row * width + width - 1])
            .unwrap();
        assert!(last > first, "ends at row {last}, started at {first}");
    }

    /// Scaled to its own range: a three-dollar move on a five-hundred-dollar
    /// stock is the whole story of the day and must not render as a flat line.
    #[test]
    fn a_small_move_on_a_large_price_still_reads_as_a_move() {
        let figure = series_figure(&[511.0, 512.5, 511.5, 514.0], None).unwrap();
        assert!(
            rows_with_ink(&figure).len() > usize::from(RADAR_FIGURE_HEIGHT) / 2,
            "the line used {} of {RADAR_FIGURE_HEIGHT} rows",
            rows_with_ink(&figure).len()
        );
    }

    #[test]
    fn a_flat_series_is_one_flat_line_rather_than_an_edge_or_a_panic() {
        let figure = series_figure(&[7.0, 7.0, 7.0, 7.0], None).unwrap();
        let rows = rows_with_ink(&figure);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0] > 0 && rows[0] < usize::from(RADAR_FIGURE_HEIGHT) - 1);
    }

    /// One point is not a line, and neither is nothing.
    #[test]
    fn too_few_points_draw_nothing() {
        assert!(series_figure(&[], None).is_none());
        assert!(series_figure(&[42.0], None).is_none());
        assert!(series_figure(&[f64::NAN, f64::INFINITY], None).is_none());
    }

    #[test]
    fn the_line_is_continuous_across_a_steep_move() {
        // Every column carries ink, so a jump never breaks the line in two.
        let figure = series_figure(&[1.0, 100.0, 1.0, 100.0], None).unwrap();
        let width = usize::from(RADAR_FIGURE_WIDTH);
        for column in 0..width {
            assert!(
                (0..usize::from(RADAR_FIGURE_HEIGHT))
                    .any(|row| figure.pixels[row * width + column]),
                "column {column} is empty"
            );
        }
    }

    #[test]
    fn the_figure_fits_the_same_box_radar_does() {
        let figure = series_figure(&[1.0, 5.0, 2.0, 9.0], None).unwrap();
        let mut frame = MonoFrame::white(PanelModel::E213);
        figure.draw(&mut frame, 132, 30);
        assert!(!frame.is_black(131, 50));
        assert!(!frame.is_black(228, 50));
        assert!(!frame.is_black(180, 29));
        assert!(!frame.is_black(180, 72));
    }
}
