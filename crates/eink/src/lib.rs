//! Monochrome rendering and physical delivery for the Heltec E213 panel.
//!
//! The crate deliberately owns a small presentation model instead of binding
//! the firmware protocol to the application's evolving domain contracts. A
//! caller projects bridge state into [`LiveSnapshot`] or another enabled
//! signal into [`ChannelCard`], renders a [`MonoFrame`], and sends the
//! resulting INK1 packet over USB or BLE.

mod channel;
mod channel_render;
mod frame;
mod model;
mod preview;
mod protocol;
pub mod radar;
mod render;
mod render_primitives;
pub mod transport;

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
pub use preview::{PreviewError, preview_png_bytes, save_preview_png, save_scaled_preview_png};
pub use protocol::{
    FLAG_FULL_REFRESH, HEADER_SIZE, HEIGHT, INK1_MAGIC, PACKET_SIZE, PAYLOAD_SIZE, ProtocolError,
    RefreshMode, STRIDE, ValidatedPacket, WIDTH, encode_packet, validate_packet,
};
pub use radar::{
    RADAR_FIGURE_HEIGHT, RADAR_FIGURE_WIDTH, RadarError, RadarFigure, radar_figure_from_png,
};
pub use render::{RenderConfig, RenderError, render_snapshot};
