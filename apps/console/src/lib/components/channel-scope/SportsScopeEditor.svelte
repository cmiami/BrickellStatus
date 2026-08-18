<script lang="ts">
  import type { ChannelPreference } from '$lib/types';
  import FeedCatalogPicker from './FeedCatalogPicker.svelte';
  import { scopeList, setScope, type ChannelChange } from './scope';

  let { channel, onchannelchange }: { channel: ChannelPreference; onchannelchange: ChannelChange } = $props();

  function commas(event: Event, key: string) {
    const values = (event.currentTarget as HTMLInputElement).value
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    setScope(channel, onchannelchange, key, values);
  }
</script>

<div class="sports-scope">
  <FeedCatalogPicker {channel} {onchannelchange} kind="sports" />

  <div class="topic-fields">
    <label class="field">
      <span>Include keywords</span>
      <input
        value={scopeList(channel, 'topics').join(', ')}
        maxlength="500"
        oninput={(event) => commas(event, 'topics')}
        placeholder="Dolphins, Heat, draft"
      />
      <small class="field-note">Comma-separated. Leave blank to see everything from the feeds above; a league feed narrowed to one team name is how you follow that team.</small>
    </label>
    <label class="field">
      <span>Exclude keywords</span>
      <input
        value={scopeList(channel, 'excludeTopics').join(', ')}
        maxlength="500"
        oninput={(event) => commas(event, 'excludeTopics')}
        placeholder="odds, betting, promo code"
      />
      <!-- Every sports desk we ship runs betting promotions in the same feed as
           its reporting, so this is the field most readers will actually use. -->
      <small class="field-note">Items containing any of these are dropped. Useful for the odds and promo posts publishers file alongside reporting.</small>
    </label>
  </div>
</div>

<style>
  .sports-scope {
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
