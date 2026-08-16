use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use btleplug::{
    api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Adapter, Manager, Peripheral},
};
use futures::StreamExt;
use tokio::{sync::Mutex, time::sleep};

use super::{
    DeviceReply, PacketTransport, RX_UUID, SERVICE_UUID, TX_UUID, TransportError, TransportKind,
    TransportReceipt, device_reply, validate_for_transport,
};

const BLE_FRAME_ATTEMPTS: usize = 2;
const BLE_FRAME_RETRY_DELAY: Duration = Duration::from_millis(100);

/// BLE scan and write configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BleConfig {
    /// Compatibility name advertised by the proven sibling firmware.
    pub device_name: String,
    /// Exact platform peripheral identifier selected during discovery.
    /// `None` accepts a matching service UUID or compatibility name.
    pub device_id: Option<String>,
    /// Maximum peripheral scan duration.
    pub scan_timeout: Duration,
    /// GATT write chunk size below the common 185-byte MTU payload.
    pub chunk_size: usize,
    /// Delay between write-without-response chunks.
    pub chunk_delay: Duration,
    /// Maximum display-refresh/acknowledgement time.
    pub acknowledgement_timeout: Duration,
}

impl Default for BleConfig {
    fn default() -> Self {
        Self {
            device_name: "InkDock E213".into(),
            device_id: None,
            scan_timeout: Duration::from_secs(8),
            chunk_size: 180,
            chunk_delay: Duration::from_millis(8),
            acknowledgement_timeout: Duration::from_secs(15),
        }
    }
}

/// One compatible BLE peripheral observed during a bounded scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BleDeviceInfo {
    /// Stable platform peripheral identifier for the current host.
    pub id: String,
    /// Advertised local name, or a compatibility fallback.
    pub name: String,
    /// Last observed RSSI in dBm when the platform provides it.
    pub signal_strength: Option<i16>,
}

/// Verified BLE GATT connection retained by [`BleTransport`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BleConnectionInfo {
    /// Stable platform peripheral identifier.
    pub id: String,
    /// Advertised local name.
    pub name: String,
}

struct BleConnection {
    peripheral: Peripheral,
    rx: Characteristic,
    tx: Characteristic,
    info: BleConnectionInfo,
}

/// `btleplug` writer for the backward-compatible E213 GATT service.
pub struct BleTransport {
    config: BleConfig,
    connection: Mutex<Option<BleConnection>>,
}

impl BleTransport {
    /// Creates a lazily scanned BLE transport.
    pub fn new(config: BleConfig) -> Self {
        Self {
            config,
            connection: Mutex::new(None),
        }
    }

    async fn connect(&self) -> Result<BleConnection, TransportError> {
        let (peripheral, observed) = find_ble_device(&self.config).await?;

        if !peripheral
            .is_connected()
            .await
            .map_err(|error| ble_io(error.to_string()))?
        {
            peripheral
                .connect()
                .await
                .map_err(|error| ble_io(error.to_string()))?;
        }
        peripheral
            .discover_services()
            .await
            .map_err(|error| ble_io(error.to_string()))?;
        let characteristics = peripheral.characteristics();
        let rx = characteristics
            .iter()
            .find(|characteristic| characteristic.uuid == RX_UUID)
            .cloned()
            .ok_or(TransportError::MissingCharacteristic {
                which: "RX",
                uuid: RX_UUID,
            })?;
        let tx = characteristics
            .iter()
            .find(|characteristic| characteristic.uuid == TX_UUID)
            .cloned()
            .ok_or(TransportError::MissingCharacteristic {
                which: "TX",
                uuid: TX_UUID,
            })?;
        Ok(BleConnection {
            peripheral,
            rx,
            tx,
            info: BleConnectionInfo {
                id: observed.id,
                name: observed.name,
            },
        })
    }

    /// Connects and verifies the INK1 GATT characteristics without sending a
    /// frame. This is application-level BLE setup; it does not claim that the
    /// operating system created a bonded pairing record.
    pub async fn ensure_connected(&self) -> Result<BleConnectionInfo, TransportError> {
        let mut guard = self.connection.lock().await;
        if let Some(connection) = guard.as_ref()
            && connection
                .peripheral
                .is_connected()
                .await
                .map_err(|error| ble_io(error.to_string()))?
        {
            return Ok(connection.info.clone());
        }
        let connection = self.connect().await?;
        let info = connection.info.clone();
        *guard = Some(connection);
        Ok(info)
    }

    /// Disconnects the retained GATT session, if one exists.
    pub async fn disconnect(&self) -> Result<(), TransportError> {
        let mut guard = self.connection.lock().await;
        if let Some(connection) = guard.take()
            && connection
                .peripheral
                .is_connected()
                .await
                .map_err(|error| ble_io(error.to_string()))?
        {
            connection
                .peripheral
                .disconnect()
                .await
                .map_err(|error| ble_io(error.to_string()))?;
        }
        Ok(())
    }

    async fn send_on_connection(
        &self,
        connection: &BleConnection,
        packet: &[u8],
    ) -> Result<TransportReceipt, TransportError> {
        let mut notifications = connection
            .peripheral
            .notifications()
            .await
            .map_err(|error| ble_io(error.to_string()))?;

        // Subscription precedes every RX write so a fast firmware ACK cannot
        // race ahead of the host's notification stream.
        connection
            .peripheral
            .subscribe(&connection.tx)
            .await
            .map_err(|error| ble_io(error.to_string()))?;
        let ready_observed = connection
            .peripheral
            .read(&connection.tx)
            .await
            .ok()
            .as_deref()
            .and_then(device_reply)
            .is_some_and(|reply| matches!(reply, DeviceReply::Ready(_)));

        let chunk_size = self.config.chunk_size.max(1);
        for chunk in packet.chunks(chunk_size) {
            connection
                .peripheral
                .write(&connection.rx, chunk, WriteType::WithoutResponse)
                .await
                .map_err(|error| ble_io(error.to_string()))?;
            sleep(self.config.chunk_delay).await;
        }

        let result = tokio::time::timeout(self.config.acknowledgement_timeout, async {
            let mut received = Vec::new();
            while let Some(notification) = notifications.next().await {
                if notification.uuid != TX_UUID {
                    continue;
                }
                received.extend_from_slice(&notification.value);
                match device_reply(&received) {
                    Some(DeviceReply::Ack) => return Ok(()),
                    Some(DeviceReply::Nack(message)) => {
                        return Err(TransportError::Nack(message));
                    }
                    Some(DeviceReply::Ready(_)) | None => {}
                }
            }
            Err(ble_io("notification stream ended before ACK INK1".into()))
        })
        .await
        .map_err(|_| TransportError::Timeout {
            transport: TransportKind::Ble,
            waiting_for: "ACK INK1",
        })?;

        let _ = connection.peripheral.unsubscribe(&connection.tx).await;
        result?;
        Ok(TransportReceipt {
            transport: TransportKind::Ble,
            ready_observed,
            acknowledgement: "ACK INK1".into(),
        })
    }
}

/// Scans every available Bluetooth adapter for peripherals advertising the
/// compatible service or selected compatibility name.
pub async fn discover_ble_devices(
    config: &BleConfig,
) -> Result<Vec<BleDeviceInfo>, TransportError> {
    let adapters = available_adapters().await?;
    start_scans(&adapters).await?;
    let started = tokio::time::Instant::now();
    let mut found = BTreeMap::<String, BleDeviceInfo>::new();
    loop {
        for adapter in &adapters {
            for peripheral in adapter
                .peripherals()
                .await
                .map_err(|error| ble_io(error.to_string()))?
            {
                if let Some(info) = compatible_device(&peripheral, config).await? {
                    found.insert(info.id.clone(), info);
                }
            }
        }
        if started.elapsed() >= config.scan_timeout {
            break;
        }
        sleep(Duration::from_millis(200)).await;
    }
    stop_scans(&adapters).await;
    Ok(found.into_values().collect())
}

async fn find_ble_device(
    config: &BleConfig,
) -> Result<(Peripheral, BleDeviceInfo), TransportError> {
    let adapters = available_adapters().await?;
    start_scans(&adapters).await?;
    let started = tokio::time::Instant::now();
    loop {
        for adapter in &adapters {
            for peripheral in adapter
                .peripherals()
                .await
                .map_err(|error| ble_io(error.to_string()))?
            {
                if let Some(info) = compatible_device(&peripheral, config).await?
                    && config
                        .device_id
                        .as_deref()
                        .is_none_or(|selected| selected == info.id)
                {
                    stop_scans(&adapters).await;
                    return Ok((peripheral, info));
                }
            }
        }
        if started.elapsed() >= config.scan_timeout {
            stop_scans(&adapters).await;
            return Err(TransportError::NoBleDevice {
                name: config.device_name.clone(),
            });
        }
        sleep(Duration::from_millis(200)).await;
    }
}

async fn available_adapters() -> Result<Vec<Adapter>, TransportError> {
    let manager = Manager::new()
        .await
        .map_err(|error| ble_io(error.to_string()))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|error| ble_io(error.to_string()))?;
    if adapters.is_empty() {
        return Err(TransportError::NoBleAdapter);
    }
    Ok(adapters)
}

async fn start_scans(adapters: &[Adapter]) -> Result<(), TransportError> {
    for adapter in adapters {
        adapter
            .start_scan(ScanFilter {
                services: vec![SERVICE_UUID],
            })
            .await
            .map_err(|error| ble_io(error.to_string()))?;
    }
    Ok(())
}

async fn stop_scans(adapters: &[Adapter]) {
    for adapter in adapters {
        let _ = adapter.stop_scan().await;
    }
}

async fn compatible_device(
    peripheral: &Peripheral,
    config: &BleConfig,
) -> Result<Option<BleDeviceInfo>, TransportError> {
    let Some(properties) = peripheral
        .properties()
        .await
        .map_err(|error| ble_io(error.to_string()))?
    else {
        return Ok(None);
    };
    if properties.local_name.as_deref() != Some(config.device_name.as_str())
        && !properties.services.contains(&SERVICE_UUID)
    {
        return Ok(None);
    }
    Ok(Some(BleDeviceInfo {
        id: peripheral.id().to_string(),
        name: properties
            .local_name
            .unwrap_or_else(|| "INK1 E213 display".into()),
        signal_strength: properties.rssi,
    }))
}

#[async_trait]
impl PacketTransport for BleTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Ble
    }

    async fn send_packet(&self, packet: &[u8]) -> Result<TransportReceipt, TransportError> {
        validate_for_transport(packet)?;
        let mut guard = self.connection.lock().await;
        for attempt in 0..BLE_FRAME_ATTEMPTS {
            let connected = match guard.as_ref() {
                Some(connection) => connection
                    .peripheral
                    .is_connected()
                    .await
                    .map_err(|error| ble_io(error.to_string()))?,
                None => false,
            };
            if !connected {
                *guard = Some(self.connect().await?);
            }
            match self
                .send_on_connection(guard.as_ref().expect("connected above"), packet)
                .await
            {
                Ok(receipt) => return Ok(receipt),
                Err(error) => {
                    if let Some(connection) = guard.take() {
                        let _ = connection.peripheral.disconnect().await;
                    }
                    let final_attempt = attempt + 1 == BLE_FRAME_ATTEMPTS;
                    if final_attempt || !retryable_frame_error(&error) {
                        return Err(error);
                    }
                    sleep(BLE_FRAME_RETRY_DELAY).await;
                }
            }
        }
        unreachable!("BLE_FRAME_ATTEMPTS is non-zero")
    }
}

fn retryable_frame_error(error: &TransportError) -> bool {
    matches!(
        error,
        TransportError::Io {
            transport: TransportKind::Ble,
            ..
        } | TransportError::Timeout {
            transport: TransportKind::Ble,
            ..
        } | TransportError::Nack(_)
    )
}

fn ble_io(message: String) -> TransportError {
    TransportError::Io {
        transport: TransportKind::Ble,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transient_frame_failures_are_retried() {
        assert!(retryable_frame_error(&ble_io("chunk dropped".into())));
        assert!(retryable_frame_error(&TransportError::Timeout {
            transport: TransportKind::Ble,
            waiting_for: "ACK INK1",
        }));
        assert!(retryable_frame_error(&TransportError::Nack(
            "NACK TRUNCATED".into()
        )));
        assert!(!retryable_frame_error(&TransportError::NoBleAdapter));
        assert!(!retryable_frame_error(&TransportError::Protocol(
            crate::ProtocolError::PacketSize { actual: 0 },
        )));
    }

    #[test]
    fn frame_retry_is_one_bounded_second_attempt() {
        assert_eq!(BLE_FRAME_ATTEMPTS, 2);
    }
}
