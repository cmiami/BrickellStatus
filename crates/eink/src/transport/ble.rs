use std::{collections::BTreeMap, future::Future, time::Duration};

use async_trait::async_trait;
use btleplug::{
    api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Adapter, Manager, Peripheral},
};
use futures::StreamExt;
use tokio::{
    runtime::Handle,
    sync::Mutex,
    time::{sleep, timeout},
};
use tracing::warn;

use super::{
    DeviceReply, PacketTransport, RX_UUID, SERVICE_UUID, TX_UUID, TransportError, TransportKind,
    TransportReceipt, device_reply, safe_device_text, validate_for_transport,
};

/// Hands btleplug's Android backend the JVM handle it needs before first use.
///
/// Android BLE is a hybrid Rust/Java build: the Rust side reaches the platform
/// adapter through JNI, and `btleplug::platform::global_adapter()` panics if
/// this never ran. Call it exactly once, from a native method that Java
/// invoked -- the class lookups inside resolve against whichever class loader
/// is on the calling thread's stack, and only a Java-called thread carries the
/// app's own loader. A Rust-spawned thread gets the system loader and the
/// lookups fail.
///
/// Returning an error rather than panicking is deliberate: a device with
/// Bluetooth switched off, or a build where the Java companion did not make it
/// into the APK, should cost the user the e-paper output and nothing else.
#[cfg(target_os = "android")]
pub fn init_android_bluetooth(env: &jni::JNIEnv<'_>) -> Result<(), TransportError> {
    // Kept for `ensure_jvm_attached`, which cannot ask btleplug for the same
    // handle: its `global_jvm` is private to the Android backend.
    let vm = env.get_java_vm().map_err(|error| TransportError::Io {
        transport: TransportKind::Ble,
        message: error.to_string(),
    })?;
    let _ = ANDROID_JVM.set(vm);
    btleplug::platform::init(env).map_err(|error| TransportError::Io {
        transport: TransportKind::Ble,
        message: error.to_string(),
    })
}

/// The JVM handed to [`init_android_bluetooth`], kept for thread attachment.
#[cfg(target_os = "android")]
static ANDROID_JVM: std::sync::OnceLock<jni::JavaVM> = std::sync::OnceLock::new();

/// Attaches the calling thread to the JVM, so btleplug can reach it.
///
/// btleplug's Android backend reads the environment with `JavaVM::get_env`,
/// which looks the thread up rather than attaching it: on a thread Java has
/// never called into it returns `JNI_EDETACHED`, which surfaces two layers up
/// as the maximally unhelpful "JNI call failed". Every BLE call in this module
/// runs on a Tokio worker that Rust spawned, so without this the first adapter
/// call fails and the app reports no Bluetooth hardware at all.
///
/// The attachment is permanent because a detach-on-drop guard would have to be
/// held across await points, and a Tokio task may resume on a different worker
/// after any of them. Attaching is idempotent and cheap once a thread is
/// already attached, and the runtime's worker set is bounded, so the cost is a
/// handful of permanent attachments rather than one per call.
#[cfg(target_os = "android")]
fn ensure_jvm_attached() {
    let Some(vm) = ANDROID_JVM.get() else {
        return;
    };
    if let Err(error) = vm.attach_current_thread_permanently() {
        warn!(%error, "could not attach this thread to the JVM; the BLE call will fail");
    }
}

/// A future that attaches the polling thread to the JVM before each poll.
///
/// Wrapping the outermost future covers every nested btleplug await inside it,
/// which is what makes this robust against work stealing: whichever worker
/// resumes the task is attached before btleplug touches JNI again.
#[cfg(target_os = "android")]
struct JvmAttached<'a, T> {
    inner: std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>,
}

#[cfg(target_os = "android")]
impl<T> Future for JvmAttached<'_, T> {
    type Output = T;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<T> {
        ensure_jvm_attached();
        self.inner.as_mut().poll(context)
    }
}

/// Wraps a BLE future so every poll runs on a JVM-attached thread.
#[cfg(target_os = "android")]
fn attached<'a, T>(future: impl Future<Output = T> + Send + 'a) -> JvmAttached<'a, T> {
    JvmAttached {
        inner: Box::pin(future),
    }
}

/// Off Android there is no JVM to attach to, so this is the future unchanged.
#[cfg(not(target_os = "android"))]
fn attached<'a, T>(
    future: impl Future<Output = T> + Send + 'a,
) -> impl Future<Output = T> + Send + 'a {
    future
}

const BLE_FRAME_ATTEMPTS: usize = 2;
const BLE_FRAME_RETRY_DELAY: Duration = Duration::from_millis(100);
const BLE_CONNECTION_STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const BLE_CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
const BLE_SERVICE_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const BLE_BANNER_READ_TIMEOUT: Duration = Duration::from_secs(2);
const BLE_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const BLE_SCAN_STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// Picker label used only when a compatible advertisement has no local name.
///
/// It is deliberately not a durable identity: more than one old panel can
/// advertise the public service without a name and would receive this same
/// label.
pub const ANONYMOUS_BLE_DEVICE_NAME: &str = "INK1 panel";

/// Whether an advertised BLE name can safely be remembered across sessions.
pub fn is_durable_ble_device_name(name: &str) -> bool {
    let Some(board_code) = name.strip_prefix("BrickellStatus ") else {
        return false;
    };
    board_code != "0000"
        && board_code.len() == 4
        && board_code
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
}

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
            device_name: String::new(),
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
    /// The banner the board holds on its TX characteristic, when it published
    /// one. Over Bluetooth this is the only place the panel geometry is spoken,
    /// so it is read on connect rather than waited for.
    pub banner: Option<String>,
}

struct BleConnection {
    peripheral: Peripheral,
    rx: Characteristic,
    tx: Characteristic,
    info: BleConnectionInfo,
    cleanup: CleanupOnDrop,
}

/// A synchronous cancellation boundary for cleanup that itself has to be
/// asynchronous. Dropping a Tokio future runs `Drop` immediately; scheduling
/// the platform disconnect here means an outer timeout cannot skip it.
struct CleanupOnDrop {
    cleanup: Option<Box<dyn FnOnce() + Send + Sync + 'static>>,
}

impl CleanupOnDrop {
    fn new(cleanup: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    fn disarm(&mut self) {
        self.cleanup = None;
    }
}

impl Drop for CleanupOnDrop {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
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
        let cleanup_peripheral = peripheral.clone();
        let cleanup = CleanupOnDrop::new(move || {
            schedule_ble_disconnect(cleanup_peripheral);
        });

        let connected = timeout(BLE_CONNECTION_STATUS_TIMEOUT, peripheral.is_connected())
            .await
            .map_err(|_| ble_timeout("BLE connection status"))?
            .map_err(|error| ble_io(error.to_string()))?;
        if !connected {
            timeout(BLE_CONNECT_TIMEOUT, peripheral.connect())
                .await
                .map_err(|_| ble_timeout("BLE connection"))?
                .map_err(|error| ble_io(error.to_string()))?;
        }

        // Connection and verification are one ownership boundary. Every
        // platform call has its own deadline below the worker's deadline, and
        // `cleanup` remains armed across every await. CoreBluetooth does not
        // disconnect merely because its Rust handle was dropped.
        timeout(
            BLE_SERVICE_DISCOVERY_TIMEOUT,
            peripheral.discover_services(),
        )
        .await
        .map_err(|_| ble_timeout("BLE GATT service discovery"))?
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
        // Banner reads are useful but optional, so a platform read failure or
        // timeout does not discard an otherwise verified INK1 connection.
        let banner = timeout(BLE_BANNER_READ_TIMEOUT, peripheral.read(&tx))
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(|value| match device_reply(&value) {
                Some(DeviceReply::Ready(line)) => Some(line),
                _ => None,
            });
        Ok(BleConnection {
            peripheral,
            rx,
            tx,
            info: BleConnectionInfo {
                id: observed.id,
                name: observed.name,
                banner,
            },
            cleanup,
        })
    }

    /// Connects and verifies the INK1 GATT characteristics without sending a
    /// frame. This is application-level BLE setup; it does not claim that the
    /// operating system created a bonded pairing record.
    pub async fn ensure_connected(&self) -> Result<BleConnectionInfo, TransportError> {
        attached(self.ensure_connected_inner()).await
    }

    async fn ensure_connected_inner(&self) -> Result<BleConnectionInfo, TransportError> {
        let mut guard = self.connection.lock().await;
        if let Some(mut connection) = guard.take() {
            let connected = timeout(
                BLE_CONNECTION_STATUS_TIMEOUT,
                connection.peripheral.is_connected(),
            )
            .await
            .map_err(|_| ble_timeout("BLE connection status"))?
            .map_err(|error| ble_io(error.to_string()))?;
            if connected {
                let info = connection.info.clone();
                *guard = Some(connection);
                return Ok(info);
            }
            // The platform explicitly says this old handle has no link. Disarm
            // it before reconnecting so dropping it cannot disconnect the new
            // session to the same peripheral.
            connection.cleanup.disarm();
        }
        let connection = self.connect().await?;
        let info = connection.info.clone();
        *guard = Some(connection);
        Ok(info)
    }

    /// Reads the READY banner held by an existing GATT connection without
    /// sending a frame or refreshing the e-paper glass.
    ///
    /// Firmware keeps this characteristic current with its latest battery
    /// measurement, which lets the host check power on an otherwise static
    /// display. No retained connection is treated as no reading, not as an
    /// invitation to start a surprise scan.
    pub async fn read_banner(&self) -> Result<Option<String>, TransportError> {
        attached(self.read_banner_inner()).await
    }

    async fn read_banner_inner(&self) -> Result<Option<String>, TransportError> {
        let mut guard = self.connection.lock().await;
        let Some(connection) = guard.as_mut() else {
            return Ok(None);
        };
        let connected = timeout(
            BLE_CONNECTION_STATUS_TIMEOUT,
            connection.peripheral.is_connected(),
        )
        .await
        .map_err(|_| ble_timeout("BLE connection status"))?
        .map_err(|error| ble_io(error.to_string()))?;
        if !connected {
            return Ok(None);
        }
        let value = timeout(
            BLE_BANNER_READ_TIMEOUT,
            connection.peripheral.read(&connection.tx),
        )
        .await
        .map_err(|_| ble_timeout("BLE READY banner"))?
        .map_err(|error| ble_io(error.to_string()))?;
        let banner = match device_reply(&value) {
            Some(DeviceReply::Ready(line)) => Some(line),
            _ => None,
        };
        if banner.is_some() {
            connection.info.banner = banner.clone();
        }
        Ok(banner)
    }

    /// Disconnects the retained GATT session, if one exists.
    pub async fn disconnect(&self) -> Result<(), TransportError> {
        attached(self.disconnect_inner()).await
    }

    async fn disconnect_inner(&self) -> Result<(), TransportError> {
        let mut guard = self.connection.lock().await;
        if let Some(mut connection) = guard.take() {
            let connected = timeout(
                BLE_CONNECTION_STATUS_TIMEOUT,
                connection.peripheral.is_connected(),
            )
            .await
            .map_err(|_| ble_timeout("BLE connection status"))?
            .map_err(|error| ble_io(error.to_string()))?;
            if connected {
                disconnect_platform_link(&connection.peripheral).await?;
            }
            connection.cleanup.disarm();
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
                    Some(DeviceReply::Ack) => {
                        let text = String::from_utf8_lossy(&received);
                        let start = text.find("ACK INK1").expect("reply matched ACK above");
                        return Ok(safe_device_text(
                            text[start..].lines().next().unwrap_or("ACK INK1"),
                        ));
                    }
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
        let acknowledgement = result?;
        Ok(TransportReceipt {
            transport: TransportKind::Ble,
            ready_observed,
            acknowledgement,
        })
    }
}

fn spawn_cleanup(cleanup: impl Future<Output = ()> + Send + 'static) {
    match Handle::try_current() {
        Ok(handle) => {
            // Cleanup runs on a fresh task, so it needs the same JVM
            // attachment as the call whose failure scheduled it.
            handle.spawn(attached(cleanup));
        }
        Err(error) => {
            warn!(%error, "BLE cleanup could not be scheduled because the async runtime is gone");
        }
    }
}

fn schedule_ble_disconnect(peripheral: Peripheral) {
    spawn_cleanup(async move {
        if let Err(error) = disconnect_platform_link(&peripheral).await {
            warn!(%error, "best-effort BLE link cleanup failed");
        }
    });
}

fn schedule_scan_stop(adapters: Vec<Adapter>) {
    spawn_cleanup(async move {
        if !stop_scans(&adapters).await {
            warn!("best-effort BLE scan cleanup failed");
        }
    });
}

async fn disconnect_platform_link(peripheral: &Peripheral) -> Result<(), TransportError> {
    timeout(BLE_DISCONNECT_TIMEOUT, peripheral.disconnect())
        .await
        .map_err(|_| ble_timeout("BLE disconnection"))?
        .map_err(|error| ble_io(error.to_string()))
}

fn ble_timeout(waiting_for: &'static str) -> TransportError {
    TransportError::Timeout {
        transport: TransportKind::Ble,
        waiting_for,
    }
}

/// Scans every available Bluetooth adapter for peripherals advertising the
/// compatible service or selected compatibility name.
pub async fn discover_ble_devices(
    config: &BleConfig,
) -> Result<Vec<BleDeviceInfo>, TransportError> {
    attached(discover_ble_devices_inner(config)).await
}

async fn discover_ble_devices_inner(
    config: &BleConfig,
) -> Result<Vec<BleDeviceInfo>, TransportError> {
    let adapters = available_adapters().await?;
    let mut scan_cleanup = start_scans(&adapters).await?;
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
    if stop_scans(&adapters).await {
        scan_cleanup.disarm();
    }
    Ok(found.into_values().collect())
}

async fn find_ble_device(
    config: &BleConfig,
) -> Result<(Peripheral, BleDeviceInfo), TransportError> {
    let adapters = available_adapters().await?;
    let mut scan_cleanup = start_scans(&adapters).await?;
    let started = tokio::time::Instant::now();
    loop {
        for adapter in &adapters {
            for peripheral in adapter
                .peripherals()
                .await
                .map_err(|error| ble_io(error.to_string()))?
            {
                if let Some(info) = compatible_device(&peripheral, config).await?
                    && requested_device(config, &info)
                {
                    if stop_scans(&adapters).await {
                        scan_cleanup.disarm();
                    }
                    return Ok((peripheral, info));
                }
            }
        }
        if started.elapsed() >= config.scan_timeout {
            if stop_scans(&adapters).await {
                scan_cleanup.disarm();
            }
            return Err(TransportError::NoBleDevice {
                name: config.device_name.clone(),
            });
        }
        sleep(Duration::from_millis(200)).await;
    }
}

/// Whether this compatible advertisement is the panel the caller requested.
///
/// An explicit platform ID is the fastest same-session route. The firmware's
/// unique advertised name is the stable cross-session fallback when an OS
/// rebuilds its Bluetooth cache and hands the same board a different ID. A
/// configured name is exact: merely advertising the public INK1 service must
/// never make a neighbour's panel eligible for remembered reconnect.
fn requested_device(config: &BleConfig, info: &BleDeviceInfo) -> bool {
    let selected_id = config.device_id.as_deref();
    selected_id.is_some_and(|id| id == info.id)
        || (is_durable_ble_device_name(&config.device_name) && config.device_name == info.name)
        || (selected_id.is_none() && config.device_name.trim().is_empty())
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

async fn start_scans(adapters: &[Adapter]) -> Result<CleanupOnDrop, TransportError> {
    // Arm this before the first adapter call. A multi-adapter failure or an
    // outer timeout must stop every scan that may already have started.
    let cleanup_adapters = adapters.to_vec();
    let cleanup = CleanupOnDrop::new(move || schedule_scan_stop(cleanup_adapters));
    for adapter in adapters {
        adapter
            // Do not ask the OS to pre-filter by service. Deployed firmware can
            // put the board-specific name in its scan response while a host's
            // Bluetooth cache temporarily omits the service list. The filter
            // below still accepts only the public INK1 service or the exact
            // saved BrickellStatus board name; scanning broadly merely lets
            // that existing compatibility path see the advertisement.
            .start_scan(discovery_scan_filter())
            .await
            .map_err(|error| ble_io(error.to_string()))?;
    }
    Ok(cleanup)
}

fn discovery_scan_filter() -> ScanFilter {
    ScanFilter::default()
}

async fn stop_scans(adapters: &[Adapter]) -> bool {
    let mut stopped = true;
    for adapter in adapters {
        stopped &= matches!(
            timeout(BLE_SCAN_STOP_TIMEOUT, adapter.stop_scan()).await,
            Ok(Ok(()))
        );
    }
    stopped
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
            .unwrap_or_else(|| ANONYMOUS_BLE_DEVICE_NAME.into()),
        signal_strength: properties.rssi,
    }))
}

#[async_trait]
impl PacketTransport for BleTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Ble
    }

    async fn send_packet(&self, packet: &[u8]) -> Result<TransportReceipt, TransportError> {
        attached(self.send_packet_inner(packet)).await
    }
}

impl BleTransport {
    async fn send_packet_inner(&self, packet: &[u8]) -> Result<TransportReceipt, TransportError> {
        validate_for_transport(packet)?;
        let mut guard = self.connection.lock().await;
        for attempt in 0..BLE_FRAME_ATTEMPTS {
            let connected = match guard.as_ref() {
                Some(connection) => timeout(
                    BLE_CONNECTION_STATUS_TIMEOUT,
                    connection.peripheral.is_connected(),
                )
                .await
                .map_err(|_| ble_timeout("BLE connection status"))?
                .map_err(|error| ble_io(error.to_string()))?,
                None => false,
            };
            if !connected {
                if let Some(mut stale) = guard.take() {
                    // `is_connected == false` is the proof that this handle no
                    // longer owns a platform link. Do not let its Drop cleanup
                    // race the replacement connection below.
                    stale.cleanup.disarm();
                }
                *guard = Some(self.connect().await?);
            }
            match self
                .send_on_connection(guard.as_ref().expect("connected above"), packet)
                .await
            {
                Ok(receipt) => return Ok(receipt),
                Err(error) => {
                    if let Some(mut connection) = guard.take()
                        && disconnect_platform_link(&connection.peripheral)
                            .await
                            .is_ok()
                    {
                        connection.cleanup.disarm();
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
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;

    fn device(id: &str, name: &str) -> BleDeviceInfo {
        BleDeviceInfo {
            id: id.into(),
            name: name.into(),
            signal_strength: None,
        }
    }

    #[test]
    fn remembered_ble_selection_never_accepts_a_different_service_device() {
        let config = BleConfig {
            device_name: "BrickellStatus 26B4".into(),
            device_id: Some("old-platform-id".into()),
            ..BleConfig::default()
        };

        assert!(requested_device(
            &config,
            &device("current-platform-id", "BrickellStatus 26B4")
        ));
        assert!(!requested_device(
            &config,
            &device("neighbour-id", "BrickellStatus 8A10")
        ));
    }

    #[test]
    fn an_unpinned_transport_can_still_choose_a_compatible_panel() {
        assert!(requested_device(
            &BleConfig::default(),
            &device("platform-id", "BrickellStatus 26B4")
        ));
    }

    #[test]
    fn discovery_does_not_hide_the_exact_name_compatibility_path() {
        assert!(discovery_scan_filter().services.is_empty());
    }

    #[test]
    fn anonymous_picker_label_is_never_a_durable_identity() {
        assert!(!is_durable_ble_device_name(ANONYMOUS_BLE_DEVICE_NAME));
        assert!(!is_durable_ble_device_name("  INK1 panel  "));
        assert!(!is_durable_ble_device_name(""));
        assert!(!is_durable_ble_device_name("InkDock E213"));
        assert!(!is_durable_ble_device_name("BrickellStatus 26b4"));
        assert!(!is_durable_ble_device_name("BrickellStatus 26B40"));
        assert!(!is_durable_ble_device_name("BrickellStatus 0000"));
        assert!(!is_durable_ble_device_name(" BrickellStatus 26B4"));
        assert!(is_durable_ble_device_name("BrickellStatus 26B4"));

        let anonymous = device("platform-id", ANONYMOUS_BLE_DEVICE_NAME);
        let name_only = BleConfig {
            device_name: ANONYMOUS_BLE_DEVICE_NAME.into(),
            ..BleConfig::default()
        };
        assert!(!requested_device(&name_only, &anonymous));

        let exact_session_id = BleConfig {
            device_name: ANONYMOUS_BLE_DEVICE_NAME.into(),
            device_id: Some("platform-id".into()),
            ..BleConfig::default()
        };
        assert!(requested_device(&exact_session_id, &anonymous));
    }

    #[tokio::test]
    async fn cancelling_setup_schedules_async_link_cleanup() {
        let setup_started = Arc::new(tokio::sync::Notify::new());
        let cleanup_finished = Arc::new(tokio::sync::Notify::new());
        let task = tokio::spawn({
            let setup_started = Arc::clone(&setup_started);
            let cleanup_finished = Arc::clone(&cleanup_finished);
            async move {
                let _cleanup = CleanupOnDrop::new(move || {
                    spawn_cleanup(async move {
                        cleanup_finished.notify_one();
                    });
                });
                setup_started.notify_one();
                std::future::pending::<()>().await;
            }
        });

        setup_started.notified().await;
        task.abort();
        let _ = task.await;
        timeout(Duration::from_secs(1), cleanup_finished.notified())
            .await
            .expect("cancellation cleanup should run on the live runtime");
    }

    #[test]
    fn disarmed_link_cleanup_does_not_disconnect_a_retained_connection() {
        let disconnected = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&disconnected);
        let mut cleanup = CleanupOnDrop::new(move || {
            observed.store(true, Ordering::Relaxed);
        });
        cleanup.disarm();
        drop(cleanup);

        assert!(!disconnected.load(Ordering::Relaxed));
    }

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
