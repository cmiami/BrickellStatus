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

const NWS_ALERTS_URL: &str = "https://api.weather.gov/alerts/active";

pub struct NwsAlertsCollector {
    latitude: f64,
    longitude: f64,
    user_agent: String,
    endpoint: Url,
    fetcher: Arc<dyn HttpFetcher>,
}

impl NwsAlertsCollector {
    /// Constructs the collector with the built-in network client.
    ///
    /// Native only: a Worker has no socket to give this, and supplies its own
    /// fetcher through [`Self::with_fetcher`] instead.
    #[cfg(feature = "native")]
    pub fn new(
        latitude: f64,
        longitude: f64,
        user_agent: impl Into<String>,
    ) -> Result<Self, CollectorError> {
        let user_agent = user_agent.into();
        let fetcher = Arc::new(SafeHttpFetcher::new(
            user_agent.clone(),
            Default::default(),
        )?);
        Self::with_fetcher(latitude, longitude, user_agent, fetcher)
    }

    pub fn with_fetcher(
        latitude: f64,
        longitude: f64,
        user_agent: impl Into<String>,
        fetcher: Arc<dyn HttpFetcher>,
    ) -> Result<Self, CollectorError> {
        validate_point(latitude, longitude)?;
        let user_agent = user_agent.into();
        if user_agent.trim().is_empty() {
            return Err(CollectorError::Configuration(
                "NWS requires a non-empty User-Agent with operator contact information".into(),
            ));
        }
        let mut endpoint = Url::parse(NWS_ALERTS_URL).expect("constant URL is valid");
        endpoint
            .query_pairs_mut()
            .append_pair("point", &format!("{latitude:.5},{longitude:.5}"));
        Ok(Self {
            latitude,
            longitude,
            user_agent,
            endpoint,
            fetcher,
        })
    }
}

#[async_trait]
impl Collector for NwsAlertsCollector {
    fn name(&self) -> &'static str {
        "nws-alerts"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        collect_http(
            self.name(),
            self.fetcher.as_ref(),
            &self.endpoint,
            context,
            &[
                ("accept", "application/geo+json"),
                ("user-agent", self.user_agent.as_str()),
            ],
            |response| parse_nws_alerts(&response.body, self.latitude, self.longitude),
        )
        .await
    }
}

#[derive(Debug, Deserialize)]
struct AlertCollection {
    #[serde(rename = "type")]
    collection_type: Option<String>,
    features: Option<Vec<AlertFeature>>,
}

#[derive(Debug, Deserialize)]
struct AlertFeature {
    id: Option<String>,
    properties: Option<AlertProperties>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlertProperties {
    id: Option<String>,
    event: Option<String>,
    headline: Option<String>,
    description: Option<String>,
    instruction: Option<String>,
    area_desc: Option<String>,
    sent: Option<String>,
    effective: Option<String>,
    onset: Option<String>,
    expires: Option<String>,
    ends: Option<String>,
    status: Option<String>,
    message_type: Option<String>,
    category: Option<String>,
    severity: Option<String>,
    certainty: Option<String>,
    urgency: Option<String>,
    response: Option<String>,
    #[serde(rename = "@id")]
    canonical_url: Option<String>,
    parameters: Option<Value>,
}

pub fn parse_nws_alerts(
    body: &[u8],
    latitude: f64,
    longitude: f64,
) -> Result<Vec<CollectorItem>, CollectorError> {
    validate_point(latitude, longitude)?;
    let collection: AlertCollection =
        serde_json::from_slice(body).map_err(|error| CollectorError::Parse {
            collector: "nws-alerts",
            detail: error.to_string(),
        })?;
    if collection.collection_type.as_deref() != Some("FeatureCollection") {
        return Err(CollectorError::SchemaChanged {
            collector: "nws-alerts",
            detail: "expected GeoJSON FeatureCollection".into(),
        });
    }
    let features = collection
        .features
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "nws-alerts",
            detail: "features array is missing".into(),
        })?;

    features
        .into_iter()
        .enumerate()
        .map(|(index, feature)| {
            let properties = feature
                .properties
                .ok_or_else(|| CollectorError::SchemaChanged {
                    collector: "nws-alerts",
                    detail: format!("features[{index}].properties is missing"),
                })?;
            let id = feature
                .id
                .or_else(|| properties.id.clone())
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| CollectorError::SchemaChanged {
                    collector: "nws-alerts",
                    detail: format!("features[{index}] has no id"),
                })?;
            let event = properties
                .event
                .clone()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| CollectorError::SchemaChanged {
                    collector: "nws-alerts",
                    detail: format!("features[{index}].properties.event is missing"),
                })?;
            let observed_at = parse_optional_time("sent", properties.sent.as_deref())?;
            let starts_at = parse_optional_time(
                "onset/effective",
                properties
                    .onset
                    .as_deref()
                    .or(properties.effective.as_deref()),
            )?;
            let ends_at = parse_optional_time(
                "ends/expires",
                properties.ends.as_deref().or(properties.expires.as_deref()),
            )?;
            let source_url = properties
                .canonical_url
                .as_deref()
                .or_else(|| id.starts_with("http").then_some(id.as_str()))
                .and_then(|value| Url::parse(value).ok());

            let mut attributes = BTreeMap::new();
            insert_option(&mut attributes, "status", properties.status);
            insert_option(&mut attributes, "message_type", properties.message_type);
            insert_option(&mut attributes, "category", properties.category);
            insert_option(&mut attributes, "severity", properties.severity);
            insert_option(&mut attributes, "certainty", properties.certainty);
            insert_option(&mut attributes, "urgency", properties.urgency);
            insert_option(&mut attributes, "response", properties.response);
            insert_option(&mut attributes, "instruction", properties.instruction);
            if let Some(parameters) = properties.parameters {
                attributes.insert("parameters".into(), parameters);
            }

            Ok(CollectorItem {
                id: format!("nws:{id}"),
                kind: ItemKind::OfficialAlert,
                title: event,
                summary: properties.headline.or(properties.description),
                observed_at,
                starts_at,
                ends_at,
                location: Some(Location {
                    name: properties.area_desc,
                    latitude: Some(latitude),
                    longitude: Some(longitude),
                }),
                source: SourceLink {
                    name: "National Weather Service".into(),
                    url: source_url,
                },
                attributes,
            })
        })
        .collect()
}

fn validate_point(latitude: f64, longitude: f64) -> Result<(), CollectorError> {
    if latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
    {
        Ok(())
    } else {
        Err(CollectorError::Configuration(format!(
            "invalid point {latitude},{longitude}"
        )))
    }
}

fn parse_optional_time(
    field: &str,
    value: Option<&str>,
) -> Result<Option<DateTime<Utc>>, CollectorError> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|error| CollectorError::SchemaChanged {
                    collector: "nws-alerts",
                    detail: format!("invalid {field} timestamp: {error}"),
                })
        })
        .transpose()
}

fn insert_option(map: &mut BTreeMap<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        map.insert(key.into(), json!(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_active_alerts_for_a_point() {
        let alerts = parse_nws_alerts(
            include_bytes!("../fixtures/nws-alerts.json"),
            25.7699,
            -80.19005,
        )
        .unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, ItemKind::OfficialAlert);
        assert_eq!(alerts[0].title, "Flash Flood Warning");
        assert_eq!(alerts[0].attributes["severity"], json!("Severe"));
        assert!(alerts[0].ends_at.is_some());
    }

    #[test]
    fn requires_a_user_agent() {
        let result =
            NwsAlertsCollector::with_fetcher(25.7, -80.2, "", Arc::new(SafeHttpFetcher::default()));
        assert!(matches!(result, Err(CollectorError::Configuration(_))));
    }
}
