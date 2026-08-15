use std::sync::Arc;

use tokio::sync::Mutex;

use super::{
    BleConfig, BleTransport, PacketTransport, TransportError, TransportKind, TransportReceipt,
    UsbConfig, UsbTransport, validate_for_transport,
};
use crate::{MonoFrame, RefreshMode, encode_packet};

/// Physical path selection for the automatic frame writer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransportPreference {
    /// Try USB first and fall back to BLE only after a USB failure.
    #[default]
    Auto,
    /// Require native USB CDC.
    Usb,
    /// Require Bluetooth Low Energy.
    Ble,
}

/// Serialises frame writes and applies the configured USB-first fallback.
pub struct AutoTransport {
    preference: TransportPreference,
    usb: Option<Arc<dyn PacketTransport>>,
    ble: Option<Arc<dyn PacketTransport>>,
    writer: Mutex<()>,
}

impl AutoTransport {
    /// Creates a hardware writer with both standard transports available.
    pub fn hardware(preference: TransportPreference, usb: UsbConfig, ble: BleConfig) -> Self {
        Self::with_transports(
            preference,
            Some(Arc::new(UsbTransport::new(usb))),
            Some(Arc::new(BleTransport::new(ble))),
        )
    }

    /// Creates a writer from injected transports, useful for tests and custom hosts.
    pub fn with_transports(
        preference: TransportPreference,
        usb: Option<Arc<dyn PacketTransport>>,
        ble: Option<Arc<dyn PacketTransport>>,
    ) -> Self {
        Self {
            preference,
            usb,
            ble,
            writer: Mutex::new(()),
        }
    }

    /// Encodes and sends one framebuffer while holding the single-writer gate.
    pub async fn send_frame(
        &self,
        frame: &MonoFrame,
        refresh: RefreshMode,
    ) -> Result<TransportReceipt, TransportError> {
        self.send_packet(&encode_packet(frame, refresh)).await
    }

    /// Sends one already encoded packet while holding the single-writer gate.
    pub async fn send_packet(&self, packet: &[u8]) -> Result<TransportReceipt, TransportError> {
        validate_for_transport(packet)?;
        let _writer = self.writer.lock().await;
        match self.preference {
            TransportPreference::Usb => {
                self.usb
                    .as_ref()
                    .ok_or(TransportError::NotConfigured(TransportKind::Usb))?
                    .send_packet(packet)
                    .await
            }
            TransportPreference::Ble => {
                self.ble
                    .as_ref()
                    .ok_or(TransportError::NotConfigured(TransportKind::Ble))?
                    .send_packet(packet)
                    .await
            }
            TransportPreference::Auto => {
                let usb = match &self.usb {
                    Some(usb) => match usb.send_packet(packet).await {
                        Ok(receipt) => return Ok(receipt),
                        Err(error) => error.to_string(),
                    },
                    None => TransportError::NotConfigured(TransportKind::Usb).to_string(),
                };
                match &self.ble {
                    Some(ble) => {
                        ble.send_packet(packet)
                            .await
                            .map_err(|error| TransportError::AllFailed {
                                usb,
                                ble: error.to_string(),
                            })
                    }
                    None => Err(TransportError::AllFailed {
                        usb,
                        ble: TransportError::NotConfigured(TransportKind::Ble).to_string(),
                    }),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;

    use super::*;
    use crate::{MonoFrame, RefreshMode};

    struct FakeTransport {
        kind: TransportKind,
        calls: AtomicUsize,
        fail: bool,
    }

    #[async_trait]
    impl PacketTransport for FakeTransport {
        fn kind(&self) -> TransportKind {
            self.kind
        }

        async fn send_packet(&self, packet: &[u8]) -> Result<TransportReceipt, TransportError> {
            validate_for_transport(packet)?;
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(TransportError::Io {
                    transport: self.kind,
                    message: "synthetic failure".into(),
                });
            }
            Ok(TransportReceipt {
                transport: self.kind,
                ready_observed: true,
                acknowledgement: "ACK INK1".into(),
            })
        }
    }

    #[tokio::test]
    async fn auto_prefers_usb_without_touching_ble() {
        let usb = Arc::new(FakeTransport {
            kind: TransportKind::Usb,
            calls: AtomicUsize::new(0),
            fail: false,
        });
        let ble = Arc::new(FakeTransport {
            kind: TransportKind::Ble,
            calls: AtomicUsize::new(0),
            fail: false,
        });
        let transport = AutoTransport::with_transports(
            TransportPreference::Auto,
            Some(usb.clone()),
            Some(ble.clone()),
        );
        let receipt = transport
            .send_frame(&MonoFrame::white(), RefreshMode::Fast)
            .await
            .unwrap();
        assert_eq!(receipt.transport, TransportKind::Usb);
        assert_eq!(usb.calls.load(Ordering::SeqCst), 1);
        assert_eq!(ble.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn auto_falls_back_to_ble_after_usb_failure() {
        let usb = Arc::new(FakeTransport {
            kind: TransportKind::Usb,
            calls: AtomicUsize::new(0),
            fail: true,
        });
        let ble = Arc::new(FakeTransport {
            kind: TransportKind::Ble,
            calls: AtomicUsize::new(0),
            fail: false,
        });
        let transport = AutoTransport::with_transports(
            TransportPreference::Auto,
            Some(usb.clone()),
            Some(ble.clone()),
        );
        let receipt = transport
            .send_frame(&MonoFrame::white(), RefreshMode::Fast)
            .await
            .unwrap();
        assert_eq!(receipt.transport, TransportKind::Ble);
        assert_eq!(usb.calls.load(Ordering::SeqCst), 1);
        assert_eq!(ble.calls.load(Ordering::SeqCst), 1);
    }
}
