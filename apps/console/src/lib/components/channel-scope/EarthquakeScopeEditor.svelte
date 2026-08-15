<script lang="ts">
  import type { ChannelPreference } from '$lib/types';
  import { scopeNumber, scopeText, setScope, type ChannelChange } from './scope';

  let { channel, onchannelchange }: { channel: ChannelPreference; onchannelchange: ChannelChange } = $props();
</script>

<div class="earthquake-fields">
  <label class="field">
    <span>Minimum magnitude</span>
    <input type="number" min="0" max="10" step="0.1" value={scopeNumber(channel, 'minimumMagnitude', 5.5)} oninput={(event) => setScope(channel, onchannelchange, 'minimumMagnitude', event.currentTarget.valueAsNumber)} />
  </label>
  <label class="field">
    <span>Maximum event age</span>
    <input type="number" min="1" max="1440" value={scopeNumber(channel, 'eventAgeMinutes', 60)} oninput={(event) => setScope(channel, onchannelchange, 'eventAgeMinutes', event.currentTarget.valueAsNumber)} />
    <small class="field-note">Minutes since origin time.</small>
  </label>
  <label class="field">
    <span>USGS feed</span>
    <select value={scopeText(channel, 'feed', 'significant_hour')} onchange={(event) => setScope(channel, onchannelchange, 'feed', event.currentTarget.value)}>
      <option value="significant_hour">Significant · past hour</option>
      <option value="significant_day">Significant · past day</option>
      <option value="4.5_hour">Magnitude 4.5+ · past hour</option>
    </select>
  </label>
</div>

<style>
  .earthquake-fields {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 14px;
  }

  @media (max-width: 720px) {
    .earthquake-fields {
      grid-template-columns: 1fr;
    }
  }
</style>
