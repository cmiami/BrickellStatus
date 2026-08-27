//! Monochrome rendering and physical delivery for the Heltec e-paper panels.
//!
//! The crate deliberately owns a small presentation model instead of binding
//! the firmware protocol to the application's evolving domain contracts. A
//! caller projects bridge state into [`LiveSnapshot`] or another enabled
//! signal into [`ChannelCard`], renders a [`MonoFrame`] for the [`PanelModel`]
//! the attached board reports, and sends the resulting INK1 packet over USB or
//! BLE.
//!
//! Two panels are supported, the E213 and the larger E290. Nothing here asks
//! which one is attached: the board says so, and the geometry travels with the
//! frame from the grid it is laid out on all the way onto the wire.

mod banner;
mod channel;
mod channel_render;
mod frame;
mod model;
mod panel;
mod panel_grid;
mod panel_rail;
mod preview;
mod protocol;
pub mod radar;
mod render;
mod render_primitives;
#[cfg(feature = "_transport")]
pub mod transport;

/// btleplug's `jni`, re-exported so a caller writing the Android JNI entry
/// point cannot bind a different version than the one BLE was compiled
/// against -- the `&JNIEnv` handed to [`transport::init_android_bluetooth`]
/// has to be exactly this crate's type.
#[cfg(all(feature = "ble", target_os = "android"))]
pub use jni;

pub use banner::{DeviceBanner, UNKNOWN_BUILD};
pub use channel::{
    ChannelAvailability, ChannelCard, ChannelCardError, ChannelFrame, ChannelKind, ChannelSource,
    ChannelUrgency, MAX_ACTION_CHARS, MAX_CARD_TITLE_CHARS, MAX_CHANNEL_LABEL_CHARS,
    MAX_DETAIL_CHARS, MAX_HEADLINE_CHARS, MAX_SOURCE_CHARS,
};
pub use channel_render::{
    ChannelRenderError, render_channel_card, render_channel_card_with_radar, render_channel_frame,
};
pub use frame::MonoFrame;
pub use model::{
    ConfidenceBand, EtaRange, Evidence, Freshness, LiveSnapshot, SnapshotError, SnapshotState,
    SpanStatus,
};
pub use panel::{PanelHardware, PanelModel};
pub use preview::{PreviewError, preview_png_bytes, save_preview_png, save_scaled_preview_png};
pub use protocol::{
    FLAG_FULL_REFRESH, FULL_REFRESH_CHURN, HEADER_SIZE, INK1_MAGIC, MAX_PACKET_SIZE, ProtocolError,
    RefreshMode, ValidatedPacket, encode_packet, packet_size, validate_packet,
};
pub use radar::{
    RADAR_FIGURE_HEIGHT, RADAR_FIGURE_WIDTH, RadarError, RadarFigure, radar_figure_from_png,
    series_figure,
};
pub use render::{RenderConfig, RenderError, render_snapshot};
