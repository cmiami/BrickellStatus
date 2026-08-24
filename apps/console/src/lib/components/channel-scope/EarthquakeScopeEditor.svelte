<script lang="ts">
  import type { ChannelPreference } from '$lib/types';
  import { scopeNumber, setScope, type ChannelChange } from './scope';

  let { channel, onchannelchange }: { channel: ChannelPreference; onchannelchange: ChannelChange } = $props();
</script>

<div class="earthquake-fields">
  <label class="field">
    <span>Minimum magnitude</span>
    <input type="number" min="0" max="10" step="0.1" value={scopeNumber(channel, 'minimumMagnitude', 4.5)} oninput={(event) => setScope(channel, onchannelchange, 'minimumMagnitude', event.currentTarget.valueAsNumber)} />
    <small class="field-note">USGS magnitude 4.5+ events from the past day. Events below your threshold are ignored.</small>
  </label>
  <p>New events move ahead automatically. They leave the current notices after 24 hours.</p>
</div>

<style>
  .earthquake-fields {
    display: grid;
    gap: 14px;
    max-width: 620px;
  }

  .earthquake-fields > p {
    margin: 0;
    padding: 12px 14px;
    color: var(--muted);
    background: var(--paper);
    border: 1px solid var(--rule);
    font-size: var(--type-caption);
    line-height: 1.45;
  }
</style>
