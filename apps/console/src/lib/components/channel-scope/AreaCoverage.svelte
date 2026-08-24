<script lang="ts">
  import { MapPin } from '@lucide/svelte';

  import LocationPickerModal from '$lib/components/LocationPickerModal.svelte';
  import type { AlertArea, ChannelPreference, UnitSystem } from '$lib/types';
  import { scopeList, toggleScopeList, type ChannelChange } from './scope';

  let {
    channel,
    areas,
    unitSystem = 'imperial',
    onchannelchange,
    onareaadd
  }: {
    channel: ChannelPreference;
    areas: AlertArea[];
    unitSystem?: UnitSystem;
    onchannelchange: ChannelChange;
    onareaadd?: (area: AlertArea) => void;
  } = $props();

  let picking = $state(false);

  // Somewhere to open the map when there is nothing saved yet. A first pin has
  // to land somewhere, and the middle of the ocean is a worse guess than the
  // place this app is about.
  const BRICKELL = { latitude: 25.7699, longitude: -80.19005 };
  const origin = $derived(areas[0] ?? BRICKELL);

  function addPlace(latitude: number, longitude: number, name: string) {
    picking = false;
    if (!onareaadd) return;
    const area: AlertArea = {
      // Time-ordered so the id sorts by when it was dropped, and unique
      // without asking the reader to name the place before they have looked
      // at it.
      id: `area.${Date.now().toString(36)}`,
      // The reader's own name when they gave one. The generated label is still
      // the default rather than a prompt, because naming a place is not worth
      // blocking on -- but a place you have just looked at is the moment you
      // know what to call it, and the alternative was finding it filed as
      // "Coverage area 3" and going to another screen to rename it.
      label: name || `Coverage area ${areas.length + 1}`,
      latitude,
      longitude,
      timeZone: areas[0]?.timeZone ?? 'America/New_York',
      source: 'manual',
      enabled: true,
      weatherEnabled: true,
      officialAlertsEnabled: true,
      tropicalContextEnabled: true
    };
    onareaadd(area);
    toggleScopeList(channel, onchannelchange, 'areaIds', area.id, true);
  }
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
  <div class="area-actions">
    {#if onareaadd}
      <button class="secondary-action" type="button" onclick={() => (picking = true)}>
        <MapPin size={16} aria-hidden="true" /> Add a place
      </button>
    {/if}
    <a href="/map">Rename or remove on the map →</a>
  </div>
</fieldset>

{#if picking}
  <LocationPickerModal
    title="Coverage area"
    description="Drop a pin on the place this channel should watch. Saved places are hidden while you choose, so there is only ever one pin to move."
    latitude={origin.latitude}
    longitude={origin.longitude}
    label="New coverage area"
    {unitSystem}
    confirmLabel="Add this place"
    nameSuggestion={`Coverage area ${areas.length + 1}`}
    onconfirm={addPlace}
    oncancel={() => (picking = false)}
  />
{/if}

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

  .area-actions {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .area-actions a {
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
