use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use async_trait::async_trait;
use feed_rs::parser;
use scraper::Html;
use serde_json::json;
use url::Url;

use crate::{
    CollectContext, Collector, CollectorBatch, CollectorError, CollectorItem, FetchLimits,
    HttpFetcher, ItemKind, SafeHttpFetcher, SourceLink, collection::collect_http,
    validate_public_url,
};

#[derive(Clone, Debug)]
pub struct SyndicationConfig {
    pub url: Url,
    pub source_name: Option<String>,
    pub max_items: usize,
    pub user_agent: String,
    pub fetch_limits: FetchLimits,
}

impl SyndicationConfig {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            source_name: None,
            max_items: 50,
            user_agent: "PuenteGonorrea/0.1 (+https://github.com/cmiami/PuenteGonorrea)".into(),
            fetch_limits: FetchLimits::default(),
        }
    }
}

pub struct SyndicationCollector {
    config: SyndicationConfig,
    fetcher: Arc<dyn HttpFetcher>,
}

impl SyndicationCollector {
    pub fn new(config: SyndicationConfig) -> Result<Self, CollectorError> {
        validate_config(&config)?;
        let fetcher = Arc::new(SafeHttpFetcher::new(
            config.user_agent.clone(),
            config.fetch_limits.clone(),
        )?);
        Ok(Self { config, fetcher })
    }

    pub fn with_fetcher(
        config: SyndicationConfig,
        fetcher: Arc<dyn HttpFetcher>,
    ) -> Result<Self, CollectorError> {
        validate_config(&config)?;
        Ok(Self { config, fetcher })
    }
}

fn validate_config(config: &SyndicationConfig) -> Result<(), CollectorError> {
    validate_public_url(&config.url, config.fetch_limits.allow_http)?;
    if config.max_items == 0 || config.max_items > 500 {
        return Err(CollectorError::Configuration(
            "RSS/Atom max_items must be between 1 and 500".into(),
        ));
    }
    if config.user_agent.trim().is_empty() {
        return Err(CollectorError::Configuration(
            "RSS/Atom User-Agent cannot be empty".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl Collector for SyndicationCollector {
    fn name(&self) -> &'static str {
        "syndication"
    }

    async fn collect(&self, context: &CollectContext) -> Result<CollectorBatch, CollectorError> {
        collect_http(
            self.name(),
            self.fetcher.as_ref(),
            &self.config.url,
            context,
            &[(
                "accept",
                "application/atom+xml, application/rss+xml, application/xml;q=0.9, text/xml;q=0.8",
            )],
            |response| {
                parse_syndication(
                    &response.body,
                    self.config.source_name.as_deref(),
                    &response.final_url,
                    self.config.max_items,
                )
            },
        )
        .await
    }
}

pub fn parse_syndication(
    body: &[u8],
    configured_source_name: Option<&str>,
    feed_url: &Url,
    max_items: usize,
) -> Result<Vec<CollectorItem>, CollectorError> {
    let feed = parser::parse(Cursor::new(body)).map_err(|error| CollectorError::Parse {
        collector: "rss-atom",
        detail: error.to_string(),
    })?;
    let feed_title = feed.title.as_ref().map(|title| title.content.trim());
    let source_name = configured_source_name
        .filter(|name| !name.trim().is_empty())
        .or(feed_title.filter(|title| !title.is_empty()))
        .or_else(|| feed_url.host_str())
        .unwrap_or("RSS/Atom feed")
        .to_owned();

    feed.entries
        .into_iter()
        .take(max_items)
        .enumerate()
        .map(|(index, entry)| {
            let link = entry
                .links
                .iter()
                .find(|link| link.rel.as_deref().is_none_or(|rel| rel == "alternate"))
                .or_else(|| entry.links.first())
                .and_then(|link| safe_link(&link.href));
            let raw_id = if entry.id.trim().is_empty() {
                link.as_ref()
                    .map(Url::as_str)
                    .ok_or_else(|| CollectorError::SchemaChanged {
                        collector: "rss-atom",
                        detail: format!("entry {index} has neither an id nor a usable link"),
                    })?
            } else {
                entry.id.as_str()
            };
            let title = entry
                .title
                .as_ref()
                .map(|title| plain_text(&title.content, 300))
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| "Untitled update".into());
            let summary = entry
                .summary
                .as_ref()
                .map(|summary| plain_text(&summary.content, 2_000))
                .or_else(|| {
                    entry.content.as_ref().and_then(|content| {
                        content.body.as_deref().map(|body| plain_text(body, 2_000))
                    })
                })
                .filter(|summary| !summary.is_empty());
            let mut attributes = BTreeMap::new();
            if !entry.authors.is_empty() {
                attributes.insert(
                    "authors".into(),
                    json!(
                        entry
                            .authors
                            .iter()
                            .map(|author| &author.name)
                            .collect::<Vec<_>>()
                    ),
                );
            }
            if !entry.categories.is_empty() {
                attributes.insert(
                    "categories".into(),
                    json!(
                        entry
                            .categories
                            .iter()
                            .map(|category| &category.term)
                            .collect::<Vec<_>>()
                    ),
                );
            }
            Ok(CollectorItem {
                id: format!("feed:{raw_id}"),
                kind: ItemKind::News,
                title,
                summary,
                observed_at: entry.updated.or(entry.published),
                starts_at: None,
                ends_at: None,
                location: None,
                source: SourceLink {
                    name: source_name.clone(),
                    url: link.or_else(|| Some(feed_url.clone())),
                },
                attributes,
            })
        })
        .collect()
}

fn safe_link(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    matches!(url.scheme(), "https" | "http").then_some(url)
}

fn plain_text(value: &str, max_chars: usize) -> String {
    let fragment = Html::parse_fragment(value);
    let normalized = fragment
        .root_element()
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut output: String = normalized.chars().take(max_chars).collect();
    if normalized.chars().count() > max_chars {
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rss_and_strips_markup() {
        let url = Url::parse("https://example.com/feed.xml").unwrap();
        let items = parse_syndication(include_bytes!("../fixtures/sample-rss.xml"), None, &url, 10)
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Bridge maintenance update");
        assert_eq!(
            items[0].summary.as_deref(),
            Some("Work is complete & lanes are open.")
        );
        assert_eq!(items[0].source.name, "Miami Signals");
    }

    #[test]
    fn parses_atom() {
        let url = Url::parse("https://example.com/atom.xml").unwrap();
        let items = parse_syndication(
            include_bytes!("../fixtures/sample-atom.xml"),
            Some("Configured name"),
            &url,
            10,
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source.name, "Configured name");
        assert_eq!(items[0].attributes["authors"], json!(["Signals Desk"]));
    }

    #[test]
    fn unsafe_feed_urls_fail_during_configuration() {
        let result = SyndicationCollector::new(SyndicationConfig::new(
            Url::parse("https://127.0.0.1/feed").unwrap(),
        ));
        assert!(matches!(result, Err(CollectorError::UnsafeUrl(_))));
    }
}
