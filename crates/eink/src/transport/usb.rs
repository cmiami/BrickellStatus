use std::time::Duration;

use async_trait::async_trait;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::Mutex,
    time::{sleep, timeout},
};
use tokio_serial::{
    FlowControl, SerialPort as _, SerialPortBuilderExt, SerialPortType, SerialStream,
};

use super::{
    DeviceReply, PacketTransport, TransportError, TransportKind, TransportReceipt, device_reply,
    safe_device_text, validate_for_transport,
};

/// How a supported panel exposes its serial port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbDeviceKind {
    /// ESP32-S3 native USB CDC / Serial-JTAG, used by Vision Master boards.
    EspressifNative,
    /// Silicon Labs CP210x USB-to-UART bridge used by Wireless Paper.
    /// Detection uses the exact default VID/PID shipped on the board; the flash
    /// layer still constrains this interface to Wireless Paper firmware.
    Cp210xUart,
}

/// One compatible panel USB serial interface discovered on the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbDeviceInfo {
    /// The board's own identifier, stable across reboots and reflashes — on an
    /// ESP32-S3 this is its MAC address. It is the only thing about an attached
    /// board that does not change when the firmware does, which makes it the
    /// key anything wanting to remember a board has to use.
    pub serial_number: Option<String>,
    /// USB interface family, used to constrain a deliberate firmware write to
    /// the corresponding board pinout.
    pub kind: UsbDeviceKind,
    /// Stable serial device path used to open this interface.
    pub port: String,
    /// Concise device label assembled from USB descriptors.
    pub name: String,
    /// Additional non-secret descriptor text for setup UI.
    pub detail: String,
}

/// Result of opening a native USB serial interface without writing a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbConnectionInfo {
    /// Exact serial path which was opened.
    pub port: String,
    /// Whether the firmware's `READY INK1` banner was observed.
    pub ready_observed: bool,
    /// The banner exactly as the board sent it, when one arrived. It names the
    /// build, which is the only self-reported identity the device offers.
    pub banner: Option<String>,
}

/// USB vendor ID used by the ESP32-S3 native USB/JTAG interface.
pub const ESPRESSIF_USB_VID: u16 = 0x303a;
/// Silicon Labs vendor ID used by the Wireless Paper's CP2102 bridge.
pub const CP210X_USB_VID: u16 = 0x10c4;
/// Default CP2102/CP210x UART bridge product ID.
pub const CP210X_USB_PID: u16 = 0xea60;

/// Native USB CDC configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbConfig {
    /// Explicit device path; `None` discovers a compatible panel interface.
    pub port: Option<String>,
    /// Firmware serial rate.
    pub baud_rate: u32,
    /// Packet chunk size used to protect the ESP32 receive FIFO.
    pub chunk_size: usize,
    /// Delay between bounded chunks.
    pub chunk_delay: Duration,
    /// Settling time after opening the native USB device.
    pub startup_delay: Duration,
    /// Short compatibility probe for `READY INK1`.
    pub ready_timeout: Duration,
    /// Maximum display-refresh/acknowledgement time.
    pub acknowledgement_timeout: Duration,
}

impl Default for UsbConfig {
    fn default() -> Self {
        Self {
            port: None,
            baud_rate: 115_200,
            chunk_size: 256,
            chunk_delay: Duration::from_millis(4),
            startup_delay: Duration::from_millis(750),
            ready_timeout: Duration::from_millis(300),
            acknowledgement_timeout: Duration::from_secs(15),
        }
    }
}

struct UsbState {
    stream: Option<SerialStream>,
    ready_observed: bool,
    /// Retained so a later status query can read the build without reopening
    /// the port, which the display worker usually already holds.
    banner: Option<String>,
}

/// Tokio serial writer for the E213 native USB interface.
pub struct UsbTransport {
    config: UsbConfig,
    state: Mutex<UsbState>,
}

impl UsbTransport {
    /// Creates a lazily connected USB transport.
    pub fn new(config: UsbConfig) -> Self {
        Self {
            config,
            state: Mutex::new(UsbState {
                stream: None,
                ready_observed: false,
                banner: None,
            }),
        }
    }

    async fn connect(&self) -> Result<(SerialStream, Option<String>, String), TransportError> {
        let port_name = match &self.config.port {
            Some(port) => {
                let attached = discover_panel_usb_devices().await?;
                if !attached.iter().any(|device| device.port == *port) {
                    return Err(TransportError::Io {
                        transport: TransportKind::Usb,
                        message: format!(
                            "{port} is not an attached compatible panel USB interface"
                        ),
                    });
                }
                port.clone()
            }
            None => discover_panel_usb_port()
                .await?
                .ok_or(TransportError::NoUsbDevice)?,
        };
        let builder = tokio_serial::new(&port_name, self.config.baud_rate)
            .flow_control(FlowControl::None)
            .dtr_on_open(false)
            .timeout(Duration::from_millis(250));
        let mut stream = builder
            .open_native_async()
            .map_err(|error| usb_io(error.to_string()))?;

        // Clearing both modem-control signals avoids the ESP32 auto-reset
        // circuit and protects the first frame after connect.
        stream
            .write_data_terminal_ready(false)
            .map_err(|error| usb_io(error.to_string()))?;
        stream
            .write_request_to_send(false)
            .map_err(|error| usb_io(error.to_string()))?;

        sleep(self.config.startup_delay).await;
        let banner = request_identity(&mut stream, self.config.ready_timeout).await?;
        Ok((stream, banner, port_name))
    }

    /// Opens the configured interface and retains it for later frame writes.
    ///
    /// A successful result proves only that the operating system accepted the
    /// serial connection. `ready_observed` is the non-destructive firmware
    /// identity check; callers must not present a silent port as a verified
    /// E213 route before an explicit frame receives `ACK INK1`.
    pub async fn ensure_connected(&self) -> Result<UsbConnectionInfo, TransportError> {
        let mut state = self.state.lock().await;
        if state.stream.is_none() {
            let (stream, banner, port) = self.connect().await?;
            state.stream = Some(stream);
            state.ready_observed = banner.is_some();
            state.banner = banner.clone();
            return Ok(UsbConnectionInfo {
                port,
                ready_observed: banner.is_some(),
                banner,
            });
        }
        Ok(UsbConnectionInfo {
            port: self
                .config
                .port
                .clone()
                .unwrap_or_else(|| "Panel USB".into()),
            ready_observed: state.ready_observed,
            banner: state.banner.clone(),
        })
    }

    /// Asks an already-open panel for its current READY banner without drawing
    /// or refreshing the e-paper glass.
    ///
    /// Current firmware uses this lightweight identity query to expose a fresh
    /// battery reading. A legacy panel that ignores the query returns `None`.
    pub async fn read_banner(&self) -> Result<Option<String>, TransportError> {
        let mut state = self.state.lock().await;
        let Some(stream) = state.stream.as_mut() else {
            return Ok(None);
        };
        let banner = request_identity(stream, self.config.ready_timeout).await?;
        if let Some(banner) = banner.as_ref() {
            state.ready_observed = true;
            state.banner = Some(banner.clone());
        }
        Ok(banner)
    }

    /// Closes the retained serial interface without sending data.
    pub async fn disconnect(&self) {
        let mut state = self.state.lock().await;
        state.stream = None;
        state.ready_observed = false;
        state.banner = None;
    }

    async fn send_on_stream(
        &self,
        stream: &mut SerialStream,
        packet: &[u8],
    ) -> Result<String, TransportError> {
        let chunk_size = self.config.chunk_size.max(1);
        for chunk in packet.chunks(chunk_size) {
            stream
                .write_all(chunk)
                .await
                .map_err(|error| usb_io(error.to_string()))?;
            sleep(self.config.chunk_delay).await;
        }
        stream
            .flush()
            .await
            .map_err(|error| usb_io(error.to_string()))?;
        wait_for_ack(stream, self.config.acknowledgement_timeout).await
    }
}

#[async_trait]
impl PacketTransport for UsbTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Usb
    }

    async fn send_packet(&self, packet: &[u8]) -> Result<TransportReceipt, TransportError> {
        validate_for_transport(packet)?;
        let mut state = self.state.lock().await;
        if state.stream.is_none() {
            let (stream, banner, _) = self.connect().await?;
            state.stream = Some(stream);
            state.ready_observed = banner.is_some();
            state.banner = banner;
        }

        let result = self
            .send_on_stream(state.stream.as_mut().expect("connected above"), packet)
            .await;
        match result {
            Ok(acknowledgement) => Ok(TransportReceipt {
                transport: TransportKind::Usb,
                ready_observed: state.ready_observed,
                acknowledgement,
            }),
            Err(error) => {
                state.stream = None;
                state.ready_observed = false;
                state.banner = None;
                Err(error)
            }
        }
    }
}

/// Finds the first supported panel USB path without opening it.
///
/// Native USB retains priority when both product families are attached. A lone
/// exact CP2102 match is the Wireless Paper first-run path, which keeps setup to
/// the same detect-and-flash interaction as the existing boards.
pub async fn discover_panel_usb_port() -> Result<Option<String>, TransportError> {
    let devices = discover_panel_usb_devices().await?;
    Ok(devices
        .iter()
        .find(|device| device.kind == UsbDeviceKind::EspressifNative)
        .or_else(|| {
            devices
                .iter()
                .find(|device| device.kind == UsbDeviceKind::Cp210xUart)
        })
        .map(|device| device.port.clone()))
}

/// Drops the `/dev/tty.*` twin of a device that also offers `/dev/cu.*`.
///
/// macOS exposes one USB serial device under two nodes. They are the same
/// hardware, so listing both offered a picker with each board in it twice, and
/// the two entries did not behave alike: opening `cu` returns immediately,
/// while opening `tty` blocks waiting for carrier detect that a CDC device
/// never asserts, so choosing the wrong twin sat there and then timed out.
///
/// `cu` is the outbound node and the one to open. A `tty` entry is kept only
/// when nothing offers the matching `cu`, so a platform that names things
/// differently still lists its devices.
fn prefer_callout_nodes(devices: &mut Vec<UsbDeviceInfo>) {
    let callouts: std::collections::HashSet<String> = devices
        .iter()
        .filter_map(|device| device.port.strip_prefix("/dev/cu."))
        .map(str::to_owned)
        .collect();
    devices.retain(|device| match device.port.strip_prefix("/dev/tty.") {
        Some(suffix) => !callouts.contains(suffix),
        None => true,
    });
}

/// Identifies serial interfaces the supported panels use.
///
/// CP2102 matching is deliberately exact to keep the scan list narrow. A
/// running board still proves itself with `READY INK1`; a blank exact match is
/// offered only the Wireless Paper image by the desktop flash guard.
fn classify_usb_device(vid: u16, pid: u16, descriptor: &str) -> Option<UsbDeviceKind> {
    if vid == ESPRESSIF_USB_VID
        || descriptor.contains("espressif")
        || descriptor.contains("usb jtag")
    {
        return Some(UsbDeviceKind::EspressifNative);
    }
    if vid == CP210X_USB_VID && pid == CP210X_USB_PID {
        return Some(UsbDeviceKind::Cp210xUart);
    }
    None
}

/// Lists compatible panel USB serial interfaces without opening them.
pub async fn discover_panel_usb_devices() -> Result<Vec<UsbDeviceInfo>, TransportError> {
    tokio::task::spawn_blocking(|| {
        let ports = tokio_serial::available_ports().map_err(|error| usb_io(error.to_string()))?;
        let mut devices = ports
            .into_iter()
            .filter_map(|port| match port.port_type {
                SerialPortType::UsbPort(details) => {
                    let product = details.product.as_deref().unwrap_or_default().trim();
                    let manufacturer = details.manufacturer.as_deref().unwrap_or_default().trim();
                    let descriptor = format!("{product} {manufacturer}").to_ascii_lowercase();
                    classify_usb_device(details.vid, details.pid, &descriptor).map(|kind| {
                        let name = if product.is_empty() {
                            match kind {
                                UsbDeviceKind::EspressifNative => {
                                    "Unverified Espressif serial device".into()
                                }
                                UsbDeviceKind::Cp210xUart => "Wireless Paper USB".into(),
                            }
                        } else {
                            product.to_owned()
                        };
                        let mut facts =
                            vec![format!("VID {:04X} · PID {:04X}", details.vid, details.pid)];
                        if !manufacturer.is_empty() {
                            facts.push(manufacturer.to_owned());
                        }
                        UsbDeviceInfo {
                            // A native ESP32 descriptor identifies the MCU.
                            // A CP2102 serial identifies only the bridge and is
                            // commonly the non-unique factory value "0001", so
                            // it must not become a durable board identity.
                            serial_number: (kind == UsbDeviceKind::EspressifNative)
                                .then_some(details.serial_number.as_deref())
                                .flatten()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_owned),
                            kind,
                            port: port.port_name,
                            name,
                            detail: facts.join(" · "),
                        }
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        prefer_callout_nodes(&mut devices);
        devices.sort_by(|left, right| left.port.cmp(&right.port));
        Ok(devices)
    })
    .await
    .map_err(|error| usb_io(error.to_string()))?
}

/// Reads the boot banner, returning it verbatim.
///
/// The banner names the build on the board. Returning only "did it speak"
/// discarded that, which is why a device running exactly the bundled firmware
/// still reported an unknown build.
/// Asks running BrickellStatus firmware to repeat its identity before waiting.
///
/// The boot banner remains compatible with older firmware, but it is a
/// one-time line and USB re-enumeration can happen after it was printed. The
/// one-byte query makes a freshly flashed image verifiable at any point. Older
/// firmware ignores the byte, while a wrong E213 controller image is blocked
/// before its decoder loop and remains objectively silent.
async fn request_identity<Stream>(
    stream: &mut Stream,
    duration: Duration,
) -> Result<Option<String>, TransportError>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
{
    stream
        .write_all(b"?")
        .await
        .map_err(|error| usb_io(error.to_string()))?;
    stream
        .flush()
        .await
        .map_err(|error| usb_io(error.to_string()))?;
    probe_ready(stream, duration).await
}

async fn probe_ready<Stream>(
    stream: &mut Stream,
    duration: Duration,
) -> Result<Option<String>, TransportError>
where
    Stream: AsyncRead + Unpin,
{
    let mut received = Vec::new();
    let outcome = timeout(duration, async {
        let mut chunk = [0_u8; 256];
        loop {
            let count = stream
                .read(&mut chunk)
                .await
                .map_err(|error| usb_io(error.to_string()))?;
            if count == 0 {
                tokio::task::yield_now().await;
                continue;
            }
            received.extend_from_slice(&chunk[..count]);
            match device_reply(&received) {
                Some(DeviceReply::Ready(banner)) if serial_line_complete(&received, "READY") => {
                    return Ok(Some(banner));
                }
                Some(DeviceReply::Nack(message)) if serial_line_complete(&received, "NACK") => {
                    return Err(TransportError::Nack(message));
                }
                Some(DeviceReply::Ack) | None => {}
                Some(DeviceReply::Ready(_) | DeviceReply::Nack(_)) => {}
            }
        }
    })
    .await;
    match outcome {
        Ok(result) => result,
        Err(_) => Ok(None),
    }
}

async fn wait_for_ack(
    stream: &mut SerialStream,
    duration: Duration,
) -> Result<String, TransportError> {
    let mut received = Vec::new();
    timeout(duration, async {
        let mut chunk = [0_u8; 256];
        loop {
            let count = stream
                .read(&mut chunk)
                .await
                .map_err(|error| usb_io(error.to_string()))?;
            if count == 0 {
                tokio::task::yield_now().await;
                continue;
            }
            received.extend_from_slice(&chunk[..count]);
            match device_reply(&received) {
                Some(DeviceReply::Ack) if serial_line_complete(&received, "ACK INK1") => {
                    let text = String::from_utf8_lossy(&received);
                    let start = text.find("ACK INK1").expect("reply matched ACK above");
                    return Ok(safe_device_text(
                        text[start..].lines().next().unwrap_or("ACK INK1"),
                    ));
                }
                Some(DeviceReply::Nack(message)) if serial_line_complete(&received, "NACK") => {
                    return Err(TransportError::Nack(message));
                }
                Some(DeviceReply::Ack | DeviceReply::Ready(_) | DeviceReply::Nack(_)) | None => {}
            }
        }
    })
    .await
    .map_err(|_| TransportError::Timeout {
        transport: TransportKind::Usb,
        waiting_for: "ACK INK1",
    })?
}

/// Serial reads may split one firmware reply at any byte. Wait for the CR/LF
/// written by `Serial.println` before parsing it; otherwise a long READY banner
/// can be cached before its trailing `FW<n>` version reaches the host.
fn serial_line_complete(bytes: &[u8], marker: &str) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.find(marker).is_some_and(|start| {
        text[start..]
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n'))
    })
}

fn usb_io(message: String) -> TransportError {
    TransportError::Io {
        transport: TransportKind::Usb,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn identity_query_recovers_a_repeatable_ready_banner() {
        let (mut host, mut device) = duplex(256);
        let responder = tokio::spawn(async move {
            let mut query = [0_u8; 1];
            device.read_exact(&mut query).await.unwrap();
            assert_eq!(query, [b'?']);
            device
                .write_all(b"READY INK1 250x122 3904 abc1234 E213 26B4\n")
                .await
                .unwrap();
        });

        let banner = request_identity(&mut host, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            banner.as_deref(),
            Some("READY INK1 250x122 3904 abc1234 E213 26B4")
        );
        responder.await.unwrap();
    }

    #[tokio::test]
    async fn identity_query_waits_for_a_fragmented_versioned_banner() {
        let (mut host, mut device) = duplex(256);
        let responder = tokio::spawn(async move {
            let mut query = [0_u8; 1];
            device.read_exact(&mut query).await.unwrap();
            assert_eq!(query, [b'?']);
            device
                .write_all(
                    b"PROBE attempt=0 pin1=driven pin6=floating\r\nREADY INK1 250x122 3904 e0f6a4d-dirty-261db27dbf0f E213 26B4",
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
            device.write_all(b" FW4 BAT4270\r\n").await.unwrap();
        });

        let banner = request_identity(&mut host, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            banner.as_deref(),
            Some("READY INK1 250x122 3904 e0f6a4d-dirty-261db27dbf0f E213 26B4 FW4 BAT4270")
        );
        responder.await.unwrap();
    }

    #[tokio::test]
    async fn identity_query_has_a_bounded_silent_result() {
        let (mut host, _silent_device) = duplex(16);
        assert_eq!(
            request_identity(&mut host, Duration::from_millis(1))
                .await
                .unwrap(),
            None
        );
    }

    #[test]
    fn usb_classification_accepts_both_supported_panel_interfaces() {
        assert_eq!(
            classify_usb_device(ESPRESSIF_USB_VID, 0x1001, "esp32-s3 usb jtag"),
            Some(UsbDeviceKind::EspressifNative)
        );
        assert_eq!(
            classify_usb_device(CP210X_USB_VID, CP210X_USB_PID, "cp2102 usb to uart"),
            Some(UsbDeviceKind::Cp210xUart)
        );
        assert_eq!(
            classify_usb_device(0x1209, 0x0001, "custom cp210x bridge"),
            None
        );
        assert_eq!(classify_usb_device(0x1a86, 0x7523, "ch340"), None);
    }
}
