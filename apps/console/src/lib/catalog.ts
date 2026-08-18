// The shipped feed directory, read from the same file the Rust runtime embeds.
//
// One file, two readers. The alternative — a Rust list and a TypeScript list
// kept in step by hand — is exactly how the earthquake editor came to offer a
// feed window its own validator rejects, which makes preferences unsaveable the
// moment you pick it. `catalog.parity.test.ts` fails if this import ever stops
// matching what Rust sees.
import feeds from '../../../../crates/runtime/catalog/feeds.json';

export interface CatalogEntry {
  id: string;
  label: string;
  url: string;
  note?: string;
}

export interface CatalogGroup {
  id: string;
  label: string;
  note?: string;
  entries: CatalogEntry[];
}

export interface CatalogSection {
  id: string;
  kind: 'news' | 'sports';
  label: string;
  note?: string;
  groups: CatalogGroup[];
}

export interface FeedCatalog {
  schemaVersion: number;
  verifiedOn: string;
  sections: CatalogSection[];
}

export const catalog: FeedCatalog = feeds as FeedCatalog;

/** Sections a given channel kind's editor offers. */
export function sectionsFor(kind: CatalogSection['kind']): CatalogSection[] {
  return catalog.sections.filter((section) => section.kind === kind);
}

/** Every shipped entry, flattened. */
export function catalogEntries(): CatalogEntry[] {
  return catalog.sections.flatMap((section) => section.groups.flatMap((group) => group.entries));
}

const labelsByUrl = new Map(catalogEntries().map((entry) => [entry.url, entry.label]));

/**
 * The catalog label for a feed URL.
 *
 * A URL the user typed themselves has no label here, and the editor says so
 * rather than inventing one.
 */
export function labelForUrl(url: string): string | undefined {
  return labelsByUrl.get(url);
}

/** Whether a URL is one of the feeds we ship and have tested. */
export function isCatalogUrl(url: string): boolean {
  return labelsByUrl.has(url);
}
