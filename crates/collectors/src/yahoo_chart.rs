use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use crate::{
    CollectContext, Collector, CollectorBatch, CollectorError, CollectorHealth, FetchLimits,
    HttpFetcher, ItemKind, SafeHttpFetcher, SourceLink,
};

const YAHOO_CHART_ROOT: &str = "https://query2.finance.yahoo.com/v8/finance/chart/";
const YAHOO_QUOTE_ROOT: &str = "https://finance.yahoo.com/quote/";
const MAX_SYMBOL_CHARS: usize = 32;
const MAX_LABEL_CHARS: usize = 64;
const MAX_USER_AGENT_CHARS: usize = 256;
/// Points kept for the drawn line. Enough to show the session's shape at the
/// sizes this is rendered at — a 250 px card and a 96 px panel figure — without
/// carrying a full tick history through the snapshot on every poll.
const SERIES_POINTS: usize = 32;

const MAX_BODY_BYTES: usize = 512 * 1024;

/// Trading session inferred from the provider's current trading-period bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketSession {
    Pre,
    Open,
    Post,
    Closed,
    Unknown,
}

impl MarketSession {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pre => "PRE",
            Self::Open => "OPEN",
            Self::Post => "POST",
            Self::Closed => "CLOSED",
            Self::Unknown => "SESSION N/A",
        }
    }
}

/// Configuration for one Yahoo Finance Chart quote.
///
/// The endpoint is intentionally isolated in this adapter: it is public and
/// credential-free, but undocumented and may change or refuse requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YahooChartConfig {
    pub symbol: String,
    pub label: String,
    pub user_agent: String,
}

impl YahooChartConfig {
    pub fn new(
        symbol: impl Into<String>,
        label: impl Into<String>,
        user_agent: impl Into<String>,
    ) -> Result<Self, CollectorError> {
        let config = Self {
            symbol: normalize_symbol(&symbol.into()),
            label: label.into().trim().to_owned(),
            user_agent: user_agent.into().trim().to_owned(),
        };
        validate_config(&config)?;
        Ok(config)
    }
}

pub struct YahooChartCollector {
    config: YahooChartConfig,
    endpoint: Url,
    fetcher: Arc<dyn HttpFetcher>,
}

impl YahooChartCollector {
    pub fn new(config: YahooChartConfig) -> Result<Self, CollectorError> {
        validate_config(&config)?;
        let limits = FetchLimits {
            timeout: Duration::from_secs(10),
            max_body_bytes: MAX_BODY_BYTES,
            max_redirects: 2,
            allow_http: false,
        };
        let fetcher = Arc::new(SafeHttpFetcher::new(config.user_agent.clone(), limits)?);
        Self::with_fetcher(config, fetcher)
    }

    pub fn with_fetcher(
        config: YahooChartConfig,
        fetcher: Arc<dyn HttpFetcher>,
    ) -> Result<Self, CollectorError> {
        validate_config(&config)?;
        let endpoint = chart_url(&config.symbol)?;
        Ok(Self {
            config,
            endpoint,
            fetcher,
        })
    }
}

#[async_trait]
impl Collector for YahooChartCollector {
    fn name(&self) -> &'static str {
        "yahoo-chart"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        let response = self
            .fetcher
            .get(
                &self.endpoint,
                context.cursor.as_ref(),
                &[("accept", "application/json")],
            )
            .await?;
        if response.not_modified {
            return Ok(CollectorBatch {
                source: self.name().into(),
                items: Vec::new(),
                health: health_for_delay(cursor_delay(context.cursor.as_ref())),
                cursor: response.cursor,
                not_modified: true,
            });
        }

        let item = parse_yahoo_chart(
            &response.body,
            &self.config.symbol,
            &self.config.label,
            Utc::now(),
        )?;
        let delay = item
            .attributes
            .get("provider_delay_minutes")
            .and_then(Value::as_u64);
        let health = health_for_delay(Some(delay));
        let mut cursor = response.cursor;
        cursor.metadata.insert(
            "yahoo_provider_delay_minutes".into(),
            delay.map_or_else(|| "not_reported".into(), |minutes| minutes.to_string()),
        );
        Ok(CollectorBatch {
            source: self.name().into(),
            items: vec![item],
            health,
            cursor,
            not_modified: false,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ChartEnvelope {
    chart: Option<ChartBody>,
}

#[derive(Debug, Deserialize)]
struct ChartBody {
    result: Option<Vec<ChartResult>>,
    error: Option<ChartApiError>,
}

#[derive(Debug, Deserialize)]
struct ChartApiError {
    code: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChartResult {
    meta: Option<ChartMeta>,
    timestamp: Option<Vec<i64>>,
    indicators: Option<ChartIndicators>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartMeta {
    symbol: Option<String>,
    currency: Option<String>,
    exchange_name: Option<String>,
    full_exchange_name: Option<String>,
    regular_market_price: Option<f64>,
    chart_previous_close: Option<f64>,
    previous_close: Option<f64>,
    regular_market_time: Option<i64>,
    regular_market_day_high: Option<f64>,
    regular_market_day_low: Option<f64>,
    regular_market_volume: Option<f64>,
    exchange_data_delayed_by: Option<u64>,
    current_trading_period: Option<TradingPeriods>,
}

#[derive(Debug, Deserialize)]
struct ChartIndicators {
    quote: Option<Vec<QuoteSeries>>,
}

#[derive(Debug, Deserialize)]
struct QuoteSeries {
    close: Option<Vec<Option<f64>>>,
}

#[derive(Debug, Deserialize)]
struct TradingPeriods {
    pre: Option<TradingPeriod>,
    regular: Option<TradingPeriod>,
    post: Option<TradingPeriod>,
}

#[derive(Debug, Deserialize)]
struct TradingPeriod {
    start: Option<i64>,
    end: Option<i64>,
}

/// Parses one bounded Yahoo Chart response into the shared observation model.
///
/// `now` is supplied by the caller so session classification remains fully
/// deterministic in offline fixture tests.
pub fn parse_yahoo_chart(
    body: &[u8],
    configured_symbol: &str,
    configured_label: &str,
    now: DateTime<Utc>,
) -> Result<crate::CollectorItem, CollectorError> {
    if body.len() > MAX_BODY_BYTES {
        return Err(CollectorError::Parse {
            collector: "yahoo-chart",
            detail: format!("response exceeds the {MAX_BODY_BYTES}-byte parser limit"),
        });
    }
    let symbol = normalize_symbol(configured_symbol);
    let config = YahooChartConfig::new(&symbol, configured_label, "fixture-parser")?;
    let envelope: ChartEnvelope =
        serde_json::from_slice(body).map_err(|error| CollectorError::Parse {
            collector: "yahoo-chart",
            detail: error.to_string(),
        })?;
    let chart = envelope
        .chart
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "yahoo-chart",
            detail: "chart object is missing".into(),
        })?;
    if let Some(error) = chart.error {
        return Err(CollectorError::Parse {
            collector: "yahoo-chart",
            detail: format!(
                "provider error {}: {}",
                error.code.as_deref().unwrap_or("unknown"),
                error.description.as_deref().unwrap_or("no description")
            ),
        });
    }
    let mut results = chart.result.ok_or_else(|| CollectorError::SchemaChanged {
        collector: "yahoo-chart",
        detail: "chart.result is missing".into(),
    })?;
    if results.len() != 1 {
        return Err(CollectorError::SchemaChanged {
            collector: "yahoo-chart",
            detail: format!("expected one chart result, received {}", results.len()),
        });
    }
    let result = results.pop().expect("length checked above");
    let meta = result.meta.ok_or_else(|| CollectorError::SchemaChanged {
        collector: "yahoo-chart",
        detail: "chart.result[0].meta is missing".into(),
    })?;

    let closes = result
        .indicators
        .as_ref()
        .and_then(|indicators| indicators.quote.as_ref())
        .and_then(|quotes| quotes.first())
        .and_then(|quote| quote.close.as_deref());
    let series = price_series(closes);
    let last_sample = last_series_sample(
        result.timestamp.as_deref(),
        result
            .indicators
            .as_ref()
            .and_then(|indicators| indicators.quote.as_ref())
            .and_then(|quotes| quotes.first())
            .and_then(|quote| quote.close.as_deref()),
    );
    let price = required_finite(
        meta.regular_market_price
            .or_else(|| last_sample.map(|(_, value)| value)),
        "regularMarketPrice",
    )?;
    let previous_close = required_finite(
        meta.chart_previous_close.or(meta.previous_close),
        "chartPreviousClose/previousClose",
    )?;
    if previous_close == 0.0 {
        return Err(CollectorError::SchemaChanged {
            collector: "yahoo-chart",
            detail: "previous close is zero; percent change cannot be computed".into(),
        });
    }
    let observed_epoch = meta
        .regular_market_time
        .or_else(|| last_sample.map(|(timestamp, _)| timestamp))
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "yahoo-chart",
            detail: "regularMarketTime and timestamp series are both missing".into(),
        })?;
    let observed_at = DateTime::<Utc>::from_timestamp(observed_epoch, 0).ok_or_else(|| {
        CollectorError::SchemaChanged {
            collector: "yahoo-chart",
            detail: "regularMarketTime is outside the supported timestamp range".into(),
        }
    })?;
    let session = market_session(meta.current_trading_period.as_ref(), now.timestamp());
    let change = price - previous_close;
    let change_percent = change / previous_close * 100.0;
    if !change.is_finite() || !change_percent.is_finite() {
        return Err(CollectorError::SchemaChanged {
            collector: "yahoo-chart",
            detail: "computed quote change is not finite".into(),
        });
    }

    let provider_symbol = meta
        .symbol
        .as_deref()
        .map(normalize_symbol)
        .filter(|value| valid_symbol(value))
        .unwrap_or_else(|| symbol.clone());
    let mut attributes = BTreeMap::from([
        ("symbol".into(), json!(provider_symbol)),
        ("label".into(), json!(config.label)),
        ("price".into(), json!(price)),
        ("previous_close".into(), json!(previous_close)),
        ("change".into(), json!(change)),
        ("change_percent".into(), json!(change_percent)),
        ("session".into(), json!(session)),
        ("session_label".into(), json!(session.label())),
        (
            "quote_age_seconds_at_collection".into(),
            json!(now.timestamp().saturating_sub(observed_epoch).max(0)),
        ),
        (
            "price_basis".into(),
            json!("regular market price versus previous close"),
        ),
    ]);
    // The session's shape, not just its endpoints. A number tells you the stock
    // moved; the line tells you whether it climbed all day or gave it all back
    // after lunch, which is the part a glance actually reads.
    if !series.is_empty() {
        attributes.insert("series".into(), json!(series));
    }
    insert_text(&mut attributes, "currency", meta.currency, 16);
    insert_text(
        &mut attributes,
        "exchange",
        meta.full_exchange_name.or(meta.exchange_name),
        96,
    );
    insert_finite(&mut attributes, "day_high", meta.regular_market_day_high);
    insert_finite(&mut attributes, "day_low", meta.regular_market_day_low);
    insert_finite(&mut attributes, "volume", meta.regular_market_volume);
    if let Some(delay) = meta.exchange_data_delayed_by {
        attributes.insert("provider_delay_minutes".into(), json!(delay));
        attributes.insert("delay_semantics".into(), json!("provider_reported"));
    } else {
        attributes.insert("delay_semantics".into(), json!("not_reported"));
    }

    let delay_suffix = meta.exchange_data_delayed_by.map_or_else(
        || "delay not reported".into(),
        |minutes| {
            if minutes == 0 {
                "provider reports real time".into()
            } else {
                format!("provider reports {minutes} min delay")
            }
        },
    );
    let currency = attributes
        .get("currency")
        .and_then(Value::as_str)
        .unwrap_or("");
    let currency_suffix = if currency.is_empty() {
        String::new()
    } else {
        format!(" {currency}")
    };
    Ok(crate::CollectorItem {
        id: format!("yahoo-chart:{provider_symbol}"),
        kind: ItemKind::MarketQuote,
        title: config.label,
        summary: Some(format!(
            "{price:.2}{currency_suffix} · {change_percent:+.2}% · {} · {delay_suffix}",
            session.label()
        )),
        observed_at: Some(observed_at),
        starts_at: None,
        ends_at: None,
        location: None,
        source: SourceLink {
            name: "Yahoo Finance chart · unofficial".into(),
            url: Some(quote_url(&provider_symbol)?),
        },
        attributes,
    })
}

fn market_session(periods: Option<&TradingPeriods>, now_epoch: i64) -> MarketSession {
    let Some(periods) = periods else {
        return MarketSession::Unknown;
    };
    let mut has_boundary = false;
    for (period, session) in [
        (periods.pre.as_ref(), MarketSession::Pre),
        (periods.regular.as_ref(), MarketSession::Open),
        (periods.post.as_ref(), MarketSession::Post),
    ] {
        let Some((start, end)) = period.and_then(valid_period) else {
            continue;
        };
        has_boundary = true;
        if (start..=end).contains(&now_epoch) {
            return session;
        }
    }
    if has_boundary {
        MarketSession::Closed
    } else {
        MarketSession::Unknown
    }
}

fn valid_period(period: &TradingPeriod) -> Option<(i64, i64)> {
    let start = period.start?;
    let end = period.end?;
    (start <= end).then_some((start, end))
}

/// Closes across the session, oldest first, downsampled for drawing.
///
/// Gaps are dropped rather than interpolated: a halted or thin symbol has
/// missing prints, and inventing values to keep the line smooth would draw a
/// price that never traded. The result is a shape, not a record — anything that
/// needs exact values reads `price` and `previous_close`.
fn price_series(closes: Option<&[Option<f64>]>) -> Vec<f64> {
    let values = closes
        .unwrap_or_default()
        .iter()
        .filter_map(|value| *value)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect::<Vec<_>>();
    if values.len() <= SERIES_POINTS {
        return values;
    }
    // Even stride across the whole session, always keeping the last print so
    // the line ends where the quoted price does.
    let stride = values.len() as f64 / SERIES_POINTS as f64;
    let mut sampled = (0..SERIES_POINTS)
        .map(|index| values[((index as f64 * stride) as usize).min(values.len() - 1)])
        .collect::<Vec<_>>();
    if let (Some(last), Some(final_value)) = (sampled.last_mut(), values.last()) {
        *last = *final_value;
    }
    sampled
}

fn last_series_sample(
    timestamps: Option<&[i64]>,
    closes: Option<&[Option<f64>]>,
) -> Option<(i64, f64)> {
    timestamps?
        .iter()
        .copied()
        .zip(closes?.iter().copied())
        .rev()
        .find_map(|(timestamp, value)| {
            value
                .filter(|value| value.is_finite())
                .map(|value| (timestamp, value))
        })
}

fn required_finite(value: Option<f64>, field: &str) -> Result<f64, CollectorError> {
    value
        .filter(|value| value.is_finite())
        .ok_or_else(|| CollectorError::SchemaChanged {
            collector: "yahoo-chart",
            detail: format!("{field} is missing or not finite"),
        })
}

fn insert_text(
    attributes: &mut BTreeMap<String, Value>,
    key: &str,
    value: Option<String>,
    max_chars: usize,
) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        attributes.insert(
            key.into(),
            json!(value.trim().chars().take(max_chars).collect::<String>()),
        );
    }
}

fn insert_finite(attributes: &mut BTreeMap<String, Value>, key: &str, value: Option<f64>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        attributes.insert(key.into(), json!(value));
    }
}

fn validate_config(config: &YahooChartConfig) -> Result<(), CollectorError> {
    if !valid_symbol(&config.symbol) {
        return Err(CollectorError::Configuration(format!(
            "Yahoo symbol must be 1-{MAX_SYMBOL_CHARS} ASCII letters, digits, or .^=_-/"
        )));
    }
    if !(1..=MAX_LABEL_CHARS).contains(&config.label.chars().count()) {
        return Err(CollectorError::Configuration(format!(
            "Yahoo label must be 1-{MAX_LABEL_CHARS} characters"
        )));
    }
    if !(1..=MAX_USER_AGENT_CHARS).contains(&config.user_agent.chars().count())
        || config.user_agent.chars().any(char::is_control)
    {
        return Err(CollectorError::Configuration(format!(
            "Yahoo User-Agent must be 1-{MAX_USER_AGENT_CHARS} printable characters"
        )));
    }
    Ok(())
}

fn valid_symbol(symbol: &str) -> bool {
    (1..=MAX_SYMBOL_CHARS).contains(&symbol.chars().count())
        && symbol.chars().all(valid_symbol_character)
}

fn cursor_delay(cursor: Option<&crate::CollectorCursor>) -> Option<Option<u64>> {
    cursor
        .and_then(|cursor| cursor.metadata.get("yahoo_provider_delay_minutes"))
        .map(|value| value.parse::<u64>().ok())
}

fn health_for_delay(delay: Option<Option<u64>>) -> CollectorHealth {
    match delay {
        Some(Some(0)) => CollectorHealth::healthy(),
        Some(Some(minutes)) => CollectorHealth {
            state: crate::HealthState::Degraded,
            checked_at: Utc::now(),
            message: Some(format!(
                "Yahoo reports this quote is delayed by {minutes} minute(s)"
            )),
        },
        Some(None) | None => CollectorHealth {
            state: crate::HealthState::Unknown,
            checked_at: Utc::now(),
            message: Some("Yahoo did not report exchange delay metadata".into()),
        },
    }
}

fn normalize_symbol(symbol: &str) -> String {
    symbol.trim().to_ascii_uppercase()
}

fn valid_symbol_character(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '.' | '^' | '=' | '_' | '-' | '/')
}

fn chart_url(symbol: &str) -> Result<Url, CollectorError> {
    let mut url = Url::parse(YAHOO_CHART_ROOT).expect("constant Yahoo Chart URL is valid");
    url.path_segments_mut()
        .map_err(|()| CollectorError::Configuration("Yahoo Chart URL cannot hold a path".into()))?
        // A URL ending in "/" already carries a final empty segment, so pushing
        // onto it produces ".../chart//AMD" and a 404 from every request the app
        // has ever made. Drop the empty one first.
        .pop_if_empty()
        .push(symbol);
    url.query_pairs_mut()
        .append_pair("range", "1d")
        .append_pair("interval", "5m")
        .append_pair("includePrePost", "true");
    Ok(url)
}

fn quote_url(symbol: &str) -> Result<Url, CollectorError> {
    let mut url = Url::parse(YAHOO_QUOTE_ROOT).expect("constant Yahoo quote URL is valid");
    url.path_segments_mut()
        .map_err(|()| CollectorError::Configuration("Yahoo quote URL cannot hold a path".into()))?
        .pop_if_empty()
        .push(symbol);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureFetcher;

    #[async_trait]
    impl HttpFetcher for FixtureFetcher {
        async fn get(
            &self,
            url: &Url,
            _cursor: Option<&crate::CollectorCursor>,
            headers: &[(&str, &str)],
        ) -> Result<crate::FetchResponse, CollectorError> {
            assert_eq!(url.host_str(), Some("query2.finance.yahoo.com"));
            assert!(url.path().ends_with("/AMD"));
            assert_eq!(headers, &[("accept", "application/json")]);
            Ok(crate::FetchResponse {
                status: 200,
                final_url: url.clone(),
                body: include_bytes!("../fixtures/yahoo-chart-amd.json").to_vec(),
                cursor: Default::default(),
                not_modified: false,
                content_type: Some("application/json".into()),
            })
        }
    }

    fn fixture_now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_786_741_200, 0).unwrap()
    }

    #[test]
    fn parses_price_change_session_and_provider_delay_without_live_io() {
        let item = parse_yahoo_chart(
            include_bytes!("../fixtures/yahoo-chart-amd.json"),
            "amd",
            "AMD",
            fixture_now(),
        )
        .unwrap();

        assert_eq!(item.kind, ItemKind::MarketQuote);
        assert_eq!(item.id, "yahoo-chart:AMD");
        assert_eq!(item.attributes["price"], json!(172.4));
        assert_eq!(item.attributes["previous_close"], json!(161.94));
        assert_eq!(item.attributes["session"], json!("open"));
        assert_eq!(item.attributes["provider_delay_minutes"], json!(15));
        assert!(item.attributes["change_percent"].as_f64().unwrap() > 6.4);
        assert!(item.summary.as_deref().unwrap().contains("15 min delay"));
    }

    #[tokio::test]
    async fn collector_normalizes_fixture_and_retains_delay_cursor_without_live_io() {
        let config = YahooChartConfig::new(
            "AMD",
            "AMD",
            "PuenteGonorrea fixture (+https://example.invalid)",
        )
        .unwrap();
        let collector =
            YahooChartCollector::with_fetcher(config, Arc::new(FixtureFetcher)).unwrap();
        let batch = collector.collect(&CollectContext::default()).await.unwrap();

        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.health.state, crate::HealthState::Degraded);
        assert_eq!(batch.cursor.metadata["yahoo_provider_delay_minutes"], "15");
        assert!(!batch.not_modified);
    }

    #[test]
    fn falls_back_to_the_last_finite_series_sample() {
        let body = br#"{
          "chart":{"result":[{"meta":{
            "symbol":"TEST","chartPreviousClose":100.0,"exchangeDataDelayedBy":0,
            "currentTradingPeriod":{"regular":{"start":1786730000,"end":1786760000}}
          },"timestamp":[1786741100,1786741200],
          "indicators":{"quote":[{"close":[99.5,101.25]}]}}],"error":null}
        }"#;
        let item = parse_yahoo_chart(body, "TEST", "Test", fixture_now()).unwrap();
        assert_eq!(item.attributes["price"], json!(101.25));
        assert_eq!(item.observed_at.unwrap().timestamp(), 1_786_741_200);
    }

    #[test]
    fn missing_previous_close_is_an_error_not_a_zero() {
        let body = br#"{
          "chart":{"result":[{"meta":{
            "symbol":"TEST","regularMarketPrice":100.0,"regularMarketTime":1786741200
          }}],"error":null}
        }"#;
        let error = parse_yahoo_chart(body, "TEST", "Test", fixture_now()).unwrap_err();
        assert!(error.to_string().contains("previousClose"));
    }

    #[test]
    fn zero_previous_close_is_rejected() {
        let body = br#"{
          "chart":{"result":[{"meta":{
            "symbol":"TEST","regularMarketPrice":100.0,"chartPreviousClose":0.0,
            "regularMarketTime":1786741200
          }}],"error":null}
        }"#;
        let error = parse_yahoo_chart(body, "TEST", "Test", fixture_now()).unwrap_err();
        assert!(error.to_string().contains("previous close is zero"));
    }

    #[test]
    fn symbols_and_payloads_are_bounded() {
        assert!(YahooChartConfig::new("BRK/B", "Berkshire", "fixture").is_ok());
        assert!(YahooChartConfig::new("DROP TABLE;", "bad", "fixture").is_err());
        let oversized = vec![b' '; MAX_BODY_BYTES + 1];
        let error = parse_yahoo_chart(&oversized, "AMD", "AMD", fixture_now()).unwrap_err();
        assert!(error.to_string().contains("parser limit"));
    }

    #[test]
    fn symbol_is_encoded_as_one_path_segment() {
        let url = chart_url("BRK/B").unwrap();
        assert_eq!(url.host_str(), Some("query2.finance.yahoo.com"));
        assert!(url.as_str().contains("BRK%2FB"));
        assert_eq!(
            url.query_pairs().find(|(key, _)| key == "range").unwrap().1,
            "1d"
        );
    }

    #[test]
    fn delay_semantics_survive_conditional_not_modified_responses() {
        let cursor = crate::CollectorCursor {
            metadata: BTreeMap::from([("yahoo_provider_delay_minutes".into(), "15".into())]),
            ..Default::default()
        };
        let health = health_for_delay(cursor_delay(Some(&cursor)));
        assert_eq!(health.state, crate::HealthState::Degraded);
        assert!(health.message.as_deref().unwrap().contains("15 minute"));
    }

    /// The line is a shape, not a record. It has to end where the quote does,
    /// stay inside the drawing budget, and never invent a price that did not
    /// trade.
    #[test]
    fn the_price_series_is_downsampled_without_inventing_prices() {
        let raw = (0..200)
            .map(|index| Some(100.0 + f64::from(index)))
            .collect::<Vec<_>>();
        let series = price_series(Some(&raw));
        assert_eq!(series.len(), SERIES_POINTS);
        assert_eq!(series.first().copied(), Some(100.0));
        // The last point is the last print, so the line ends at the quote.
        assert_eq!(series.last().copied(), Some(299.0));
        // Every drawn value is a value that actually appeared.
        for value in &series {
            assert!(raw.contains(&Some(*value)), "{value} was never traded");
        }
    }

    /// A halted or thin symbol has missing prints. Interpolating them would
    /// draw prices that never existed; dropping them draws a shorter line.
    #[test]
    fn gaps_are_dropped_rather_than_filled() {
        let series = price_series(Some(&[Some(10.0), None, Some(12.0), None, Some(11.0)]));
        assert_eq!(series, vec![10.0, 12.0, 11.0]);
    }

    #[test]
    fn nonsense_samples_never_reach_the_line() {
        let series = price_series(Some(&[
            Some(10.0),
            Some(f64::NAN),
            Some(-4.0),
            Some(0.0),
            Some(f64::INFINITY),
            Some(11.0),
        ]));
        assert_eq!(series, vec![10.0, 11.0]);
        assert!(price_series(None).is_empty());
    }

    #[test]
    fn a_short_session_is_kept_whole() {
        let raw = [Some(1.0), Some(2.0), Some(3.0)];
        assert_eq!(price_series(Some(&raw)), vec![1.0, 2.0, 3.0]);
    }

    /// The exact URL, not just its query. A trailing slash on the base made
    /// `push` append after the empty final segment, so every request the app
    /// ever sent went to ".../chart//AMD" and came back 404. The old test only
    /// checked the query pairs, which were fine.
    #[test]
    fn the_chart_url_has_no_empty_path_segment() {
        assert_eq!(
            chart_url("AMD").unwrap().as_str(),
            "https://query2.finance.yahoo.com/v8/finance/chart/AMD\
             ?range=1d&interval=5m&includePrePost=true"
        );
        assert!(!chart_url("AMD").unwrap().path().contains("//"));
        assert_eq!(
            chart_url("BRK-B").unwrap().path(),
            "/v8/finance/chart/BRK-B"
        );
        // Index symbols land in one segment. Yahoo accepts the caret raw or
        // percent-encoded; what it will not accept is an empty segment.
        assert_eq!(
            chart_url("^GSPC").unwrap().path(),
            "/v8/finance/chart/^GSPC"
        );
    }

    #[test]
    fn the_quote_url_has_no_empty_path_segment() {
        assert_eq!(
            quote_url("AMD").unwrap().as_str(),
            "https://finance.yahoo.com/quote/AMD"
        );
        assert!(!quote_url("AMD").unwrap().path().contains("//"));
    }
}
