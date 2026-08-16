//! Physical transports for complete INK1 packets.

mod auto;
mod ble;
mod usb;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{ProtocolError, RefreshMode, encode_packet, validate_packet};

pub use auto::{AutoTransport, TransportPreference};
pub use ble::{BleConfig, BleConnectionInfo, BleDeviceInfo, BleTransport, discover_ble_devices};
pub use usb::{
    ESPRESSIF_USB_VID, UsbConfig, UsbConnectionInfo, UsbDeviceInfo, UsbTransport,
    discover_espressif_devices, discover_espressif_port,
};

/// Backward-compatible E213 GATT service UUID.
pub const SERVICE_UUID: Uuid = Uuid::from_u128(0x8b7a0000_4f4b_4a9b_9d6e_1d0c1a2b3c4d);
/// Host-to-board characteristic carrying INK1 packet chunks.
pub const RX_UUID: Uuid = Uuid::from_u128(0x8b7a0001_4f4b_4a9b_9d6e_1d0c1a2b3c4d);
/// Board-to-host characteristic carrying readiness and acknowledgements.
pub const TX_UUID: Uuid = Uuid::from_u128(0x8b7a0002_4f4b_4a9b_9d6e_1d0c1a2b3c4d);

/// Physical path which accepted a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    /// Native USB CDC serial.
    Usb,
    /// Bluetooth Low Energy GATT.
    Ble,
}

/// Positive acknowledgement from the E213 firmware.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportReceipt {
    /// Physical path used for this write.
    pub transport: TransportKind,
    /// Whether `READY INK1` was observed on this connection.
    pub ready_observed: bool,
    /// Exact non-secret acknowledgement text.
    pub acknowledgement: String,
}

/// USB/BLE transmission failure with enough detail for fallback and diagnosis.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    /// Packet failed local protocol validation and was never transmitted.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// No matching USB CDC interface was found.
    #[error("no Espressif USB serial device found")]
    NoUsbDevice,
    /// Bluetooth is unavailable on this host.
    #[error("no Bluetooth adapter is available")]
    NoBleAdapter,
    /// Scan did not find the configured E213.
    #[error("BLE device {name:?} was not found")]
    NoBleDevice {
        /// Advertised compatibility name used for discovery.
        name: String,
    },
    /// Connected peripheral did not expose the backward-compatible service.
    #[error("BLE device is missing {which} characteristic {uuid}")]
    MissingCharacteristic {
        /// Human-readable role.
        which: &'static str,
        /// Expected UUID.
        uuid: Uuid,
    },
    /// Underlying platform I/O failed.
    #[error("{transport:?} transport error: {message}")]
    Io {
        /// Path reporting the error.
        transport: TransportKind,
        /// Redacted platform message.
        message: String,
    },
    /// Firmware did not acknowledge before the deadline.
    #[error("timed out waiting for {waiting_for} over {transport:?}")]
    Timeout {
        /// Path which timed out.
        transport: TransportKind,
        /// Expected device response.
        waiting_for: &'static str,
    },
    /// Firmware explicitly rejected the packet.
    #[error("E213 rejected frame: {0}")]
    Nack(String),
    /// Requested path was not configured.
    #[error("{0:?} transport is not configured")]
    NotConfigured(TransportKind),
    /// Both paths failed while operating in automatic mode.
    #[error("USB failed ({usb}); BLE failed ({ble})")]
    AllFailed {
        /// USB failure text.
        usb: String,
        /// BLE failure text.
        ble: String,
    },
}

/// Async sink for one complete, validated INK1 packet.
#[async_trait]
pub trait PacketTransport: Send + Sync {
    /// Physical path implemented by this sink.
    fn kind(&self) -> TransportKind;

    /// Sends exactly one complete packet and waits for `ACK INK1`.
    async fn send_packet(&self, packet: &[u8]) -> Result<TransportReceipt, TransportError>;
}

/// Encodes and sends a frame through any packet transport.
pub async fn send_frame(
    transport: &dyn PacketTransport,
    frame: &crate::MonoFrame,
    refresh: RefreshMode,
) -> Result<TransportReceipt, TransportError> {
    transport.send_packet(&encode_packet(frame, refresh)).await
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DeviceReply {
    /// Carries the whole banner line, because it names the build running on the
    /// board. That identity used to be read off the wire and dropped, leaving
    /// the app unable to tell an up-to-date device from an unknown one.
    Ready(String),
    Ack,
    Nack(String),
}

pub(crate) fn device_reply(bytes: &[u8]) -> Option<DeviceReply> {
    let text = String::from_utf8_lossy(bytes);
    if let Some(start) = text.find("NACK") {
        let line = safe_device_text(text[start..].lines().next().unwrap_or("NACK"));
        return Some(DeviceReply::Nack(line));
    }
    if text.contains("ACK INK1") {
        return Some(DeviceReply::Ack);
    }
    if let Some(start) = text.find("READY INK1") {
        return Some(DeviceReply::Ready(safe_device_text(
            text[start..].lines().next().unwrap_or("READY INK1"),
        )));
    }
    if text.trim() == "READY" {
        return Some(DeviceReply::Ready("READY".into()));
    }
    None
}

fn safe_device_text(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len().min(96));
    let mut pending_space = false;
    for character in text.chars().take(96) {
        if character.is_control()
            || matches!(
                character,
                '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
            )
        {
            continue;
        }
        if character.is_whitespace() {
            pending_space = !sanitized.is_empty();
            continue;
        }
        if pending_space {
            sanitized.push(' ');
            pending_space = false;
        }
        sanitized.push(character);
    }
    if sanitized.is_empty() {
        "NACK".into()
    } else {
        sanitized
    }
}

pub(crate) fn validate_for_transport(packet: &[u8]) -> Result<(), TransportError> {
    validate_packet(packet)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nack_is_not_misread_as_ack() {
        assert_eq!(
            device_reply(b"NACK CRC\nACK INK1\n"),
            Some(DeviceReply::Nack("NACK CRC".into()))
        );
    }

    #[test]
    fn nack_text_strips_bidirectional_controls() {
        assert_eq!(
            device_reply("NACK safe\u{202e}txt\u{2066}\n".as_bytes()),
            Some(DeviceReply::Nack("NACK safetxt".into()))
        );
    }
}
