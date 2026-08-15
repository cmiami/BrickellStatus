<script lang="ts">
  import type { AlertArea, ChannelPreference } from '$lib/types';
  import { scopeList, toggleScopeList, type ChannelChange } from './scope';

  let {
    channel,
    areas,
    onchannelchange
  }: {
    channel: ChannelPreference;
    areas: AlertArea[];
    onchannelchange: ChannelChange;
  } = $props();
</script>

<fieldset class="area-coverage">
  <legend>Area coverage</legend>
  {#if areas.length}
    <div class="area-list">
      {#each areas as area (area.id)}
        <label class:disabled={!area.enabled}>
          <input
            type="checkbox"
            checked={scopeList(channel, 'areaIds').includes(area.id)}
            disabled={!area.enabled}
            onchange={(event) =>
              toggleScopeList(channel, onchannelchange, 'areaIds', area.id, event.currentTarget.checked)}
          />
          <span>
            <strong>{area.label}</strong>
            <small>{area.latitude.toFixed(4)}, {area.longitude.toFixed(4)} · {area.timeZone}</small>
          </span>
        </label>
      {/each}
    </div>
  {:else}
    <p>No named areas are configured. Add an area before enabling this channel.</p>
  {/if}
  <a href="/map">Edit coverage on the map →</a>
</fieldset>

<style>
  .area-coverage {
    display: grid;
    gap: 12px;
    margin: 0;
    border: 0;
    padding: 0;
  }

  .area-coverage legend {
    margin-bottom: 2px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .area-list {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    border-block: 1px solid var(--rule);
  }

  .area-list label {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: start;
    gap: 9px;
    padding: 12px;
    background: var(--paper);
    border-inline-end: 1px solid var(--rule);
    border-bottom: 1px solid var(--rule);
  }

  .area-list label:nth-child(even) {
    border-inline-end: 0;
  }

  .area-list label.disabled {
    opacity: 0.52;
  }

  .area-list input {
    width: 18px;
    height: 18px;
    margin: 1px 0 0;
    accent-color: var(--marine);
  }

  .area-list span {
    display: grid;
    min-width: 0;
    gap: 3px;
  }

  .area-list strong {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    text-transform: uppercase;
  }

  .area-list small,
  .area-coverage p {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.35;
  }

  .area-coverage > a {
    width: fit-content;
    color: var(--channel);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    text-underline-offset: 4px;
  }

  @media (max-width: 680px) {
    .area-list {
      grid-template-columns: 1fr;
    }

    .area-list label {
      border-inline-end: 0;
    }
  }
</style>
