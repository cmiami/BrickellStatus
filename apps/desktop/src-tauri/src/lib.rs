//! Native BrickellStatus companion: runtime, tray lifetime, delivery, and E213 I/O.

#[cfg(target_os = "android")]
mod android_bridge;
pub mod firmware;
mod secret_store;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    future::Future,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use brickellstatus_collectors::{
    CollectContext, Collector, CollectorItem, HttpFetcher, ItemKind, RainViewerCollector,
    SafeHttpFetcher,
};
use brickellstatus_delivery::{
    DeliveryAdapter, DeliveryFailureKind, DeliveryReason, DeliveryRequest, Destination,
    EnvironmentSecretResolver, EtaRange as DeliveryEtaRange, MessagingConsent, Notice, NoticeState,
    ReqwestExecutor, SecretValue, TokenSource, WhatsAppCloud, WhatsAppConfig,
};
use brickellstatus_projection::{
    bounded_text, bps_to_percent, channel_card, display_snapshot, interrupt_allows,
};
// Exercised only by the projection's tests, which live with the app because
// they assert against real snapshots the engine produces.
#[cfg(test)]
use brickellstatus_projection::{local_clock, span_code, upstream_spans};
// Re-exported because the live-frame binary drives the same path the app does.
use brickellstatus_eink::{
    DeviceBanner, FULL_REFRESH_CHURN, MonoFrame, PanelModel, RadarFigure, RefreshMode,
    RenderConfig, preview_png_bytes, radar_figure_from_png, render_channel_card,
    render_channel_card_with_radar, render_snapshot, series_figure,
    transport::{
        BleConfig, BleDeviceInfo, BleTransport, TransportError, TransportKind, TransportReceipt,
        UsbConfig, UsbTransport, discover_ble_devices, discover_espressif_devices,
        is_durable_ble_device_name,
    },
};
pub use brickellstatus_projection::render_live_bridge_frame;
use brickellstatus_runtime::{
    AUTOMATIC_FRAME_DWELL_SECONDS, AisConnectionStateDto, AppPreferences, AppSnapshot,
    AvailabilityDto, BridgeStateDto, ChannelKindDto, ChannelSnapshot,
    CredentialFreeCollectorFactory, DeliveryStateDto, DestinationIdDto, DispatchRecord,
    DisplayOrientation, DisplayTransport, LocationSearchResult, MutationResult, OutputStateDto,
    RuntimeConfig, RuntimeEngine, SchedulerHandle, UrgencyDto, VesselDetailDto,
    whatsapp_consent_is_current,
};
#[cfg(test)]
use brickellstatus_runtime::{InterruptPreset, SurfacePresence};
use brickellstatus_storage::{IncidentRecord, OutboxLease, OutboxRecord, Store};
use jiff::{Timestamp, tz::TimeZone};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
// `tauri::menu` is #[cfg(desktop)] and `tauri::tray` is
// #[cfg(all(desktop, feature = "tray-icon"))]; neither name exists on Android.
// `WindowEvent` does, but only the desktop close-to-tray handler names it.
#[cfg(desktop)]
use tauri::{
    WindowEvent,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{Mutex as AsyncMutex, Mutex as TokioMutex, RwLock};
use tracing::{debug, info, warn};
use url::Url;
use uuid::Uuid;

use secret_store::LocalSecretStore;

#[cfg(desktop)]
const MENU_STATUS_ID: &str = "e213-status";
#[cfg(desktop)]
const MENU_DETAIL_ID: &str = "e213-detail";
#[cfg(desktop)]
const MENU_OPEN_ID: &str = "open-main";
#[cfg(desktop)]
const MENU_QUIT_ID: &str = "quit";
#[cfg(desktop)]
const TRAY_ID: &str = "brickellstatus-tray";
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
    /// Exact slide identity after the panel acknowledges a frame. The channel
    /// alone is insufficient because one channel may carry several notices.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_notice_key: Option<String>,
    /// Which panel the connected board reports carrying. Absent until a board
    /// has said, because this is a fact read off the device and never a
    /// setting: the interface names what was detected, or names nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel: Option<PanelModel>,
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
            active_channel_id: None,
            active_notice_key: None,
            panel: None,
        }
    }
}

impl DisplayConnectionStatus {
    // Only a tray reads this, and mobile has none; the tests still do.
    #[cfg_attr(mobile, allow(dead_code))]
    fn menu_lines(&self) -> (String, String) {
        let first = match (self.state, self.transport) {
            (DisplayConnectionState::Connected, Some(DisplayConnectionTransport::Usb)) => {
                "USB active"
            }
            (DisplayConnectionState::Connected, Some(DisplayConnectionTransport::Ble)) => {
                "BLE connected"
            }
            (DisplayConnectionState::Connecting, Some(DisplayConnectionTransport::Usb)) => {
                "USB connecting"
            }
            (DisplayConnectionState::Connecting, Some(DisplayConnectionTransport::Ble)) => {
                "BLE connecting"
            }
            (DisplayConnectionState::Connecting, None) => "scanning",
            (DisplayConnectionState::Disconnected, _) => "disconnected",
            (DisplayConnectionState::Unavailable, Some(DisplayConnectionTransport::Usb)) => {
                "USB unavailable"
            }
            (DisplayConnectionState::Unavailable, Some(DisplayConnectionTransport::Ble)) => {
                "BLE unavailable"
            }
            (DisplayConnectionState::Unavailable, None) => "unavailable",
            (DisplayConnectionState::Error, Some(DisplayConnectionTransport::Usb)) => "USB error",
            (DisplayConnectionState::Error, Some(DisplayConnectionTransport::Ble)) => "BLE error",
            (DisplayConnectionState::Error, None) => "transport error",
            (DisplayConnectionState::Connected, None) => "connected",
        };
        // The board names itself, so the menu bar names the board that is there
        // rather than the one this project happened to start with. Before any
        // board has spoken there is nothing to name, and the line simply says
        // what the panel route is doing.
        let subject = self.panel.map_or("Panel", PanelModel::label);
        (
            format!("{subject} · {first}"),
            clean_menu_text(&self.detail),
        )
    }

    // Only a tray reads this, and mobile has none; the tests still do.
    #[cfg_attr(mobile, allow(dead_code))]
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

/// The display connection status the whole app reads: the display worker
/// writes it, two commands read it, and the frontend gets it as an event.
///
/// This used to live inside the tray state, which made the status cache a
/// casualty of not having a tray. It is managed on every platform now, and the
/// tray -- where there is one -- mirrors it.
struct DisplayStatusState {
    current: StdMutex<DisplayConnectionStatus>,
}

/// The two disabled menu items whose text mirrors [`DisplayStatusState`].
#[cfg(desktop)]
struct E213TrayState {
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
        /// Full READY line read from TX while this exact GATT session opened.
        banner: Option<String>,
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
                brickellstatus_eink::transport::send_frame(transport.as_ref(), frame, refresh).await
            }
            Self::Ble { transport, .. } => {
                brickellstatus_eink::transport::send_frame(transport.as_ref(), frame, refresh).await
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

    async fn read_banner(&self) -> Result<Option<String>, String> {
        match self {
            Self::Usb { transport, .. } => transport
                .read_banner()
                .await
                .map_err(|error| error.to_string()),
            Self::Ble { transport, .. } => transport
                .read_banner()
                .await
                .map_err(|error| error.to_string()),
        }
    }
}

/// How many refused frames in a row it takes before the app is willing to say
/// the board is not running this firmware. One is a dropped frame; two in a row
/// on a route that has never been acknowledged is a board that cannot take one.
const UNANSWERED_FRAMES_BEFORE_BLAME: u32 = 2;

/// How long the kernel is given to actually free the serial descriptor after
/// the display connection drops. espflash sees a stale one and reports the port
/// busy, which reads to the operator as a flash that failed for no reason.
const PORT_HANDOFF_DELAY: Duration = Duration::from_millis(600);

/// How long a freshly flashed board is given to come back on the USB bus before
/// the app tries to talk to it. espflash finishes with a hard reset, so the
/// device disappears and re-enumerates; opening into that gap fails for a reason
/// that has nothing to do with the board.
const BOOT_SETTLE_AFTER_FLASH: Duration = Duration::from_millis(1_800);
/// Maximum time for the native USB interface to return after espflash resets it.
const FIRMWARE_REENUMERATION_TIMEOUT: Duration = Duration::from_secs(6);
/// A full e-paper initialization happens before the decoder loop starts. Give
/// that bounded work enough time to finish and answer the repeatable `?` query.
const FIRMWARE_READY_TIMEOUT: Duration = Duration::from_secs(20);
/// A status read does not redraw the glass. Five minutes catches a weak battery
/// promptly without turning the panel connection into constant radio traffic.
const PANEL_BATTERY_POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

struct DisplayController {
    operation: AsyncMutex<()>,
    active: RwLock<Option<ActiveDisplay>>,
    last_frame: AsyncMutex<Option<Vec<u8>>>,
    frames_sent: AtomicU64,
    rotation_index: AtomicU64,
    automatic_reconnect: std::sync::atomic::AtomicBool,
    delivery_armed: std::sync::atomic::AtomicBool,
    /// Suppresses repeated native alerts while the same low-voltage condition
    /// is present. A valid non-low reading clears it for the next transition;
    /// legacy firmware with no battery reading leaves it unchanged.
    battery_low_notified: std::sync::atomic::AtomicBool,
    /// Frames sent to a connected board that went unacknowledged in a row.
    ///
    /// A board that will not take a frame is the only reliable sign of a board
    /// that is not running this firmware — the banner that would say so is
    /// spoken at boot and nowhere else. Counted, not latched, so one dropped
    /// frame does not condemn a working board.
    unanswered_frames: AtomicU32,
    /// Which panel the attached board reports carrying, which is what every
    /// frame is drawn for. A fact learned from the device, never a setting.
    attached_panel: std::sync::RwLock<PanelModel>,
    preferred_usb_port: AsyncMutex<Option<String>>,
    preferred_ble_id: AsyncMutex<Option<String>>,
}

/// BLE discovery that refuses to touch the platform adapter until it exists.
///
/// On Android the adapter only exists once the JNI bridge has run; reaching
/// for it early panics inside btleplug rather than returning. Everywhere else
/// this is a plain forward.
async fn discover_ble(config: &BleConfig) -> Result<Vec<BleDeviceInfo>, TransportError> {
    #[cfg(target_os = "android")]
    if !android_bridge::bluetooth_ready() {
        return Err(TransportError::NoBleAdapter);
    }
    discover_ble_devices(config).await
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
        let saved_ble = matches!(
            preferences.display.transport,
            DisplayTransport::Ble | DisplayTransport::Auto
        ) && is_durable_ble_device_name(&preferences.display.ble_name);
        Self {
            operation: AsyncMutex::new(()),
            active: RwLock::new(None),
            last_frame: AsyncMutex::new(None),
            frames_sent: AtomicU64::new(0),
            rotation_index: AtomicU64::new(0),
            // A remembered route is always one the user selected earlier.
            // USB keeps its exact port; BLE keeps the board's unique advertised
            // name, which survives host Bluetooth cache churn without storing
            // a platform hardware identifier in ordinary preferences.
            automatic_reconnect: std::sync::atomic::AtomicBool::new(
                saved_usb.is_some() || saved_ble,
            ),
            delivery_armed: std::sync::atomic::AtomicBool::new(false),
            battery_low_notified: std::sync::atomic::AtomicBool::new(false),
            unanswered_frames: AtomicU32::new(0),
            attached_panel: std::sync::RwLock::new(PanelModel::default()),
            preferred_usb_port: AsyncMutex::new(saved_usb),
            preferred_ble_id: AsyncMutex::new(None),
        }
    }

    async fn has_active(&self) -> bool {
        self.active.read().await.is_some()
    }

    /// Reads current device status without sending a frame or refreshing the
    /// e-paper glass. This still works when an unchanged image is deduplicated.
    async fn read_banner(&self) -> Result<Option<DeviceBanner>, String> {
        let Some(active) = self.active.read().await.clone() else {
            return Ok(None);
        };
        active
            .read_banner()
            .await
            .map(|line| line.as_deref().map(DeviceBanner::parse))
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

    /// A frame came back with `ACK INK1`. Only this firmware sends that, so the
    /// route is proven and any earlier refusals are history.
    fn note_frame_acknowledged(&self) {
        self.delivery_armed.store(true, Ordering::Relaxed);
        self.unanswered_frames.store(0, Ordering::Relaxed);
    }

    /// A frame went unanswered. Counted rather than latched: one dropped frame
    /// is not a board running the wrong firmware.
    fn note_frame_unanswered(&self) {
        self.unanswered_frames.fetch_add(1, Ordering::Relaxed);
    }

    /// Records the firmware's measured low-battery state and returns whether
    /// this is a new low transition that deserves a native notification.
    fn note_battery_state(&self, low_battery: Option<bool>) -> bool {
        match low_battery {
            Some(true) => !self.battery_low_notified.swap(true, Ordering::Relaxed),
            Some(false) => {
                self.battery_low_notified.store(false, Ordering::Relaxed);
                false
            }
            None => false,
        }
    }

    /// What the live route says about the board, for the firmware decision.
    ///
    /// An acknowledged frame is `ACK INK1`, which only this firmware sends, so
    /// it settles the question outright. Repeated refusals settle it the other
    /// way. Anything less is not evidence, and the firmware prompt must not
    /// invent any: reading "has not spoken" as "is not running our firmware" is
    /// what asked someone to reflash a board they had just flashed.
    async fn usb_route_evidence(&self) -> firmware::RouteEvidence {
        if !matches!(
            self.active.read().await.as_ref(),
            Some(ActiveDisplay::Usb { .. })
        ) {
            return firmware::RouteEvidence::Absent;
        }
        // Refusals are read before the acknowledgement, not after it. A board
        // that answered once and then stopped taking frames is a board that has
        // died since, and reading the old acknowledgement first would make that
        // unsayable for the rest of the session — the route would report itself
        // healthy while the panel sat frozen.
        if self.unanswered_frames.load(Ordering::Relaxed) >= UNANSWERED_FRAMES_BEFORE_BLAME {
            firmware::RouteEvidence::Failing
        } else if self.delivery_armed() {
            firmware::RouteEvidence::Acknowledged
        } else {
            firmware::RouteEvidence::Pending
        }
    }

    async fn scan(
        &self,
        preferences: &AppPreferences,
    ) -> (Vec<DisplayDeviceCandidate>, Vec<String>) {
        let _operation = self.operation.lock().await;
        let ble_config = BleConfig {
            device_name: if is_durable_ble_device_name(&preferences.display.ble_name) {
                preferences.display.ble_name.clone()
            } else {
                String::new()
            },
            ..BleConfig::default()
        };
        let (usb, ble) = tokio::join!(discover_espressif_devices(), discover_ble(&ble_config));
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
                // A platform ID is exact for this app session. Arm it before
                // the OS connection attempt so one transient failure starts the
                // reconnect ladder without requiring an app restart.
                self.arm_selected_ble_route(id).await;
                let configured_name = if is_durable_ble_device_name(&preferences.display.ble_name) {
                    preferences.display.ble_name.as_str()
                } else {
                    ""
                };
                self.connect_ble(Some(id.to_owned()), configured_name)
                    .await?
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
        let status = connected_status(&active, Some(self.panel()), None, None, detail);
        *self.active.write().await = Some(active);
        self.automatic_reconnect.store(true, Ordering::Relaxed);
        self.delivery_armed.store(false, Ordering::Relaxed);
        self.unanswered_frames.store(0, Ordering::Relaxed);
        *self.last_frame.lock().await = None;
        Ok(status)
    }

    async fn arm_selected_ble_route(&self, id: &str) {
        *self.preferred_ble_id.lock().await = Some(id.to_owned());
        *self.preferred_usb_port.lock().await = None;
        self.automatic_reconnect.store(true, Ordering::Relaxed);
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
            Some(self.panel()),
            None,
            None,
            "Connection restored; waiting to send the current rotation frame.",
        );
        *self.active.write().await = Some(active);
        Ok(status)
    }

    /// Runs a flash with the serial port released, and hands the port back
    /// afterwards no matter how the write ended.
    ///
    /// Releasing and restoring are one operation, expressed as one, because the
    /// reported bug was the seam between them: the release was unconditional and
    /// the restore was not, so flashing a board that nothing was connected to
    /// parked automatic reconnect for the rest of the session. There is now no
    /// call site that can release the port and forget to give it back.
    async fn holding_the_port_for_flash<T>(
        &self,
        preferences: &AppPreferences,
        write: impl Future<Output = T>,
    ) -> T {
        let was_connected = self.release_port_for_flash().await;
        debug!(was_connected, "released the display port to flash");
        // The kernel does not always free the descriptor the instant the handle
        // drops; espflash sees the stale one and reports it busy.
        tokio::time::sleep(PORT_HANDOFF_DELAY).await;
        let outcome = write.await;
        self.restore_port_after_flash(preferences).await;
        outcome
    }

    /// Drops the connection and parks automatic reconnect for the write.
    ///
    /// Private to the bracket above: on its own this leaves the display parked,
    /// which is only ever correct for the length of a flash.
    ///
    /// Flashing drives the same USB CDC device the display transport holds
    /// open, and macOS hands that out exclusively: leaving the connection up
    /// makes espflash fail immediately with "Device or resource busy".
    /// Automatic reconnect is suppressed for the same reason — a background
    /// reconnect that wins the port mid-write would leave a half-written
    /// bootloader, which is the one outcome that actually bricks the board.
    ///
    /// Returns whether a display was connected, for the log line.
    async fn release_port_for_flash(&self) -> bool {
        let was_connected = self.active.read().await.is_some();
        let _ = self.disconnect(true).await;
        was_connected
    }

    /// Restores the display connection after flashing.
    ///
    /// Called whether or not a display was connected when the flash started,
    /// and this is the whole point: releasing the port parks automatic
    /// reconnect, and a flash offered *because* nothing was talking to the
    /// board is exactly the case where nothing was connected to restore. Leaving
    /// the park in place then stranded the freshly flashed board on its boot
    /// screen for the rest of the session — the app had written the firmware
    /// and then refused to talk to it until it was relaunched.
    async fn restore_port_after_flash(&self, preferences: &AppPreferences) {
        self.automatic_reconnect.store(true, Ordering::Relaxed);
        // espflash leaves the board in a hard reset, so it is re-enumerating
        // over USB for a moment. Reconnecting into that gap fails for no reason
        // worth reporting; waiting also puts the open near the board's boot,
        // which is the only moment the READY banner can still be heard.
        tokio::time::sleep(BOOT_SETTLE_AFTER_FLASH).await;
        if let Err(error) = self.reconnect_preferred(preferences).await {
            // Not fatal: automatic reconnect is on again, so the display worker
            // retries on its own ladder. Worth recording, because a board that
            // never comes back after a flash is a genuine failure.
            warn!(%error, "display did not reconnect immediately after flashing");
        }
    }

    /// Whether a connected USB display already announced itself.
    ///
    /// Reusing the live connection's observation avoids opening the port a
    /// second time just to ask a question it already answered, which would
    /// contend with the transport on every status poll.
    /// The banner the connected board sent, if it is on USB and spoke one.
    ///
    /// Read back from the transport rather than copied into this enum: the
    /// transport already retains it, and a second copy is a second thing that
    /// can go stale. `ensure_connected` returns the cached state for an already
    /// open port, so this never reopens anything.
    async fn usb_banner(&self) -> Option<String> {
        let transport = match self.active.read().await.as_ref() {
            Some(ActiveDisplay::Usb { transport, .. }) => Arc::clone(transport),
            _ => return None,
        };
        transport.ensure_connected().await.ok()?.banner
    }

    /// The READY banner cached when the active BLE session was verified.
    ///
    /// This is application memory only: status probes may inspect firmware
    /// identity without issuing another GATT read or disturbing the live link.
    async fn ble_banner(&self) -> Option<String> {
        match self.active.read().await.as_ref() {
            Some(ActiveDisplay::Ble { banner, .. }) => banner.clone(),
            _ => None,
        }
    }

    async fn usb_ready_observed(&self) -> Option<bool> {
        match self.active.read().await.as_ref() {
            Some(ActiveDisplay::Usb { ready_observed, .. }) => Some(*ready_observed),
            _ => None,
        }
    }

    /// The panel every frame is drawn for.
    ///
    /// Taken from what the connected board said about itself, which is the only
    /// party that knows. A board that has not said — an older firmware, a BLE
    /// route that has not read its banner yet, nothing connected at all — leaves
    /// the last one that did in place, and the original panel is the answer
    /// before any board has ever spoken. Nothing here reads a preference,
    /// because a preference is a person being asked to identify hardware they
    /// are holding.
    fn panel(&self) -> PanelModel {
        *self
            .attached_panel
            .read()
            .expect("panel lock is not poisoned")
    }

    /// Records what a board reported, so the next frame is drawn for it.
    fn observe_panel(&self, panel: Option<PanelModel>) {
        let Some(panel) = panel else { return };
        let mut attached = self
            .attached_panel
            .write()
            .expect("panel lock is not poisoned");
        if *attached != panel {
            tracing::info!(panel = panel.label(), "attached panel changed");
            *attached = panel;
        }
    }

    async fn disconnect_locked(&self) -> Result<(), String> {
        let active = self.active.write().await.take();
        // Delivery history belongs to a connection: the next one may not even
        // be the same board, so it starts with nothing held against it.
        self.unanswered_frames.store(0, Ordering::Relaxed);
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
        // The board names its panel in the same breath as it says it is ready,
        // so the route knows what it is drawing for before it draws anything.
        let banner = connected.banner.as_deref().map(DeviceBanner::parse);
        self.observe_panel(banner.as_ref().and_then(|banner| banner.panel));
        let name = match banner.as_ref().and_then(|banner| banner.board) {
            Some(panel) => format!("{} on {}", panel.label(), connected.port),
            None if connected.ready_observed => format!("Panel on {}", connected.port),
            None => format!("USB display on {}", connected.port),
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
        #[cfg(target_os = "android")]
        if !android_bridge::bluetooth_ready() {
            return Err(TransportError::NoBleAdapter.to_string());
        }
        let transport = Arc::new(BleTransport::new(BleConfig {
            device_name: configured_name.to_owned(),
            device_id,
            ..BleConfig::default()
        }));
        let connected = transport
            .ensure_connected()
            .await
            .map_err(|error| error.to_string())?;
        // Over Bluetooth the banner is held on the TX characteristic rather
        // than spoken once at boot, so the panel is known from the moment the
        // connection opens.
        self.observe_panel(
            connected
                .banner
                .as_deref()
                .map(DeviceBanner::parse)
                .and_then(|banner| banner.panel),
        );
        Ok(ActiveDisplay::Ble {
            name: connected.name,
            transport,
            banner: connected.banner,
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
                        "No USB panel has been selected. Scan and choose the panel before any bytes are written."
                            .to_owned()
                    })?;
                self.connect_usb(port).await
            }
            DisplayTransport::Ble => {
                let saved_name = if is_durable_ble_device_name(&preferences.display.ble_name) {
                    preferences.display.ble_name.as_str()
                } else {
                    ""
                };
                if selected_ble.is_none() && saved_name.is_empty() {
                    return Err(
                        "No Bluetooth panel has been selected. Scan and choose the panel before connecting."
                            .to_owned(),
                    );
                }
                self.connect_ble(selected_ble, saved_name).await
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
                let saved_name = if is_durable_ble_device_name(&preferences.display.ble_name) {
                    preferences.display.ble_name.as_str()
                } else {
                    ""
                };
                if selected_ble.is_none() && saved_name.is_empty() {
                    return Err(format!(
                        "Automatic reconnect is parked until a device is explicitly selected. USB: {usb_attempt}."
                    ));
                }
                self.connect_ble(selected_ble, saved_name)
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
        // Turned here, before the repeat check, so the check compares what will
        // actually go on the wire. Rotating afterwards would leave an
        // orientation change looking like the same frame, and the panel would
        // stay the old way up until something else happened to be drawn.
        let turned;
        let frame = if preferences.display.orientation == DisplayOrientation::Inverted {
            turned = frame.inverted();
            &turned
        } else {
            frame
        };
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
        // Two reasons to clear the glass, and the periodic one is the weaker.
        //
        // A fast refresh only settles the pixels it is told changed, so it is
        // right for a frame that mostly matches the one before it -- a figure
        // ticking over inside an otherwise identical card. Rotation does not do
        // that: every dwell swaps in a different channel's whole layout, and
        // eleven of those in a row between maintenance refreshes leaves the
        // previous slide legible underneath the current one.
        //
        // So the amount of glass actually changing decides it, and the counted
        // cadence stays as a floor for the case this cannot see: slow drift,
        // where each frame is a small change but the residue still accumulates.
        let churn = self.frame_churn(frame.packed()).await;
        let refresh = if next.is_multiple_of(cadence) || churn >= FULL_REFRESH_CHURN {
            RefreshMode::Full
        } else {
            RefreshMode::Fast
        };
        let receipt = match active.send(frame, refresh).await {
            Ok(receipt) => receipt,
            Err(error) => {
                self.note_frame_unanswered();
                self.delivery_armed.store(false, Ordering::Relaxed);
                // A failed BLE frame can leave CoreBluetooth holding a live
                // GATT link even though the UI correctly reports an error. A
                // connected peripheral does not advertise, so keeping that
                // stale route here makes the panel disappear from the app's
                // own scanner. Release it and let the ordinary reconnect
                // ladder establish a fresh session.
                let failed = self.active.write().await.take();
                if let Some(failed) = failed
                    && let Err(disconnect_error) = failed.disconnect().await
                {
                    warn!(%disconnect_error, "failed display route did not disconnect cleanly");
                }
                return Err(error);
            }
        };
        self.note_frame_acknowledged();
        self.frames_sent.fetch_add(1, Ordering::Relaxed);
        *self.last_frame.lock().await = Some(frame.packed().to_vec());
        Ok(Some((receipt, active.name().to_owned())))
    }

    /// The fraction of the panel this frame repaints, against the last one sent.
    ///
    /// Nothing to compare against means everything is changing: the first frame
    /// after a connect lands on glass holding whatever the last session left
    /// there. A length change means the panel itself changed, which is the same
    /// answer for the same reason.
    async fn frame_churn(&self, packed: &[u8]) -> f32 {
        let guard = self.last_frame.lock().await;
        let Some(last) = guard.as_deref() else {
            return 1.0;
        };
        if last.len() != packed.len() || packed.is_empty() {
            return 1.0;
        }
        let changed: u32 = last
            .iter()
            .zip(packed)
            .map(|(before, after)| (before ^ after).count_ones())
            .sum();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a ratio of pixel counts, where f32's 24-bit mantissa is far more precision than a threshold comparison needs"
        )]
        {
            changed as f32 / (packed.len() as f32 * 8.0)
        }
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
    *app.state::<DisplayStatusState>()
        .current
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = status.clone();
    #[cfg(desktop)]
    mirror_status_to_tray(app, &status);
    if let Err(error) = app.emit(STATUS_EVENT, status) {
        warn!(%error, "display status event emission failed");
    }
}

#[cfg(desktop)]
fn mirror_status_to_tray(app: &AppHandle, status: &DisplayConnectionStatus) {
    let state = app.state::<E213TrayState>();
    let (status_line, detail_line) = status.menu_lines();
    if let Err(error) = state.status_item.set_text(&status_line) {
        warn!(%error, "display tray status text update failed");
    }
    if let Err(error) = state.detail_item.set_text(&detail_line) {
        warn!(%error, "display tray detail text update failed");
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(error) = tray.set_tooltip(Some(format!(
            "BrickellStatus · {status_line} · {detail_line}"
        ))) {
            warn!(%error, "display tray tooltip update failed");
        }
        // macOS renders this compact monochrome status beside the template
        // icon. Other platforms retain the state in tooltip/menu text.
        if let Err(error) = tray.set_title(Some(status.tray_badge())) {
            warn!(%error, "display tray badge update failed");
        }
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
    /// Monotonic firmware release this app ships. Unlike `bundled_build`, this
    /// is orderable and is the only basis for an update/downgrade decision.
    pub bundled_version: Option<u32>,
    /// Which board is attached, as the board itself reported it.
    pub board: Option<PanelModel>,
    /// The build to write to it, worked out from that report and from whatever
    /// answered the one question hardware cannot. The interface flashes this;
    /// it does not offer a menu.
    pub recommended_variant_id: Option<String>,
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
    /// Board this build drives, so the interface can offer only the builds that
    /// could possibly apply to what is attached.
    pub panel: PanelModel,
    pub panel_revision: Option<firmware::PanelRevision>,
    pub total_bytes: usize,
}

fn firmware_root(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .resolve("firmware", tauri::path::BaseDirectory::Resource)
        .ok()
        // `resolve` builds a path without checking it exists, so a build that
        // bundles no firmware -- the Android one, and any dev run before
        // `firmware:bundle` -- would otherwise report a directory that is not
        // there and fail later with a confusing read error.
        .filter(|path| path.is_dir())
}

/// Reports whether an attached board needs the bundled firmware.
///
/// Which board it is comes from the board: its boot probe reports the panel it
/// found, and the build to write follows from that. E213's two internal
/// controller images remain an implementation detail. The flash transaction
/// requires a repeatable READY response, tries the alternate controller once
/// on silence, and records only the image that objectively answers.
#[tauri::command]
async fn get_firmware_status(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<FirmwareStatus, String> {
    let connected_ready = state.display.usb_ready_observed().await;
    let connected_ble_banner = state.display.ble_banner().await;
    let preferences = state.engine.get_preferences().await;
    let legacy_saved_ble_route = needs_panel_identity_migration(
        preferences.display.transport,
        &preferences.display.ble_name,
    );
    let devices = brickellstatus_eink::transport::discover_espressif_devices()
        .await
        .unwrap_or_default();
    let port = devices.first().map(|device| device.port.clone());
    let attached_serial = devices
        .first()
        .and_then(|device| device.serial_number.clone());

    let Some(root) = firmware_root(&app) else {
        return Ok(FirmwareStatus {
            port,
            bundled_build: None,
            bundled_version: None,
            board: None,
            recommended_variant_id: None,
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
                bundled_version: None,
                board: None,
                recommended_variant_id: None,
                variants: Vec::new(),
                requirement: firmware::FlashRequirement::NoDevice,
                unavailable: Some(error.to_string()),
            });
        }
    };

    // Enumeration alone cannot say whether the board is running our firmware,
    // only that something Espressif is attached. The board names itself in its
    // READY INK1 banner, but it speaks that line once, at boot, and opening the
    // port deliberately does not reset it — so hearing nothing is the ordinary
    // case for a board that has been running happily for a minute, and means
    // only that this app did not happen to be listening at the right moment.
    // Whether that silence is worth a prompt is not decided here; it is decided
    // against what the app remembers writing and what the display route is
    // actually managing to deliver.
    let probe = match port.as_deref() {
        None if connected_ble_banner.is_some() => firmware::DeviceProbe::Answered(
            firmware::DeviceBanner::parse(connected_ble_banner.as_deref().unwrap_or_default()),
        ),
        None => firmware::DeviceProbe::NoPort,
        // A connected display already answered this question when it connected,
        // so reuse that rather than opening the port a second time and
        // contending with the transport on every status poll.
        Some(_) if connected_ready.is_some() => {
            // The connected display already heard the banner; parse the build
            // out of it rather than reducing it to "something answered".
            match state.display.usb_banner().await {
                Some(banner) => {
                    firmware::DeviceProbe::Answered(firmware::DeviceBanner::parse(&banner))
                }
                None => firmware::DeviceProbe::Silent,
            }
        }
        Some(port) => {
            let transport = brickellstatus_eink::transport::UsbTransport::new(
                brickellstatus_eink::transport::UsbConfig {
                    port: Some(port.to_owned()),
                    ..Default::default()
                },
            );
            // An error here means the port could not be opened — usually
            // because the display worker is connecting to it at the same
            // moment. That says nothing about the firmware on the board, so it
            // must not be reported as a board that failed to answer.
            let probe = match transport.ensure_connected().await {
                Ok(info) => match info.banner.as_deref() {
                    Some(banner) => {
                        firmware::DeviceProbe::Answered(firmware::DeviceBanner::parse(banner))
                    }
                    None => firmware::DeviceProbe::Silent,
                },
                Err(error) => {
                    debug!(%error, "firmware probe could not open the port; not prompting");
                    firmware::DeviceProbe::Unreachable
                }
            };
            transport.disconnect().await;
            probe
        }
    };
    let remembered = state
        .store
        .get_json::<firmware::FlashRecord>(FLASH_RECORD_KEY)
        .await
        .ok()
        .flatten()
        // Only for the board actually in front of us. A record from a different
        // device says nothing about this one.
        .filter(|record| {
            attached_serial
                .as_deref()
                .is_some_and(|serial| serial == record.serial_number)
        });
    let mut requirement = firmware::evaluate_versioned_flash_requirement(
        &probe,
        bundle.firmware_version,
        bundle.source_revision.as_deref(),
        remembered.as_ref(),
        state.display.usb_route_evidence().await,
    );
    if matches!(
        requirement,
        firmware::FlashRequirement::NoDevice | firmware::FlashRequirement::UnknownBuild
    ) && legacy_saved_ble_route
    {
        requirement = firmware::FlashRequirement::Required {
            reason: firmware::FlashReason::LegacyConnection,
        };
    }

    // What the board said it is. A board that has not spoken this session is
    // not guessed at here; the last build written to it is the better answer,
    // and that is what the record holds.
    let reported = match &probe {
        firmware::DeviceProbe::Answered(banner) => banner.board,
        _ => None,
    };
    // Drawing follows the device, so a board that named itself has just told
    // the display route which panel to render for.
    if let firmware::DeviceProbe::Answered(banner) = &probe {
        state.display.observe_panel(banner.panel);
    }
    let remembered_revision = remembered
        .as_ref()
        .and_then(|record| bundle.variant(&record.variant_id))
        .and_then(|variant| variant.panel_revision);
    let board = reported.or_else(|| {
        remembered
            .as_ref()
            .and_then(|record| bundle.variant(&record.variant_id))
            .map(|variant| variant.panel)
    });
    let recommended_variant_id = board
        .and_then(|board| bundle.for_board(board, remembered_revision))
        // With no board known at all — a virgin board that has never spoken —
        // the first build in the bundle is the one to try, and the firmware
        // will say so if it lands on something else.
        .or_else(|| bundle.variants().first())
        .map(|variant| variant.id.clone());

    Ok(FirmwareStatus {
        port,
        bundled_build: bundle.source_revision.clone(),
        bundled_version: Some(bundle.firmware_version),
        board,
        recommended_variant_id,
        variants: bundle
            .variants()
            .iter()
            .map(|variant| FirmwareVariantSummary {
                id: variant.id.clone(),
                label: variant.label.clone(),
                panel: variant.panel,
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
    let initial_variant = bundle
        .variant(&variant_id)
        .ok_or_else(|| format!("unknown firmware variant {variant_id:?}"))?
        .clone();
    let bundled_build = bundle.source_revision.clone();
    let bundled_version = bundle.firmware_version;
    if let Some(banner) = panel_banner_before_flash(&state.display, &port).await {
        refuse_firmware_downgrade(&banner, bundled_version)?;
    }
    // Read the board's identity now, while it is still enumerated and holding
    // still. Afterwards it is in a hard reset and re-enumerating, and a lookup
    // that comes back empty in that gap loses the record — which is the record
    // that stops the next launch asking to flash the board it just flashed.
    let identity_before_write = attached_board_serial(&port).await;
    let trusted_rollback = match identity_before_write.as_deref() {
        Some(serial) => state
            .store
            .get_json::<firmware::FlashRecord>(FLASH_RECORD_KEY)
            .await
            .ok()
            .flatten()
            .and_then(|record| bundle.trusted_rollback(&record, serial).cloned()),
        None => None,
    };
    let port_for_status = port.clone();

    let preferences = state.engine.get_preferences().await;
    // Keep automatic reconnect parked for the complete write / verify /
    // recovery transaction. Restoring the display route between controller
    // images can seize the serial port before the READY query gets to ask what
    // actually booted.
    let (variant_id_for_record, verified_banner) = state
        .display
        .holding_the_port_for_flash(&preferences, async {
            // espflash drives the serial bootloader synchronously and a flash
            // takes tens of seconds, so each write runs off the async runtime.
            let write = async |variant: firmware::FirmwareVariant,
                               app: AppHandle,
                               port: String| {
                tauri::async_runtime::spawn_blocking(move || {
                    let mut progress = EmitProgress {
                        app,
                        total: variant.total_bytes(),
                        done: 0,
                        current: 0,
                    };
                    firmware::flash_variant(&port, &variant, &mut progress)
                })
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())
            };

            let mut attempted = BTreeSet::new();
            let mut current = initial_variant;
            loop {
                attempted.insert(current.id.clone());
                write(current.clone(), app.clone(), port.clone()).await?;
                let total = current.total_bytes();
                let _ = app.emit(
                    "firmware://progress",
                    serde_json::json!({
                        "stage": "checking",
                        "written": total,
                        "total": total,
                    }),
                );

                // The glass is deliberately ignored here. E-paper can retain
                // a readable image from a previous build while the image just
                // written is deadlocked on the opposite E213 BUSY polarity.
                // Only a repeatable READY response proves this build booted.
                let banner = banner_after_flash(&port, identity_before_write.as_deref()).await?;
                if let Some(answer) = banner.as_ref() {
                    match firmware::validate_current_banner(
                        answer,
                        &current,
                        bundled_version,
                        bundled_build.as_deref(),
                    ) {
                        Ok(None) => {
                            break Ok::<(String, DeviceBanner), String>((
                                current.id.clone(),
                                answer.clone(),
                            ));
                        }
                        Ok(Some(_reported_board)) => {
                            // The complete current banner explicitly requests
                            // the board redirect selected below.
                        }
                        Err(error) => {
                            break Err(format!(
                                "{} answered after flashing, but its identity was invalid: {error}. Controller recovery was not attempted.",
                                current.label
                            ));
                        }
                    }
                }

                let Some(recovery) = bundle
                    .recovery_after_boot(&current, banner.as_ref(), &attempted)
                    .cloned()
                else {
                    let observation = match banner.as_ref() {
                        Some(banner) if banner.mismatch => format!(
                            "it reported a {} board mismatch",
                            banner
                                .board
                                .map(|board| board.label())
                                .unwrap_or("panel")
                        ),
                        _ => "it never reported READY INK1".to_owned(),
                    };
                    let failure = format!(
                        "{} was written, but {observation}. No untried recovery image remains, so no working firmware was recorded.",
                        current.label
                    );
                    // Only an earlier objective READY may nominate rollback.
                    // Legacy records from the visual-confirm flow deserialize
                    // unverified, including the currently stranded v1.1
                    // record, and must never overwrite the last candidate.
                    if let Some(rollback) = trusted_rollback.as_ref() {
                        if rollback.id == current.id {
                            break Err(format!(
                                "{failure} The previously verified {} image remains installed.",
                                rollback.label
                            ));
                        }
                        tokio::time::sleep(PORT_HANDOFF_DELAY).await;
                        match write(rollback.clone(), app.clone(), port.clone()).await {
                            Ok(()) => {
                                break Err(format!(
                                    "{failure} Restored the previously verified {} image without recording this attempt.",
                                    rollback.label
                                ));
                            }
                            Err(rollback_error) => {
                                break Err(format!(
                                    "{failure} Restoring the previously verified {} image also failed: {rollback_error}",
                                    rollback.label
                                ));
                            }
                        }
                    }
                    break Err(format!(
                        "{failure} No previously verified rollback image exists; the last candidate remains installed."
                    ));
                };

                if let Some(board) = banner.as_ref().and_then(|banner| banner.board) {
                    info!(
                        wrote = %current.id,
                        board = %board.label(),
                        correcting_to = %recovery.id,
                        "the board rejected the written panel image; flashing the image it named",
                    );
                } else {
                    warn!(
                        wrote = %current.id,
                        recovering_to = %recovery.id,
                        "E213 image never answered READY; trying the other controller image once",
                    );
                }
                // `banner_after_flash` closed its verification handle, but
                // macOS may retain the descriptor briefly. Give the kernel the
                // same handoff interval used before the first espflash write.
                tokio::time::sleep(PORT_HANDOFF_DELAY).await;
                current = recovery;
            }
        })
        .await?;

    // A verified flash also gives us the exact post-rename BLE identity. Keep
    // that device-derived name when Bluetooth participates in the selected
    // route; otherwise a board recovered over USB comes back advertising
    // `BrickellStatus 26B4` while automatic reconnect keeps searching for the
    // stale legacy `InkDock E213` name.
    let mut preferences = state.engine.get_preferences().await;
    let mut preferences_changed = false;
    if matches!(
        preferences.display.transport,
        DisplayTransport::Ble | DisplayTransport::Auto
    ) && let Some(ble_name) = verified_ble_name(&verified_banner)
        && preferences.display.ble_name != ble_name
    {
        preferences.display.ble_name = ble_name;
        preferences_changed = true;
    }

    // A board that was just flashed through this app is a board the reader
    // wants driven. Fresh installs default to the preview transport so nothing
    // reaches for a serial port unasked; a deliberate flash settles that.
    if preferences.display.transport == DisplayTransport::Preview {
        preferences.display.transport = DisplayTransport::Usb;
        preferences.display.serial_port = port_for_status.clone();
        preferences_changed = true;
    }
    if preferences_changed {
        let saved_ble_name = preferences.display.ble_name.clone();
        let saved_transport = preferences.display.transport;
        if let Err(error) = state.engine.save_preferences(preferences).await {
            warn!(%error, "could not save the verified panel route");
        } else if matches!(
            saved_transport,
            DisplayTransport::Ble | DisplayTransport::Auto
        ) {
            info!(ble_name = %saved_ble_name, "saved the verified panel BLE identity");
        } else {
            info!(port = %port_for_status, "driving the panel that was just flashed");
        }
    }

    // Remember what went onto which board. The banner is only spoken at boot
    // and the port is usually held by the display worker, so a device that is
    // running exactly this build routinely cannot say so — and was being
    // prompted to flash again on every launch as a result.
    //
    // Asked again only if the first reading came back empty: by now the board
    // is back on the bus, so a lookup that landed in the enumeration gap still
    // has a second chance rather than costing the record entirely.
    let board_serial = board_identity_for_record(identity_before_write, || {
        attached_board_serial(&port_for_status)
    })
    .await;
    match board_serial {
        Some(serial) => {
            let record = firmware::FlashRecord {
                serial_number: serial,
                build: bundled_build.clone().unwrap_or_default(),
                firmware_version: Some(bundled_version),
                variant_id: variant_id_for_record,
                verified: true,
                flashed_at: Timestamp::now().to_string(),
            };
            if let Err(error) = state
                .store
                .set_json(FLASH_RECORD_KEY, &record, &Timestamp::now().to_string())
                .await
            {
                warn!(%error, "could not record the firmware that was just written");
            }
        }
        // Without an identity for the board there is nothing to key the record
        // to, and the next launch has only the board's own silence to go on.
        None => warn!(
            port = %port_for_status,
            "flashed a board that reports no USB serial number; it cannot be remembered"
        ),
    }
    Ok(())
}

/// Reads the device identity before any destructive write. Prefer the active
/// USB session's cached banner; otherwise ask the exact selected port once.
async fn panel_banner_before_flash(
    display: &DisplayController,
    port: &str,
) -> Option<DeviceBanner> {
    if let Some(line) = display.usb_banner().await {
        return Some(DeviceBanner::parse(&line));
    }
    let transport = UsbTransport::new(UsbConfig {
        port: Some(port.to_owned()),
        ..Default::default()
    });
    let banner = transport
        .ensure_connected()
        .await
        .ok()
        .and_then(|info| info.banner)
        .map(|line| DeviceBanner::parse(&line));
    transport.disconnect().await;
    banner
}

fn refuse_firmware_downgrade(banner: &DeviceBanner, bundled_version: u32) -> Result<(), String> {
    if let Some(device) = banner
        .firmware_version
        .filter(|device| *device > bundled_version)
    {
        return Err(format!(
            "This panel runs firmware version {device}, newer than version {bundled_version} bundled with this app. Update BrickellStatus; no firmware was written."
        ));
    }
    Ok(())
}

/// The port of an attached board that answered with our firmware's banner.
///
/// `None` covers every reason not to touch it: no board, a board that does not
/// speak, or a port somebody else already holds. Each of those is a reason to
/// leave the reader's transport choice alone rather than guess.
async fn adoptable_panel_port() -> Option<String> {
    let port = brickellstatus_eink::transport::discover_espressif_port()
        .await
        .ok()
        .flatten()?;
    let banner = banner_after_flash(&port, None).await.ok()??;
    banner.saw_ready.then_some(port)
}

/// Reads the banner a freshly flashed board speaks at boot.
///
/// The native USB device disappears while espflash resets it, so discovery is
/// retried for a bounded interval. Once open, `UsbTransport` sends the
/// non-destructive `?` identity query; verification therefore does not depend
/// on catching a one-time boot line. `None` means the image never answered.
async fn banner_after_flash(
    port: &str,
    expected_serial: Option<&str>,
) -> Result<Option<DeviceBanner>, String> {
    let deadline = tokio::time::Instant::now() + FIRMWARE_REENUMERATION_TIMEOUT;
    let attached = loop {
        match discover_espressif_devices().await {
            Ok(devices) => {
                if let Some(device) = devices.into_iter().find(|device| device.port == port) {
                    break device;
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(format!(
                        "the flashed panel did not return at {port}; controller recovery was not attempted"
                    ));
                }
            }
            Err(error) if tokio::time::Instant::now() >= deadline => {
                return Err(format!(
                    "could not verify the flashed panel on USB: {error}"
                ));
            }
            Err(error) => {
                debug!(%error, %port, "waiting for USB panel discovery after flash");
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };

    if !flash_serial_matches(expected_serial, attached.serial_number.as_deref()) {
        return Err(format!(
            "the device now at {port} is not the panel that started this flash; controller recovery was not attempted"
        ));
    }

    let transport = brickellstatus_eink::transport::UsbTransport::new(
        brickellstatus_eink::transport::UsbConfig {
            port: Some(port.to_owned()),
            ready_timeout: FIRMWARE_READY_TIMEOUT,
            ..Default::default()
        },
    );
    let info = transport
        .ensure_connected()
        .await
        .map_err(|error| format!("could not query the flashed panel identity: {error}"))?;
    let banner = info.banner.map(|line| DeviceBanner::parse(&line));
    transport.disconnect().await;
    Ok(banner)
}

/// Recovery can continue only while the exact physical USB board remains.
fn flash_serial_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    expected.is_none() || expected == actual
}

/// A pre-board-id BLE preference cannot safely identify one panel after an app
/// restart. Keep it as migration evidence only; never auto-attach by the old
/// generic name or quote that stale product label back to the reader.
fn needs_panel_identity_migration(transport: DisplayTransport, ble_name: &str) -> bool {
    matches!(transport, DisplayTransport::Ble | DisplayTransport::Auto)
        && !ble_name.trim().is_empty()
        && !is_durable_ble_device_name(ble_name)
}

/// Exact BLE identity learned from firmware that objectively answered READY.
///
/// Kept defensive even though the banner parser validates current ids: this is
/// the boundary that turns device text into a persisted reconnect key, so a
/// future parser relaxation must not save an arbitrary token as a panel name.
fn verified_ble_name(banner: &DeviceBanner) -> Option<String> {
    if !banner.saw_ready || banner.mismatch {
        return None;
    }
    let board_id = banner.board_id.as_deref()?;
    let name = format!("BrickellStatus {}", board_id.to_ascii_uppercase());
    is_durable_ble_device_name(&name).then_some(name)
}

/// Key for the last-flashed record in the settings table.
const FLASH_RECORD_KEY: &str = "firmware.last_flash";

/// Which reading of the board's identity the flash record is keyed to.
///
/// The reading taken before the write wins whenever there is one. A flash ends
/// in a hard reset, so the lookup afterwards is asking about a port that was
/// vacant a moment ago and may not be the same board when it fills again —
/// recording that answer would attach this build to somebody else's hardware.
/// The later reading is a fallback for the case that would otherwise be lost
/// entirely: no identity at all, and a board that gets asked to flash again on
/// the next launch because nothing remembers it.
async fn board_identity_for_record<F, Fut>(
    before_write: Option<String>,
    after_write: F,
) -> Option<String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<String>>,
{
    match before_write {
        Some(serial) => Some(serial),
        None => after_write().await,
    }
}

/// The USB serial number of the board at `port`, when it reports one.
async fn attached_board_serial(port: &str) -> Option<String> {
    brickellstatus_eink::transport::discover_espressif_devices()
        .await
        .ok()?
        .into_iter()
        .find(|device| device.port == port)
        .and_then(|device| device.serial_number)
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

/// What this build can physically do, so the console stops offering hardware
/// paths the platform does not have.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformCapabilities {
    /// Whether a USB serial panel connection can be opened at all.
    usb_display: bool,
    /// Whether this build ships firmware it could write to a board.
    firmware_flashing: bool,
}

#[tauri::command]
fn get_platform_capabilities(app: AppHandle) -> PlatformCapabilities {
    // Neither phone platform can drive a panel over the cable, for different
    // reasons, and both end in the same place: BLE is the only transport, and
    // flashing is impossible because it needs a serial bootloader.
    //
    // Android links the serial stack but cannot open a port from an
    // unprivileged app: available_ports() answers "Not implemented for this
    // OS". iOS does not link it at all -- espflash and serialport are excluded
    // from the build for both phone targets in Cargo.toml -- because iOS
    // exposes no USB serial interface to a sandboxed app in the first place.
    //
    // Written as `mobile` rather than as a list of triples so a future phone
    // target inherits the honest answer instead of claiming a cable it has no
    // way to open. Reporting this up front is kinder than a scan that always
    // finds nothing, and flashing needs both a port and a bundled image.
    let usb_display = cfg!(not(mobile));
    PlatformCapabilities {
        usb_display,
        firmware_flashing: usb_display && firmware_root(&app).is_some(),
    }
}

#[tauri::command]
fn get_display_status(state: State<'_, DisplayStatusState>) -> DisplayConnectionStatus {
    state
        .current
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tauri::command]
async fn get_app_snapshot(
    state: State<'_, DesktopState>,
    display: State<'_, DisplayStatusState>,
) -> Result<AppSnapshot, String> {
    let mut snapshot = state
        .engine
        .get_snapshot()
        .await
        .map_err(|error| error.to_string())?;
    let preferences = state.engine.get_preferences().await;
    let display_status = display
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

fn validated_mmsi_argument(mmsi: &str) -> Result<&str, String> {
    (mmsi.len() == 9 && mmsi.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(mmsi)
        .ok_or_else(|| "MMSI must contain exactly 9 digits.".to_owned())
}

#[tauri::command]
async fn get_vessel_detail(
    state: State<'_, DesktopState>,
    mmsi: String,
) -> Result<Option<VesselDetailDto>, String> {
    let mmsi = validated_mmsi_argument(&mmsi)?;
    state
        .engine
        .get_vessel_detail(mmsi)
        .await
        .map_err(|error| error.to_string())
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
                active_channel_id: None,
                active_notice_key: None,
                panel: None,
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
                    "No powered e-paper panel found. E-ink can keep this screen visible after the battery dies; connect USB power, then look again."
                        .into()
                } else {
                    format!("No compatible display found. {}", errors.join(" "))
                },
                last_frame_at: previous.last_frame_at,
                last_ack_at: previous.last_ack_at,
                active_channel_id: None,
                active_notice_key: None,
                panel: None,
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
                active_channel_id: None,
                active_notice_key: None,
                panel: None,
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
            active_channel_id: None,
            active_notice_key: None,
            panel: None,
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
    // The preview is a picture of what the board is showing, so it is drawn on
    // the board's own panel rather than on a nominal one.
    let panel = state.display.panel();
    let frame = if channel.kind == ChannelKindDto::Bridge {
        render_snapshot(
            &display_snapshot(&snapshot),
            &RenderConfig::default().for_panel(panel),
        )
        .map_err(|error| format!("Bridge preview render failed: {error}"))?
    } else {
        render_channel_card(&channel_card(channel, &preferences, &snapshot), panel)
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
    // For the attached panel, not the default one. Geometry is baked into the
    // packet length, so rendering an E213 frame for an E290 sends 3,922 bytes
    // to a board waiting for 4,754: it never completes the packet, times out a
    // second later and answers NACK TRUNCATED. The rotation path already asks
    // the controller which panel is attached; this one did not, so the test
    // frame -- the very thing offered as proof the link works -- was the one
    // frame guaranteed to fail on an E290.
    let frame = match render_snapshot(
        &presentation,
        &RenderConfig::default().for_panel(display.panel()),
    ) {
        Ok(frame) => frame,
        Err(error) => return mutation_error(format!("Frame render failed: {error}")),
    };
    send_rendered_frame_to_display(app, display, &frame, preferences, force, None).await
}

async fn send_rotating_snapshot_to_display(
    app: &AppHandle,
    display: &DisplayController,
    snapshot: &AppSnapshot,
    preferences: &AppPreferences,
    selection: &PanelSelection,
    radar: Option<&RadarFigure>,
) -> (MutationResult, Option<String>) {
    let (channel_id, notice_key) = match selection {
        PanelSelection::Alert {
            channel_id,
            notice_key,
            ..
        }
        | PanelSelection::Rotation {
            channel_id,
            notice_key,
        } => (channel_id, notice_key.as_deref()),
    };
    let Some(base_channel) = snapshot
        .channels
        .iter()
        .find(|channel| &channel.id == channel_id)
    else {
        return (
            mutation_error("The selected e-paper channel is no longer present."),
            None,
        );
    };
    // A channel is a subscription; each current item is its own slide. Clone
    // the compatibility view for projection so existing renderers can draw the
    // selected notice without learning carousel state.
    let mut selected_channel = base_channel.clone();
    if let Some(notice_key) = notice_key
        && let Some(notice) = base_channel
            .notices
            .iter()
            .find(|notice| notice.key == notice_key)
    {
        selected_channel.signal = Some(notice.signal.clone());
        selected_channel.priority = notice.priority;
        selected_channel.material_key.clone_from(&notice.key);
    }
    let channel = &selected_channel;
    let panel = display.panel();
    let frame = if channel.kind == ChannelKindDto::Bridge {
        match render_snapshot(
            &display_snapshot(snapshot),
            &RenderConfig::default().for_panel(panel),
        ) {
            Ok(frame) => frame,
            Err(error) => {
                return (
                    mutation_error(format!("Bridge frame render failed: {error}")),
                    Some(channel.id.clone()),
                );
            }
        }
    } else {
        // One box, two things that can occupy it: radar corroborates a weather
        // card, a price line is the market card's whole point, and neither
        // belongs on any other kind.
        let drawn = match channel.kind {
            ChannelKindDto::Weather => radar.cloned(),
            ChannelKindDto::Markets => channel
                .signal
                .as_ref()
                .and_then(|signal| series_figure(&signal.series, signal.previous_close)),
            _ => None,
        };
        match render_channel_card_with_radar(
            &channel_card(channel, preferences, snapshot),
            panel,
            drawn.as_ref(),
        ) {
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
        send_rendered_frame_to_display(
            app,
            display,
            &frame,
            preferences,
            false,
            Some((channel.id.as_str(), notice_key)),
        )
        .await,
        Some(channel.id.clone()),
    )
}

async fn send_rendered_frame_to_display(
    app: &AppHandle,
    display: &DisplayController,
    frame: &MonoFrame,
    preferences: &AppPreferences,
    force: bool,
    slide: Option<(&str, Option<&str>)>,
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
                detail: "Connecting to the configured e-paper panel…".into(),
                last_frame_at: None,
                last_ack_at: None,
                active_channel_id: None,
                active_notice_key: None,
                panel: None,
            },
        );
    }
    match display.send_frame(frame, preferences, force).await {
        Ok(Some((receipt, name))) => {
            let at = Timestamp::now().to_string();
            let transport = receipt_transport(receipt.transport);
            let detail = observe_panel_battery(
                app,
                display,
                receipt.battery_millivolts(),
                receipt.low_battery(),
            )
            .unwrap_or_else(|| "Panel connected".into());
            let status = DisplayConnectionStatus {
                state: DisplayConnectionState::Connected,
                transport: Some(transport),
                device_name: Some(name),
                detail,
                last_frame_at: Some(at.clone()),
                last_ack_at: Some(at),
                active_channel_id: slide.map(|(channel_id, _)| channel_id.to_owned()),
                active_notice_key: slide.and_then(|(_, notice_key)| notice_key.map(str::to_owned)),
                panel: Some(display.panel()),
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

fn observe_panel_battery(
    app: &AppHandle,
    display: &DisplayController,
    battery_millivolts: Option<u16>,
    low_battery: Option<bool>,
) -> Option<String> {
    if display.note_battery_state(low_battery)
        && let Err(error) = app
            .notification()
            .builder()
            .title("Panel battery low")
            .body("Connect the BrickellStatus panel to USB power.")
            .show()
    {
        warn!(%error, "low-battery notification could not be shown");
    }
    match (battery_millivolts, low_battery) {
        (Some(millivolts), Some(true)) => format!(
            "Panel connected · Battery low — connect USB ({})",
            battery_voltage_label(millivolts)
        )
        .into(),
        (Some(millivolts), Some(false)) => format!(
            "Panel connected · Battery {}",
            battery_voltage_label(millivolts)
        )
        .into(),
        _ => None,
    }
}

fn publish_banner_battery_status(
    app: &AppHandle,
    display: &DisplayController,
    banner: &DeviceBanner,
) {
    let Some(detail) =
        observe_panel_battery(app, display, banner.battery_millivolts, banner.low_battery)
    else {
        return;
    };
    let mut status = get_current_status(app);
    if status.state != DisplayConnectionState::Connected {
        return;
    }
    status.detail = detail;
    status.panel = banner.panel.or(status.panel);
    set_e213_transport_status(app, status);
}

fn battery_voltage_label(millivolts: u16) -> String {
    let centivolts = (u32::from(millivolts) + 5) / 10;
    format!("{}.{:02} V", centivolts / 100, centivolts % 100)
}

fn connected_status(
    active: &ActiveDisplay,
    panel: Option<PanelModel>,
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
        active_channel_id: None,
        active_notice_key: None,
        panel,
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
        active_channel_id: None,
        active_notice_key: None,
        panel: None,
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
        active_channel_id: None,
        active_notice_key: None,
        panel: None,
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
    app.state::<DisplayStatusState>()
        .current
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

// Only a tray reads this, and mobile has none; the tests still do.
#[cfg_attr(mobile, allow(dead_code))]
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

#[cfg(desktop)]
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Manages the display status cache. Runs on every platform, before
/// `install_tray`, because commands and the display worker both need it and
/// only the mirroring is a tray concern.
fn install_display_status(app: &mut tauri::App) {
    app.manage(DisplayStatusState {
        current: StdMutex::new(DisplayConnectionStatus::default()),
    });
}

#[cfg(desktop)]
fn install_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let initial = DisplayConnectionStatus::default();
    let (status_line, detail_line) = initial.menu_lines();
    let status_item = MenuItem::with_id(app, MENU_STATUS_ID, status_line, false, None::<&str>)?;
    let detail_item = MenuItem::with_id(app, MENU_DETAIL_ID, detail_line, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let open_item =
        MenuItem::with_id(app, MENU_OPEN_ID, "Open BrickellStatus", true, None::<&str>)?;
    let quit_item =
        MenuItem::with_id(app, MENU_QUIT_ID, "Quit BrickellStatus", true, None::<&str>)?;
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
    // macOS recolors the monochrome template glyph to match the menu bar;
    // everywhere else the colored mark is required or the icon disappears
    // against a dark taskbar.
    #[cfg(target_os = "macos")]
    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
    #[cfg(not(target_os = "macos"))]
    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .title(initial.tray_badge())
        .tooltip("BrickellStatus · panel disconnected")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| {
            // Double-click is only emitted on Windows, where it is the
            // convention for opening a tray app's window.
            if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                show_main_window(tray.app_handle());
            }
        })
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
        status_item,
        detail_item,
    });
    Ok(())
}

fn install_runtime(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let database_path = data_dir.join("brickellstatus.sqlite3");
    // The bundle identifier changed before the AIS learning loop was complete.
    // Preserve the useful vessel/crossing/track observations from that local
    // database, but never import its bridge intervals: those rows predate
    // successful-poll continuity and cannot prove the app was watching.
    let legacy_database_path = data_dir.parent().map(|parent| {
        parent
            .join("com.cmiami.puentegonorrea")
            .join("tenders-log.sqlite3")
    });
    let secret_store = LocalSecretStore::new(data_dir.join("credentials.json"));
    let engine = tauri::async_runtime::block_on(async {
        let store = Store::open(database_path).await?;
        if let Some(path) = legacy_database_path.as_deref()
            && path.is_file()
        {
            match store.import_legacy_learning(path).await {
                Ok(report) => info!(
                    vessels = report.vessels_added,
                    crossings = report.transits_added,
                    track_fixes = report.track_fixes_added,
                    river_transits = report.river_transits_added,
                    "older local AIS learning merged"
                ),
                // Import is intentionally opportunistic and idempotent. A
                // locked or damaged old database must not strand the live app;
                // the next launch gets another chance.
                Err(error) => warn!(%error, "older local AIS learning could not be merged"),
            }
        }
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
        Ok::<_, brickellstatus_runtime::RuntimeError>((store, engine, ais_key_fingerprint))
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
    let broker = Arc::new(PanelBroker::default());
    // Snapshot ingestion must keep running while the display task is dwelling.
    // When both jobs lived in this loop, `wait_or_preempt` had nobody capable
    // of enqueueing the new alert that was supposed to wake it.
    let ingest_broker = Arc::clone(&broker);
    let ingest_engine = Arc::clone(&engine);
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let Ok(snapshot) = ingest_engine.get_snapshot().await else {
                continue;
            };
            let preferences = ingest_engine.get_preferences().await;
            ingest_broker.ingest(&snapshot, &preferences);
        }
    });
    let mut next_prove_at = tokio::time::Instant::now();
    let mut prove_failures = 0_u32;
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    let mut next_display_at = tokio::time::Instant::now();
    let mut next_reconnect_at = tokio::time::Instant::now();
    let mut reconnect_failures = 0_u32;
    let mut next_battery_poll_at = tokio::time::Instant::now();
    // Opening the port resets the board, so looking for one to adopt is done
    // sparingly rather than on every pass of a five-second loop.
    let mut next_adopt_at = tokio::time::Instant::now();
    loop {
        interval.tick().await;
        let snapshot = match engine.get_snapshot().await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                warn!(%error, "background display snapshot failed");
                continue;
            }
        };
        let mut preferences = engine.get_preferences().await;
        // A board running our firmware, plugged in, and drawing nothing is the
        // state this used to sit in forever: the preview transport is the safe
        // default for a fresh install, and nothing ever moved it off. Adopting
        // a panel that has announced itself is not reaching for hardware
        // unasked — the board asked first.
        if preferences.display.transport == DisplayTransport::Preview
            && tokio::time::Instant::now() >= next_adopt_at
        {
            next_adopt_at = tokio::time::Instant::now() + Duration::from_secs(60);
            if let Some(port) = adoptable_panel_port().await {
                preferences.display.transport = DisplayTransport::Usb;
                preferences.display.serial_port = port.clone();
                match engine.save_preferences(preferences.clone()).await {
                    Ok(_) => info!(%port, "a panel announced itself; driving it"),
                    Err(error) => warn!(%error, "could not adopt the attached panel"),
                }
            }
        }
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
                    active_channel_id: None,
                    active_notice_key: None,
                    panel: None,
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

        if display.has_active().await && tokio::time::Instant::now() >= next_battery_poll_at {
            next_battery_poll_at = tokio::time::Instant::now() + PANEL_BATTERY_POLL_INTERVAL;
            match tokio::time::timeout(Duration::from_secs(3), display.read_banner()).await {
                Ok(Ok(Some(banner))) => publish_banner_battery_status(&app, &display, &banner),
                Ok(Ok(None)) => {}
                Ok(Err(error)) => debug!(%error, "panel battery status read failed"),
                Err(_) => debug!("panel battery status read timed out"),
            }
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
                | PanelSelection::Rotation { channel_id, .. } => {
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
            let (result, _) = match tokio::time::timeout(
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
                Err(_) => {
                    // Cancellation happens outside `send_frame`, so its normal
                    // error cleanup cannot run. Explicitly release any GATT
                    // link: otherwise the panel remains connected, stops
                    // advertising, and cannot be found by the next scan.
                    if let Err(error) = display.disconnect(false).await {
                        warn!(%error, "timed-out display route did not disconnect cleanly");
                    }
                    (
                        mutation_error("Display frame exceeded its 30 second deadline"),
                        None,
                    )
                }
            };
            // An alert holds for a fixed, bounded time. Every ordinary frame
            // uses the same readable cadence; channel order and timing are no
            // longer configuration concerns.
            let (dwell, holding_score) = match &selection {
                PanelSelection::Alert { score, .. } => (PanelBroker::alert_hold(), *score),
                PanelSelection::Rotation { .. } => (
                    Duration::from_secs(u64::from(AUTOMATIC_FRAME_DWELL_SECONDS)),
                    0,
                ),
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

/// One line of standing status for the Android watch notification.
///
/// Picks the channel the engine already ranked highest rather than deriving a
/// second opinion here: `priority.score` exists so the panel, the alerts and
/// this agree on what matters most right now.
#[cfg(target_os = "android")]
fn watch_status_copy(snapshot: &AppSnapshot) -> (String, String) {
    let leading = snapshot
        .channels
        .iter()
        .filter(|channel| channel.enabled && channel.active)
        .max_by_key(|channel| channel.priority.score);
    match leading {
        Some(channel) => {
            let title = channel
                .signal
                .as_ref()
                .map_or_else(|| channel.title.clone(), |signal| signal.headline.clone());
            (
                bounded_text(&title, 96),
                bounded_text(&channel.summary, 240),
            )
        }
        None => {
            let watching = snapshot
                .channels
                .iter()
                .filter(|channel| channel.enabled)
                .count();
            (
                "Nothing needs your attention".to_owned(),
                match watching {
                    0 => "No channels are enabled.".to_owned(),
                    1 => "Watching 1 channel.".to_owned(),
                    count => format!("Watching {count} channels."),
                },
            )
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
                // The watch notification is the only surface a backgrounded
                // phone has, so it carries the current decision rather than a
                // fixed "running" line.
                #[cfg(target_os = "android")]
                {
                    let (title, body) = watch_status_copy(&snapshot);
                    android_bridge::publish_status(&title, &body);
                }
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
            // Keep a full seasonal cycle for every observed hull. Storage
            // exempts confirmed bridge-openers from this cutoff, preserving
            // their movement catalog indefinitely.
            let track_cutoff_ms = Store::default_ais_track_cutoff_ms(now_ms);
            let forecast_cutoff_ms = Store::default_forecast_cutoff_ms(now_ms);
            match delivery_cutoff {
                Ok(delivery_cutoff) => {
                    match store.prune_history(&delivery_cutoff, track_cutoff_ms).await {
                        Ok(report) => {
                            match store.prune_forecast_samples(forecast_cutoff_ms).await {
                                Ok(forecast_samples) => {
                                    debug!(
                                        scrubbed_destinations = report.scrubbed_destinations,
                                        outbox_rows = report.outbox_rows,
                                        incidents = report.incidents,
                                        track_fixes = report.track_fixes,
                                        forecast_samples,
                                        "local history retention completed"
                                    );
                                    next_maintenance_at = tokio::time::Instant::now()
                                        + Duration::from_secs(24 * 60 * 60);
                                }
                                Err(error) => {
                                    warn!(%error, "forecast history retention failed");
                                    next_maintenance_at =
                                        tokio::time::Instant::now() + Duration::from_secs(60 * 60);
                                }
                            }
                        }
                        Err(error) => {
                            warn!(%error, "local history retention failed");
                            next_maintenance_at =
                                tokio::time::Instant::now() + Duration::from_secs(60 * 60);
                        }
                    }
                }
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
    tauri_plugin_opener::open_url(parsed.as_str(), None::<&str>)
        .map_err(|error| format!("The system browser could not open this link: {error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = dotenvy::dotenv();
    // Every TLS client in the process resolves the default rustls provider;
    // it must be installed before the first collector builds a client.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .try_init();
    let builder = tauri::Builder::default();
    // A phone has no second instance to fold into the first, and the plugin
    // itself is desktop-only.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        show_main_window(app);
    }));
    let application = builder
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            install_display_status(app);
            #[cfg(desktop)]
            install_tray(app)?;
            install_runtime(app)
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            get_preferences,
            get_aisstream_status,
            get_vessel_detail,
            save_preferences,
            refresh_sources,
            search_locations,
            get_display_status,
            get_platform_capabilities,
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
        // Closing the window hides the app to the tray rather than quitting
        // it. On Android there is no tray to hide into, and intercepting the
        // back gesture this way would strand the user in a live process with
        // nothing on screen.
        .on_window_event(|_window, _event| {
            #[cfg(desktop)]
            if _window.label() == "main"
                && let WindowEvent::CloseRequested { api, .. } = _event
            {
                api.prevent_close();
                let _ = _window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("BrickellStatus could not start");
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
