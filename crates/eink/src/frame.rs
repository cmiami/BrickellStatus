use embedded_graphics::{
    Pixel,
    geometry::{OriginDimensions, Size},
    pixelcolor::BinaryColor,
    prelude::DrawTarget,
};

use crate::{PanelModel, panel::PanelGrid};

/// A one-bit framebuffer sized to the panel it will be shown on.
///
/// A set bit is black. Within each byte the left-most pixel occupies the most
/// significant bit, matching the INK1 firmware contract.
///
/// The frame carries its own panel rather than taking dimensions from a global
/// constant. That is what lets one host drive either board without a mode
/// switch: the geometry travels with the picture, all the way onto the wire.
#[derive(Clone, PartialEq, Eq)]
pub struct MonoFrame {
    model: PanelModel,
    bytes: Box<[u8]>,
}

impl std::fmt::Debug for MonoFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MonoFrame")
            .field("panel", &self.model)
            .field("width", &self.width())
            .field("height", &self.height())
            .field("black_pixels", &self.black_pixel_count())
            .finish()
    }
}

impl Default for MonoFrame {
    fn default() -> Self {
        Self::white(PanelModel::default())
    }
}

impl MonoFrame {
    /// Creates an all-white frame for a panel.
    pub fn white(model: PanelModel) -> Self {
        Self {
            model,
            bytes: vec![0; model.payload_size()].into_boxed_slice(),
        }
    }

    /// Creates an all-black frame while leaving row-padding bits white.
    pub fn black(model: PanelModel) -> Self {
        let mut frame = Self::white(model);
        for y in 0..frame.height() {
            for x in 0..frame.width() {
                frame.set_black(x, y, true);
            }
        }
        frame
    }

    /// Constructs a frame from an already packed INK1 payload.
    ///
    /// Refuses a payload that is not exactly this panel's size: a short buffer
    /// silently drawn would shift every row after the shortfall by a few pixels,
    /// which is a legible picture of the wrong thing.
    pub fn from_packed(model: PanelModel, bytes: &[u8]) -> Option<Self> {
        (bytes.len() == model.payload_size()).then(|| Self {
            model,
            bytes: bytes.to_vec().into_boxed_slice(),
        })
    }

    /// Which panel this frame was drawn for.
    pub const fn panel(&self) -> PanelModel {
        self.model
    }

    /// Panel width in pixels.
    pub const fn width(&self) -> u16 {
        self.model.width()
    }

    /// Panel height in pixels.
    pub const fn height(&self) -> u16 {
        self.model.height()
    }

    /// Packed bytes per row.
    pub const fn stride(&self) -> usize {
        self.model.stride()
    }

    /// The grid this frame's furniture is placed on.
    pub(crate) const fn grid(&self) -> PanelGrid {
        self.model.grid()
    }

    /// Returns the exact packed bytes the firmware consumes.
    pub fn packed(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns whether the addressed pixel is black.
    pub fn is_black(&self, x: u16, y: u16) -> bool {
        if x >= self.width() || y >= self.height() {
            return false;
        }
        let offset = usize::from(y) * self.stride() + usize::from(x) / 8;
        self.bytes[offset] & (0x80 >> (x % 8)) != 0
    }

    /// Sets a pixel, ignoring coordinates outside the physical panel.
    pub fn set_black(&mut self, x: u16, y: u16, black: bool) {
        if x >= self.width() || y >= self.height() {
            return;
        }
        let offset = usize::from(y) * self.stride() + usize::from(x) / 8;
        let mask = 0x80 >> (x % 8);
        if black {
            self.bytes[offset] |= mask;
        } else {
            self.bytes[offset] &= !mask;
        }
    }

    /// Counts black pixels, excluding unused padding bits.
    pub fn black_pixel_count(&self) -> usize {
        (0..self.height())
            .map(|y| (0..self.width()).filter(|x| self.is_black(*x, y)).count())
            .sum()
    }
}

impl OriginDimensions for MonoFrame {
    fn size(&self) -> Size {
        Size::new(u32::from(self.width()), u32::from(self.height()))
    }
}

impl DrawTarget for MonoFrame {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            let (Ok(x), Ok(y)) = (u16::try_from(point.x), u16::try_from(point.y)) else {
                continue;
            };
            self.set_black(x, y, color == BinaryColor::On);
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        *self = if color == BinaryColor::On {
            Self::black(self.model)
        } else {
            Self::white(self.model)
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_bits_stay_white() {
        for model in PanelModel::ALL {
            let frame = MonoFrame::black(model);
            assert_eq!(
                frame.black_pixel_count(),
                usize::from(frame.width()) * usize::from(frame.height()),
                "{model:?} must count every panel pixel and no padding bit"
            );
            let stride = frame.stride();
            let used_bits = usize::from(frame.width()) % 8;
            let last_byte = if used_bits == 0 {
                0xff
            } else {
                0xffu8 << (8 - used_bits)
            };
            for row in frame.packed().chunks_exact(stride) {
                assert_eq!(row[stride - 1], last_byte, "{model:?} padding bits");
            }
        }
    }

    #[test]
    fn panel_corners_match_sibling_bit_order() {
        for model in PanelModel::ALL {
            let mut frame = MonoFrame::white(model);
            frame.set_black(0, 0, true);
            frame.set_black(frame.width() - 1, frame.height() - 1, true);
            let packed = frame.packed();
            assert_eq!(packed[0], 0x80);
            // The E213's last pixel lands two bits into its final byte; the
            // E290's width is not a multiple of eight either, so both panels
            // exercise the padding path rather than only the tidy case.
            let used_bits = usize::from(frame.width()) % 8;
            let expected = if used_bits == 0 {
                0x01
            } else {
                0x80u8 >> (used_bits - 1)
            };
            assert_eq!(packed[packed.len() - 1], expected, "{model:?} last pixel");
        }
    }

    /// A payload from one panel must never be adopted by the other.
    #[test]
    fn a_payload_of_the_wrong_size_is_refused() {
        let e290 = MonoFrame::white(PanelModel::E290);
        assert!(MonoFrame::from_packed(PanelModel::E213, e290.packed()).is_none());
        assert!(MonoFrame::from_packed(PanelModel::E290, e290.packed()).is_some());
    }

    #[test]
    fn drawing_outside_the_panel_is_ignored_rather_than_wrapping() {
        let mut frame = MonoFrame::white(PanelModel::E290);
        frame.set_black(frame.width(), 0, true);
        frame.set_black(0, frame.height(), true);
        assert_eq!(frame.black_pixel_count(), 0);
    }
}
