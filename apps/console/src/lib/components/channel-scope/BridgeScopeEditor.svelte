<script lang="ts">
  import VesselSource from './VesselSource.svelte';
  import { snapshot } from '$lib/state';
  import type { AisSettings, ChannelPreference, RiverStation, UnitSystem } from '$lib/types';
  import { scopeText, type ChannelChange } from './scope';

  let {
    channel,
    ais,
    onchannelchange,
    onaischange
  }: {
    channel: ChannelPreference;
    ais: AisSettings;
    unitSystem?: UnitSystem;
    onchannelchange: ChannelChange;
    onaischange: (ais: AisSettings) => void;
  } = $props();

  // The spans come from the same charted geometry the engine projects vessels
  // onto, so the list cannot name a bridge the corridor does not know. Picking
  // a point on a map was asking the reader to hit a span within a few hundred
  // metres, when there are nine of them and they all have names.
  const bascules = $derived<RiverStation[]>(
    ($snapshot?.riverCorridor.branches ?? [])
      .flatMap((branch) => branch.stations)
      .filter((station) => station.kind === 'target' || station.kind === 'bridge')
  );

  const selected = $derived(scopeText(channel, 'bridge', 'Brickell Avenue Bridge'));
  const zone = $derived(scopeText(channel, 'timeZone', 'America/New_York'));

  function pick(label: string) {
    const station = bascules.find((candidate) => candidate.label === label);
    if (!station) return;
    onchannelchange(
      {
        ...channel,
        scope: {
          ...channel.scope,
          bridge: station.label,
          latitude: Number(station.latitude.toFixed(5)),
          longitude: Number(station.longitude.toFixed(5))
        }
      },
      true
    );
  }
</script>

<div class="bridge-scope">
  <div class="identity-fields">
    <label class="field">
      <span>Watching</span>
      <select value={selected} onchange={(event) => pick(event.currentTarget.value)}>
        {#each bascules as station (station.label)}
          <option value={station.label}>{station.label}</option>
        {/each}
        {#if !bascules.some((station) => station.label === selected)}
          <option value={selected}>{selected}</option>
        {/if}
      </select>
      <small class="field-note">Every bascule on the charted river, seaward first.</small>
    </label>
    <div class="field">
      <span>Local time zone</span>
      <!-- Read from the machine. It was a two-entry dropdown, which is not a
           choice so much as a chance to be wrong. -->
      <p class="zone-readout">{zone}</p>
      <small class="field-note">Taken from this computer.</small>
    </div>
  </div>

  <VesselSource {ais} {onaischange} />
</div>

<style>
  .bridge-scope {
    display: grid;
    gap: 16px;
  }

  .identity-fields {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
  }

  .zone-readout {
    display: flex;
    align-items: center;
    min-height: 44px;
    margin: 0;
    padding: 10px 12px;
    color: var(--graphite);
    background: var(--frost);
    border: 1px solid var(--rule);
    border-radius: 2px;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    letter-spacing: 0.03em;
  }

  @media (max-width: 680px) {
    .identity-fields {
      grid-template-columns: 1fr;
    }
  }
</style>
