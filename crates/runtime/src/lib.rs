//! Tauri-ready orchestration for collection, prediction, persistence, and DTOs.
//!
//! This crate deliberately owns no HTTP listener and has no dependency on
//! Tauri. Desktop commands can hold an [`std::sync::Arc<RuntimeEngine>`] as
//! managed state and expose its narrow APIs.

mod engine;
mod factory;
mod location_search;

pub use bridgestatus_contract::*;
pub use bridgestatus_contract::{
    PreferencesError, default_alert_areas, default_channel_preferences, validate_preferences,
    whatsapp_consent_is_current,
};
pub use engine::{
    AisStreamKeyChange, CollectorFactory, CollectorRegistration, RefreshReport, RuntimeConfig,
    RuntimeEngine, RuntimeError, SchedulerHandle,
};
pub use factory::CredentialFreeCollectorFactory;
pub use location_search::{
    LocationSearchError, LocationSearchService, parse_location_search_response,
};
