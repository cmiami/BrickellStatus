use embedded_graphics::{
    Pixel,
    geometry::{OriginDimensions, Size},
    pixelcolor::BinaryColor,
    prelude::DrawTarget,
};

use crate::{HEIGHT, PAYLOAD_SIZE, STRIDE, WIDTH};

/// A fixed 250×122, one-bit framebuffer.
///
/// A set bit is black. Within each byte the left-most pixel occupies the most
/// significant bit, matching the INK1 firmware contract.
#[derive(Clone, PartialEq, Eq)]
pub struct MonoFrame {
    bytes: Box<[u8; PAYLOAD_SIZE]>,
}

impl std::fmt::Debug for MonoFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MonoFrame")
            .field("width", &WIDTH)
            .field("height", &HEIGHT)
            .field("black_pixels", &self.black_pixel_count())
            .finish()
    }
}

impl Default for MonoFrame {
    fn default() -> Self {
        Self::white()
    }
}

impl MonoFrame {
    /// Creates an all-white frame.
    pub fn white() -> Self {
        Self {
            bytes: Box::new([0; PAYLOAD_SIZE]),
        }
    }

    /// Creates an all-black frame while leaving row-padding bits white.
    pub fn black() -> Self {
        let mut frame = Self::white();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                frame.set_black(x, y, true);
            }
        }
        frame
    }

    /// Constructs a frame from an already packed INK1 payload.
    pub fn from_packed(bytes: [u8; PAYLOAD_SIZE]) -> Self {
        Self {
            bytes: Box::new(bytes),
        }
    }

    /// Returns the exact packed bytes consumed by the E213 firmware.
    pub fn packed(&self) -> &[u8; PAYLOAD_SIZE] {
        &self.bytes
    }

    /// Returns whether the addressed pixel is black.
    pub fn is_black(&self, x: u16, y: u16) -> bool {
        if x >= WIDTH || y >= HEIGHT {
            return false;
        }
        let offset = usize::from(y) * STRIDE + usize::from(x) / 8;
        self.bytes[offset] & (0x80 >> (x % 8)) != 0
    }

    /// Sets a pixel, ignoring coordinates outside the physical panel.
    pub fn set_black(&mut self, x: u16, y: u16, black: bool) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        let offset = usize::from(y) * STRIDE + usize::from(x) / 8;
        let mask = 0x80 >> (x % 8);
        if black {
            self.bytes[offset] |= mask;
        } else {
            self.bytes[offset] &= !mask;
        }
    }

    /// Counts black pixels, excluding unused padding bits.
    pub fn black_pixel_count(&self) -> usize {
        (0..HEIGHT)
            .map(|y| (0..WIDTH).filter(|x| self.is_black(*x, y)).count())
            .sum()
    }
}

impl OriginDimensions for MonoFrame {
    fn size(&self) -> Size {
        Size::new(u32::from(WIDTH), u32::from(HEIGHT))
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
            Self::black()
        } else {
            Self::white()
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_bits_stay_white() {
        let frame = MonoFrame::black();
        assert_eq!(
            frame.black_pixel_count(),
            usize::from(WIDTH) * usize::from(HEIGHT)
        );
        for row in frame.packed().chunks_exact(STRIDE) {
            assert_eq!(row[STRIDE - 1], 0b1100_0000);
        }
    }

    #[test]
    fn panel_corners_match_sibling_bit_order() {
        let mut frame = MonoFrame::white();
        frame.set_black(0, 0, true);
        frame.set_black(WIDTH - 1, HEIGHT - 1, true);
        assert_eq!(frame.packed()[0], 0x80);
        assert_eq!(frame.packed()[PAYLOAD_SIZE - 1], 0x40);
    }
}
