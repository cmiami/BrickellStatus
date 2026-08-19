<script lang="ts">
  import { MapPin } from '@lucide/svelte';

  import LocationPickerModal from '$lib/components/LocationPickerModal.svelte';
  import AisOutputPanel from '$lib/components/outputs/AisOutputPanel.svelte';
  import type { AisSettings, ChannelPreference, UnitSystem } from '$lib/types';
  import { formatDistanceKilometers } from '$lib/units';
  import { scopeBool, scopeNumber, scopeText, setScope, type ChannelChange } from './scope';

  let {
    channel,
    ais,
    unitSystem,
    onchannelchange,
    onaischange
  }: {
    channel: ChannelPreference;
    ais: AisSettings;
    unitSystem: UnitSystem;
    onchannelchange: ChannelChange;
    onaischange: (ais: AisSettings) => void;
  } = $props();

  let pickingTarget = $state(false);

  function set(key: string, value: string | number | boolean) {
    setScope(channel, onchannelchange, key, value);
  }

  function setCoordinates(latitude: number, longitude: number) {
    onchannelchange({
      ...channel,
      scope: {
        ...channel.scope,
        latitude: Number(latitude.toFixed(5)),
        longitude: Number(longitude.toFixed(5))
      }
    });
  }

</script>

<div class="bridge-scope">
  <div class="identity-fields">
    <label class="field">
      <span>Bridge</span>
      <input
        value={scopeText(channel, 'bridge', 'Brickell Avenue Bridge')}
        maxlength="120"
        oninput={(event) => set('bridge', event.currentTarget.value)}
      />
    </label>
    <label class="field">
      <span>Local time zone</span>
      <select
        value={scopeText(channel, 'timeZone', 'America/New_York')}
        onchange={(event) => set('timeZone', event.currentTarget.value)}
      >
        <option value="America/New_York">Miami · Eastern time</option>
        <option value="UTC">UTC</option>
      </select>
    </label>
  </div>

  <section class="bridge-target" aria-labelledby={`${channel.id}-map-heading`}>
    <header>
      <div>
        <h4 id={`${channel.id}-map-heading`}>Controller target</h4>
        <p>Bridge status reporting is discovered around this exact point.</p>
      </div>
      <button class="secondary-action" type="button" onclick={() => (pickingTarget = true)}>
        <MapPin size={16} aria-hidden="true" /> Set on map
      </button>
    </header>
    <p class="bridge-target-readout">
      {scopeNumber(channel, 'latitude', 25.7699).toFixed(5)}, {scopeNumber(channel, 'longitude', -80.19005).toFixed(5)}
    </p>
    <details>
      <summary>Advanced coordinates</summary>
      <div class="coordinate-fields">
        <label class="field">
          <span>Latitude</span>
          <input type="number" step="0.00001" min="-90" max="90" value={scopeNumber(channel, 'latitude', 25.7699)} oninput={(event) => set('latitude', event.currentTarget.valueAsNumber)} />
        </label>
        <label class="field">
          <span>Longitude</span>
          <input type="number" step="0.00001" min="-180" max="180" value={scopeNumber(channel, 'longitude', -80.19005)} oninput={(event) => set('longitude', event.currentTarget.valueAsNumber)} />
        </label>
      </div>
    </details>
  </section>

  <!-- Bridge status reporting and upstream progression used to be switches
       here. Neither is a preference: turning one off does not express an
       intent, it just makes the forecast worse in a way nothing on screen
       explains. The engine decides what evidence it weighs. -->
  <AisOutputPanel {ais} {unitSystem} {onaischange} />
</div>

{#if pickingTarget}
  <LocationPickerModal
    title="Controller target"
    description="Drop the pin on the span itself. Bridge status reporting is discovered around this exact point, so a pin on the wrong side of the river finds the wrong bridge."
    latitude={scopeNumber(channel, 'latitude', 25.7699)}
    longitude={scopeNumber(channel, 'longitude', -80.19005)}
    label={scopeText(channel, 'bridge', 'Bridge target')}
    {unitSystem}
    confirmLabel="Use this point"
    onconfirm={(latitude, longitude) => {
      setCoordinates(latitude, longitude);
      pickingTarget = false;
    }}
    oncancel={() => (pickingTarget = false)}
  />
{/if}

<style>
  .bridge-scope {
    display: grid;
    gap: 16px;
  }

  .identity-fields,
  .coordinate-fields {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
  }

  .coordinate-fields {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .bridge-target {
    display: grid;
    overflow: hidden;
    border: 1px solid var(--rule-strong);
  }

  .bridge-target > header {
    padding: 16px;
    color: var(--white);
    background: var(--marine);
  }

  .bridge-target h4,
  .bridge-target p {
    margin: 0;
  }

  .bridge-target h4 {
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .bridge-target p {
    max-width: 70ch;
    margin-top: 5px;
    color: var(--nav-muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .bridge-target details {
    padding: 14px 16px 16px;
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
  }

  .bridge-target summary {
    width: fit-content;
    color: var(--channel);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .coordinate-fields {
    margin-top: 14px;
  }

  @media (max-width: 680px) {
    .identity-fields,
    .coordinate-fields {
      grid-template-columns: 1fr;
    }
  }
</style>
