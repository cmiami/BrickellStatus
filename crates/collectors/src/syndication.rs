use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use async_trait::async_trait;
use feed_rs::parser;
use scraper::Html;
use serde_json::json;
use url::Url;

#[cfg(feature = "native")]
use crate::SafeHttpFetcher;
use crate::{
    CollectContext, Collector, CollectorBatch, CollectorError, CollectorItem, FetchLimits,
    HttpFetcher, ItemKind, SourceLink, collection::collect_http, validate_public_url,
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
            user_agent: "BrickellStatus/0.1 (+https://github.com/cmiami/BrickellStatus)".into(),
            fetch_limits: FetchLimits::default(),
        }
    }
}

pub struct SyndicationCollector {
    config: SyndicationConfig,
    fetcher: Arc<dyn HttpFetcher>,
}

impl SyndicationCollector {
    /// Constructs the collector with the built-in network client.
    ///
    /// Native only: a Worker has no socket to give this, and supplies its own
    /// fetcher through [`Self::with_fetcher`] instead.
    #[cfg(feature = "native")]
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
    // An explicitly configured name is the user's word and outranks anything
    // we could infer from the items.
    let unaggregate = is_google_news(feed_url)
        && configured_source_name.is_none_or(|name| name.trim().is_empty());

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
            // Lift the publisher out of the headline and into the field built
            // to hold it. The card prints the source on its own line, so
            // leaving it appended would show the name twice and cost the
            // headline the characters.
            let (title, item_source_name) = match unaggregate
                .then(|| split_aggregated_title(&title))
                .flatten()
            {
                Some((headline, publisher)) => (headline.to_owned(), publisher.to_owned()),
                None => (title, source_name.clone()),
            };
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
                    name: item_source_name,
                    url: link.or_else(|| Some(feed_url.clone())),
                },
                attributes,
            })
        })
        .collect()
}

/// Whether this feed is a Google News edition or search.
///
/// Google News is an aggregator, so its feed title is the query rather than a
/// publisher, and every headline carries the real publisher appended after a
/// spaced hyphen. Left alone, six Miami team feeds would all attribute
/// themselves to "Google News" and burn a quarter of a 36-character panel line
/// repeating the publisher the card already has a slot for.
fn is_google_news(feed_url: &Url) -> bool {
    feed_url
        .host_str()
        .is_some_and(|host| host == "news.google.com")
}

/// Splits `Headline - Publisher` into its two halves.
///
/// This has to be a judgement rather than a lookup: feed-rs 2.4 does not
/// surface the RSS `<source>` element, so the publisher Google states per item
/// never reaches us and the title is the only place the name appears.
///
/// The risk being managed is a headline that contains a spaced hyphen of its
/// own — "Butler on the ruthless standard - and what it cost him" must come
/// back whole. What separates the two is how the tail opens: a masthead is a
/// name ("Miami Herald") or a domain ("heavy.com"), while a continuing clause
/// starts mid-sentence in lower case. When the tail does not look like a name,
/// nothing is split and the headline keeps every word.
fn split_aggregated_title(title: &str) -> Option<(&str, &str)> {
    const MAX_PUBLISHER_WORDS: usize = 6;
    const MAX_PUBLISHER_CHARS: usize = 48;

    let (headline, publisher) = title.rsplit_once(" - ")?;
    let (headline, publisher) = (headline.trim(), publisher.trim());
    if headline.is_empty() || publisher.is_empty() {
        return None;
    }
    if publisher.chars().count() > MAX_PUBLISHER_CHARS
        || publisher.split_whitespace().count() > MAX_PUBLISHER_WORDS
        || publisher.ends_with(['.', '?', '!', ','])
    {
        return None;
    }
    let opens_like_a_name = publisher
        .chars()
        .next()
        .is_some_and(|character| character.is_uppercase() || character.is_numeric());
    // A bare domain is a masthead too, and those are conventionally lower case.
    let reads_like_a_domain = publisher.contains('.') && !publisher.contains(' ');
    (opens_like_a_name || reads_like_a_domain).then_some((headline, publisher))
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
    fn google_news_items_are_attributed_to_the_publisher_that_filed_them() {
        let url = Url::parse(
            "https://news.google.com/rss/search?q=%22Miami+Heat%22&hl=en-US&gl=US&ceid=US:en",
        )
        .unwrap();
        let items = parse_syndication(
            include_bytes!("../fixtures/google-news-search.xml"),
            None,
            &url,
            10,
        )
        .unwrap();
        assert_eq!(items.len(), 4);

        // The suffix moves out of the headline and into the source field the
        // card already prints on its own line.
        assert_eq!(
            items[0].title,
            "Heat just got intriguing Klay Thompson update they desperately needed"
        );
        assert_eq!(items[0].source.name, "All U Can Heat");
        assert_eq!(items[2].title, "What is next for the roster?");
        assert_eq!(items[2].source.name, "Miami Herald");

        // A headline with no publisher suffix keeps every word, and falls back
        // to the feed's own name rather than losing its tail to the split.
        assert_eq!(
            items[3].title,
            "Butler on the ruthless standard - and what it cost him"
        );
        assert_eq!(items[3].source.name, "\"Miami Heat\" - Google News");

        // Nothing is left attributed to the aggregator itself.
        assert!(
            items[..3]
                .iter()
                .all(|item| item.source.name != "Google News"),
            "an aggregator name is not a publisher"
        );
    }

    #[test]
    fn a_configured_name_outranks_the_aggregator_split() {
        let url = Url::parse("https://news.google.com/rss/search?q=test").unwrap();
        let items = parse_syndication(
            include_bytes!("../fixtures/google-news-search.xml"),
            Some("My sports desk"),
            &url,
            10,
        )
        .unwrap();
        assert!(
            items
                .iter()
                .all(|item| item.source.name == "My sports desk")
        );
        // The headline keeps its suffix, because the user asked for one name
        // and splitting would contradict it.
        assert!(items[0].title.ends_with(" - All U Can Heat"));
    }

    #[test]
    fn ordinary_feeds_keep_hyphenated_headlines_intact() {
        // The split is aggregator-only. A publisher feed whose headline happens
        // to contain " - " must not lose its tail.
        let url = Url::parse("https://example.com/feed.xml").unwrap();
        let items = parse_syndication(include_bytes!("../fixtures/sample-rss.xml"), None, &url, 10)
            .unwrap();
        assert_eq!(items[0].source.name, "Miami Signals");
    }

    #[test]
    fn unsafe_feed_urls_fail_during_configuration() {
        let result = SyndicationCollector::new(SyndicationConfig::new(
            Url::parse("https://127.0.0.1/feed").unwrap(),
        ));
        assert!(matches!(result, Err(CollectorError::UnsafeUrl(_))));
    }
}
