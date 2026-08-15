use crc32fast::Hasher;
use thiserror::Error;

use crate::MonoFrame;

/// Physical panel width in pixels.
pub const WIDTH: u16 = 250;
/// Physical panel height in pixels.
pub const HEIGHT: u16 = 122;
/// Packed bytes per row, including six white padding bits.
pub const STRIDE: usize = (WIDTH as usize).div_ceil(8);
/// Size of a packed 250×122 frame.
pub const PAYLOAD_SIZE: usize = STRIDE * HEIGHT as usize;
/// Size of the INK1 header.
pub const HEADER_SIZE: usize = 18;
/// Total size of one complete INK1 packet.
pub const PACKET_SIZE: usize = HEADER_SIZE + PAYLOAD_SIZE;
/// Four-byte packet synchronisation marker.
pub const INK1_MAGIC: [u8; 4] = *b"INK1";
/// Requests the firmware's slower full-refresh waveform.
pub const FLAG_FULL_REFRESH: u8 = 0x01;

/// E-paper refresh waveform requested for a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RefreshMode {
    /// Faster refresh for routine updates.
    #[default]
    Fast,
    /// Full refresh to clear accumulated ghosting.
    Full,
}

impl RefreshMode {
    const fn flags(self) -> u8 {
        match self {
            Self::Fast => 0,
            Self::Full => FLAG_FULL_REFRESH,
        }
    }
}

/// A packet which passed all INK1 structural and checksum checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedPacket<'a> {
    /// Packet flags as transmitted.
    pub flags: u8,
    /// Validated packed framebuffer bytes.
    pub payload: &'a [u8],
}

/// INK1 packet validation failure.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    /// Packet byte count differs from the fixed protocol size.
    #[error("INK1 packet must be {PACKET_SIZE} bytes, got {actual}")]
    PacketSize {
        /// Observed packet byte count.
        actual: usize,
    },
    /// Synchronisation marker was not `INK1`.
    #[error("invalid INK1 magic")]
    Magic,
    /// Header dimensions do not describe the E213 panel.
    #[error("INK1 dimensions must be {WIDTH}x{HEIGHT}, got {width}x{height}")]
    Dimensions {
        /// Header width.
        width: u16,
        /// Header height.
        height: u16,
    },
    /// Reserved header byte is non-zero.
    #[error("INK1 reserved byte must be zero")]
    Reserved,
    /// Header contains a flag unsupported by the fixed protocol.
    #[error("INK1 contains unsupported flags {0:#04x}")]
    Flags(u8),
    /// Header payload length differs from the protocol constant.
    #[error("INK1 payload length must be {PAYLOAD_SIZE}, got {actual}")]
    PayloadSize {
        /// Length declared in the packet header.
        actual: u32,
    },
    /// Payload was altered or truncated.
    #[error("INK1 CRC mismatch: expected {expected:08x}, calculated {actual:08x}")]
    Crc {
        /// Checksum declared by the packet.
        expected: u32,
        /// Checksum calculated from the payload.
        actual: u32,
    },
}

/// Encodes a framebuffer using the byte-for-byte-compatible INK1 contract.
pub fn encode_packet(frame: &MonoFrame, refresh: RefreshMode) -> Vec<u8> {
    let payload = frame.packed();
    let checksum = crc32(payload);
    let mut packet = Vec::with_capacity(PACKET_SIZE);
    packet.extend_from_slice(&INK1_MAGIC);
    packet.extend_from_slice(&WIDTH.to_le_bytes());
    packet.extend_from_slice(&HEIGHT.to_le_bytes());
    packet.push(refresh.flags());
    packet.push(0);
    packet.extend_from_slice(&(PAYLOAD_SIZE as u32).to_le_bytes());
    packet.extend_from_slice(&checksum.to_le_bytes());
    packet.extend_from_slice(payload);
    debug_assert_eq!(packet.len(), PACKET_SIZE);
    packet
}

/// Validates a complete packet without copying its framebuffer.
pub fn validate_packet(packet: &[u8]) -> Result<ValidatedPacket<'_>, ProtocolError> {
    if packet.len() != PACKET_SIZE {
        return Err(ProtocolError::PacketSize {
            actual: packet.len(),
        });
    }
    if packet[..4] != INK1_MAGIC {
        return Err(ProtocolError::Magic);
    }
    let width = u16::from_le_bytes([packet[4], packet[5]]);
    let height = u16::from_le_bytes([packet[6], packet[7]]);
    if width != WIDTH || height != HEIGHT {
        return Err(ProtocolError::Dimensions { width, height });
    }
    if packet[9] != 0 {
        return Err(ProtocolError::Reserved);
    }
    if packet[8] & !FLAG_FULL_REFRESH != 0 {
        return Err(ProtocolError::Flags(packet[8]));
    }
    let length = u32::from_le_bytes(packet[10..14].try_into().expect("fixed slice"));
    if length != PAYLOAD_SIZE as u32 {
        return Err(ProtocolError::PayloadSize { actual: length });
    }
    let expected = u32::from_le_bytes(packet[14..18].try_into().expect("fixed slice"));
    let payload = &packet[HEADER_SIZE..];
    let actual = crc32(payload);
    if actual != expected {
        return Err(ProtocolError::Crc { expected, actual });
    }
    Ok(ValidatedPacket {
        flags: packet[8],
        payload,
    })
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn two_corner_packet_matches_python_sibling_golden() {
        let mut frame = MonoFrame::white();
        frame.set_black(0, 0, true);
        frame.set_black(WIDTH - 1, HEIGHT - 1, true);

        let packet = encode_packet(&frame, RefreshMode::Full);
        assert_eq!(
            hex(&packet[..HEADER_SIZE]),
            "494e4b31fa007a000100400f000042bf3f65"
        );
        assert_eq!(
            hex(&Sha256::digest(&packet)),
            "e74d18dc6424f4d04c710b98df6986f52d63d4380f8eea4cc05b378236a3313b"
        );
        let validated = validate_packet(&packet).unwrap();
        assert_eq!(validated.flags & FLAG_FULL_REFRESH, FLAG_FULL_REFRESH);
        assert_eq!(validated.payload[0], 0x80);
        assert_eq!(validated.payload[PAYLOAD_SIZE - 1], 0x40);
    }

    #[test]
    fn white_packet_matches_python_sibling_golden() {
        let packet = encode_packet(&MonoFrame::white(), RefreshMode::Fast);
        assert_eq!(
            hex(&packet[..HEADER_SIZE]),
            "494e4b31fa007a000000400f0000630cd070"
        );
        assert_eq!(
            hex(&Sha256::digest(&packet)),
            "e0468d504b0dd148ba9361764f8a0dab4d1fd58f4ca9628f9bcef7a4bb559a61"
        );
    }

    #[test]
    fn corrupt_payload_is_rejected() {
        let mut packet = encode_packet(&MonoFrame::white(), RefreshMode::Fast);
        packet[PACKET_SIZE - 1] ^= 1;
        assert!(matches!(
            validate_packet(&packet),
            Err(ProtocolError::Crc { .. })
        ));
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
