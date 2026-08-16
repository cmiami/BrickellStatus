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
/// a size the layout has no room for.
pub const RADAR_FIGURE_WIDTH: u16 = 96;
/// Height of the radar figure in panel pixels.
pub const RADAR_FIGURE_HEIGHT: u16 = 42;

/// A radar figure, ready to stamp into a frame. One byte per pixel, `true`
/// meaning black, in row-major order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadarFigure {
    pixels: Vec<bool>,
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

    // Centre-crop to square before scaling, so the aspect ratio cannot stretch
    // a band of rain into a different shape than the sky has.
    let side = luma.width().min(luma.height());
    let cropped = image::imageops::crop_imm(
        &luma,
        (luma.width() - side) / 2,
        (luma.height() - side) / 2,
        side,
        side,
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
    })
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

        let mut frame = MonoFrame::white();
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
