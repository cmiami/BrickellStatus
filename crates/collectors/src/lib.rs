//! Collectors which turn public feeds and optional backend-only adapters into one
//! small, stable model.
//!
//! The types here deliberately do not depend on the application's core crate.  The
//! application can map [`CollectorBatch`] into its domain model without making a
//! source adapter aware of policy, persistence, or presentation concerns.

mod ais_stream;
mod collection;
mod fl511;
mod geo;
mod http;
mod model;
mod nhc;
mod nws;
mod open_meteo;
mod syndication;
mod usgs;
mod yahoo_chart;

pub use ais_stream::{
    AIS_VESSEL_TRACKS_CURSOR_KEY, AisStreamApiKey, AisStreamCollector, AisStreamConfig,
    AisStreamSubscription,
};
pub use fl511::{
    BridgeRelation, BridgeSelector, BridgeState, BridgeTooltip, Fl511BridgeCollector, Fl511Config,
    Fl511Discovery, Fl511LayerEntry, Fl511ParseError, parse_bridge_layer, parse_bridge_tooltip,
};
pub use http::{FetchLimits, FetchResponse, HttpFetcher, SafeHttpFetcher, validate_public_url};
pub use model::{
    CollectContext, Collector, CollectorBatch, CollectorCursor, CollectorError, CollectorHealth,
    CollectorItem, HealthState, ItemKind, Location, SourceLink,
};
pub use nhc::{NhcCurrentStormsCollector, NhcRssCollector, parse_current_storms};
pub use nws::{NwsAlertsCollector, parse_nws_alerts};
pub use open_meteo::{OpenMeteoCollector, parse_open_meteo};
pub use syndication::{SyndicationCollector, SyndicationConfig, parse_syndication};
pub use usgs::{UsgsEarthquakesCollector, UsgsWindow, parse_usgs_earthquakes};
pub use yahoo_chart::{MarketSession, YahooChartCollector, YahooChartConfig, parse_yahoo_chart};
