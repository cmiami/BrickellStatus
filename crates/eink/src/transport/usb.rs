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
    validate_for_transport,
};

/// One Espressif-compatible native USB serial interface discovered on the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbDeviceInfo {
    /// The board's own identifier, stable across reboots and reflashes — on an
    /// ESP32-S3 this is its MAC address. It is the only thing about an attached
    /// board that does not change when the firmware does, which makes it the
    /// key anything wanting to remember a board has to use.
    pub serial_number: Option<String>,
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

/// Native USB CDC configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbConfig {
    /// Explicit device path; `None` discovers an Espressif interface.
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
                let attached = discover_espressif_devices().await?;
                if !attached.iter().any(|device| device.port == *port) {
                    return Err(TransportError::Io {
                        transport: TransportKind::Usb,
                        message: format!(
                            "{port} is not an attached Espressif USB display interface"
                        ),
                    });
                }
                port.clone()
            }
            None => discover_espressif_port()
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
                .unwrap_or_else(|| "Espressif USB".into()),
            ready_observed: state.ready_observed,
            banner: state.banner.clone(),
        })
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
    ) -> Result<(), TransportError> {
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
            Ok(()) => Ok(TransportReceipt {
                transport: TransportKind::Usb,
                ready_observed: state.ready_observed,
                acknowledgement: "ACK INK1".into(),
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

/// Finds the best available Espressif USB CDC path without opening it.
pub async fn discover_espressif_port() -> Result<Option<String>, TransportError> {
    Ok(discover_espressif_devices()
        .await?
        .into_iter()
        .next()
        .map(|device| device.port))
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

/// Lists compatible Espressif USB serial interfaces without opening them.
pub async fn discover_espressif_devices() -> Result<Vec<UsbDeviceInfo>, TransportError> {
    tokio::task::spawn_blocking(|| {
        let ports = tokio_serial::available_ports().map_err(|error| usb_io(error.to_string()))?;
        let mut devices = ports
            .into_iter()
            .filter_map(|port| match port.port_type {
                SerialPortType::UsbPort(details) => {
                    let product = details.product.as_deref().unwrap_or_default().trim();
                    let manufacturer = details.manufacturer.as_deref().unwrap_or_default().trim();
                    let descriptor = format!("{product} {manufacturer}").to_ascii_lowercase();
                    let compatible = details.vid == ESPRESSIF_USB_VID
                        || descriptor.contains("espressif")
                        || descriptor.contains("usb jtag");
                    compatible.then(|| {
                        let name = if product.is_empty() {
                            "Unverified Espressif serial device".into()
                        } else {
                            product.to_owned()
                        };
                        let mut facts =
                            vec![format!("VID {:04X} · PID {:04X}", details.vid, details.pid)];
                        if !manufacturer.is_empty() {
                            facts.push(manufacturer.to_owned());
                        }
                        UsbDeviceInfo {
                            serial_number: details
                                .serial_number
                                .as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_owned),
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
                Some(DeviceReply::Ready(banner)) => return Ok(Some(banner)),
                Some(DeviceReply::Nack(message)) => return Err(TransportError::Nack(message)),
                Some(DeviceReply::Ack) | None => {}
            }
        }
    })
    .await;
    match outcome {
        Ok(result) => result,
        Err(_) => Ok(None),
    }
}

async fn wait_for_ack(stream: &mut SerialStream, duration: Duration) -> Result<(), TransportError> {
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
                Some(DeviceReply::Ack) => return Ok(()),
                Some(DeviceReply::Nack(message)) => return Err(TransportError::Nack(message)),
                Some(DeviceReply::Ready(_)) | None => {}
            }
        }
    })
    .await
    .map_err(|_| TransportError::Timeout {
        transport: TransportKind::Usb,
        waiting_for: "ACK INK1",
    })?
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
    async fn identity_query_has_a_bounded_silent_result() {
        let (mut host, _silent_device) = duplex(16);
        assert_eq!(
            request_identity(&mut host, Duration::from_millis(1))
                .await
                .unwrap(),
            None
        );
    }
}
