use crc32fast::Hasher;
use thiserror::Error;

use crate::{MonoFrame, PanelModel};

/// Size of the INK1 header.
pub const HEADER_SIZE: usize = 18;
/// Four-byte packet synchronisation marker.
pub const INK1_MAGIC: [u8; 4] = *b"INK1";
/// Requests the firmware's slower full-refresh waveform.
pub const FLAG_FULL_REFRESH: u8 = 0x01;

/// Largest complete packet any supported panel produces.
///
/// The firmware and the transports size their assembly buffers from this, so a
/// panel added later that draws more pixels than the E290 must be reflected
/// here or its frames will not fit the buffer meant to hold them.
pub const MAX_PACKET_SIZE: usize = HEADER_SIZE + max_payload_size();

const fn max_payload_size() -> usize {
    let mut largest = 0;
    let mut index = 0;
    while index < PanelModel::ALL.len() {
        let payload = PanelModel::ALL[index].payload_size();
        if payload > largest {
            largest = payload;
        }
        index += 1;
    }
    largest
}

/// Size of one complete packet for a panel.
pub const fn packet_size(panel: PanelModel) -> usize {
    HEADER_SIZE + panel.payload_size()
}

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
    /// Panel the header's dimensions describe.
    pub panel: PanelModel,
    /// Packet flags as transmitted.
    pub flags: u8,
    /// Validated packed framebuffer bytes.
    pub payload: &'a [u8],
}

/// INK1 packet validation failure.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    /// Packet byte count matches no supported panel.
    #[error("INK1 packet size {actual} matches no supported panel")]
    PacketSize {
        /// Observed packet byte count.
        actual: usize,
    },
    /// Synchronisation marker was not `INK1`.
    #[error("invalid INK1 magic")]
    Magic,
    /// Header dimensions describe no panel this host draws.
    #[error("INK1 dimensions {width}x{height} match no supported panel")]
    Dimensions {
        /// Header width.
        width: u16,
        /// Header height.
        height: u16,
    },
    /// Header dimensions name a panel other than the one the packet is sized
    /// for, so one of the two is a lie.
    #[error("INK1 header claims {claimed} but the packet carries {carried} bytes of payload")]
    DimensionsDisagree {
        /// Panel named by the header.
        claimed: &'static str,
        /// Payload bytes actually present.
        carried: usize,
    },
    /// Reserved header byte is non-zero.
    #[error("INK1 reserved byte must be zero")]
    Reserved,
    /// Header contains a flag unsupported by the fixed protocol.
    #[error("INK1 contains unsupported flags {0:#04x}")]
    Flags(u8),
    /// Header payload length differs from what the named panel requires.
    #[error("INK1 payload length must be {expected} for this panel, got {actual}")]
    PayloadSize {
        /// Length the named panel requires.
        expected: usize,
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
///
/// The dimensions come from the frame rather than from a constant, which is the
/// whole of what makes a second panel possible on this wire: the header always
/// said how big the picture was, and only the host's assumption that there was
/// one answer had to go.
pub fn encode_packet(frame: &MonoFrame, refresh: RefreshMode) -> Vec<u8> {
    let payload = frame.packed();
    let checksum = crc32(payload);
    let mut packet = Vec::with_capacity(HEADER_SIZE + payload.len());
    packet.extend_from_slice(&INK1_MAGIC);
    packet.extend_from_slice(&frame.width().to_le_bytes());
    packet.extend_from_slice(&frame.height().to_le_bytes());
    packet.push(refresh.flags());
    packet.push(0);
    packet.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    packet.extend_from_slice(&checksum.to_le_bytes());
    packet.extend_from_slice(payload);
    debug_assert_eq!(packet.len(), packet_size(frame.panel()));
    packet
}

/// Validates a complete packet without copying its framebuffer.
pub fn validate_packet(packet: &[u8]) -> Result<ValidatedPacket<'_>, ProtocolError> {
    if packet.len() < HEADER_SIZE {
        return Err(ProtocolError::PacketSize {
            actual: packet.len(),
        });
    }
    if packet[..4] != INK1_MAGIC {
        return Err(ProtocolError::Magic);
    }
    let width = u16::from_le_bytes([packet[4], packet[5]]);
    let height = u16::from_le_bytes([packet[6], packet[7]]);
    let panel = PanelModel::from_dimensions(width, height)
        .ok_or(ProtocolError::Dimensions { width, height })?;
    if packet.len() != packet_size(panel) {
        return Err(ProtocolError::DimensionsDisagree {
            claimed: panel.label(),
            carried: packet.len().saturating_sub(HEADER_SIZE),
        });
    }
    if packet[9] != 0 {
        return Err(ProtocolError::Reserved);
    }
    if packet[8] & !FLAG_FULL_REFRESH != 0 {
        return Err(ProtocolError::Flags(packet[8]));
    }
    let length = u32::from_le_bytes(packet[10..14].try_into().expect("fixed slice"));
    if length as usize != panel.payload_size() {
        return Err(ProtocolError::PayloadSize {
            expected: panel.payload_size(),
            actual: length,
        });
    }
    let expected = u32::from_le_bytes(packet[14..18].try_into().expect("fixed slice"));
    let payload = &packet[HEADER_SIZE..];
    let actual = crc32(payload);
    if actual != expected {
        return Err(ProtocolError::Crc { expected, actual });
    }
    Ok(ValidatedPacket {
        panel,
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

    /// The E213 packet is a published contract with a working sibling
    /// implementation. Byte-for-byte, it must survive learning about a second
    /// panel.
    #[test]
    fn two_corner_packet_matches_python_sibling_golden() {
        let mut frame = MonoFrame::white(PanelModel::E213);
        frame.set_black(0, 0, true);
        frame.set_black(frame.width() - 1, frame.height() - 1, true);

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
        assert_eq!(validated.panel, PanelModel::E213);
        assert_eq!(validated.flags & FLAG_FULL_REFRESH, FLAG_FULL_REFRESH);
        assert_eq!(validated.payload[0], 0x80);
        assert_eq!(validated.payload[validated.payload.len() - 1], 0x40);
    }

    #[test]
    fn white_packet_matches_python_sibling_golden() {
        let packet = encode_packet(&MonoFrame::white(PanelModel::E213), RefreshMode::Fast);
        assert_eq!(
            hex(&packet[..HEADER_SIZE]),
            "494e4b31fa007a000000400f0000630cd070"
        );
        assert_eq!(
            hex(&Sha256::digest(&packet)),
            "e0468d504b0dd148ba9361764f8a0dab4d1fd58f4ca9628f9bcef7a4bb559a61"
        );
    }

    /// The E290 travels the same wire: same magic, same header shape, its own
    /// dimensions and length.
    #[test]
    fn an_e290_packet_announces_its_own_geometry() {
        let packet = encode_packet(&MonoFrame::white(PanelModel::E290), RefreshMode::Fast);
        assert_eq!(packet.len(), 18 + 4736);
        assert_eq!(&packet[..4], b"INK1");
        assert_eq!(u16::from_le_bytes([packet[4], packet[5]]), 296);
        assert_eq!(u16::from_le_bytes([packet[6], packet[7]]), 128);
        assert_eq!(u32::from_le_bytes(packet[10..14].try_into().unwrap()), 4736);
        assert_eq!(validate_packet(&packet).unwrap().panel, PanelModel::E290);
    }

    #[test]
    fn corrupt_payload_is_rejected() {
        let mut packet = encode_packet(&MonoFrame::white(PanelModel::E290), RefreshMode::Fast);
        let last = packet.len() - 1;
        packet[last] ^= 1;
        assert!(matches!(
            validate_packet(&packet),
            Err(ProtocolError::Crc { .. })
        ));
    }

    /// A header naming one panel on a packet sized for the other is the failure
    /// a second geometry makes possible, and it must not be drawn.
    #[test]
    fn a_header_that_disagrees_with_the_payload_is_refused() {
        let mut packet = encode_packet(&MonoFrame::white(PanelModel::E290), RefreshMode::Fast);
        packet[4..6].copy_from_slice(&250u16.to_le_bytes());
        packet[6..8].copy_from_slice(&122u16.to_le_bytes());
        assert!(matches!(
            validate_packet(&packet),
            Err(ProtocolError::DimensionsDisagree { .. })
        ));
    }

    #[test]
    fn an_unknown_geometry_is_refused_rather_than_rounded() {
        let mut packet = encode_packet(&MonoFrame::white(PanelModel::E213), RefreshMode::Fast);
        packet[4..6].copy_from_slice(&400u16.to_le_bytes());
        assert!(matches!(
            validate_packet(&packet),
            Err(ProtocolError::Dimensions {
                width: 400,
                height: 122
            })
        ));
    }

    #[test]
    fn the_buffer_bound_covers_every_panel() {
        for panel in PanelModel::ALL {
            assert!(packet_size(panel) <= MAX_PACKET_SIZE);
        }
        assert_eq!(MAX_PACKET_SIZE, 18 + 4736);
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
