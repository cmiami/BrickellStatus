//! The shipped directory of news and sports feeds a user can subscribe to.
//!
//! Before this existed the only way onto the panel was pasting a URL you had
//! already found somewhere else. The catalog is the tested alternative: every
//! entry was fetched and confirmed to answer with current items before it was
//! written down.
//!
//! `catalog/feeds.json` is the single source of truth. Rust embeds it here and
//! the console imports the same file, because the two used to drift — the
//! earthquake editor still offers a feed window its own validator rejects, and
//! picking it makes preferences unsaveable. One file cannot drift from itself.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::dto::ChannelKindDto;

const CATALOG_JSON: &str = include_str!("../catalog/feeds.json");

/// A shipped, tested feed a user can subscribe to by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    /// Stable identifier, unique across the whole catalog.
    pub id: String,
    /// Publisher and desk, as the picker lists it.
    pub label: String,
    /// The feed URL written into channel scope when the entry is ticked.
    pub url: String,
    /// Short qualifier shown beside the label, such as the language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A named cluster of entries inside a section: one country, one sport, one desk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGroup {
    /// Stable identifier, unique across the whole catalog.
    pub id: String,
    /// Group heading, such as `Cuba` or `Pro football / NFL`.
    pub label: String,
    /// Context the heading cannot carry on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The feeds in this group.
    pub entries: Vec<CatalogEntry>,
}

/// A top-level division of one channel kind's catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSection {
    /// Stable identifier, unique across the whole catalog.
    pub id: String,
    /// The channel kind whose editor offers this section.
    pub kind: ChannelKindDto,
    /// Section heading, such as `By country`.
    pub label: String,
    /// Context the heading cannot carry on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The groups in this section.
    pub groups: Vec<CatalogGroup>,
}

/// The shipped feed directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedCatalog {
    /// Shape version of `feeds.json`.
    pub schema_version: u32,
    /// The date every entry was last confirmed to answer with current items.
    pub verified_on: String,
    /// Sections, in the order the picker lists them.
    pub sections: Vec<CatalogSection>,
}

impl FeedCatalog {
    /// Sections offered by the editor for `kind`.
    pub fn sections_for(&self, kind: ChannelKindDto) -> impl Iterator<Item = &CatalogSection> {
        self.sections
            .iter()
            .filter(move |section| section.kind == kind)
    }

    /// Every entry in the catalog, flattened.
    pub fn entries(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.sections
            .iter()
            .flat_map(|section| section.groups.iter())
            .flat_map(|group| group.entries.iter())
    }

    /// The catalog label for a feed URL, when the URL is one we ship.
    ///
    /// A feed the user typed themselves has no label here, and that is the
    /// point: it falls back to the publisher name the feed states about
    /// itself, which is the more trustworthy of the two anyway.
    pub fn label_for_url(&self, url: &str) -> Option<&str> {
        self.entries()
            .find(|entry| entry.url == url)
            .map(|entry| entry.label.as_str())
    }

    /// The feed URLs of one group, used to seed defaults by name rather than
    /// by repeating a URL literal that could quietly drift from the catalog.
    pub fn group_urls(&self, group_id: &str) -> Vec<String> {
        self.sections
            .iter()
            .flat_map(|section| section.groups.iter())
            .find(|group| group.id == group_id)
            .map(|group| {
                group
                    .entries
                    .iter()
                    .map(|entry| entry.url.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The URL of a single catalog entry.
    pub fn entry_url(&self, entry_id: &str) -> Option<&str> {
        self.entries()
            .find(|entry| entry.id == entry_id)
            .map(|entry| entry.url.as_str())
    }
}

/// The shipped catalog, parsed once.
///
/// The file is embedded, so a parse failure is a build-time authoring mistake
/// rather than anything a running install can provoke. Panicking here surfaces
/// it in the first test that touches the catalog instead of degrading to an
/// empty picker in front of a user.
pub fn catalog() -> &'static FeedCatalog {
    static CATALOG: OnceLock<FeedCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(CATALOG_JSON).expect("shipped catalog/feeds.json must parse")
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use bridgestatus_collectors::validate_public_url;
    use url::Url;

    use super::*;

    #[test]
    fn the_shipped_catalog_parses() {
        let catalog = catalog();
        assert_eq!(catalog.schema_version, 1);
        assert!(
            catalog.entries().count() >= 60,
            "catalog shrank unexpectedly: {} entries",
            catalog.entries().count()
        );
    }

    #[test]
    fn every_catalog_url_survives_the_fetcher_gate() {
        // A shipped default that the SSRF gate rejects would fail at collector
        // construction, which today fails the whole preferences save. Catching
        // it here means it can never reach a user.
        for entry in catalog().entries() {
            let url = Url::parse(&entry.url)
                .unwrap_or_else(|error| panic!("{} has an unparseable URL: {error}", entry.id));
            validate_public_url(&url, false)
                .unwrap_or_else(|error| panic!("{} is not a usable feed URL: {error}", entry.id));
        }
    }

    #[test]
    fn identifiers_and_urls_are_unique() {
        let mut ids = BTreeSet::new();
        let mut urls = BTreeSet::new();
        for section in &catalog().sections {
            assert!(
                ids.insert(section.id.as_str()),
                "duplicate id {}",
                section.id
            );
            for group in &section.groups {
                assert!(ids.insert(group.id.as_str()), "duplicate id {}", group.id);
                for entry in &group.entries {
                    assert!(ids.insert(entry.id.as_str()), "duplicate id {}", entry.id);
                    assert!(
                        urls.insert(entry.url.as_str()),
                        "{} repeats a URL already in the catalog",
                        entry.id
                    );
                }
            }
        }
    }

    #[test]
    fn no_section_or_group_is_empty() {
        for section in &catalog().sections {
            assert!(!section.groups.is_empty(), "{} has no groups", section.id);
            for group in &section.groups {
                assert!(!group.entries.is_empty(), "{} has no entries", group.id);
            }
        }
    }

    #[test]
    fn both_channel_kinds_are_covered() {
        for kind in [ChannelKindDto::News, ChannelKindDto::Sports] {
            assert!(
                catalog().sections_for(kind).count() > 0,
                "{kind:?} has no catalog sections"
            );
        }
    }

    #[test]
    fn a_shipped_url_resolves_to_its_catalog_label() {
        let catalog = catalog();
        let url = catalog.entry_url("bbc.world").expect("bbc.world ships");
        assert_eq!(catalog.label_for_url(url), Some("BBC News / World"));
        assert_eq!(catalog.label_for_url("https://example.com/feed"), None);
    }
}
