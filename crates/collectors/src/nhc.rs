use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::{
    CollectContext, Collector, CollectorBatch, CollectorError, CollectorItem, FetchLimits,
    HttpFetcher, ItemKind, Location, SafeHttpFetcher, SourceLink, SyndicationCollector,
    SyndicationConfig, collection::collect_http,
};

const CURRENT_STORMS_URL: &str = "https://www.nhc.noaa.gov/CurrentStorms.json";
const ATLANTIC_RSS_URL: &str = "https://www.nhc.noaa.gov/index-at.xml";

pub struct NhcCurrentStormsCollector {
    endpoint: Url,
    fetcher: Arc<dyn HttpFetcher>,
}

impl NhcCurrentStormsCollector {
    pub fn new() -> Self {
        Self {
            endpoint: Url::parse(CURRENT_STORMS_URL).expect("constant URL is valid"),
            fetcher: Arc::new(SafeHttpFetcher::default()),
        }
    }

    pub fn with_fetcher(fetcher: Arc<dyn HttpFetcher>) -> Self {
        Self {
            endpoint: Url::parse(CURRENT_STORMS_URL).expect("constant URL is valid"),
            fetcher,
        }
    }
}

impl Default for NhcCurrentStormsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Collector for NhcCurrentStormsCollector {
    fn name(&self) -> &'static str {
        "nhc-current-storms"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        collect_http(
            self.name(),
            self.fetcher.as_ref(),
            &self.endpoint,
            context,
            &[("accept", "application/json")],
            |response| parse_current_storms(&response.body),
        )
        .await
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentStorms {
    active_storms: Option<Vec<Storm>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Storm {
    id: Option<String>,
    name: Option<String>,
    classification: Option<String>,
    intensity: Option<Value>,
    pressure: Option<Value>,
    latitude: Option<String>,
    longitude: Option<String>,
    latitude_numeric: Option<f64>,
    longitude_numeric: Option<f64>,
    movement_dir: Option<Value>,
    movement_speed: Option<Value>,
    last_update: Option<String>,
    public_advisory: Option<Product>,
    forecast_advisory: Option<Product>,
    forecast_discussion: Option<Product>,
    forecast_graphics: Option<Product>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Product {
    adv_num: Option<String>,
    issuance: Option<String>,
    url: Option<String>,
}

pub fn parse_current_storms(body: &[u8]) -> Result<Vec<CollectorItem>, CollectorError> {
    let response: CurrentStorms =
        serde_json::from_slice(body).map_err(|error| CollectorError::Parse {
            collector: "nhc-current-storms",
            detail: error.to_string(),
        })?;
    let storms = response
        .active_storms
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "nhc-current-storms",
            detail: "activeStorms array is missing".into(),
        })?;
    storms
        .into_iter()
        .enumerate()
        .map(|(index, storm)| {
            let id = required_string(storm.id, index, "id")?;
            let name = required_string(storm.name, index, "name")?;
            let observed_at = storm
                .last_update
                .as_deref()
                .map(|value| {
                    DateTime::parse_from_rfc3339(value)
                        .map(|value| value.with_timezone(&Utc))
                        .map_err(|error| CollectorError::SchemaChanged {
                            collector: "nhc-current-storms",
                            detail: format!("activeStorms[{index}].lastUpdate is invalid: {error}"),
                        })
                })
                .transpose()?;
            let mut attributes = BTreeMap::new();
            insert_option(
                &mut attributes,
                "classification",
                storm.classification.clone(),
            );
            insert_value(&mut attributes, "intensity_knots", storm.intensity.clone());
            insert_value(&mut attributes, "pressure_mb", storm.pressure.clone());
            insert_option(&mut attributes, "latitude_text", storm.latitude);
            insert_option(&mut attributes, "longitude_text", storm.longitude);
            insert_value(&mut attributes, "movement_degrees", storm.movement_dir);
            insert_value(&mut attributes, "movement_knots", storm.movement_speed);
            insert_product(
                &mut attributes,
                "public_advisory",
                storm.public_advisory.as_ref(),
            );
            insert_product(
                &mut attributes,
                "forecast_advisory",
                storm.forecast_advisory.as_ref(),
            );
            insert_product(
                &mut attributes,
                "forecast_discussion",
                storm.forecast_discussion.as_ref(),
            );
            insert_product(
                &mut attributes,
                "forecast_graphics",
                storm.forecast_graphics.as_ref(),
            );

            let classification = storm.classification.as_deref().unwrap_or("Cyclone");
            let intensity = storm
                .intensity
                .as_ref()
                .and_then(value_text)
                .map(|value| format!(" · {value} kt"))
                .unwrap_or_default();
            let public_url = storm
                .public_advisory
                .as_ref()
                .and_then(|product| product.url.as_deref())
                .and_then(|value| Url::parse(value).ok())
                .or_else(|| Url::parse("https://www.nhc.noaa.gov/cyclones/").ok());
            Ok(CollectorItem {
                id: format!("nhc:{id}"),
                kind: ItemKind::TropicalCyclone,
                title: format!("{classification} {name}"),
                summary: Some(format!("{classification} {name}{intensity}")),
                observed_at,
                starts_at: None,
                ends_at: None,
                location: validated_storm_location(
                    storm.latitude_numeric,
                    storm.longitude_numeric,
                    &name,
                    index,
                )?,
                source: SourceLink {
                    name: "National Hurricane Center".into(),
                    url: public_url,
                },
                attributes,
            })
        })
        .collect()
}

fn validated_storm_location(
    latitude: Option<f64>,
    longitude: Option<f64>,
    name: &str,
    index: usize,
) -> Result<Option<Location>, CollectorError> {
    match (latitude, longitude) {
        (None, None) => Ok(None),
        (Some(latitude), Some(longitude))
            if latitude.is_finite()
                && longitude.is_finite()
                && (-90.0..=90.0).contains(&latitude)
                && (-180.0..=180.0).contains(&longitude) =>
        {
            Ok(Some(Location {
                name: Some(format!("{name} center")),
                latitude: Some(latitude),
                longitude: Some(longitude),
            }))
        }
        _ => Err(CollectorError::SchemaChanged {
            collector: "nhc-current-storms",
            detail: format!(
                "activeStorms[{index}] numeric coordinates are incomplete or outside valid ranges"
            ),
        }),
    }
}

fn required_string(
    value: Option<String>,
    index: usize,
    field: &str,
) -> Result<String, CollectorError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "nhc-current-storms",
            detail: format!("activeStorms[{index}].{field} is missing"),
        })
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn insert_option(map: &mut BTreeMap<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        map.insert(key.into(), json!(value));
    }
}

fn insert_value(map: &mut BTreeMap<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value.filter(|value| !value.is_null()) {
        map.insert(key.into(), value);
    }
}

fn insert_product(map: &mut BTreeMap<String, Value>, key: &str, product: Option<&Product>) {
    if let Some(product) = product {
        map.insert(
            key.into(),
            json!({
                "advisory_number": product.adv_num,
                "issuance": product.issuance,
                "url": product.url,
            }),
        );
    }
}

/// NHC's RSS feed adds basin outlooks and plain-language summaries which are not
/// present in CurrentStorms.json.
pub struct NhcRssCollector {
    inner: SyndicationCollector,
}

impl NhcRssCollector {
    pub fn atlantic() -> Result<Self, CollectorError> {
        Self::new(Url::parse(ATLANTIC_RSS_URL).expect("constant URL is valid"))
    }

    pub fn new(url: Url) -> Result<Self, CollectorError> {
        let mut config = SyndicationConfig::new(url);
        config.source_name = Some("National Hurricane Center".into());
        config.max_items = 30;
        config.fetch_limits = FetchLimits {
            max_body_bytes: 1024 * 1024,
            ..FetchLimits::default()
        };
        Ok(Self {
            inner: SyndicationCollector::new(config)?,
        })
    }

    pub fn with_fetcher(url: Url, fetcher: Arc<dyn HttpFetcher>) -> Result<Self, CollectorError> {
        let mut config = SyndicationConfig::new(url);
        config.source_name = Some("National Hurricane Center".into());
        config.max_items = 30;
        Ok(Self {
            inner: SyndicationCollector::with_fetcher(config, fetcher)?,
        })
    }
}

#[async_trait]
impl Collector for NhcRssCollector {
    fn name(&self) -> &'static str {
        "nhc-rss"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        let mut batch = self.inner.collect(context).await?;
        batch.source = self.name().into();
        Ok(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_syndication;

    #[test]
    fn parses_current_storm_products() {
        let storms =
            parse_current_storms(include_bytes!("../fixtures/nhc-current-storms.json")).unwrap();
        assert_eq!(storms.len(), 1);
        assert_eq!(storms[0].title, "TS Iris");
        assert_eq!(storms[0].attributes["intensity_knots"], json!(50));
        assert_eq!(storms[0].location.as_ref().unwrap().latitude, Some(21.4));
    }

    #[test]
    fn nhc_rss_summary_is_feed_compatible() {
        let url = Url::parse(ATLANTIC_RSS_URL).unwrap();
        let summaries = parse_syndication(
            include_bytes!("../fixtures/nhc-atlantic-rss.xml"),
            Some("National Hurricane Center"),
            &url,
            30,
        )
        .unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].title, "Atlantic Tropical Weather Outlook");
    }

    #[test]
    fn storm_coordinates_fail_closed_when_incomplete_or_out_of_range() {
        assert!(validated_storm_location(Some(21.4), Some(-71.2), "Iris", 0).is_ok());
        assert!(validated_storm_location(Some(21.4), None, "Iris", 0).is_err());
        assert!(validated_storm_location(Some(121.4), Some(-71.2), "Iris", 0).is_err());
    }
}
