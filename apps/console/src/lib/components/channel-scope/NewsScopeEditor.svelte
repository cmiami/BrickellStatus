<script lang="ts">
  import SwitchField from '$lib/components/SwitchField.svelte';
  import type { ChannelPreference } from '$lib/types';
  import { scopeBool, scopeList, setScope, type ChannelChange } from './scope';

  let { channel, onchannelchange }: { channel: ChannelPreference; onchannelchange: ChannelChange } = $props();

  function lines(event: Event, key: string) {
    const values = (event.currentTarget as HTMLTextAreaElement).value.split('\n').map((item) => item.trim()).filter(Boolean);
    setScope(channel, onchannelchange, key, values);
  }

  function commas(event: Event, key: string) {
    const values = (event.currentTarget as HTMLInputElement).value.split(',').map((item) => item.trim()).filter(Boolean);
    setScope(channel, onchannelchange, key, values);
  }
</script>

<div class="news-scope">
  <label class="field">
    <span>RSS or Atom feeds</span>
    <textarea
      value={scopeList(channel, 'feeds').join('\n')}
      rows="5"
      spellcheck="false"
      placeholder={'https://www.miamidade.gov/global/rss-news.page\nhttps://wsvn.com/feed/'}
      oninput={(event) => lines(event, 'feeds')}
    ></textarea>
    <small class="field-note">One public HTTPS feed per line. The defaults are live Miami-Dade County, WSVN, and Local 10 feeds.</small>
  </label>
  <div class="topic-fields">
    <label class="field">
      <span>Include keywords</span>
      <input value={scopeList(channel, 'topics').join(', ')} maxlength="500" oninput={(event) => commas(event, 'topics')} placeholder="Miami, transit, weather" />
      <small class="field-note">Comma-separated; blank accepts every item.</small>
    </label>
    <label class="field">
      <span>Exclude keywords</span>
      <input value={scopeList(channel, 'excludeTopics').join(', ')} maxlength="500" oninput={(event) => commas(event, 'excludeTopics')} placeholder="sports scores, sponsored" />
    </label>
  </div>
  <SwitchField
    checked={scopeBool(channel, 'breakingOnly', false)}
    label="Breaking labels only"
    description="Require the publisher title or categories to identify the item as breaking."
    onchange={(enabled) => setScope(channel, onchannelchange, 'breakingOnly', enabled)}
  />
</div>

<style>
  .news-scope {
    display: grid;
    gap: 14px;
  }

  .topic-fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }

  @media (max-width: 720px) {
    .topic-fields {
      grid-template-columns: 1fr;
    }
  }
</style>
