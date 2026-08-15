use std::{io::Cursor, path::Path};

use image::{DynamicImage, GrayImage, ImageError, ImageFormat, Luma, imageops::FilterType};
use thiserror::Error;

use crate::{HEIGHT, MonoFrame, WIDTH};

/// Preview generation failure.
#[derive(Debug, Error)]
pub enum PreviewError {
    /// PNG encoding or filesystem failure.
    #[error("could not write e-paper preview: {0}")]
    Image(#[from] ImageError),
    /// Scale zero cannot produce a visible preview.
    #[error("preview scale must be at least 1")]
    ZeroScale,
}

/// Writes an exact-size 250×122 grayscale PNG.
pub fn save_preview_png(frame: &MonoFrame, path: impl AsRef<Path>) -> Result<(), PreviewError> {
    image_for(frame).save(path).map_err(Into::into)
}

/// Encodes an exact-size 250×122 grayscale PNG in memory.
pub fn preview_png_bytes(frame: &MonoFrame) -> Result<Vec<u8>, PreviewError> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(image_for(frame)).write_to(&mut output, ImageFormat::Png)?;
    Ok(output.into_inner())
}

/// Writes a nearest-neighbour preview enlarged by an integer scale.
pub fn save_scaled_preview_png(
    frame: &MonoFrame,
    path: impl AsRef<Path>,
    scale: u32,
) -> Result<(), PreviewError> {
    if scale == 0 {
        return Err(PreviewError::ZeroScale);
    }
    let image = image_for(frame);
    let scaled = image::imageops::resize(
        &image,
        u32::from(WIDTH) * scale,
        u32::from(HEIGHT) * scale,
        FilterType::Nearest,
    );
    scaled.save(path).map_err(Into::into)
}

fn image_for(frame: &MonoFrame) -> GrayImage {
    GrayImage::from_fn(u32::from(WIDTH), u32::from(HEIGHT), |x, y| {
        if frame.is_black(x as u16, y as u16) {
            Luma([0])
        } else {
            Luma([255])
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;

    #[test]
    fn exact_preview_round_trips_dimensions() {
        let path = std::env::temp_dir().join(format!(
            "bridgestatus-eink-{:?}.png",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        save_preview_png(&MonoFrame::white(), &path).unwrap();
        let decoded = image::open(&path).unwrap();
        assert_eq!(decoded.width(), u32::from(WIDTH));
        assert_eq!(decoded.height(), u32::from(HEIGHT));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn in_memory_preview_is_a_decodable_exact_frame() {
        let bytes = preview_png_bytes(&MonoFrame::white()).unwrap();
        let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png).unwrap();
        assert_eq!(decoded.width(), u32::from(WIDTH));
        assert_eq!(decoded.height(), u32::from(HEIGHT));
    }
}
