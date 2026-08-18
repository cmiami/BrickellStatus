use std::time::Duration;

use reqwest::{
    StatusCode,
    header::{ACCEPT, HeaderValue},
    redirect::Policy,
};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use url::Url;

use crate::LocationSearchResult;

const OPEN_METEO_GEOCODING_ENDPOINT: &str = "https://geocoding-api.open-meteo.com/v1/search";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_RESULTS: usize = 8;
const MIN_QUERY_CHARS: usize = 2;
const MAX_QUERY_CHARS: usize = 160;
const MIN_USER_AGENT_CHARS: usize = 8;
const MAX_USER_AGENT_CHARS: usize = 256;

/// Errors returned by [`LocationSearchService`] and
/// [`parse_location_search_response`].
#[derive(Debug, Error)]
pub enum LocationSearchError {
    #[error(
        "location query must contain between {MIN_QUERY_CHARS} and {MAX_QUERY_CHARS} characters after trimming (got {length})"
    )]
    InvalidQuery { length: usize },
    #[error("location search requires a descriptive User-Agent: {reason}")]
    InvalidUserAgent { reason: String },
    #[error("failed to configure the location search HTTP client")]
    Client,
    #[error("location search network request failed ({category})")]
    Network { category: String },
    #[error("location search returned HTTP {status}")]
    Http { status: StatusCode },
    #[error("failed while reading the location search response body ({category})")]
    Body { category: String },
    #[error("location search response body exceeded the {limit}-byte limit")]
    BodyTooLarge { limit: usize },
    #[error("location search response body is not valid JSON: {source}")]
    InvalidJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("Open-Meteo geocoding schema mismatch: {detail}")]
    Schema { detail: String },
}

/// Production HTTP client for Open-Meteo's fixed geocoding endpoint.
///
/// The client rejects redirects and applies both connect and whole-request
/// timeouts. Responses are streamed into a bounded buffer before parsing.
#[derive(Clone, Debug)]
pub struct LocationSearchService {
    client: reqwest::Client,
}

impl LocationSearchService {
    pub fn new(user_agent: impl AsRef<str>) -> Result<Self, LocationSearchError> {
        let user_agent = validated_user_agent(user_agent.as_ref())?;
        // reqwest's provider-free rustls refuses to build a client until a
        // process-default CryptoProvider exists; installing is idempotent.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            .user_agent(user_agent)
            .build()
            .map_err(|_| LocationSearchError::Client)?;
        Ok(Self { client })
    }

    /// Searches Open-Meteo for at most eight locations matching `query`.
    pub async fn search(
        &self,
        query: &str,
    ) -> Result<Vec<LocationSearchResult>, LocationSearchError> {
        let query = validated_query(query)?;
        let url = search_url(query);
        let mut response = self
            .client
            .get(url)
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|source| LocationSearchError::Network {
                category: request_failure_category(&source).into(),
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(LocationSearchError::Http { status });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(LocationSearchError::BodyTooLarge {
                limit: MAX_RESPONSE_BYTES,
            });
        }

        let mut body = Vec::with_capacity(
            response
                .content_length()
                .unwrap_or(0)
                .min(MAX_RESPONSE_BYTES as u64) as usize,
        );
        while let Some(chunk) =
            response
                .chunk()
                .await
                .map_err(|source| LocationSearchError::Body {
                    category: request_failure_category(&source).into(),
                })?
        {
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(LocationSearchError::BodyTooLarge {
                    limit: MAX_RESPONSE_BYTES,
                });
            }
            body.extend_from_slice(&chunk);
        }

        parse_location_search_response(&body)
    }
}

/// Parses an Open-Meteo geocoding JSON response without performing network I/O.
///
/// Open-Meteo omits `results` when no locations match, which is represented as
/// an empty vector. Unknown fields are accepted so additive upstream changes do
/// not break the application.
pub fn parse_location_search_response(
    body: &[u8],
) -> Result<Vec<LocationSearchResult>, LocationSearchError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(LocationSearchError::BodyTooLarge {
            limit: MAX_RESPONSE_BYTES,
        });
    }

    let root: Value = serde_json::from_slice(body)
        .map_err(|source| LocationSearchError::InvalidJson { source })?;
    let object = root
        .as_object()
        .ok_or_else(|| LocationSearchError::Schema {
            detail: "expected a JSON object at the response root".into(),
        })?;
    let Some(raw_results) = object.get("results") else {
        return Ok(Vec::new());
    };
    let raw_results = raw_results
        .as_array()
        .ok_or_else(|| LocationSearchError::Schema {
            detail: "results must be an array when present".into(),
        })?;

    raw_results
        .iter()
        .take(MAX_RESULTS)
        .enumerate()
        .map(|(index, value)| parse_result(index, value.clone()))
        .collect()
}

#[derive(Debug, Deserialize)]
struct OpenMeteoLocation {
    id: u64,
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default)]
    admin1: Option<String>,
    #[serde(default)]
    country: Option<String>,
}

fn parse_result(index: usize, value: Value) -> Result<LocationSearchResult, LocationSearchError> {
    let raw: OpenMeteoLocation =
        serde_json::from_value(value).map_err(|error| LocationSearchError::Schema {
            detail: format!("results[{index}] is invalid: {error}"),
        })?;

    let name = required_nonempty(raw.name, index, "name")?;
    validate_coordinate(raw.latitude, -90.0..=90.0, index, "latitude")?;
    validate_coordinate(raw.longitude, -180.0..=180.0, index, "longitude")?;

    let admin_area = optional_nonempty(raw.admin1);
    let country = optional_nonempty(raw.country);
    let country_code = optional_nonempty(raw.country_code);
    if country_code
        .as_ref()
        .is_some_and(|code| code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_alphabetic()))
    {
        return Err(LocationSearchError::Schema {
            detail: format!("results[{index}].country_code must be a two-letter country code"),
        });
    }
    let time_zone = optional_nonempty(raw.timezone).unwrap_or_else(|| "UTC".into());
    let label = std::iter::once(Some(name.as_str()))
        .chain([admin_area.as_deref(), country.as_deref()])
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");

    Ok(LocationSearchResult {
        id: format!("open-meteo:{}", raw.id),
        label,
        latitude: raw.latitude,
        longitude: raw.longitude,
        time_zone,
        country_code,
        admin_area,
    })
}

fn required_nonempty(
    value: String,
    index: usize,
    field: &str,
) -> Result<String, LocationSearchError> {
    optional_nonempty(Some(value)).ok_or_else(|| LocationSearchError::Schema {
        detail: format!("results[{index}].{field} cannot be empty"),
    })
}

fn optional_nonempty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn validate_coordinate(
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    index: usize,
    field: &str,
) -> Result<(), LocationSearchError> {
    if value.is_finite() && range.contains(&value) {
        Ok(())
    } else {
        Err(LocationSearchError::Schema {
            detail: format!("results[{index}].{field} is outside its valid range"),
        })
    }
}

fn validated_query(query: &str) -> Result<&str, LocationSearchError> {
    let query = query.trim();
    let length = query.chars().count();
    if (MIN_QUERY_CHARS..=MAX_QUERY_CHARS).contains(&length) {
        Ok(query)
    } else {
        Err(LocationSearchError::InvalidQuery { length })
    }
}

fn validated_user_agent(user_agent: &str) -> Result<HeaderValue, LocationSearchError> {
    let user_agent = user_agent.trim();
    let length = user_agent.chars().count();
    if !(MIN_USER_AGENT_CHARS..=MAX_USER_AGENT_CHARS).contains(&length) {
        return Err(LocationSearchError::InvalidUserAgent {
            reason: format!(
                "must contain between {MIN_USER_AGENT_CHARS} and {MAX_USER_AGENT_CHARS} characters"
            ),
        });
    }
    if !user_agent.contains('/') || !user_agent.chars().any(char::is_alphabetic) {
        return Err(LocationSearchError::InvalidUserAgent {
            reason:
                "use a product/version token, optionally followed by operator contact information"
                    .into(),
        });
    }
    HeaderValue::from_str(user_agent).map_err(|error| LocationSearchError::InvalidUserAgent {
        reason: format!("contains characters that are not valid in an HTTP header: {error}"),
    })
}

fn search_url(query: &str) -> Url {
    let mut url = Url::parse(OPEN_METEO_GEOCODING_ENDPOINT).expect("constant URL must be valid");
    url.query_pairs_mut()
        .append_pair("name", query)
        .append_pair("count", &MAX_RESULTS.to_string())
        .append_pair("language", "en")
        .append_pair("format", "json");
    url
}

fn request_failure_category(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connection"
    } else if error.is_body() {
        "response body"
    } else {
        "transport"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_into_runtime_dto() {
        let results =
            parse_location_search_response(include_bytes!("../fixtures/open-meteo-geocoding.json"))
                .unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, "open-meteo:4164138");
        assert_eq!(results[0].label, "Miami, Florida, United States");
        assert_eq!(results[0].latitude, 25.77427);
        assert_eq!(results[0].longitude, -80.19366);
        assert_eq!(results[0].time_zone, "America/New_York");
        assert_eq!(results[0].country_code.as_deref(), Some("US"));
        assert_eq!(results[0].admin_area.as_deref(), Some("Florida"));

        assert_eq!(results[2].label, "Null Island");
        assert_eq!(results[2].time_zone, "UTC");
        assert_eq!(results[2].country_code, None);
        assert_eq!(results[2].admin_area, None);
    }

    #[test]
    fn missing_results_is_a_valid_empty_response() {
        assert_eq!(
            parse_location_search_response(br#"{"generationtime_ms": 0.12}"#).unwrap(),
            Vec::new()
        );
    }

    #[test]
    fn parser_never_returns_more_than_eight_results() {
        let results = (1..=10)
            .map(|id| {
                serde_json::json!({
                    "id": id,
                    "name": format!("Place {id}"),
                    "latitude": 1.0,
                    "longitude": 2.0
                })
            })
            .collect::<Vec<_>>();
        let body = serde_json::to_vec(&serde_json::json!({ "results": results })).unwrap();

        let parsed = parse_location_search_response(&body).unwrap();
        assert_eq!(parsed.len(), MAX_RESULTS);
        assert_eq!(parsed.last().unwrap().id, "open-meteo:8");
    }

    #[test]
    fn distinguishes_invalid_json_from_schema_changes() {
        assert!(matches!(
            parse_location_search_response(br#"{"results": ["#),
            Err(LocationSearchError::InvalidJson { .. })
        ));
        assert!(matches!(
            parse_location_search_response(br#"{"results": {}}"#),
            Err(LocationSearchError::Schema { .. })
        ));
        assert!(matches!(
            parse_location_search_response(
                br#"{"results":[{"id":1,"name":"Miami","latitude":91,"longitude":-80}]}"#
            ),
            Err(LocationSearchError::Schema { .. })
        ));
    }

    #[test]
    fn rejects_oversized_parser_input() {
        let body = vec![b' '; MAX_RESPONSE_BYTES + 1];
        assert!(matches!(
            parse_location_search_response(&body),
            Err(LocationSearchError::BodyTooLarge { .. })
        ));
    }

    #[test]
    fn query_validation_uses_trimmed_unicode_character_count() {
        assert_eq!(validated_query("  Mí  ").unwrap(), "Mí");
        assert!(matches!(
            validated_query(" x "),
            Err(LocationSearchError::InvalidQuery { length: 1 })
        ));
        let maximum = "x".repeat(MAX_QUERY_CHARS);
        assert_eq!(validated_query(&maximum).unwrap(), maximum);
        let too_long = "x".repeat(MAX_QUERY_CHARS + 1);
        assert!(matches!(
            validated_query(&too_long),
            Err(LocationSearchError::InvalidQuery { .. })
        ));
    }

    #[test]
    fn validates_a_descriptive_user_agent_without_network_io() {
        assert!(LocationSearchService::new("TenderStatus/0.1 (ops@example.test)").is_ok());
        assert!(matches!(
            LocationSearchService::new("reqwest"),
            Err(LocationSearchError::InvalidUserAgent { .. })
        ));
        assert!(matches!(
            LocationSearchService::new("TenderStatus/0.1\nInjected: value"),
            Err(LocationSearchError::InvalidUserAgent { .. })
        ));
    }

    #[test]
    fn builds_only_the_fixed_https_endpoint_and_bounded_query() {
        let url = search_url(validated_query(" Miami Beach ").unwrap());
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("geocoding-api.open-meteo.com"));
        assert_eq!(url.path(), "/v1/search");
        let pairs = url
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            pairs.get("name").map(|value| value.as_ref()),
            Some("Miami Beach")
        );
        assert_eq!(pairs.get("count").map(|value| value.as_ref()), Some("8"));
        assert_eq!(
            pairs.get("language").map(|value| value.as_ref()),
            Some("en")
        );
        assert_eq!(
            pairs.get("format").map(|value| value.as_ref()),
            Some("json")
        );
    }
}
