//! The shapes that cross every boundary in the system.
//!
//! Snapshots, preferences, and the enums they are built from — and nothing
//! else. This crate exists because those types are needed in three places that
//! cannot all afford the engine: the desktop app, the renderer's projection,
//! and a Cloudflare Worker with no filesystem and no sockets.
//!
//! Keeping them here means one definition of what a channel is, rather than a
//! copy per deployment that drifts the first time either changes.

mod dto;
mod preferences;

pub use dto::*;
pub use preferences::{
    PreferencesError, default_alert_areas, default_channel_preferences, validate_preferences,
    whatsapp_consent_is_current,
};
