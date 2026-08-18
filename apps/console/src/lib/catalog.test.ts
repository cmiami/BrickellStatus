import { describe, expect, it } from 'vitest';

import { catalog, catalogEntries, isCatalogUrl, labelForUrl, sectionsFor } from './catalog';

// These mirror `crates/runtime/src/catalog.rs`. Both sides read the same
// `feeds.json`, so what is actually being guarded here is that the console can
// resolve and parse it, and that a catalog edit cannot satisfy one language's
// rules while breaking the other's.
describe('the shipped feed catalog', () => {
  it('parses and carries a verification date', () => {
    expect(catalog.schemaVersion).toBe(1);
    expect(catalog.verifiedOn).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(catalogEntries().length).toBeGreaterThanOrEqual(60);
  });

  it('only names channel kinds the runtime has', () => {
    for (const section of catalog.sections) {
      expect(['news', 'sports']).toContain(section.kind);
    }
  });

  it('offers sections for both pickers', () => {
    expect(sectionsFor('news').length).toBeGreaterThan(0);
    expect(sectionsFor('sports').length).toBeGreaterThan(0);
  });

  it('gives every feed a unique id and url', () => {
    const ids = new Set<string>();
    const urls = new Set<string>();
    for (const section of catalog.sections) {
      expect(ids.has(section.id)).toBe(false);
      ids.add(section.id);
      for (const group of section.groups) {
        expect(ids.has(group.id)).toBe(false);
        ids.add(group.id);
        for (const entry of group.entries) {
          expect(ids.has(entry.id), `duplicate id ${entry.id}`).toBe(false);
          ids.add(entry.id);
          expect(urls.has(entry.url), `duplicate url on ${entry.id}`).toBe(false);
          urls.add(entry.url);
        }
      }
    }
  });

  it('ships only fetchable https feeds', () => {
    // The runtime refuses plain http and credential-bearing queries, and a
    // shipped default that the fetcher rejects would be dead on arrival.
    const credentialKeys = ['access_token', 'api_key', 'apikey', 'auth', 'authorization', 'key', 'secret', 'sig', 'signature', 'token'];
    for (const entry of catalogEntries()) {
      const url = new URL(entry.url);
      expect(url.protocol, `${entry.id} must use https`).toBe('https:');
      expect(url.hash, `${entry.id} must not carry a fragment`).toBe('');
      expect(url.username, `${entry.id} must not carry credentials`).toBe('');
      for (const parameter of url.searchParams.keys()) {
        expect(credentialKeys, `${entry.id} carries ${parameter}`).not.toContain(parameter.toLowerCase());
      }
    }
  });

  it('leaves no section or group empty', () => {
    for (const section of catalog.sections) {
      expect(section.groups.length, `${section.id} has no groups`).toBeGreaterThan(0);
      for (const group of section.groups) {
        expect(group.entries.length, `${group.id} has no entries`).toBeGreaterThan(0);
      }
    }
  });

  it('resolves a shipped url to its label and leaves a custom one unnamed', () => {
    const bbc = catalogEntries().find((entry) => entry.id === 'bbc.world');
    expect(bbc).toBeDefined();
    expect(labelForUrl(bbc!.url)).toBe('BBC News / World');
    expect(isCatalogUrl(bbc!.url)).toBe(true);
    expect(labelForUrl('https://example.com/feed')).toBeUndefined();
    expect(isCatalogUrl('https://example.com/feed')).toBe(false);
  });

  it('covers the five countries the product names', () => {
    const countries = catalog.sections.find((section) => section.id === 'news.country');
    expect(countries).toBeDefined();
    expect(countries!.groups.map((group) => group.label)).toEqual([
      'Cuba',
      'Colombia',
      'Haiti',
      'Nicaragua',
      'Venezuela'
    ]);
  });

  it('covers the top sports and a transactions desk', () => {
    const bySport = catalog.sections.find((section) => section.id === 'sports.sport');
    expect(bySport!.groups.length).toBeGreaterThanOrEqual(10);
    const transactions = catalog.sections.find((section) => section.id === 'sports.transactions');
    expect(transactions!.groups[0].entries.length).toBe(4);
  });
});
