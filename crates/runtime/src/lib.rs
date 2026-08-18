//! Tauri-ready orchestration for collection, prediction, persistence, and DTOs.
//!
//! This crate deliberately owns no HTTP listener and has no dependency on
//! Tauri. Desktop commands can hold an [`std::sync::Arc<RuntimeEngine>`] as
//! managed state and expose its narrow APIs.

mod catalog;
mod dto;
mod engine;
mod factory;
mod location_search;
mod preferences;

pub use catalog::{CatalogEntry, CatalogGroup, CatalogSection, FeedCatalog, catalog};
pub use dto::*;
pub use engine::{
    AisStreamKeyChange, CollectorFactory, CollectorRegistration, RefreshReport, RuntimeConfig,
    RuntimeEngine, RuntimeError, SchedulerHandle,
};
pub use factory::CredentialFreeCollectorFactory;
pub use location_search::{
    LocationSearchError, LocationSearchService, parse_location_search_response,
};
pub use preferences::{
    PreferencesError, default_alert_areas, default_channel_preferences, validate_preferences,
    whatsapp_consent_is_current,
};
