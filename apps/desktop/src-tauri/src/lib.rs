//! Native Tender's Log companion: runtime, tray lifetime, delivery, and E213 I/O.

pub mod firmware;
mod secret_store;

use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    process::Command,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bridgestatus_collectors::{
    CollectContext, Collector, CollectorItem, HttpFetcher, ItemKind, RainViewerCollector,
    SafeHttpFetcher,
};
use bridgestatus_delivery::{
    DeliveryAdapter, DeliveryFailureKind, DeliveryReason, DeliveryRequest, Destination,
    EnvironmentSecretResolver, EtaRange as DeliveryEtaRange, MessagingConsent, Notice, NoticeState,
    ReqwestExecutor, SecretValue, TokenSource, WhatsAppCloud, WhatsAppConfig,
};
use bridgestatus_eink::{
    ChannelAvailability, ChannelCard, ChannelKind, ChannelSource, ChannelUrgency, EtaRange,
    Evidence, Freshness, LiveSnapshot, MonoFrame, RadarFigure, RefreshMode, RenderConfig,
    SnapshotState, preview_png_bytes, radar_figure_from_png, render_channel_card,
    render_channel_card_with_radar, render_snapshot,
    transport::{
        BleConfig, BleTransport, TransportKind, TransportReceipt, UsbConfig, UsbTransport,
        discover_ble_devices, discover_espressif_devices,
    },
};
use bridgestatus_runtime::{
    AisConnectionStateDto, AppPreferences, AppSnapshot, AvailabilityDto, BridgeStateDto,
    ChannelKindDto, ChannelSnapshot, CredentialFreeCollectorFactory, DeliveryStateDto,
    DestinationIdDto, DispatchRecord, DisplayTransport, InterruptPreset, LocationSearchResult,
    MutationResult, OutputStateDto, RuntimeConfig, RuntimeEngine, SchedulerHandle, SurfacePresence,
    UrgencyDto, whatsapp_consent_is_current,
};
use jiff::{Timestamp, tz::TimeZone};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, State, WindowEvent,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_notification::NotificationExt;
use tenders_storage::{IncidentRecord, OutboxLease, OutboxRecord, Store};
use tokio::sync::{Mutex as AsyncMutex, Mutex as TokioMutex, RwLock};
use tracing::{debug, warn};
use url::Url;
use uuid::Uuid;

use secret_store::LocalSecretStore;

const MENU_STATUS_ID: &str = "e213-status";
const MENU_DETAIL_ID: &str = "e213-detail";
const MENU_OPEN_ID: &str = "open-main";
const MENU_QUIT_ID: &str = "quit";
const TRAY_ID: &str = "tenders-log-tray";
const STATUS_EVENT: &str = "display-connection-status";
const DISPATCH_TRACKER_KEY: &str = "desktop.whatsapp.dispatch";
const NOTIFICATION_TRACKER_KEY: &str = "desktop.notifications.dispatch";
const WHATSAPP_ROUTE_ID: &str = "meta.whatsapp.cloud";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayConnectionState {
    Connected,
    Connecting,
    Disconnected,
    Unavailable,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayConnectionTransport {
    Usb,
    Ble,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayConnectionStatus {
    pub state: DisplayConnectionState,
    pub transport: Option<DisplayConnectionTransport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_frame_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_ack_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EinkPreviewDto {
    channel_id: String,
    png: Vec<u8>,
}

impl Default for DisplayConnectionStatus {
    fn default() -> Self {
        Self {
            state: DisplayConnectionState::Disconnected,
            transport: None,
            device_name: None,
            detail: "No display connected. Scan for USB or Bluetooth Low Energy devices.".into(),
            last_frame_at: None,
            last_ack_at: None,
        }
    }
}

impl DisplayConnectionStatus {
    fn menu_lines(&self) -> (String, String) {
        let first = match (self.state, self.transport) {
            (DisplayConnectionState::Connected, Some(DisplayConnectionTransport::Usb)) => {
                "E213 · USB active"
            }
            (DisplayConnectionState::Connected, Some(DisplayConnectionTransport::Ble)) => {
                "E213 · BLE connected"
            }
            (DisplayConnectionState::Connecting, Some(DisplayConnectionTransport::Usb)) => {
                "E213 · USB connecting"
            }
            (DisplayConnectionState::Connecting, Some(DisplayConnectionTransport::Ble)) => {
                "E213 · BLE connecting"
            }
            (DisplayConnectionState::Connecting, None) => "E213 · scanning",
            (DisplayConnectionState::Disconnected, _) => "E213 · disconnected",
            (DisplayConnectionState::Unavailable, Some(DisplayConnectionTransport::Usb)) => {
                "E213 · USB unavailable"
            }
            (DisplayConnectionState::Unavailable, Some(DisplayConnectionTransport::Ble)) => {
                "E213 · BLE unavailable"
            }
            (DisplayConnectionState::Unavailable, None) => "E213 · unavailable",
            (DisplayConnectionState::Error, Some(DisplayConnectionTransport::Usb)) => {
                "E213 · USB error"
            }
            (DisplayConnectionState::Error, Some(DisplayConnectionTransport::Ble)) => {
                "E213 · BLE error"
            }
            (DisplayConnectionState::Error, None) => "E213 · transport error",
            (DisplayConnectionState::Connected, None) => "E213 · connected",
        };
        (first.into(), clean_menu_text(&self.detail))
    }

    fn tray_badge(&self) -> &'static str {
        match (self.state, self.transport) {
            (DisplayConnectionState::Connected, Some(DisplayConnectionTransport::Usb)) => "•USB",
            (DisplayConnectionState::Connected, Some(DisplayConnectionTransport::Ble)) => "•BLE",
            (DisplayConnectionState::Connecting, _) => "…",
            (DisplayConnectionState::Disconnected, _) => "○",
            (DisplayConnectionState::Unavailable, _) => "×",
            (DisplayConnectionState::Error, _) => "!",
            (DisplayConnectionState::Connected, None) => "•",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayDeviceCandidate {
    pub id: String,
    pub name: String,
    pub transport: DisplayConnectionTransport,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AisStreamSourceState {
    Disabled,
    MissingKey,
    Ready,
    Live,
    Degraded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AisStreamStatus {
    pub configured: bool,
    pub enabled: bool,
    pub state: AisStreamSourceState,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_position_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vessels_in_range: Option<usize>,
}

struct E213TrayState {
    current: StdMutex<DisplayConnectionStatus>,
    status_item: MenuItem<tauri::Wry>,
    detail_item: MenuItem<tauri::Wry>,
}

#[derive(Clone)]
enum ActiveDisplay {
    Usb {
        name: String,
        transport: Arc<UsbTransport>,
        ready_observed: bool,
    },
    Ble {
        name: String,
        transport: Arc<BleTransport>,
    },
}

impl ActiveDisplay {
    fn name(&self) -> &str {
        match self {
            Self::Usb { name, .. } | Self::Ble { name, .. } => name,
        }
    }

    fn transport(&self) -> DisplayConnectionTransport {
        match self {
            Self::Usb { .. } => DisplayConnectionTransport::Usb,
            Self::Ble { .. } => DisplayConnectionTransport::Ble,
        }
    }

    async fn send(
        &self,
        frame: &MonoFrame,
        refresh: RefreshMode,
    ) -> Result<TransportReceipt, String> {
        let receipt = match self {
            Self::Usb { transport, .. } => {
                bridgestatus_eink::transport::send_frame(transport.as_ref(), frame, refresh).await
            }
            Self::Ble { transport, .. } => {
                bridgestatus_eink::transport::send_frame(transport.as_ref(), frame, refresh).await
            }
        };
        receipt.map_err(|error| error.to_string())
    }

    async fn disconnect(&self) -> Result<(), String> {
        match self {
            Self::Usb { transport, .. } => {
                transport.disconnect().await;
                Ok(())
            }
            Self::Ble { transport, .. } => transport
                .disconnect()
                .await
                .map_err(|error| error.to_string()),
        }
    }
}

struct DisplayController {
    operation: AsyncMutex<()>,
    active: RwLock<Option<ActiveDisplay>>,
    last_frame: AsyncMutex<Option<Vec<u8>>>,
    frames_sent: AtomicU64,
    rotation_index: AtomicU64,
    automatic_reconnect: std::sync::atomic::AtomicBool,
    delivery_armed: std::sync::atomic::AtomicBool,
    preferred_usb_port: AsyncMutex<Option<String>>,
    preferred_ble_id: AsyncMutex<Option<String>>,
}

impl DisplayController {
    fn new(preferences: &AppPreferences) -> Self {
        let saved_usb = preferences.display.serial_port.trim();
        let saved_usb = matches!(
            preferences.display.transport,
            DisplayTransport::Usb | DisplayTransport::Auto
        )
        .then_some(saved_usb)
        .filter(|port| !port.is_empty() && !port.eq_ignore_ascii_case("auto"))
        .map(ToOwned::to_owned);
        Self {
            operation: AsyncMutex::new(()),
            active: RwLock::new(None),
            last_frame: AsyncMutex::new(None),
            frames_sent: AtomicU64::new(0),
            rotation_index: AtomicU64::new(0),
            // An exact saved USB route was explicitly selected in an earlier
            // session and is revalidated against attached Espressif hardware
            // before it opens. BLE remains session-selected because its ID is
            // intentionally not persisted.
            automatic_reconnect: std::sync::atomic::AtomicBool::new(saved_usb.is_some()),
            delivery_armed: std::sync::atomic::AtomicBool::new(false),
            preferred_usb_port: AsyncMutex::new(saved_usb),
            preferred_ble_id: AsyncMutex::new(None),
        }
    }

    async fn has_active(&self) -> bool {
        self.active.read().await.is_some()
    }

    /// Current slot without consuming it. An alert reads the rotation position
    /// but must not advance it, or it would eat the anchor's turn.
    fn rotation_index(&self) -> u64 {
        self.rotation_index.load(Ordering::Relaxed)
    }

    fn next_rotation_index(&self) -> u64 {
        self.rotation_index.fetch_add(1, Ordering::Relaxed)
    }

    fn automatic_reconnect_enabled(&self) -> bool {
        self.automatic_reconnect.load(Ordering::Relaxed)
    }

    fn delivery_armed(&self) -> bool {
        self.delivery_armed.load(Ordering::Relaxed)
    }

    async fn scan(
        &self,
        preferences: &AppPreferences,
    ) -> (Vec<DisplayDeviceCandidate>, Vec<String>) {
        let _operation = self.operation.lock().await;
        let ble_config = BleConfig {
            device_name: preferences.display.ble_name.clone(),
            ..BleConfig::default()
        };
        let (usb, ble) = tokio::join!(
            discover_espressif_devices(),
            discover_ble_devices(&ble_config)
        );
        let mut devices = Vec::new();
        let mut errors = Vec::new();
        match usb {
            Ok(found) => devices.extend(found.into_iter().map(|device| DisplayDeviceCandidate {
                id: format!("usb:{}", device.port),
                name: device.name,
                transport: DisplayConnectionTransport::Usb,
                detail: format!(
                    "{} · unverified Espressif serial candidate · {}",
                    device.port, device.detail
                ),
                signal_strength: None,
            })),
            Err(error) => errors.push(format!("USB: {error}")),
        }
        match ble {
            Ok(found) => devices.extend(found.into_iter().map(|device| DisplayDeviceCandidate {
                id: format!("ble:{}", device.id),
                name: device.name,
                transport: DisplayConnectionTransport::Ble,
                detail:
                    "Matching public INK1 service advertisement · app-level GATT connection".into(),
                signal_strength: device.signal_strength,
            })),
            Err(error) => errors.push(format!("Bluetooth: {error}")),
        }
        devices.sort_by(|left, right| {
            (left.transport as u8, left.name.as_str())
                .cmp(&(right.transport as u8, right.name.as_str()))
        });
        (devices, errors)
    }

    async fn connect_selected(
        &self,
        device_id: &str,
        transport: DisplayConnectionTransport,
        preferences: &AppPreferences,
    ) -> Result<DisplayConnectionStatus, String> {
        let _operation = self.operation.lock().await;
        self.disconnect_locked().await?;
        let active = match transport {
            DisplayConnectionTransport::Usb => {
                let port = device_id
                    .strip_prefix("usb:")
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "Selected USB device identifier is invalid.".to_owned())?;
                let active = self.connect_usb(port.to_owned()).await?;
                *self.preferred_usb_port.lock().await = Some(port.to_owned());
                *self.preferred_ble_id.lock().await = None;
                active
            }
            DisplayConnectionTransport::Ble => {
                let id = device_id
                    .strip_prefix("ble:")
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "Selected Bluetooth device identifier is invalid.".to_owned())?;
                let active = self
                    .connect_ble(Some(id.to_owned()), &preferences.display.ble_name)
                    .await?;
                *self.preferred_ble_id.lock().await = Some(id.to_owned());
                *self.preferred_usb_port.lock().await = None;
                active
            }
        };
        let detail = match &active {
            ActiveDisplay::Usb {
                ready_observed: false,
                ..
            } => {
                "Selected serial port opened, but READY INK1 was not observed. Only an explicit test frame and ACK INK1 can make this route healthy."
            }
            _ => {
                "INK1 protocol compatibility confirmed; send a test frame to prove end-to-end acknowledgement."
            }
        };
        let status = connected_status(&active, None, None, detail);
        *self.active.write().await = Some(active);
        self.automatic_reconnect.store(true, Ordering::Relaxed);
        self.delivery_armed.store(false, Ordering::Relaxed);
        *self.last_frame.lock().await = None;
        Ok(status)
    }

    async fn disconnect(&self, suppress_reconnect: bool) -> Result<(), String> {
        let _operation = self.operation.lock().await;
        self.automatic_reconnect
            .store(!suppress_reconnect, Ordering::Relaxed);
        self.delivery_armed.store(false, Ordering::Relaxed);
        self.disconnect_locked().await
    }

    async fn reconnect_preferred(
        &self,
        preferences: &AppPreferences,
    ) -> Result<DisplayConnectionStatus, String> {
        let _operation = self.operation.lock().await;
        if !self.automatic_reconnect_enabled() {
            return Err("Automatic reconnect is paused for this session.".into());
        }
        self.disconnect_locked().await?;
        let active = self.connect_preferred_locked(preferences).await?;
        let status = connected_status(
            &active,
            None,
            None,
            "Connection restored; waiting to send the current rotation frame.",
        );
        *self.active.write().await = Some(active);
        Ok(status)
    }

    /// Frees the serial interface so the flasher can take it.
    ///
    /// Flashing drives the same USB CDC device the display transport holds
    /// open, and macOS hands that out exclusively: leaving the connection up
    /// makes espflash fail immediately with "Device or resource busy".
    /// Automatic reconnect is suppressed for the same reason -- a background
    /// reconnect that wins the port mid-write would leave a half-written
    /// bootloader, which is the one outcome that actually bricks the board.
    ///
    /// Returns whether a display was connected, so it can be restored after.
    async fn release_port_for_flash(&self) -> bool {
        let was_connected = self.active.read().await.is_some();
        let _ = self.disconnect(true).await;
        was_connected
    }

    /// Restores the display connection after flashing.
    async fn restore_port_after_flash(&self, preferences: &AppPreferences) {
        self.automatic_reconnect.store(true, Ordering::Relaxed);
        let _ = self.reconnect_preferred(preferences).await;
    }

    /// Whether a connected USB display already announced itself.
    ///
    /// Reusing the live connection's observation avoids opening the port a
    /// second time just to ask a question it already answered, which would
    /// contend with the transport on every status poll.
    async fn usb_ready_observed(&self) -> Option<bool> {
        match self.active.read().await.as_ref() {
            Some(ActiveDisplay::Usb { ready_observed, .. }) => Some(*ready_observed),
            _ => None,
        }
    }

    async fn disconnect_locked(&self) -> Result<(), String> {
        let active = self.active.write().await.take();
        *self.last_frame.lock().await = None;
        if let Some(active) = active {
            active.disconnect().await?;
        }
        Ok(())
    }

    async fn connect_usb(&self, port: String) -> Result<ActiveDisplay, String> {
        let transport = Arc::new(UsbTransport::new(UsbConfig {
            port: Some(port.clone()),
            ..UsbConfig::default()
        }));
        let connected = transport
            .ensure_connected()
            .await
            .map_err(|error| error.to_string())?;
        let name = if connected.ready_observed {
            format!("E213 on {}", connected.port)
        } else {
            format!("USB display on {}", connected.port)
        };
        Ok(ActiveDisplay::Usb {
            name,
            transport,
            ready_observed: connected.ready_observed,
        })
    }

    async fn connect_ble(
        &self,
        device_id: Option<String>,
        configured_name: &str,
    ) -> Result<ActiveDisplay, String> {
        let transport = Arc::new(BleTransport::new(BleConfig {
            device_name: configured_name.to_owned(),
            device_id,
            ..BleConfig::default()
        }));
        let connected = transport
            .ensure_connected()
            .await
            .map_err(|error| error.to_string())?;
        Ok(ActiveDisplay::Ble {
            name: connected.name,
            transport,
        })
    }

    async fn connect_preferred_locked(
        &self,
        preferences: &AppPreferences,
    ) -> Result<ActiveDisplay, String> {
        let serial = preferences.display.serial_port.trim();
        let selected_usb = self.preferred_usb_port.lock().await.clone();
        let selected_ble = self.preferred_ble_id.lock().await.clone();
        match preferences.display.transport {
            DisplayTransport::Preview => Err(
                "Display output is set to Preview only; select USB, Bluetooth, or Automatic first."
                    .into(),
            ),
            DisplayTransport::Usb => {
                let port = selected_usb.or_else(|| {
                    (!serial.is_empty() && !serial.eq_ignore_ascii_case("auto"))
                        .then(|| serial.to_owned())
                })
                    .ok_or_else(|| {
                        "No USB device has been explicitly selected. Scan and choose the E213 before any bytes are written."
                            .to_owned()
                    })?;
                self.connect_usb(port).await
            }
            DisplayTransport::Ble => {
                let device_id = selected_ble.ok_or_else(|| {
                    "No Bluetooth device has been explicitly selected. Scan and choose the E213 before connecting."
                        .to_owned()
                })?;
                self.connect_ble(Some(device_id), &preferences.display.ble_name)
                    .await
            }
            DisplayTransport::Auto => {
                let pinned_port = selected_usb.or_else(|| {
                    (!serial.is_empty() && !serial.eq_ignore_ascii_case("auto"))
                        .then(|| serial.to_owned())
                });
                let usb_attempt = match pinned_port {
                    Some(port) => match self.connect_usb(port).await {
                        Ok(active) => return Ok(active),
                        Err(error) => error,
                    },
                    None => "no explicitly selected USB device".into(),
                };
                let Some(ble_id) = selected_ble else {
                    return Err(format!(
                        "Automatic reconnect is parked until a device is explicitly selected. USB: {usb_attempt}."
                    ));
                };
                self.connect_ble(Some(ble_id), &preferences.display.ble_name)
                    .await
                    .map_err(|ble| {
                        format!("Automatic connection failed. USB: {usb_attempt}. Bluetooth: {ble}")
                    })
            }
        }
    }

    async fn send_frame(
        &self,
        frame: &MonoFrame,
        preferences: &AppPreferences,
        force: bool,
    ) -> Result<Option<(TransportReceipt, String)>, String> {
        let _operation = self.operation.lock().await;
        if !force && !self.delivery_armed.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if !force
            && self
                .last_frame
                .lock()
                .await
                .as_deref()
                .is_some_and(|last| last == frame.packed())
        {
            return Ok(None);
        }
        if self.active.read().await.is_none() {
            self.automatic_reconnect.store(true, Ordering::Relaxed);
            let active = self.connect_preferred_locked(preferences).await?;
            *self.active.write().await = Some(active);
        }
        let active = self
            .active
            .read()
            .await
            .clone()
            .ok_or_else(|| "No display connection is active.".to_owned())?;
        let next = self.frames_sent.load(Ordering::Relaxed).saturating_add(1);
        let cadence = u64::from(preferences.display.full_refresh_every.max(1));
        let refresh = if next.is_multiple_of(cadence) {
            RefreshMode::Full
        } else {
            RefreshMode::Fast
        };
        let receipt = active.send(frame, refresh).await?;
        self.delivery_armed.store(true, Ordering::Relaxed);
        self.frames_sent.fetch_add(1, Ordering::Relaxed);
        *self.last_frame.lock().await = Some(frame.packed().to_vec());
        Ok(Some((receipt, active.name().to_owned())))
    }
}

struct DesktopState {
    engine: Arc<RuntimeEngine>,
    store: Store,
    secret_store: LocalSecretStore,
    dispatch_lock: Arc<AsyncMutex<()>>,
    ais_lock: Arc<AsyncMutex<()>>,
    ais_key_fingerprint: StdMutex<Option<String>>,
    scheduler: StdMutex<Option<SchedulerHandle>>,
    display: Arc<DisplayController>,
    display_task: StdMutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    dispatch_task: StdMutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    radar: RadarCache,
    radar_collector: Arc<dyn Collector>,
    radar_fetcher: Arc<dyn HttpFetcher>,
}

impl DesktopState {
    fn shutdown(&self) {
        self.engine.cancel();
        self.scheduler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(task) = self
            .display_task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
        if let Some(task) = self
            .dispatch_task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}

impl Drop for DesktopState {
    fn drop(&mut self) {
        self.engine.cancel();
        if let Ok(task) = self.display_task.get_mut()
            && let Some(task) = task.take()
        {
            task.abort();
        }
        if let Ok(task) = self.dispatch_task.get_mut()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

pub fn set_e213_transport_status(app: &AppHandle, status: DisplayConnectionStatus) {
    let state = app.state::<E213TrayState>();
    *state
        .current
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = status.clone();
    let (status_line, detail_line) = status.menu_lines();
    if let Err(error) = state.status_item.set_text(&status_line) {
        warn!(%error, "display tray status text update failed");
    }
    if let Err(error) = state.detail_item.set_text(&detail_line) {
        warn!(%error, "display tray detail text update failed");
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(error) = tray.set_tooltip(Some(format!(
            "Tender's Log · {status_line} · {detail_line}"
        ))) {
            warn!(%error, "display tray tooltip update failed");
        }
        // macOS renders this compact monochrome status beside the template
        // icon. Other platforms retain the state in tooltip/menu text.
        if let Err(error) = tray.set_title(Some(status.tray_badge())) {
            warn!(%error, "display tray badge update failed");
        }
    }
    if let Err(error) = app.emit(STATUS_EVENT, status) {
        warn!(%error, "display status event emission failed");
    }
}

/// What the app knows about firmware on an attached board, for the UI.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareStatus {
    /// Serial port of the attached Espressif board, when one is present.
    pub port: Option<String>,
    /// Build this app ships, when it ships one.
    pub bundled_build: Option<String>,
    /// Variants available to flash.
    pub variants: Vec<FirmwareVariantSummary>,
    /// Whether the operator should be prompted, and why.
    pub requirement: firmware::FlashRequirement,
    /// Why firmware is unavailable, when it is.
    pub unavailable: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareVariantSummary {
    pub id: String,
    pub label: String,
    pub panel_revision: firmware::PanelRevision,
    pub total_bytes: usize,
}

fn firmware_root(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .resolve("firmware", tauri::path::BaseDirectory::Resource)
        .ok()
}

/// Reports whether an attached board needs the bundled firmware.
///
/// Detection of the board is automatic. The panel revision is not and cannot
/// be: both builds run on the same ESP32-S3 with the same USB identifiers, and
/// only the physical display differs, so the variant stays an operator choice.
#[tauri::command]
async fn get_firmware_status(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<FirmwareStatus, String> {
    let connected_ready = state.display.usb_ready_observed().await;
    let devices = bridgestatus_eink::transport::discover_espressif_devices()
        .await
        .unwrap_or_default();
    let port = devices.first().map(|device| device.port.clone());

    let Some(root) = firmware_root(&app) else {
        return Ok(FirmwareStatus {
            port,
            bundled_build: None,
            variants: Vec::new(),
            requirement: firmware::FlashRequirement::NoDevice,
            unavailable: Some("this build ships no firmware".into()),
        });
    };

    let bundle = match firmware::FirmwareBundle::load(&root) {
        Ok(bundle) => bundle,
        Err(error) => {
            return Ok(FirmwareStatus {
                port,
                bundled_build: None,
                variants: Vec::new(),
                requirement: firmware::FlashRequirement::NoDevice,
                unavailable: Some(error.to_string()),
            });
        }
    };

    // Enumeration alone cannot say whether the board is running our firmware,
    // only that something Espressif is attached. Opening the port and waiting
    // for the READY INK1 banner is the only way to tell a working device from a
    // blank one, and a blank one is the case worth prompting about.
    //
    // The banner does not yet carry a build id in the field -- the firmware that
    // reports one has not shipped to any device -- so a board that answers is
    // reported as an unknown build rather than as matching. That is the honest
    // reading: it works, and we cannot say which build it is.
    let banner = match port.as_deref() {
        None => None,
        // A connected display already answered this question when it connected,
        // so reuse that rather than opening the port a second time and
        // contending with the transport on every status poll.
        Some(_) if connected_ready.is_some() => Some(firmware::DeviceBanner {
            saw_ready: connected_ready.unwrap_or(false),
            build: None,
        }),
        Some(port) => {
            let transport = bridgestatus_eink::transport::UsbTransport::new(
                bridgestatus_eink::transport::UsbConfig {
                    port: Some(port.to_owned()),
                    ..Default::default()
                },
            );
            let ready = transport
                .ensure_connected()
                .await
                .map(|info| info.ready_observed)
                .unwrap_or(false);
            transport.disconnect().await;
            Some(firmware::DeviceBanner {
                saw_ready: ready,
                build: None,
            })
        }
    };
    let requirement =
        firmware::evaluate_flash_requirement(banner.as_ref(), bundle.source_revision.as_deref());

    Ok(FirmwareStatus {
        port,
        bundled_build: bundle.source_revision.clone(),
        variants: bundle
            .variants()
            .iter()
            .map(|variant| FirmwareVariantSummary {
                id: variant.id.clone(),
                label: variant.label.clone(),
                panel_revision: variant.panel_revision,
                total_bytes: variant.total_bytes(),
            })
            .collect(),
        requirement,
        unavailable: None,
    })
}

/// Writes a bundled variant to the attached board, emitting progress events.
#[tauri::command]
async fn flash_firmware(
    app: AppHandle,
    state: State<'_, DesktopState>,
    variant_id: String,
    port: String,
) -> Result<(), String> {
    let root = firmware_root(&app).ok_or("this build ships no firmware")?;
    let bundle = firmware::FirmwareBundle::load(&root).map_err(|error| error.to_string())?;
    let variant = bundle
        .variant(&variant_id)
        .ok_or_else(|| format!("unknown firmware variant {variant_id:?}"))?
        .clone();

    // The display transport holds this same USB CDC device open and macOS hands
    // it out exclusively, so flashing while connected fails instantly with
    // "Device or resource busy". Release it first, and keep automatic reconnect
    // suppressed: a reconnect that wins the port mid-write would leave a
    // half-written bootloader, which is the one outcome that truly bricks the
    // board.
    let was_connected = state.display.release_port_for_flash().await;
    // The kernel does not always free the descriptor the instant the handle
    // drops; espflash sees the stale one and reports it busy.
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

    // espflash drives the serial bootloader synchronously and a flash takes
    // tens of seconds, so it must not run on the async runtime.
    let emitter = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let mut progress = EmitProgress {
            app: emitter,
            total: variant.total_bytes(),
            done: 0,
            current: 0,
        };
        firmware::flash_variant(&port, &variant, &mut progress)
    })
    .await
    .map_err(|error| error.to_string())?;

    // Restore the display whether or not the flash worked. A failed flash that
    // also leaves the display disconnected turns one problem into two.
    if was_connected {
        let preferences = state.engine.get_preferences().await;
        state.display.restore_port_after_flash(&preferences).await;
    }
    outcome.map_err(|error| error.to_string())
}

struct EmitProgress {
    app: AppHandle,
    total: usize,
    done: usize,
    current: usize,
}

impl EmitProgress {
    fn emit(&self, stage: &str) {
        let written = self.done + self.current;
        let _ = self.app.emit(
            "firmware://progress",
            serde_json::json!({
                "stage": stage,
                "written": written,
                "total": self.total,
            }),
        );
    }
}

impl firmware::FlashProgress for EmitProgress {
    fn segment_started(&mut self, _offset: u32, _total: usize) {
        self.current = 0;
        self.emit("writing");
    }

    fn segment_advanced(&mut self, written: usize) {
        self.current = written;
        self.emit("writing");
    }

    fn verifying(&mut self) {
        self.emit("verifying");
    }

    fn segment_finished(&mut self, _skipped: bool) {
        self.done += self.current;
        self.current = 0;
        self.emit("writing");
    }
}

#[tauri::command]
fn get_display_status(state: State<'_, E213TrayState>) -> DisplayConnectionStatus {
    state
        .current
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tauri::command]
async fn get_app_snapshot(
    state: State<'_, DesktopState>,
    tray: State<'_, E213TrayState>,
) -> Result<AppSnapshot, String> {
    let mut snapshot = state
        .engine
        .get_snapshot()
        .await
        .map_err(|error| error.to_string())?;
    let preferences = state.engine.get_preferences().await;
    let display_status = tray
        .current
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(output) = snapshot
        .outputs
        .iter_mut()
        .find(|output| output.id == DestinationIdDto::Epaper)
    {
        if preferences.display.transport == DisplayTransport::Preview {
            output.state = OutputStateDto::Unconfigured;
            output.detail = "Preview only · no hardware delivery".into();
        } else {
            output.state = match display_status.state {
                DisplayConnectionState::Connected if display_status.last_ack_at.is_some() => {
                    OutputStateDto::Ready
                }
                DisplayConnectionState::Connected => OutputStateDto::Degraded,
                DisplayConnectionState::Connecting => OutputStateDto::Degraded,
                DisplayConnectionState::Disconnected
                | DisplayConnectionState::Unavailable
                | DisplayConnectionState::Error => OutputStateDto::Offline,
            };
            output.detail = display_status.detail;
        }
    }
    snapshot.dispatches = dispatch_history(&state.store).await?;
    if let Some(output) = snapshot
        .outputs
        .iter_mut()
        .find(|output| output.id == DestinationIdDto::Whatsapp)
    {
        if preferences.whatsapp.enabled
            && preferences.whatsapp.token_configured
            && whatsapp_consent_is_current(&preferences.whatsapp)
        {
            output.state = OutputStateDto::Ready;
            output.detail =
                "Desktop outbox worker attached · material changes only · Meta acceptance recorded locally"
                    .into();
        }
        if let Some(latest) = snapshot
            .dispatches
            .iter()
            .find(|record| record.destinations.contains(&DestinationIdDto::Whatsapp))
        {
            output.delivery_state = Some(latest.delivery_state);
        }
        output.last_accepted_at = snapshot
            .dispatches
            .iter()
            .find(|record| {
                record.destinations.contains(&DestinationIdDto::Whatsapp)
                    && matches!(
                        record.delivery_state,
                        DeliveryStateDto::Accepted | DeliveryStateDto::Delivered
                    )
            })
            .map(|record| record.at.clone());
    }
    Ok(snapshot)
}

async fn dispatch_history(store: &Store) -> Result<Vec<DispatchRecord>, String> {
    let rows = store
        .list_outbox_history(200)
        .await
        .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            if row.route_id != WHATSAPP_ROUTE_ID {
                return None;
            }
            let request = match serde_json::from_str::<DeliveryRequest>(&row.request_json) {
                Ok(request) => request,
                Err(error) => {
                    warn!(outbox_id = %row.id, %error, "stored delivery request could not be rendered in the ledger");
                    return None;
                }
            };
            Some(DispatchRecord {
                id: row.id,
                incident_id: row.incident_id,
                material_revision: u32::try_from(row.material_revision.max(1))
                    .unwrap_or(u32::MAX),
                at: row.updated_at,
                channel_id: request.destination.id,
                title: request.notice.subject,
                state: notice_state_key(request.notice.state).into(),
                urgency: persisted_urgency(row.urgency.as_deref()),
                destinations: vec![DestinationIdDto::Whatsapp],
                delivery_state: delivery_state_for_outbox(&row.status),
            })
        })
        .collect())
}

const fn notice_state_key(state: NoticeState) -> &'static str {
    match state {
        NoticeState::Alert => "alert",
        NoticeState::Resolved => "resolved",
        NoticeState::Clear => "clear",
        NoticeState::Watch => "watch",
        NoticeState::Likely => "likely",
        NoticeState::Open => "open",
        NoticeState::AllClear => "all_clear",
        NoticeState::Unknown => "unknown",
    }
}

fn persisted_urgency(value: Option<&str>) -> UrgencyDto {
    match value {
        Some("emergency" | "confirmed_only") => UrgencyDto::Emergency,
        Some("action" | "meaningful") => UrgencyDto::Action,
        Some("routine" | "off" | "custom") => UrgencyDto::Routine,
        Some("heads_up" | "recommended") | None | Some(_) => UrgencyDto::HeadsUp,
    }
}

const fn delivery_state_for_outbox(status: &str) -> DeliveryStateDto {
    match status.as_bytes() {
        b"accepted" => DeliveryStateDto::Accepted,
        b"delivered" => DeliveryStateDto::Delivered,
        b"failed" => DeliveryStateDto::Failed,
        b"suppressed" => DeliveryStateDto::Suppressed,
        _ => DeliveryStateDto::Pending,
    }
}

#[tauri::command]
async fn get_preferences(state: State<'_, DesktopState>) -> Result<AppPreferences, String> {
    let _ais_guard = state.ais_lock.lock().await;
    let ais_truth = reconcile_aisstream_secret_locked(&state).await;
    let mut preferences = state.engine.get_preferences().await;
    drop(_ais_guard);
    preferences.whatsapp.token_configured = state.secret_store.whatsapp_token().await?.is_some();
    preferences.ais.api_key_configured = match ais_truth {
        Ok(truth) => truth.present && !truth.invalid,
        Err(error) => {
            warn!(%error, "AISStream local credential could not be reconciled");
            preferences.ais.api_key_configured
        }
    };
    Ok(preferences)
}

#[tauri::command]
async fn get_aisstream_status(state: State<'_, DesktopState>) -> Result<AisStreamStatus, String> {
    let runtime = state
        .engine
        .get_aisstream_status()
        .await
        .map_err(|error| error.to_string())?;
    let configured = runtime.api_key_configured;
    let source_state = match runtime.connection_state {
        AisConnectionStateDto::Disabled => AisStreamSourceState::Disabled,
        AisConnectionStateDto::NeedsKey => AisStreamSourceState::MissingKey,
        AisConnectionStateDto::Armed => AisStreamSourceState::Ready,
        AisConnectionStateDto::Live => AisStreamSourceState::Live,
        AisConnectionStateDto::Rejected | AisConnectionStateDto::Disconnected => {
            AisStreamSourceState::Degraded
        }
    };
    let state = if !runtime.enabled {
        AisStreamSourceState::Disabled
    } else if !configured {
        AisStreamSourceState::MissingKey
    } else {
        source_state
    };
    let vessels_in_range = (runtime.last_success_at.is_some()
        || state == AisStreamSourceState::Live)
        .then_some(runtime.fresh_vessel_count);
    Ok(AisStreamStatus {
        configured,
        enabled: runtime.enabled,
        state,
        detail: runtime.detail,
        last_position_at: runtime.last_position_at,
        vessels_in_range,
    })
}

#[tauri::command]
async fn save_preferences(
    app: AppHandle,
    state: State<'_, DesktopState>,
    mut preferences: AppPreferences,
) -> Result<MutationResult, String> {
    let _ais_guard = state.ais_lock.lock().await;
    let ais_truth = match reconcile_aisstream_secret_locked(&state).await {
        Ok(truth) => truth,
        Err(error) => {
            return Ok(mutation_error(format!(
                "Preferences were not saved because the AIS credential could not be verified: {error}"
            )));
        }
    };
    let _dispatch_guard = state.dispatch_lock.lock().await;
    let old = state.engine.get_preferences().await;
    preferences.whatsapp.token_configured = match state.secret_store.whatsapp_token().await {
        Ok(token) => token.is_some(),
        Err(error) => {
            return Ok(mutation_error(format!(
                "Preferences were not saved because the Meta credential could not be read: {error}"
            )));
        }
    };
    preferences.ais.api_key_configured = ais_truth.present && !ais_truth.invalid;
    let result = state
        .engine
        .save_preferences(preferences.clone())
        .await
        .map_err(|error| error.to_string())?;
    let persisted_tracker = state
        .store
        .get_json::<WhatsAppDispatchTracker>(DISPATCH_TRACKER_KEY)
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let current_route_fingerprint = whatsapp_route_fingerprint(&preferences);
    let tracker_needs_reconciliation = persisted_tracker.route_fingerprint.as_deref()
        != current_route_fingerprint.as_deref()
        || (current_route_fingerprint.is_none() && !persisted_tracker.channels.is_empty());
    if whatsapp_route_changed_or_revoked(&old, &preferences) || tracker_needs_reconciliation {
        let now = Timestamp::now().to_string();
        if let Err(error) = suppress_and_reset_whatsapp_route(
            &state.store,
            &now,
            "WhatsApp route, recipient, or consent changed",
        )
        .await
        {
            return Ok(mutation_error(format!(
                "Preferences were saved, but old WhatsApp work could not be scrubbed: {error}"
            )));
        }
    }
    // Alert routing is now reconciled. Never hold the dispatch lane across
    // platform display teardown: an OS Bluetooth/USB future may be slow or
    // wedged, while WhatsApp and native notices must keep progressing.
    drop(_dispatch_guard);
    drop(_ais_guard);
    if old.display != preferences.display && state.display.has_active().await {
        match tokio::time::timeout(Duration::from_secs(10), state.display.disconnect(false)).await {
            Err(_) => {
                set_e213_transport_status(
                    &app,
                    DisplayConnectionStatus {
                        state: DisplayConnectionState::Error,
                        detail: "Display disconnect exceeded its 10 second deadline; alert dispatch remains active."
                            .into(),
                        ..DisplayConnectionStatus::default()
                    },
                );
            }
            Ok(Ok(())) => {
                set_e213_transport_status(
                    &app,
                    DisplayConnectionStatus {
                        detail: "Display settings changed; reconnect to apply them.".into(),
                        ..DisplayConnectionStatus::default()
                    },
                );
            }
            Ok(Err(error)) => {
                set_e213_transport_status(&app, error_status(None, error));
            }
        }
    }
    Ok(result)
}

#[tauri::command]
async fn refresh_sources(state: State<'_, DesktopState>) -> Result<MutationResult, String> {
    state
        .engine
        .refresh_sources()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn search_locations(
    state: State<'_, DesktopState>,
    query: String,
) -> Result<Vec<LocationSearchResult>, String> {
    state
        .engine
        .search_locations(&query)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn scan_display_devices(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Vec<DisplayDeviceCandidate>, String> {
    let preferences = state.engine.get_preferences().await;
    let previous = get_current_status(&app);
    if previous.state != DisplayConnectionState::Connected {
        set_e213_transport_status(
            &app,
            DisplayConnectionStatus {
                state: DisplayConnectionState::Connecting,
                transport: None,
                device_name: None,
                detail: "Scanning USB and Bluetooth Low Energy for an INK1 display…".into(),
                last_frame_at: previous.last_frame_at.clone(),
                last_ack_at: previous.last_ack_at.clone(),
            },
        );
    }
    let (devices, errors) = state.display.scan(&preferences).await;
    if previous.state != DisplayConnectionState::Connected {
        let status = if devices.is_empty() {
            DisplayConnectionStatus {
                state: DisplayConnectionState::Unavailable,
                transport: None,
                device_name: None,
                detail: if errors.is_empty() {
                    "No compatible E213 display found. Check USB power or Bluetooth advertising."
                        .into()
                } else {
                    format!("No compatible display found. {}", errors.join(" "))
                },
                last_frame_at: previous.last_frame_at,
                last_ack_at: previous.last_ack_at,
            }
        } else {
            DisplayConnectionStatus {
                state: DisplayConnectionState::Disconnected,
                transport: None,
                device_name: None,
                detail: format!(
                    "{} compatible display{} found; choose one to connect.",
                    devices.len(),
                    if devices.len() == 1 { "" } else { "s" }
                ),
                last_frame_at: previous.last_frame_at,
                last_ack_at: previous.last_ack_at,
            }
        };
        set_e213_transport_status(&app, status);
    }
    Ok(devices)
}

#[tauri::command]
async fn connect_display_device(
    app: AppHandle,
    state: State<'_, DesktopState>,
    device_id: String,
    transport: DisplayConnectionTransport,
) -> Result<DisplayConnectionStatus, String> {
    let previous = get_current_status(&app);
    set_e213_transport_status(
        &app,
        DisplayConnectionStatus {
            state: DisplayConnectionState::Connecting,
            transport: Some(transport),
            device_name: None,
            detail: match transport {
                DisplayConnectionTransport::Usb => {
                    "Opening the selected USB serial interface…".into()
                }
                DisplayConnectionTransport::Ble => {
                    "Connecting and verifying the INK1 BLE service…".into()
                }
            },
            last_frame_at: previous.last_frame_at,
            last_ack_at: previous.last_ack_at,
        },
    );
    let preferences = state.engine.get_preferences().await;
    let status = match state
        .display
        .connect_selected(&device_id, transport, &preferences)
        .await
    {
        Ok(status) => status,
        Err(error) => error_status(Some(transport), error),
    };
    set_e213_transport_status(&app, status.clone());
    Ok(status)
}

#[tauri::command]
async fn disconnect_display_device(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<DisplayConnectionStatus, String> {
    let status = match state.display.disconnect(true).await {
        Ok(()) => DisplayConnectionStatus {
            detail: "Display disconnected. Collectors and alerts continue in the background."
                .into(),
            ..DisplayConnectionStatus::default()
        },
        Err(error) => error_status(None, error),
    };
    set_e213_transport_status(&app, status.clone());
    Ok(status)
}

#[tauri::command]
async fn send_display_test_frame(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<MutationResult, String> {
    let snapshot = match state.engine.get_snapshot().await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Ok(mutation_error(format!(
                "Could not build the current frame: {error}"
            )));
        }
    };
    let preferences = state.engine.get_preferences().await;
    Ok(send_snapshot_to_display(&app, &state.display, &snapshot, &preferences, true).await)
}

#[tauri::command]
async fn get_eink_preview(
    state: State<'_, DesktopState>,
    channel_id: Option<String>,
) -> Result<EinkPreviewDto, String> {
    let snapshot = state
        .engine
        .get_snapshot()
        .await
        .map_err(|error| format!("Could not build the current preview: {error}"))?;
    let preferences = state.engine.get_preferences().await;
    let requested = channel_id
        .as_deref()
        .unwrap_or(&snapshot.decision.channel_id);
    let channel = snapshot
        .channels
        .iter()
        .find(|channel| channel.id == requested)
        .ok_or_else(|| format!("No current channel exists for {requested:?}."))?;
    let frame = if channel.kind == ChannelKindDto::Bridge {
        render_snapshot(&display_snapshot(&snapshot), &RenderConfig::default())
            .map_err(|error| format!("Bridge preview render failed: {error}"))?
    } else {
        render_channel_card(&channel_card(channel, &preferences, &snapshot))
            .map_err(|error| format!("Channel preview render failed: {error}"))?
    };
    let png = preview_png_bytes(&frame)
        .map_err(|error| format!("E-paper preview encoding failed: {error}"))?;
    Ok(EinkPreviewDto {
        channel_id: channel.id.clone(),
        png,
    })
}

#[tauri::command]
async fn set_whatsapp_token(
    state: State<'_, DesktopState>,
    token: String,
) -> Result<MutationResult, String> {
    let token = token.trim();
    if token.is_empty() {
        return Ok(mutation_error("Meta access token cannot be empty."));
    }
    if token.chars().count() > 4096 {
        return Ok(mutation_error(
            "Meta access token is longer than the supported local entry.",
        ));
    }
    if let Err(error) = state
        .secret_store
        .store_whatsapp_token(token.to_owned())
        .await
    {
        return Ok(mutation_error(error));
    }
    let _dispatch_guard = state.dispatch_lock.lock().await;
    let now = Timestamp::now().to_string();
    if let Err(error) = suppress_and_reset_whatsapp_route(
        &state.store,
        &now,
        "Meta credential was added or replaced",
    )
    .await
    {
        return Ok(mutation_error(format!(
            "The new token is saved, but the existing route could not be reset safely: {error}. Retry Store secret before relying on delivery."
        )));
    }
    let mut preferences = state.engine.get_preferences().await;
    preferences.whatsapp.token_configured = true;
    Ok(match state.engine.save_preferences(preferences).await {
        Ok(_) => MutationResult {
            ok: true,
            message: "Meta token stored in the app's local credential file.".into(),
        },
        Err(error) => mutation_error(format!(
            "Token is saved locally, but its configured flag could not be saved: {error}"
        )),
    })
}

#[tauri::command]
async fn clear_whatsapp_token(state: State<'_, DesktopState>) -> Result<MutationResult, String> {
    let _dispatch_guard = state.dispatch_lock.lock().await;
    let mut preferences = state.engine.get_preferences().await;
    park_whatsapp_before_secret_delete(&mut preferences);
    if let Err(error) = state.engine.save_preferences(preferences).await {
        return Ok(mutation_error(format!(
            "WhatsApp delivery could not be parked safely, so the local token was left intact: {error}"
        )));
    }
    let now = Timestamp::now().to_string();
    if let Err(error) =
        suppress_and_reset_whatsapp_route(&state.store, &now, "Meta credential was removed").await
    {
        return Ok(mutation_error(format!(
            "WhatsApp delivery is parked, but queued recipient data could not be scrubbed: {error}. The local token was left intact."
        )));
    }
    drop(_dispatch_guard);
    if let Err(error) = state.secret_store.delete_whatsapp_token().await {
        return Ok(mutation_error(format!(
            "WhatsApp delivery is parked and queued work is scrubbed, but the local token could not be removed: {error}"
        )));
    }
    Ok(MutationResult {
        ok: true,
        message: "Meta token removed; unsent work was cancelled and recipient data was scrubbed."
            .into(),
    })
}

#[tauri::command]
async fn set_aisstream_api_key(
    state: State<'_, DesktopState>,
    api_key: String,
) -> Result<MutationResult, String> {
    let _ais_guard = state.ais_lock.lock().await;
    let api_key = api_key.trim().to_owned();
    if !aisstream_key_shape_valid(&api_key) {
        return Ok(mutation_error(
            "AISStream API key must contain 8 to 512 non-control characters.",
        ));
    }
    let new_fingerprint = aisstream_key_fingerprint(&api_key);
    let previous = match state.secret_store.aisstream_key().await {
        Ok(previous) => previous,
        Err(error) => return Ok(mutation_error(error)),
    };

    if let Err(error) = state.engine.set_aisstream_key(Some(api_key.clone())).await {
        return Ok(mutation_error(format!(
            "AISStream key was rejected before it reached local storage: {error}"
        )));
    }
    if let Err(error) = state.secret_store.store_aisstream_key(api_key).await {
        let rollback_key = previous.clone();
        let rollback = state.engine.set_aisstream_key(previous).await;
        if rollback.is_ok() {
            *state
                .ais_key_fingerprint
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                rollback_key.as_deref().map(aisstream_key_fingerprint);
        }
        return Ok(mutation_error(match rollback {
            Ok(_) => format!("{error}. The prior live AIS session was restored."),
            Err(rollback_error) => {
                format!("{error}. The prior AIS session could not be restored: {rollback_error}")
            }
        }));
    }
    *state
        .ais_key_fingerprint
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(new_fingerprint);

    Ok(MutationResult {
        ok: true,
        message: "AISStream key saved in the app's local credential file; the live source is armed without an app restart."
            .into(),
    })
}

#[tauri::command]
async fn clear_aisstream_api_key(state: State<'_, DesktopState>) -> Result<MutationResult, String> {
    let _ais_guard = state.ais_lock.lock().await;
    match state.secret_store.aisstream_key().await {
        Ok(_) => {}
        Err(error) => return Ok(mutation_error(error)),
    }

    let mut preferences = state.engine.get_preferences().await;
    park_aisstream_before_secret_delete(&mut preferences);
    if let Err(error) = state.engine.save_preferences(preferences).await {
        return Ok(mutation_error(format!(
            "AISStream could not be parked safely, so the local key was left intact: {error}"
        )));
    }

    if let Err(error) = state.engine.set_aisstream_key(None).await {
        return Ok(mutation_error(format!(
            "AISStream collection could not be stopped safely; the local key was left intact: {error}"
        )));
    }
    *state
        .ais_key_fingerprint
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    if let Err(error) = state.secret_store.delete_aisstream_key().await {
        return Ok(mutation_error(format!(
            "AISStream is disabled and its live socket is stopped, but the local key could not be removed: {error}"
        )));
    }

    Ok(MutationResult {
        ok: true,
        message: "AISStream key removed; the WebSocket was cancelled and cached vessel evidence was retired."
            .into(),
    })
}

// Material-change tracking, native notices, quiet-hours gating, and the
// WhatsApp outbox form one delivery runtime independent of window plumbing.
include!("desktop/delivery_runtime.rs");
include!("desktop/panel_broker.rs");
include!("desktop/radar_layer.rs");
async fn send_snapshot_to_display(
    app: &AppHandle,
    display: &DisplayController,
    snapshot: &AppSnapshot,
    preferences: &AppPreferences,
    force: bool,
) -> MutationResult {
    let presentation = display_snapshot(snapshot);
    let frame = match render_snapshot(&presentation, &RenderConfig::default()) {
        Ok(frame) => frame,
        Err(error) => return mutation_error(format!("Frame render failed: {error}")),
    };
    send_rendered_frame_to_display(app, display, &frame, preferences, force).await
}

async fn send_rotating_snapshot_to_display(
    app: &AppHandle,
    display: &DisplayController,
    snapshot: &AppSnapshot,
    preferences: &AppPreferences,
    selection: &PanelSelection,
    radar: Option<&RadarFigure>,
) -> (MutationResult, Option<String>) {
    let channel_id = match selection {
        PanelSelection::Alert { channel_id, .. } | PanelSelection::Rotation { channel_id } => {
            channel_id
        }
    };
    let Some(channel) = snapshot
        .channels
        .iter()
        .find(|channel| &channel.id == channel_id)
    else {
        return (
            mutation_error("The selected e-paper channel is no longer present."),
            None,
        );
    };
    let frame = if channel.kind == ChannelKindDto::Bridge {
        match render_snapshot(&display_snapshot(snapshot), &RenderConfig::default()) {
            Ok(frame) => frame,
            Err(error) => {
                return (
                    mutation_error(format!("Bridge frame render failed: {error}")),
                    Some(channel.id.clone()),
                );
            }
        }
    } else {
        // Radar corroborates a weather card and would be noise on any other.
        let radar = radar.filter(|_| channel.kind == ChannelKindDto::Weather);
        match render_channel_card_with_radar(&channel_card(channel, preferences, snapshot), radar) {
            Ok(frame) => frame,
            Err(error) => {
                return (
                    mutation_error(format!("Channel frame render failed: {error}")),
                    Some(channel.id.clone()),
                );
            }
        }
    };
    (
        send_rendered_frame_to_display(app, display, &frame, preferences, false).await,
        Some(channel.id.clone()),
    )
}

async fn send_rendered_frame_to_display(
    app: &AppHandle,
    display: &DisplayController,
    frame: &MonoFrame,
    preferences: &AppPreferences,
    force: bool,
) -> MutationResult {
    if !force && !display.delivery_armed() {
        return MutationResult {
            ok: true,
            message:
                "Display transport is open but parked until an explicit test frame receives ACK INK1."
                    .into(),
        };
    }
    if !display.has_active().await {
        set_e213_transport_status(
            app,
            DisplayConnectionStatus {
                state: DisplayConnectionState::Connecting,
                transport: preferred_transport(preferences.display.transport),
                device_name: None,
                detail: "Connecting to the configured E213 display…".into(),
                last_frame_at: None,
                last_ack_at: None,
            },
        );
    }
    match display.send_frame(frame, preferences, force).await {
        Ok(Some((receipt, name))) => {
            let at = Timestamp::now().to_string();
            let transport = receipt_transport(receipt.transport);
            let detail = format!(
                "{}{}",
                receipt.acknowledgement,
                if receipt.ready_observed {
                    " · READY observed"
                } else {
                    ""
                }
            );
            let status = DisplayConnectionStatus {
                state: DisplayConnectionState::Connected,
                transport: Some(transport),
                device_name: Some(name),
                detail,
                last_frame_at: Some(at.clone()),
                last_ack_at: Some(at),
            };
            set_e213_transport_status(app, status);
            MutationResult {
                ok: true,
                message: format!(
                    "Current frame acknowledged over {}.",
                    transport_label(transport)
                ),
            }
        }
        Ok(None) => MutationResult {
            ok: true,
            message: "Display already has this frame; no duplicate write was sent.".into(),
        },
        Err(error) => {
            let transport = get_current_status(app)
                .transport
                .or_else(|| preferred_transport(preferences.display.transport));
            set_e213_transport_status(app, error_status(transport, error.clone()));
            mutation_error(format!("Display did not acknowledge the frame: {error}"))
        }
    }
}

fn channel_card(
    channel: &ChannelSnapshot,
    preferences: &AppPreferences,
    snapshot: &AppSnapshot,
) -> ChannelCard {
    let kind = match channel.kind {
        ChannelKindDto::Weather => ChannelKind::Weather,
        ChannelKindDto::Official => ChannelKind::OfficialAlert,
        ChannelKindDto::Hurricane => ChannelKind::Tropical,
        ChannelKindDto::News => ChannelKind::News,
        ChannelKindDto::Earthquake => ChannelKind::Earthquake,
        ChannelKindDto::Markets => ChannelKind::Markets,
        ChannelKindDto::System => ChannelKind::Custom {
            label: "SYSTEM".into(),
            code: "SY".into(),
        },
        ChannelKindDto::Bridge => ChannelKind::Custom {
            label: "BRIDGE".into(),
            code: "BR".into(),
        },
    };
    let urgency = if interrupt_allows(channel, preferences, snapshot) {
        match channel.priority.urgency {
            UrgencyDto::Routine => ChannelUrgency::Routine,
            UrgencyDto::HeadsUp => ChannelUrgency::Advisory,
            UrgencyDto::Action => ChannelUrgency::Urgent,
            UrgencyDto::Emergency => ChannelUrgency::Critical,
        }
    } else {
        ChannelUrgency::Routine
    };
    let availability = match channel.availability {
        AvailabilityDto::Fresh | AvailabilityDto::Delayed => ChannelAvailability::Current,
        AvailabilityDto::Stale => ChannelAvailability::Stale,
        AvailabilityDto::Offline => ChannelAvailability::Offline,
    };
    let source = if matches!(
        availability,
        ChannelAvailability::Current | ChannelAvailability::Stale
    ) {
        ChannelSource::aged(bounded_text(&channel.source_label, 96), channel.age_seconds)
    } else {
        ChannelSource::unavailable(bounded_text(&channel.source_label, 96))
    };
    let signal = channel.signal.as_ref();
    let headline = if channel.active {
        signal.map_or(channel.summary.as_str(), |signal| signal.headline.as_str())
    } else {
        channel.summary.as_str()
    };
    let detail = if channel.active {
        signal.map_or("ACTIVE SIGNAL", |signal| signal.detail.as_str())
    } else {
        "MONITORING · NO MATERIAL ALERT"
    };
    let action = if channel.active {
        signal.map_or("ACTIVE SIGNAL", |signal| signal.action.as_str())
    } else {
        "NO MATERIAL CHANGE"
    };
    ChannelCard::new(
        kind,
        urgency,
        availability,
        bounded_text(&channel.title, 96),
        bounded_text(headline, 160),
        bounded_text(detail, 240),
        bounded_text(action, 160),
        source,
    )
}

fn bounded_text(value: &str, maximum: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let value = if normalized.is_empty() {
        "NO CURRENT DETAIL".to_owned()
    } else {
        normalized
    };
    value.chars().take(maximum).collect()
}

fn display_snapshot(snapshot: &AppSnapshot) -> LiveSnapshot {
    let state = match snapshot.decision.state {
        BridgeStateDto::Clear => SnapshotState::Clear,
        BridgeStateDto::Possible => SnapshotState::Watch,
        BridgeStateDto::Likely => SnapshotState::Likely,
        BridgeStateDto::Open => SnapshotState::Open,
    };
    let source = snapshot
        .evidence
        .iter()
        .find(|item| item.state == bridgestatus_runtime::EvidenceStateDto::Live)
        .map(|item| item.source_label.clone())
        .unwrap_or_else(|| "Tender's Log".into());
    let mut output = LiveSnapshot::brickell(
        state,
        Freshness::new(source, snapshot.decision.source_age_seconds, 300),
    );
    output.channel = snapshot.decision.subject.to_ascii_uppercase();
    output.road_meaning = snapshot.decision.meaning.to_ascii_uppercase();
    output.eta = snapshot
        .decision
        .eta_min
        .map(|minimum| EtaRange::new(minimum, snapshot.decision.eta_max.unwrap_or(minimum)));
    if state.is_predictive() {
        output.confidence_percent = snapshot.decision.confidence_bps.map(bps_to_percent);
    }
    output.evidence = snapshot
        .evidence
        .iter()
        .filter(|item| item.state == bridgestatus_runtime::EvidenceStateDto::Live)
        .take(3)
        .map(|item| {
            Evidence::new(
                item.title.to_ascii_uppercase(),
                item.source_label.to_ascii_uppercase(),
            )
        })
        .collect();
    output.spans = upstream_spans(
        &snapshot.bridge_intervals,
        TimeZone::get(&snapshot.local_time_zone).ok().as_ref(),
    );
    output
}

/// Condenses a bridge name into the two or three characters the E213 has room
/// for beside a clock time. Falls back to the leading alphanumerics of the key
/// so an unrecognized upstream span still appears rather than vanishing.
fn span_code(bridge_key: &str, bridge_name: &str) -> String {
    match bridge_key {
        "sw_2_ave" => "2AV".into(),
        "sw_1_st" => "1ST".into(),
        "w_flagler" => "FLG".into(),
        "nw_5_st" => "5ST".into(),
        "nw_12_ave" => "12A".into(),
        "nw_17_ave" => "17A".into(),
        "nw_22_ave" => "22A".into(),
        "nw_27_ave" => "27A".into(),
        _ => {
            let source = if bridge_key.is_empty() {
                bridge_name
            } else {
                bridge_key
            };
            let code: String = source
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .take(3)
                .collect();
            code.to_ascii_uppercase()
        }
    }
}

/// Current state of each upstream span, newest observation per bridge.
///
/// Only an interval that has not ended describes the present. A completed one
/// is history, and reporting it as still up would tell a driver the river is
/// blocked when it is not.
fn upstream_spans(
    intervals: &[bridgestatus_runtime::BridgeStateIntervalDto],
    zone: Option<&TimeZone>,
) -> Vec<bridgestatus_eink::SpanStatus> {
    let mut latest: BTreeMap<&str, &bridgestatus_runtime::BridgeStateIntervalDto> = BTreeMap::new();
    for interval in intervals {
        if interval.relation != bridgestatus_runtime::BridgeRelationDto::Upstream {
            continue;
        }
        let winner = match latest.get(interval.bridge_key.as_str()) {
            None => true,
            // An in-progress interval always beats a completed one, however
            // recent the completed one is; otherwise the later start wins.
            Some(held) => match (held.ended_at.is_none(), interval.ended_at.is_none()) {
                (false, true) => true,
                (true, false) => false,
                _ => interval.started_at > held.started_at,
            },
        };
        if winner {
            latest.insert(interval.bridge_key.as_str(), interval);
        }
    }

    let mut ordered = latest.values().copied().collect::<Vec<_>>();
    ordered.sort_by_key(|interval| interval.river_order);
    ordered
        .into_iter()
        .map(|interval| {
            let open = interval.ended_at.is_none()
                && interval.state == bridgestatus_runtime::ObservedBridgeStateDto::Up;
            let mut span = bridgestatus_eink::SpanStatus::new(
                span_code(&interval.bridge_key, &interval.bridge_name),
                open,
            );
            if open {
                span.opened_at = zone.and_then(|zone| local_clock(&interval.started_at, zone));
            }
            span
        })
        .collect()
}

/// Formats an RFC 3339 instant as a bare local `HH:MM`, which is all the E213
/// has room for and all a reader needs to line two openings up.
fn local_clock(instant: &str, zone: &TimeZone) -> Option<String> {
    let timestamp: Timestamp = instant.parse().ok()?;
    let zoned = timestamp.to_zoned(zone.clone());
    Some(format!("{:02}:{:02}", zoned.hour(), zoned.minute()))
}

/// Renders the current runtime snapshot through the same E213 path used by the
/// desktop application.
pub fn render_live_bridge_frame(
    snapshot: &AppSnapshot,
) -> Result<MonoFrame, bridgestatus_eink::RenderError> {
    render_snapshot(&display_snapshot(snapshot), &RenderConfig::default())
}

fn bps_to_percent(bps: u16) -> u8 {
    ((bps.saturating_add(50) / 100).min(100)) as u8
}

fn connected_status(
    active: &ActiveDisplay,
    last_frame_at: Option<String>,
    last_ack_at: Option<String>,
    detail: &str,
) -> DisplayConnectionStatus {
    DisplayConnectionStatus {
        state: DisplayConnectionState::Connected,
        transport: Some(active.transport()),
        device_name: Some(active.name().to_owned()),
        detail: detail.into(),
        last_frame_at,
        last_ack_at,
    }
}

fn error_status(
    transport: Option<DisplayConnectionTransport>,
    error: impl Into<String>,
) -> DisplayConnectionStatus {
    DisplayConnectionStatus {
        state: DisplayConnectionState::Error,
        transport,
        device_name: None,
        detail: error.into(),
        last_frame_at: None,
        last_ack_at: None,
    }
}

fn unavailable_or_error_status(
    transport: Option<DisplayConnectionTransport>,
    error: String,
    retry_seconds: u64,
) -> DisplayConnectionStatus {
    let unavailable = error.contains("not found")
        || error.contains("No compatible")
        || error.contains("no Bluetooth adapter")
        || error.contains("no Espressif USB");
    DisplayConnectionStatus {
        state: if unavailable {
            DisplayConnectionState::Unavailable
        } else {
            DisplayConnectionState::Error
        },
        transport,
        device_name: None,
        detail: format!("{error} · retrying in {retry_seconds}s"),
        last_frame_at: None,
        last_ack_at: None,
    }
}

fn preferred_transport(transport: DisplayTransport) -> Option<DisplayConnectionTransport> {
    match transport {
        DisplayTransport::Usb => Some(DisplayConnectionTransport::Usb),
        DisplayTransport::Ble => Some(DisplayConnectionTransport::Ble),
        DisplayTransport::Auto | DisplayTransport::Preview => None,
    }
}

fn receipt_transport(transport: TransportKind) -> DisplayConnectionTransport {
    match transport {
        TransportKind::Usb => DisplayConnectionTransport::Usb,
        TransportKind::Ble => DisplayConnectionTransport::Ble,
    }
}

fn transport_label(transport: DisplayConnectionTransport) -> &'static str {
    match transport {
        DisplayConnectionTransport::Usb => "USB",
        DisplayConnectionTransport::Ble => "Bluetooth Low Energy",
    }
}

fn mutation_error(message: impl Into<String>) -> MutationResult {
    MutationResult {
        ok: false,
        message: message.into(),
    }
}

fn park_whatsapp_before_secret_delete(preferences: &mut AppPreferences) {
    preferences.whatsapp.enabled = false;
    preferences.whatsapp.token_configured = false;
}

fn park_aisstream_before_secret_delete(preferences: &mut AppPreferences) {
    preferences.ais.enabled = false;
    preferences.ais.api_key_configured = false;
}

fn aisstream_key_shape_valid(key: &str) -> bool {
    key.trim() == key
        && (8..=512).contains(&key.chars().count())
        && !key.chars().any(char::is_control)
}

fn aisstream_key_fingerprint(key: &str) -> String {
    format!("{:x}", Sha256::digest(key.as_bytes()))
}

fn safe_id(value: &str) -> String {
    let compact = value.split_whitespace().collect::<String>();
    if compact.chars().count() <= 28 {
        compact
    } else {
        format!("{}…", compact.chars().take(27).collect::<String>())
    }
}

struct AisSecretTruth {
    present: bool,
    invalid: bool,
}

async fn reconcile_aisstream_secret_locked(state: &DesktopState) -> Result<AisSecretTruth, String> {
    let stored = state.secret_store.aisstream_key().await?;
    let present = stored.is_some();
    let invalid = stored
        .as_deref()
        .is_some_and(|key| !aisstream_key_shape_valid(key));
    let valid_key = stored.filter(|key| aisstream_key_shape_valid(key));
    let desired_fingerprint = valid_key.as_deref().map(aisstream_key_fingerprint);
    let installed_fingerprint = state
        .ais_key_fingerprint
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if installed_fingerprint != desired_fingerprint {
        state
            .engine
            .set_aisstream_key(valid_key)
            .await
            .map_err(|error| format!("AISStream runtime reconciliation failed: {error}"))?;
        *state
            .ais_key_fingerprint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = desired_fingerprint;
    }
    Ok(AisSecretTruth { present, invalid })
}

fn get_current_status(app: &AppHandle) -> DisplayConnectionStatus {
    app.state::<E213TrayState>()
        .current
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn clean_menu_text(value: &str) -> String {
    const MAX_CHARS: usize = 96;
    let flattened = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = flattened.chars();
    let mut output = characters.by_ref().take(MAX_CHARS).collect::<String>();
    if characters.next().is_some() {
        output.push('…');
    }
    output
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn install_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let initial = DisplayConnectionStatus::default();
    let (status_line, detail_line) = initial.menu_lines();
    let status_item = MenuItem::with_id(app, MENU_STATUS_ID, status_line, false, None::<&str>)?;
    let detail_item = MenuItem::with_id(app, MENU_DETAIL_ID, detail_line, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let open_item = MenuItem::with_id(app, MENU_OPEN_ID, "Open Tender’s Log", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT_ID, "Quit Tender’s Log", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &detail_item,
            &separator,
            &open_item,
            &quit_item,
        ],
    )?;
    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .title(initial.tray_badge())
        .tooltip("Tender’s Log · E213 disconnected")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN_ID => show_main_window(app),
            MENU_QUIT_ID => {
                if let Some(state) = app.try_state::<DesktopState>() {
                    state.shutdown();
                }
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    app.manage(E213TrayState {
        current: StdMutex::new(initial),
        status_item,
        detail_item,
    });
    Ok(())
}

fn install_runtime(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let database_path = data_dir.join("tenders-log.sqlite3");
    let secret_store = LocalSecretStore::new(data_dir.join("credentials.json"));
    let engine = tauri::async_runtime::block_on(async {
        let store = Store::open(database_path).await?;
        let config = RuntimeConfig::default();
        let factory = Arc::new(CredentialFreeCollectorFactory::new(
            config.user_agent.clone(),
        )?);
        let engine = RuntimeEngine::with_factory(store.clone(), config, factory).await?;
        let aisstream_key = match secret_store.aisstream_key().await {
            Ok(key) => key,
            Err(error) => {
                warn!(%error, "AISStream local credential was unavailable during startup");
                None
            }
        };
        let meta_token = match secret_store.whatsapp_token().await {
            Ok(token) => token,
            Err(error) => {
                warn!(%error, "Meta local credential was unavailable during startup");
                None
            }
        };
        let mut ais_key_fingerprint = aisstream_key
            .as_deref()
            .filter(|key| aisstream_key_shape_valid(key))
            .map(aisstream_key_fingerprint);
        if let Err(error) = engine.set_aisstream_key(aisstream_key).await {
            // AIS is optional. A corrupt local entry parks only this source.
            warn!(%error, "AISStream credential was rejected during startup; source parked");
            engine.set_aisstream_key(None).await?;
            ais_key_fingerprint = None;
        }
        let mut preferences = engine.get_preferences().await;
        let configured = meta_token.is_some();
        if preferences.whatsapp.token_configured != configured {
            preferences.whatsapp.token_configured = configured;
            engine.save_preferences(preferences).await?;
        }
        Ok::<_, bridgestatus_runtime::RuntimeError>((store, engine, ais_key_fingerprint))
    })?;
    let (store, engine, ais_key_fingerprint) = engine;
    let engine = Arc::new(engine);
    let scheduler = tauri::async_runtime::block_on(async { engine.spawn_scheduler() });
    let display_preferences = tauri::async_runtime::block_on(engine.get_preferences());
    let display = Arc::new(DisplayController::new(&display_preferences));
    let dispatch_lock = Arc::new(AsyncMutex::new(()));
    let ais_lock = Arc::new(AsyncMutex::new(()));
    let (display_task, dispatch_task) = spawn_background_pair(
        run_display_worker(
            app.handle().clone(),
            Arc::clone(&engine),
            Arc::clone(&display),
        ),
        run_dispatch_worker(
            app.handle().clone(),
            Arc::clone(&engine),
            store.clone(),
            secret_store.clone(),
            Arc::clone(&dispatch_lock),
        ),
    );
    app.manage(DesktopState {
        engine,
        store: store.clone(),
        secret_store,
        dispatch_lock,
        ais_lock,
        ais_key_fingerprint: StdMutex::new(ais_key_fingerprint),
        scheduler: StdMutex::new(Some(scheduler)),
        display,
        display_task: StdMutex::new(Some(display_task)),
        dispatch_task: StdMutex::new(Some(dispatch_task)),
        radar: RadarCache::default(),
        radar_collector: Arc::new(RainViewerCollector::new()),
        radar_fetcher: Arc::new(SafeHttpFetcher::default()),
    });
    Ok(())
}

fn spawn_background_pair<D, N>(
    display: D,
    notifications: N,
) -> (
    tauri::async_runtime::JoinHandle<()>,
    tauri::async_runtime::JoinHandle<()>,
)
where
    D: Future<Output = ()> + Send + 'static,
    N: Future<Output = ()> + Send + 'static,
{
    (
        tauri::async_runtime::spawn(display),
        tauri::async_runtime::spawn(notifications),
    )
}

async fn run_display_worker(
    app: AppHandle,
    engine: Arc<RuntimeEngine>,
    display: Arc<DisplayController>,
) {
    let broker = PanelBroker::default();
    let mut next_prove_at = tokio::time::Instant::now();
    let mut prove_failures = 0_u32;
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    let mut next_display_at = tokio::time::Instant::now();
    let mut next_reconnect_at = tokio::time::Instant::now();
    let mut reconnect_failures = 0_u32;
    loop {
        interval.tick().await;
        let snapshot = match engine.get_snapshot().await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                warn!(%error, "background display snapshot failed");
                continue;
            }
        };
        let preferences = engine.get_preferences().await;
        if !display.has_active().await
            && display.automatic_reconnect_enabled()
            && preferences.display.transport != DisplayTransport::Preview
            && tokio::time::Instant::now() >= next_reconnect_at
        {
            let selected_transport = preferred_transport(preferences.display.transport);
            set_e213_transport_status(
                &app,
                DisplayConnectionStatus {
                    state: DisplayConnectionState::Connecting,
                    transport: selected_transport,
                    device_name: None,
                    detail: "Background companion is reconnecting to the configured display…"
                        .into(),
                    last_frame_at: None,
                    last_ack_at: None,
                },
            );
            match tokio::time::timeout(
                Duration::from_secs(25),
                display.reconnect_preferred(&preferences),
            )
            .await
            .map_err(|_| "Display reconnect exceeded its 25 second deadline".to_owned())
            .and_then(std::convert::identity)
            {
                Ok(status) => {
                    reconnect_failures = 0;
                    next_reconnect_at = tokio::time::Instant::now();
                    set_e213_transport_status(&app, status);
                    let proof =
                        send_snapshot_to_display(&app, &display, &snapshot, &preferences, true)
                            .await;
                    if !proof.ok {
                        warn!(message = %proof.message, "saved display route did not acknowledge its startup frame");
                    }
                }
                Err(error) => {
                    reconnect_failures = reconnect_failures.saturating_add(1);
                    let exponent = reconnect_failures.saturating_sub(1).min(6);
                    let delay = 15_u64.saturating_mul(1_u64 << exponent).min(15 * 60);
                    next_reconnect_at = tokio::time::Instant::now() + Duration::from_secs(delay);
                    set_e213_transport_status(
                        &app,
                        unavailable_or_error_status(selected_transport, error, delay),
                    );
                }
            }
        }
        // Queue anything new before choosing, so an alert raised on this pass is
        // eligible immediately rather than one tick late.
        broker.ingest(&snapshot, &preferences);

        // Connected but unproven: send one forced frame to earn the acknowledgement
        // the panel requires. Without this the display stays parked after every
        // reconnect until somebody presses the test-frame button by hand.
        if should_prove_now(
            display.has_active().await,
            display.delivery_armed(),
            tokio::time::Instant::now(),
            next_prove_at,
        ) {
            let proof =
                send_snapshot_to_display(&app, &display, &snapshot, &preferences, true).await;
            if proof.ok {
                prove_failures = 0;
                debug!("display route proved; rotation is live");
            } else {
                prove_failures = prove_failures.saturating_add(1);
                warn!(message = %proof.message, "display proof frame failed");
            }
            next_prove_at = tokio::time::Instant::now() + prove_backoff(prove_failures);
        }

        if display.has_active().await && tokio::time::Instant::now() >= next_display_at {
            // The rotation index is read without advancing it. Only serving the
            // rotation lane consumes a slot; an alert must not, or a burst of
            // them would silently skip the anchor's home cadence.
            let selection = broker.next(&snapshot, &preferences, display.rotation_index());
            let Some(selection) = selection else {
                next_display_at = tokio::time::Instant::now() + Duration::from_secs(5);
                continue;
            };
            if matches!(selection, PanelSelection::Rotation { .. }) {
                display.next_rotation_index();
            }
            // Fetched only for the frame that will use it, and cached on the
            // frame identity, so a rotation that revisits weather every minute
            // does not re-fetch a composite that changes every ten.
            let radar = match &selection {
                PanelSelection::Alert { channel_id, .. }
                | PanelSelection::Rotation { channel_id } => {
                    // Bounded hard. Radar is decoration and the panel is the
                    // product: a tile host that hangs must cost the frame its
                    // figure, never its repaint.
                    tokio::time::timeout(
                        RADAR_FETCH_BUDGET,
                        panel_radar_figure(&app, &snapshot, &preferences, channel_id),
                    )
                    .await
                    .unwrap_or_default()
                }
            };
            let (result, channel_id) = match tokio::time::timeout(
                Duration::from_secs(30),
                send_rotating_snapshot_to_display(
                    &app,
                    &display,
                    &snapshot,
                    &preferences,
                    &selection,
                    radar.as_ref(),
                ),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => (
                    mutation_error("Display frame exceeded its 30 second deadline"),
                    None,
                ),
            };
            // An alert holds for a fixed, bounded time; a rotation frame keeps
            // its channel's configured dwell.
            let (dwell, holding_score) = match &selection {
                PanelSelection::Alert { score, .. } => (PanelBroker::alert_hold(), *score),
                PanelSelection::Rotation { .. } => {
                    let seconds = channel_id
                        .as_deref()
                        .and_then(|id| {
                            preferences
                                .profile
                                .channels
                                .iter()
                                .find(|channel| channel.id == id)
                        })
                        .map(|channel| channel.rotation_seconds)
                        .unwrap_or(preferences.display.dwell_seconds)
                        .max(5);
                    // Score zero: anything queued may preempt an ordinary frame.
                    (Duration::from_secs(u64::from(seconds)), 0)
                }
            };
            // The dwell and the interrupt wait are one primitive, so a new alert
            // lands within milliseconds instead of at the end of this frame.
            broker.wait_or_preempt(holding_score, dwell).await;
            next_display_at = tokio::time::Instant::now();
            if !result.ok {
                warn!(message = %result.message, "background display update failed");
            } else {
                debug!(message = %result.message, "background display update complete");
            }
        }
    }
}

async fn run_dispatch_worker(
    app: AppHandle,
    engine: Arc<RuntimeEngine>,
    store: Store,
    secret_store: LocalSecretStore,
    dispatch_lock: Arc<AsyncMutex<()>>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    let mut next_maintenance_at = tokio::time::Instant::now();
    loop {
        interval.tick().await;
        let preferences = engine.get_preferences().await;
        match engine.get_snapshot().await {
            Ok(snapshot) => {
                // Native notices do not depend on Meta. Submit them before
                // attempting the serialized WhatsApp lane.
                if let Err(error) =
                    dispatch_desktop_notifications(&app, &store, &preferences, &snapshot).await
                {
                    warn!(%error, "native notification dispatch failed");
                }
                match tokio::time::timeout(Duration::from_secs(1), dispatch_lock.lock()).await {
                    Ok(_dispatch_guard) => {
                        // A route mutation may have completed while native
                        // submission ran or while this worker waited for the
                        // lane. Re-read both values under the same lock used by
                        // consent/recipient/token mutations; never dispatch a
                        // stale recipient snapshot.
                        match current_whatsapp_dispatch_context(&engine).await {
                            Ok((whatsapp_preferences, whatsapp_snapshot)) => {
                                if let Err(error) = enqueue_material_whatsapp_updates(
                                    &store,
                                    &whatsapp_preferences,
                                    &whatsapp_snapshot,
                                )
                                .await
                                {
                                    warn!(%error, "WhatsApp material-update enqueue failed");
                                }
                                if let Err(error) = process_whatsapp_outbox(
                                    &store,
                                    &secret_store,
                                    &whatsapp_preferences,
                                    &whatsapp_snapshot,
                                )
                                .await
                                {
                                    warn!(%error, "WhatsApp outbox worker failed");
                                }
                            }
                            Err(error) => {
                                warn!(%error, "WhatsApp context refresh failed under dispatch lock")
                            }
                        }
                    }
                    Err(_) => debug!(
                        "WhatsApp dispatch lane is busy with a configuration mutation; retrying next cycle"
                    ),
                }
            }
            Err(error) => warn!(%error, "dispatch snapshot refresh failed"),
        }
        if tokio::time::Instant::now() >= next_maintenance_at {
            let now_ms = Timestamp::now().as_millisecond();
            let delivery_cutoff = iso_at(now_ms.saturating_sub(90 * 24 * 60 * 60 * 1_000));
            match delivery_cutoff {
                Ok(delivery_cutoff) => match store.prune_history(&delivery_cutoff).await {
                    Ok(report) => {
                        debug!(
                            scrubbed_destinations = report.scrubbed_destinations,
                            outbox_rows = report.outbox_rows,
                            incidents = report.incidents,
                            "local history retention completed"
                        );
                        next_maintenance_at =
                            tokio::time::Instant::now() + Duration::from_secs(24 * 60 * 60);
                    }
                    Err(error) => {
                        warn!(%error, "local history retention failed");
                        next_maintenance_at =
                            tokio::time::Instant::now() + Duration::from_secs(60 * 60);
                    }
                },
                Err(_) => {
                    warn!("local history retention cutoffs could not be represented");
                    next_maintenance_at =
                        tokio::time::Instant::now() + Duration::from_secs(60 * 60);
                }
            }
        }
    }
}

async fn current_whatsapp_dispatch_context(
    engine: &RuntimeEngine,
) -> Result<(AppPreferences, AppSnapshot), String> {
    let preferences = engine.get_preferences().await;
    let snapshot = engine
        .get_snapshot()
        .await
        .map_err(|error| error.to_string())?;
    Ok((preferences, snapshot))
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|_| "External link is not a valid URL.".to_owned())?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Only credential-free HTTPS links can open outside the app.".into());
    }
    Command::new("open")
        .arg(parsed.as_str())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("The system browser could not open this link: {error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = dotenvy::dotenv();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .try_init();
    let application = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            install_tray(app)?;
            install_runtime(app)
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            get_preferences,
            get_aisstream_status,
            save_preferences,
            refresh_sources,
            search_locations,
            get_display_status,
            scan_display_devices,
            connect_display_device,
            disconnect_display_device,
            send_display_test_frame,
            get_eink_preview,
            get_firmware_status,
            flash_firmware,
            set_whatsapp_token,
            clear_whatsapp_token,
            set_aisstream_api_key,
            clear_aisstream_api_key,
            test_whatsapp,
            get_radar_layer,
            open_external_url,
        ])
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("Tender's Log could not start");
    application.run(|app, event| {
        if matches!(event, RunEvent::ExitRequested { .. })
            && let Some(state) = app.try_state::<DesktopState>()
        {
            state.shutdown();
        }
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = event
        {
            show_main_window(app);
        }
    });
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
