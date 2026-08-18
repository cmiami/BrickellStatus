use std::{io::Cursor, path::Path};

use image::{DynamicImage, GrayImage, ImageError, ImageFormat, Luma, imageops::FilterType};
use thiserror::Error;

use crate::MonoFrame;

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

/// Writes a grayscale PNG at the frame's own panel size.
pub fn save_preview_png(frame: &MonoFrame, path: impl AsRef<Path>) -> Result<(), PreviewError> {
    image_for(frame).save(path).map_err(Into::into)
}

/// Encodes a grayscale PNG at the frame's own panel size, in memory.
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
        u32::from(frame.width()) * scale,
        u32::from(frame.height()) * scale,
        FilterType::Nearest,
    );
    scaled.save(path).map_err(Into::into)
}

fn image_for(frame: &MonoFrame) -> GrayImage {
    GrayImage::from_fn(
        u32::from(frame.width()),
        u32::from(frame.height()),
        |x, y| {
            if frame.is_black(x as u16, y as u16) {
                Luma([0])
            } else {
                Luma([255])
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;
    use crate::PanelModel;

    #[test]
    fn exact_preview_round_trips_dimensions() {
        let path = std::env::temp_dir().join(format!(
            "bridgestatus-eink-{:?}.png",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        save_preview_png(&MonoFrame::white(PanelModel::E290), &path).unwrap();
        let decoded = image::open(&path).unwrap();
        assert_eq!(decoded.width(), 296);
        assert_eq!(decoded.height(), 128);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn in_memory_preview_is_a_decodable_exact_frame() {
        for model in crate::PanelModel::ALL {
            let bytes = preview_png_bytes(&MonoFrame::white(model)).unwrap();
            let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png).unwrap();
            assert_eq!(decoded.width(), u32::from(model.width()));
            assert_eq!(decoded.height(), u32::from(model.height()));
        }
    }
}
