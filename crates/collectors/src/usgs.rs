use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

#[cfg(feature = "native")]
use crate::SafeHttpFetcher;
use crate::{
    CollectContext, Collector, CollectorBatch, CollectorError, CollectorItem, HttpFetcher,
    ItemKind, Location, SourceLink, collection::collect_http,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsgsWindow {
    Hour,
    Day,
    Week,
    Month,
    Magnitude45Hour,
}

impl UsgsWindow {
    fn url(self) -> Url {
        let feed = match self {
            Self::Hour => "significant_hour",
            Self::Day => "significant_day",
            Self::Week => "significant_week",
            Self::Month => "significant_month",
            Self::Magnitude45Hour => "4.5_hour",
        };
        Url::parse(&format!(
            "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/{feed}.geojson"
        ))
        .expect("generated USGS URL is valid")
    }
}

pub struct UsgsEarthquakesCollector {
    endpoint: Url,
    fetcher: Arc<dyn HttpFetcher>,
}

impl UsgsEarthquakesCollector {
    /// Constructs the collector with the built-in network client.
    ///
    /// Native only: a Worker has no socket to give this, and supplies its own
    /// fetcher through [`Self::with_fetcher`] instead.
    #[cfg(feature = "native")]
    pub fn new(window: UsgsWindow) -> Self {
        Self {
            endpoint: window.url(),
            fetcher: Arc::new(SafeHttpFetcher::default()),
        }
    }

    pub fn with_fetcher(window: UsgsWindow, fetcher: Arc<dyn HttpFetcher>) -> Self {
        Self {
            endpoint: window.url(),
            fetcher,
        }
    }
}

#[async_trait]
impl Collector for UsgsEarthquakesCollector {
    fn name(&self) -> &'static str {
        "usgs-significant-earthquakes"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        collect_http(
            self.name(),
            self.fetcher.as_ref(),
            &self.endpoint,
            context,
            &[("accept", "application/geo+json, application/json")],
            |response| parse_usgs_earthquakes(&response.body),
        )
        .await
    }
}

#[derive(Debug, Deserialize)]
struct FeatureCollection {
    #[serde(rename = "type")]
    collection_type: Option<String>,
    features: Option<Vec<Feature>>,
}

#[derive(Debug, Deserialize)]
struct Feature {
    id: Option<String>,
    properties: Option<Properties>,
    geometry: Option<Geometry>,
}

#[derive(Debug, Deserialize)]
struct Properties {
    mag: Option<f64>,
    place: Option<String>,
    time: Option<i64>,
    updated: Option<i64>,
    url: Option<String>,
    detail: Option<String>,
    felt: Option<i64>,
    cdi: Option<f64>,
    mmi: Option<f64>,
    alert: Option<String>,
    status: Option<String>,
    tsunami: Option<i64>,
    sig: Option<i64>,
    net: Option<String>,
    code: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Geometry {
    #[serde(rename = "type")]
    geometry_type: Option<String>,
    coordinates: Option<Vec<f64>>,
}

pub fn parse_usgs_earthquakes(body: &[u8]) -> Result<Vec<CollectorItem>, CollectorError> {
    let collection: FeatureCollection =
        serde_json::from_slice(body).map_err(|error| CollectorError::Parse {
            collector: "usgs-earthquakes",
            detail: error.to_string(),
        })?;
    if collection.collection_type.as_deref() != Some("FeatureCollection") {
        return Err(CollectorError::SchemaChanged {
            collector: "usgs-earthquakes",
            detail: "expected GeoJSON FeatureCollection".into(),
        });
    }
    let features = collection
        .features
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "usgs-earthquakes",
            detail: "features array is missing".into(),
        })?;
    features
        .into_iter()
        .enumerate()
        .map(|(index, feature)| {
            let id = feature
                .id
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| CollectorError::SchemaChanged {
                    collector: "usgs-earthquakes",
                    detail: format!("features[{index}].id is missing"),
                })?;
            let properties = feature
                .properties
                .ok_or_else(|| CollectorError::SchemaChanged {
                    collector: "usgs-earthquakes",
                    detail: format!("features[{index}].properties is missing"),
                })?;
            let geometry = feature
                .geometry
                .ok_or_else(|| CollectorError::SchemaChanged {
                    collector: "usgs-earthquakes",
                    detail: format!("features[{index}].geometry is missing"),
                })?;
            if geometry.geometry_type.as_deref() != Some("Point") {
                return Err(CollectorError::SchemaChanged {
                    collector: "usgs-earthquakes",
                    detail: format!("features[{index}].geometry is not a Point"),
                });
            }
            let coordinates = geometry
                .coordinates
                .filter(|values| values.len() >= 3)
                .ok_or_else(|| CollectorError::SchemaChanged {
                    collector: "usgs-earthquakes",
                    detail: format!(
                        "features[{index}].geometry.coordinates is not [longitude, latitude, depth]"
                    ),
                })?;
            validate_coordinates(&coordinates, index)?;
            let time = properties
                .time
                .and_then(DateTime::<Utc>::from_timestamp_millis)
                .ok_or_else(|| CollectorError::SchemaChanged {
                    collector: "usgs-earthquakes",
                    detail: format!("features[{index}].properties.time is missing or invalid"),
                })?;
            let updated = properties
                .updated
                .and_then(DateTime::<Utc>::from_timestamp_millis);
            let title = properties
                .title
                .clone()
                .or_else(|| properties.place.clone())
                .unwrap_or_else(|| "Significant earthquake".into());
            let source_url = properties
                .url
                .as_deref()
                .and_then(|value| Url::parse(value).ok());

            let mut attributes = BTreeMap::new();
            insert_value(
                &mut attributes,
                "magnitude",
                properties.mag.map(|value| json!(value)),
            );
            insert_value(&mut attributes, "depth_km", Some(json!(coordinates[2])));
            insert_value(
                &mut attributes,
                "felt_reports",
                properties.felt.map(|value| json!(value)),
            );
            insert_value(
                &mut attributes,
                "community_intensity",
                properties.cdi.map(|value| json!(value)),
            );
            insert_value(
                &mut attributes,
                "modified_mercalli",
                properties.mmi.map(|value| json!(value)),
            );
            insert_value(
                &mut attributes,
                "alert",
                properties.alert.map(|value| json!(value)),
            );
            insert_value(
                &mut attributes,
                "status",
                properties.status.map(|value| json!(value)),
            );
            insert_value(
                &mut attributes,
                "tsunami",
                properties.tsunami.map(|value| json!(value != 0)),
            );
            insert_value(
                &mut attributes,
                "significance",
                properties.sig.map(|value| json!(value)),
            );
            insert_value(
                &mut attributes,
                "network",
                properties.net.map(|value| json!(value)),
            );
            insert_value(
                &mut attributes,
                "code",
                properties.code.map(|value| json!(value)),
            );
            insert_value(
                &mut attributes,
                "detail_url",
                properties.detail.map(|value| json!(value)),
            );
            if let Some(updated) = updated {
                attributes.insert("updated_at".into(), json!(updated));
            }
            let summary = match (properties.mag, properties.place.as_deref()) {
                (Some(magnitude), Some(place)) => Some(format!("M{magnitude:.1} · {place}")),
                (Some(magnitude), None) => Some(format!("Magnitude {magnitude:.1}")),
                (None, Some(place)) => Some(place.into()),
                _ => None,
            };
            Ok(CollectorItem {
                id: format!("usgs:{id}"),
                kind: ItemKind::Earthquake,
                title,
                summary,
                observed_at: Some(time),
                starts_at: Some(time),
                ends_at: None,
                location: Some(Location {
                    name: properties.place,
                    latitude: Some(coordinates[1]),
                    longitude: Some(coordinates[0]),
                }),
                source: SourceLink {
                    name: "U.S. Geological Survey".into(),
                    url: source_url,
                },
                attributes,
            })
        })
        .collect()
}

fn validate_coordinates(coordinates: &[f64], index: usize) -> Result<(), CollectorError> {
    let valid = coordinates.len() >= 3
        && coordinates[0].is_finite()
        && coordinates[1].is_finite()
        && coordinates[2].is_finite()
        && (-180.0..=180.0).contains(&coordinates[0])
        && (-90.0..=90.0).contains(&coordinates[1]);
    if valid {
        Ok(())
    } else {
        Err(CollectorError::SchemaChanged {
            collector: "usgs-earthquakes",
            detail: format!(
                "features[{index}].geometry.coordinates contains invalid longitude, latitude, or depth"
            ),
        })
    }
}

fn insert_value(map: &mut BTreeMap<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        map.insert(key.into(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_significant_earthquake_geojson() {
        let items = parse_usgs_earthquakes(include_bytes!(
            "../fixtures/usgs-significant-earthquakes.json"
        ))
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ItemKind::Earthquake);
        assert_eq!(items[0].attributes["magnitude"], json!(6.4));
        assert_eq!(items[0].location.as_ref().unwrap().longitude, Some(-72.4));
    }

    #[test]
    fn magnitude_feed_selection_matches_the_usgs_contract() {
        assert_eq!(
            UsgsWindow::Magnitude45Hour.url().as_str(),
            "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/4.5_hour.geojson"
        );
        assert_eq!(
            UsgsWindow::Hour.url().as_str(),
            "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/significant_hour.geojson"
        );
    }

    #[test]
    fn earthquake_coordinates_must_be_finite_and_in_range() {
        assert!(validate_coordinates(&[-72.4, 18.2, 10.0], 0).is_ok());
        assert!(validate_coordinates(&[-272.4, 18.2, 10.0], 0).is_err());
        assert!(validate_coordinates(&[-72.4, f64::NAN, 10.0], 0).is_err());
    }
}
