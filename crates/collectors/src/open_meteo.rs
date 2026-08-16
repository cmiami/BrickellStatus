use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{Map, Value, json};
use url::Url;

use crate::{
    CollectContext, Collector, CollectorBatch, CollectorError, CollectorItem, HttpFetcher,
    ItemKind, Location, SafeHttpFetcher, SourceLink, collection::collect_http,
};

const OPEN_METEO_URL: &str = "https://api.open-meteo.com/v1/forecast";
const CURRENT_FIELDS: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,rain,showers,weather_code,cloud_cover,wind_speed_10m,wind_direction_10m,wind_gusts_10m";
const HOURLY_FIELDS: &str = "temperature_2m,apparent_temperature,precipitation_probability,precipitation,rain,showers,weather_code,cloud_cover,visibility,wind_speed_10m,wind_direction_10m,wind_gusts_10m";
// Open-Meteo resolves a request onto a forecast-model grid and can therefore
// return a nearby grid coordinate rather than echoing the requested point.
// Half a degree per axis is a deliberately conservative upper bound for that
// snap: it tolerates coarse grids while still binding a response to the
// configured region instead of accepting coordinates from elsewhere.
const RESPONSE_GRID_SNAP_TOLERANCE_DEGREES: f64 = 0.5;
/// Precipitation amount, not probability. A bin that says how much rain falls
/// in a named quarter-hour supports an actual ETA; an hourly chance does not.
const MINUTELY_FIELDS: &str = "precipitation,rain,showers";
/// One hour of bins. The rain rule looks half an hour ahead, and the first bin
/// is the one already in progress, so four leaves margin without asking the
/// provider for a forecast nobody reads.
const MINUTELY_BINS: u16 = 4;

pub struct OpenMeteoCollector {
    latitude: f64,
    longitude: f64,
    endpoint: Url,
    fetcher: Arc<dyn HttpFetcher>,
}

impl OpenMeteoCollector {
    pub fn new(latitude: f64, longitude: f64, forecast_hours: u16) -> Result<Self, CollectorError> {
        Self::with_fetcher(
            latitude,
            longitude,
            forecast_hours,
            Arc::new(SafeHttpFetcher::default()),
        )
    }

    pub fn with_fetcher(
        latitude: f64,
        longitude: f64,
        forecast_hours: u16,
        fetcher: Arc<dyn HttpFetcher>,
    ) -> Result<Self, CollectorError> {
        validate_point(latitude, longitude)?;
        if !(1..=168).contains(&forecast_hours) {
            return Err(CollectorError::Configuration(
                "Open-Meteo forecast_hours must be between 1 and 168".into(),
            ));
        }
        let mut endpoint = Url::parse(OPEN_METEO_URL).expect("constant URL is valid");
        endpoint
            .query_pairs_mut()
            .append_pair("latitude", &format!("{latitude:.5}"))
            .append_pair("longitude", &format!("{longitude:.5}"))
            .append_pair("current", CURRENT_FIELDS)
            .append_pair("hourly", HOURLY_FIELDS)
            .append_pair("minutely_15", MINUTELY_FIELDS)
            .append_pair("forecast_minutely_15", &MINUTELY_BINS.to_string())
            .append_pair("forecast_hours", &forecast_hours.to_string())
            .append_pair("timezone", "UTC");
        Ok(Self {
            latitude,
            longitude,
            endpoint,
            fetcher,
        })
    }
}

#[async_trait]
impl Collector for OpenMeteoCollector {
    fn name(&self) -> &'static str {
        "open-meteo"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        collect_http(
            self.name(),
            self.fetcher.as_ref(),
            &self.endpoint,
            context,
            &[("accept", "application/json")],
            |response| parse_open_meteo(&response.body, self.latitude, self.longitude),
        )
        .await
    }
}

pub fn parse_open_meteo(
    body: &[u8],
    configured_latitude: f64,
    configured_longitude: f64,
) -> Result<Vec<CollectorItem>, CollectorError> {
    validate_point(configured_latitude, configured_longitude)?;
    let root: Value = serde_json::from_slice(body).map_err(|error| CollectorError::Parse {
        collector: "open-meteo",
        detail: error.to_string(),
    })?;
    let object = root
        .as_object()
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: "expected a JSON object".into(),
        })?;
    let latitude = required_response_coordinate(object, "latitude")?;
    let longitude = required_response_coordinate(object, "longitude")?;
    validate_response_point(
        latitude,
        longitude,
        configured_latitude,
        configured_longitude,
    )?;
    let location = Location::point(latitude, longitude);
    let source = SourceLink {
        name: "Open-Meteo".into(),
        url: Some(Url::parse(OPEN_METEO_URL).expect("constant URL is valid")),
    };

    let current = object
        .get("current")
        .and_then(Value::as_object)
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: "current object is missing".into(),
        })?;
    let current_time = required_time(current.get("time"), "current.time")?;
    let current_units = object
        .get("current_units")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut current_attributes = scalar_attributes(current, &["time"])?;
    current_attributes.insert("units".into(), current_units);
    let summary = temperature_summary(current, object.get("current_units"));
    let mut items = vec![CollectorItem {
        id: format!("open-meteo:current:{}", current_time.timestamp()),
        kind: ItemKind::WeatherCurrent,
        title: "Current weather".into(),
        summary,
        observed_at: Some(current_time),
        starts_at: None,
        ends_at: None,
        location: Some(location.clone()),
        source: source.clone(),
        attributes: current_attributes,
    }];

    let hourly = object
        .get("hourly")
        .and_then(Value::as_object)
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: "hourly object is missing".into(),
        })?;
    let times = hourly
        .get("time")
        .and_then(Value::as_array)
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: "hourly.time array is missing".into(),
        })?;
    let hourly_units = object
        .get("hourly_units")
        .cloned()
        .unwrap_or_else(|| json!({}));
    for (key, values) in hourly {
        if key != "time"
            && values
                .as_array()
                .is_none_or(|values| values.len() != times.len())
        {
            return Err(CollectorError::SchemaChanged {
                collector: "open-meteo",
                detail: format!("hourly.{key} length does not match hourly.time"),
            });
        }
    }
    for (index, value) in times.iter().enumerate() {
        let time = required_time(Some(value), &format!("hourly.time[{index}]"))?;
        let mut attributes = BTreeMap::new();
        for (key, values) in hourly {
            if key == "time" {
                continue;
            }
            attributes.insert(
                key.clone(),
                values
                    .as_array()
                    .and_then(|values| values.get(index))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        attributes.insert("units".into(), hourly_units.clone());
        let summary = attributes
            .get("temperature_2m")
            .filter(|value| !value.is_null())
            .map(|temperature| format!("{temperature}° hourly forecast"));
        items.push(CollectorItem {
            id: format!("open-meteo:hourly:{}", time.timestamp()),
            kind: ItemKind::WeatherHourly,
            title: "Hourly forecast".into(),
            summary,
            observed_at: None,
            starts_at: Some(time),
            ends_at: None,
            location: Some(location.clone()),
            source: source.clone(),
            attributes,
        });
    }

    // Absent rather than required. `minutely_15` coverage is region-dependent,
    // so demanding it would turn an uncovered location into a schema error and
    // take the hourly forecast down with it. When it is missing the rain rule
    // falls back to hourly probability and says so.
    if let Some(minutely) = object.get("minutely_15").and_then(Value::as_object) {
        let times = minutely
            .get("time")
            .and_then(Value::as_array)
            .ok_or_else(|| CollectorError::SchemaChanged {
                collector: "open-meteo",
                detail: "minutely_15.time array is missing".into(),
            })?;
        let minutely_units = object
            .get("minutely_15_units")
            .cloned()
            .unwrap_or_else(|| json!({}));
        for (key, values) in minutely {
            if key != "time"
                && values
                    .as_array()
                    .is_none_or(|values| values.len() != times.len())
            {
                return Err(CollectorError::SchemaChanged {
                    collector: "open-meteo",
                    detail: format!("minutely_15.{key} length does not match minutely_15.time"),
                });
            }
        }
        for (index, value) in times.iter().enumerate() {
            let time = required_time(Some(value), &format!("minutely_15.time[{index}]"))?;
            let mut attributes = BTreeMap::new();
            for (key, values) in minutely {
                if key == "time" {
                    continue;
                }
                attributes.insert(
                    key.clone(),
                    values
                        .as_array()
                        .and_then(|values| values.get(index))
                        .cloned()
                        .unwrap_or(Value::Null),
                );
            }
            attributes.insert("units".into(), minutely_units.clone());
            items.push(CollectorItem {
                id: format!("open-meteo:minutely-15:{}", time.timestamp()),
                kind: ItemKind::WeatherMinutely,
                title: "15-minute forecast".into(),
                summary: attributes
                    .get("precipitation")
                    .filter(|value| !value.is_null())
                    .map(|amount| format!("{amount} mm in 15 minutes")),
                observed_at: None,
                starts_at: Some(time),
                ends_at: None,
                location: Some(location.clone()),
                source: source.clone(),
                attributes,
            });
        }
    }

    Ok(items)
}

fn required_response_coordinate(
    object: &Map<String, Value>,
    field: &str,
) -> Result<f64, CollectorError> {
    object
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: format!("response {field} is missing or not numeric"),
        })
}

fn validate_response_point(
    latitude: f64,
    longitude: f64,
    configured_latitude: f64,
    configured_longitude: f64,
) -> Result<(), CollectorError> {
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err(CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: "response coordinates are non-finite or outside valid ranges".into(),
        });
    }

    let latitude_delta = (latitude - configured_latitude).abs();
    let longitude_delta = longitude_delta_degrees(longitude, configured_longitude);
    if latitude_delta > RESPONSE_GRID_SNAP_TOLERANCE_DEGREES
        || longitude_delta > RESPONSE_GRID_SNAP_TOLERANCE_DEGREES
    {
        return Err(CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: "response coordinates are outside the accepted forecast-grid snap tolerance"
                .into(),
        });
    }

    Ok(())
}

fn longitude_delta_degrees(left: f64, right: f64) -> f64 {
    ((left - right + 180.0).rem_euclid(360.0) - 180.0).abs()
}

fn required_time(value: Option<&Value>, field: &str) -> Result<DateTime<Utc>, CollectorError> {
    let value = value
        .and_then(Value::as_str)
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: format!("{field} is missing or not a string"),
        })?;
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M").map(|value| value.and_utc())
        })
        .map_err(|error| CollectorError::SchemaChanged {
            collector: "open-meteo",
            detail: format!("invalid {field}: {error}"),
        })
}

fn scalar_attributes(
    object: &Map<String, Value>,
    exclude: &[&str],
) -> Result<BTreeMap<String, Value>, CollectorError> {
    object
        .iter()
        .filter(|(key, _)| !exclude.contains(&key.as_str()))
        .map(|(key, value)| {
            if value.is_array() || value.is_object() {
                Err(CollectorError::SchemaChanged {
                    collector: "open-meteo",
                    detail: format!("current.{key} unexpectedly contains structured data"),
                })
            } else {
                Ok((key.clone(), value.clone()))
            }
        })
        .collect()
}

fn temperature_summary(current: &Map<String, Value>, units: Option<&Value>) -> Option<String> {
    let temperature = current.get("temperature_2m")?;
    let unit = units
        .and_then(Value::as_object)
        .and_then(|units| units.get("temperature_2m"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let apparent = current.get("apparent_temperature");
    Some(match apparent {
        Some(apparent) => format!("{temperature}{unit}, feels like {apparent}{unit}"),
        None => format!("{temperature}{unit}"),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_hourly_and_minutely_weather() {
        let items = parse_open_meteo(
            include_bytes!("../fixtures/open-meteo.json"),
            25.7699,
            -80.19005,
        )
        .unwrap();
        assert_eq!(items.len(), 8);
        assert_eq!(items[0].kind, ItemKind::WeatherCurrent);
        assert_eq!(items[1].kind, ItemKind::WeatherHourly);
        assert_eq!(items[1].attributes["precipitation_probability"], json!(70));

        let bins = items
            .iter()
            .filter(|item| item.kind == ItemKind::WeatherMinutely)
            .collect::<Vec<_>>();
        assert_eq!(bins.len(), 4);
        // Each bin is dated, which is the whole point: an hourly bucket cannot
        // say which quarter of the hour the rain arrives in.
        assert_eq!(bins[2].attributes["precipitation"], json!(0.6));
        assert_eq!(bins[2].attributes["units"]["precipitation"], json!("mm"));
        assert_ne!(bins[0].starts_at, bins[1].starts_at);
    }

    /// `minutely_15` coverage is region-dependent. A location without it must
    /// still get its hourly forecast rather than a schema error.
    #[test]
    fn a_response_without_minutely_bins_still_parses() {
        let mut without: Value =
            serde_json::from_slice(include_bytes!("../fixtures/open-meteo.json")).unwrap();
        let object = without.as_object_mut().unwrap();
        object.remove("minutely_15");
        object.remove("minutely_15_units");
        let items =
            parse_open_meteo(&serde_json::to_vec(&without).unwrap(), 25.7699, -80.19005).unwrap();
        assert_eq!(items.len(), 4);
        assert!(
            items
                .iter()
                .all(|item| item.kind != ItemKind::WeatherMinutely)
        );
    }

    /// Present but malformed is a different matter: a length mismatch means the
    /// bins cannot be paired with their times, and pairing them anyway would
    /// date rain to the wrong quarter-hour.
    #[test]
    fn ragged_minutely_bins_are_a_schema_error_rather_than_a_guess() {
        let mut ragged: Value =
            serde_json::from_slice(include_bytes!("../fixtures/open-meteo.json")).unwrap();
        ragged["minutely_15"]["precipitation"] = json!([0.0, 0.6]);
        assert!(
            parse_open_meteo(&serde_json::to_vec(&ragged).unwrap(), 25.7699, -80.19005).is_err()
        );
    }

    #[test]
    fn response_coordinates_must_be_finite_and_in_range() {
        assert!(validate_response_point(25.77, -80.19, 25.7699, -80.19005).is_ok());
        assert!(validate_response_point(91.0, -80.19, 25.7699, -80.19005).is_err());
        assert!(validate_response_point(25.77, f64::INFINITY, 25.7699, -80.19005).is_err());
    }

    #[test]
    fn response_coordinates_are_required_and_numeric() {
        let mut missing: Value =
            serde_json::from_slice(include_bytes!("../fixtures/open-meteo.json")).unwrap();
        missing.as_object_mut().unwrap().remove("latitude");
        assert!(matches!(
            parse_open_meteo(&serde_json::to_vec(&missing).unwrap(), 25.7699, -80.19005),
            Err(CollectorError::SchemaChanged { .. })
        ));

        let mut malformed: Value =
            serde_json::from_slice(include_bytes!("../fixtures/open-meteo.json")).unwrap();
        malformed["longitude"] = json!("-80.19");
        assert!(matches!(
            parse_open_meteo(&serde_json::to_vec(&malformed).unwrap(), 25.7699, -80.19005),
            Err(CollectorError::SchemaChanged { .. })
        ));
    }

    #[test]
    fn response_coordinates_must_match_the_requested_region() {
        let result = parse_open_meteo(
            include_bytes!("../fixtures/open-meteo.json"),
            40.7128,
            -74.006,
        );
        assert!(matches!(result, Err(CollectorError::SchemaChanged { .. })));
    }

    #[test]
    fn response_coordinate_binding_is_dateline_safe() {
        let mut response: Value =
            serde_json::from_slice(include_bytes!("../fixtures/open-meteo.json")).unwrap();
        response["longitude"] = json!(-179.9);
        let items =
            parse_open_meteo(&serde_json::to_vec(&response).unwrap(), 25.7699, 179.9).unwrap();

        assert_eq!(
            items[0].location.as_ref().and_then(|value| value.longitude),
            Some(-179.9)
        );
    }
}
