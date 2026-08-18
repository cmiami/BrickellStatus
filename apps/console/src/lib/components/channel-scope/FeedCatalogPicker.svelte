<script lang="ts">
  import type { ChannelPreference } from '$lib/types';
  import { isCatalogUrl, labelForUrl, sectionsFor, catalog, type CatalogSection } from '$lib/catalog';
  import { scopeList, setScope, toggleScopeList, type ChannelChange } from './scope';

  let {
    channel,
    onchannelchange,
    kind,
    maxFeeds = 40
  }: {
    channel: ChannelPreference;
    onchannelchange: ChannelChange;
    kind: CatalogSection['kind'];
    maxFeeds?: number;
  } = $props();

  const sections = $derived(sectionsFor(kind));
  const subscribed = $derived(scopeList(channel, 'feeds'));
  const subscribedSet = $derived(new Set(subscribed));

  // Feeds the user typed themselves. They are listed separately because the
  // catalog cannot vouch for them and the register is the only place they can
  // be removed.
  const custom = $derived(subscribed.filter((url) => !isCatalogUrl(url)));

  // A section starts open when it already holds something, so a returning
  // reader sees their own choices without hunting for them.
  let openSections = $state(new Set<string>());
  let initialized = false;
  $effect(() => {
    if (initialized) return;
    initialized = true;
    const open = new Set<string>();
    for (const section of sections) {
      const holdsAPick = section.groups.some((group) =>
        group.entries.some((entry) => subscribedSet.has(entry.url))
      );
      if (holdsAPick) open.add(section.id);
    }
    // Nothing picked yet: open the first section so the screen is never a wall
    // of closed headings.
    if (open.size === 0 && sections.length) open.add(sections[0].id);
    openSections = open;
  });

  function toggleSection(id: string) {
    const next = new Set(openSections);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    openSections = next;
  }

  function countIn(section: CatalogSection): number {
    return section.groups.reduce(
      (total, group) => total + group.entries.filter((entry) => subscribedSet.has(entry.url)).length,
      0
    );
  }

  let issue = $state('');

  function toggleFeed(url: string, enabled: boolean) {
    if (enabled && subscribed.length >= maxFeeds) {
      issue = `This channel already holds ${maxFeeds} feeds, the most it can poll.`;
      return;
    }
    issue = '';
    toggleScopeList(channel, onchannelchange, 'feeds', url, enabled);
  }

  function removeFeed(url: string) {
    issue = '';
    setScope(
      channel,
      onchannelchange,
      'feeds',
      subscribed.filter((existing) => existing !== url),
      true
    );
  }

  // --- Custom entry -------------------------------------------------------

  let customDraft = $state('');
  let customIssue = $state('');
  let customOpen = $state(false);

  // Mirrors `validate_syndication_scope` in the Rust runtime. Checking here is
  // not the security boundary — the fetcher still resolves DNS and re-checks
  // every redirect hop — it is so the reader learns which feed is wrong while
  // they are still looking at it, instead of losing an unrelated save.
  const CREDENTIAL_QUERY_KEYS = [
    'access_token',
    'api_key',
    'apikey',
    'auth',
    'authorization',
    'key',
    'secret',
    'sig',
    'signature',
    'token'
  ];

  function addCustomFeed() {
    const raw = customDraft.trim();
    if (!raw) {
      customIssue = 'Paste a feed address first.';
      return;
    }
    let url: URL;
    try {
      url = new URL(raw);
    } catch {
      customIssue = 'That is not a web address. A feed looks like https://example.com/feed.';
      return;
    }
    if (url.protocol !== 'https:') {
      customIssue = 'Feeds must use https, so the address cannot be rewritten in transit.';
      return;
    }
    if (url.hash) {
      customIssue = 'Remove the # part of the address; a feed never needs one.';
      return;
    }
    if (url.username || url.password) {
      customIssue = 'Remove the username and password from the address.';
      return;
    }
    const credential = [...url.searchParams.keys()].find((parameter) =>
      CREDENTIAL_QUERY_KEYS.includes(parameter.toLowerCase())
    );
    if (credential) {
      customIssue = `This address carries a "${credential}" parameter. Use a public feed URL instead.`;
      return;
    }
    if (subscribedSet.has(url.href) || subscribedSet.has(raw)) {
      customIssue = 'That feed is already on the list.';
      return;
    }
    const shipped = labelForUrl(url.href);
    if (shipped) {
      customIssue = `That is ${shipped}, which is in the list above. Tick it there instead.`;
      return;
    }
    if (subscribed.length >= maxFeeds) {
      customIssue = `This channel already holds ${maxFeeds} feeds, the most it can poll.`;
      return;
    }
    setScope(channel, onchannelchange, 'feeds', [...subscribed, url.href], true);
    customDraft = '';
    customIssue = '';
  }

  function handleCustomKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      addCustomFeed();
    }
  }

  function hostOf(url: string): string {
    try {
      return new URL(url).host;
    } catch {
      return url;
    }
  }
</script>

<section class="catalog" aria-labelledby={`${channel.id}-catalog-heading`}>
  <header>
    <div>
      <h4 id={`${channel.id}-catalog-heading`}>Sources</h4>
      <p>
        Every listed feed was fetched and confirmed to answer with current items on
        {catalog.verifiedOn}. Tick the ones you want.
      </p>
    </div>
    <span>{subscribed.length} / {maxFeeds}</span>
  </header>

  <div class="sections">
    {#each sections as section (section.id)}
      {@const picked = countIn(section)}
      {@const open = openSections.has(section.id)}
      <div class="section" class:open>
        <h5>
          <button
            type="button"
            aria-expanded={open}
            aria-controls={`${channel.id}-${section.id}`}
            onclick={() => toggleSection(section.id)}
          >
            <span class="marker" aria-hidden="true">{open ? '–' : '+'}</span>
            <span class="section-label">{section.label}</span>
            <span class="tally" class:none={picked === 0}>
              {picked === 0 ? 'none' : `${picked} on`}
            </span>
          </button>
        </h5>

        {#if open}
          <div class="section-body" id={`${channel.id}-${section.id}`}>
            {#if section.note}
              <p class="section-note">{section.note}</p>
            {/if}
            {#each section.groups as group (group.id)}
              <fieldset>
                <legend>{group.label}</legend>
                {#if group.note}
                  <p class="group-note">{group.note}</p>
                {/if}
                <div class="entries">
                  {#each group.entries as entry (entry.id)}
                    <label class="entry" class:on={subscribedSet.has(entry.url)}>
                      <input
                        type="checkbox"
                        checked={subscribedSet.has(entry.url)}
                        onchange={(event) =>
                          toggleFeed(entry.url, (event.currentTarget as HTMLInputElement).checked)}
                      />
                      <span class="entry-label">{entry.label}</span>
                      {#if entry.note}<span class="entry-note">{entry.note}</span>{/if}
                    </label>
                  {/each}
                </div>
              </fieldset>
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>

  {#if issue}
    <p class="issue" role="alert">{issue}</p>
  {/if}

  <div class="custom" class:open={customOpen}>
    <h5>
      <button
        type="button"
        aria-expanded={customOpen}
        aria-controls={`${channel.id}-custom`}
        onclick={() => (customOpen = !customOpen)}
      >
        <span class="marker" aria-hidden="true">{customOpen ? '–' : '+'}</span>
        <span class="section-label">Custom feed</span>
        <span class="tally" class:none={custom.length === 0}>
          {custom.length === 0 ? 'none' : `${custom.length} added`}
        </span>
      </button>
    </h5>

    {#if customOpen}
      <div class="custom-body" id={`${channel.id}-custom`}>
        <div class="custom-entry">
          <label>
            <span>Feed address</span>
            <input
              bind:value={customDraft}
              type="url"
              inputmode="url"
              spellcheck="false"
              placeholder="https://example.com/feed"
              aria-invalid={customIssue ? 'true' : undefined}
              aria-describedby={`${channel.id}-custom-help`}
              onkeydown={handleCustomKeydown}
            />
          </label>
          <button class="secondary-action" type="button" onclick={addCustomFeed}>Add feed</button>
        </div>
        <p id={`${channel.id}-custom-help`} class="custom-help" class:error={Boolean(customIssue)} aria-live="polite">
          {customIssue || 'A public HTTPS address ending in something like /feed, /rss, or .xml.'}
        </p>

        {#if custom.length}
          <ul class="custom-register" aria-label="Feeds you added">
            {#each custom as url (url)}
              <li>
                <span class="custom-host">{hostOf(url)}</span>
                <button type="button" aria-label={`Remove ${hostOf(url)}`} onclick={() => removeFeed(url)}>
                  ×
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .catalog {
    display: grid;
    overflow: hidden;
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .catalog > header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 20px;
    padding: 16px;
    color: var(--white);
    background: var(--marine);
  }

  .catalog > header div {
    display: grid;
    gap: 5px;
  }

  .catalog h4,
  .catalog p {
    margin: 0;
  }

  .catalog h4 {
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .catalog header p {
    max-width: 70ch;
    color: var(--nav-muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  .catalog header > span {
    flex: 0 0 auto;
    color: var(--nav-muted);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 700;
    letter-spacing: 0.06em;
  }

  .sections {
    display: grid;
  }

  .section,
  .custom {
    border-block-start: 1px solid var(--rule);
  }

  .section:first-child {
    border-block-start: 0;
  }

  .custom {
    border-block-start: 1px solid var(--rule-strong);
  }

  .section h5,
  .custom h5 {
    margin: 0;
  }

  .section h5 button,
  .custom h5 button {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 46px;
    padding: 10px 16px;
    color: var(--graphite);
    background: var(--paper);
    border: 0;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 650;
    letter-spacing: 0.05em;
    text-align: start;
    text-transform: uppercase;
    cursor: pointer;
  }

  .section h5 button:hover,
  .custom h5 button:hover {
    background: var(--frost);
  }

  .section.open h5 button,
  .custom.open h5 button {
    background: var(--frost);
    /* A registration rule marks the open section; the marker glyph alone
       would carry the state on shape only. */
    box-shadow: inset 3px 0 0 var(--marine);
  }

  .marker {
    color: var(--muted);
    font-size: var(--type-title);
    line-height: 1;
    text-align: center;
  }

  .tally {
    color: var(--marine);
    font-size: var(--type-micro);
    font-weight: 700;
    letter-spacing: 0.08em;
  }

  .tally.none {
    color: var(--muted);
  }

  .section-body,
  .custom-body {
    display: grid;
    gap: 14px;
    padding: 4px 16px 18px;
  }

  .section-note,
  .group-note {
    max-width: 70ch;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
  }

  legend {
    padding: 0 0 7px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .entries {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 1px;
    background: var(--rule);
    border: 1px solid var(--rule);
  }

  .entry {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    min-height: 42px;
    padding: 8px 12px;
    background: var(--white);
    cursor: pointer;
  }

  .entry:hover {
    background: var(--frost);
  }

  .entry.on {
    background: var(--frost);
    box-shadow: inset 3px 0 0 var(--marine);
  }

  .entry input {
    width: 17px;
    height: 17px;
    accent-color: var(--marine);
    cursor: pointer;
  }

  .entry-label {
    font-size: var(--type-body-small);
    line-height: 1.3;
  }

  .entry-note {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .issue {
    margin: 0;
    padding: 12px 16px;
    color: var(--danger);
    background: var(--frost);
    border-block-start: 1px solid var(--rule);
    font-size: var(--type-caption);
  }

  .custom-entry {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 10px;
  }

  .custom-entry label {
    display: grid;
    gap: 7px;
  }

  .custom-entry label > span {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .custom-entry input {
    width: 100%;
    min-height: 44px;
    color: var(--graphite);
    background: var(--frost);
    border: 1px solid var(--steel);
    border-radius: 2px;
    padding: 10px 12px;
    font: inherit;
  }

  .custom-entry input[aria-invalid='true'] {
    border-color: var(--danger);
    outline-color: var(--danger);
  }

  .custom-help {
    margin: -4px 0 0;
    color: var(--muted);
    font-size: var(--type-caption);
  }

  .custom-help.error {
    color: var(--danger);
  }

  .custom-register {
    display: grid;
    gap: 7px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .custom-register li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 34px;
    align-items: center;
    min-height: 38px;
    background: var(--white);
    border: 1px solid var(--rule-strong);
    border-radius: 2px;
  }

  .custom-host {
    overflow: hidden;
    padding: 0 12px;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    letter-spacing: 0.03em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .custom-register button {
    align-self: stretch;
    color: var(--muted);
    background: var(--frost);
    border: 0;
    border-inline-start: 1px solid var(--rule);
    font-size: var(--type-title);
    cursor: pointer;
  }

  .custom-register button:hover {
    color: var(--white);
    background: var(--danger);
  }

  @media (max-width: 680px) {
    .custom-entry {
      grid-template-columns: 1fr;
    }

    .custom-entry .secondary-action {
      width: 100%;
      justify-content: center;
    }

    .entries {
      grid-template-columns: 1fr;
    }
  }
</style>
