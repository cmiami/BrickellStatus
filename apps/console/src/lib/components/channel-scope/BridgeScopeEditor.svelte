<script lang="ts">
  import LocationMap from '$lib/components/LocationMap.svelte';
  import SwitchField from '$lib/components/SwitchField.svelte';
  import type { AisSettings, ChannelPreference, LocationMapPoint, UnitSystem } from '$lib/types';
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

  const bridgePoint = $derived<LocationMapPoint>({
    id: `${channel.id}.target`,
    label: scopeText(channel, 'bridge', 'Brickell Avenue Bridge'),
    latitude: scopeNumber(channel, 'latitude', 25.7699),
    longitude: scopeNumber(channel, 'longitude', -80.19005),
    detail: 'Controller discovery target',
    kind: 'bridge',
    enabled: channel.enabled,
    draggable: true
  });

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

  <section class="bridge-map" aria-labelledby={`${channel.id}-map-heading`}>
    <header>
      <div>
        <h4 id={`${channel.id}-map-heading`}>Controller target</h4>
        <p>Drag the marker or click the map. FL511 discovery searches around this exact point.</p>
      </div>
    </header>
    <LocationMap
      variant="compact"
      points={[]}
      candidate={bridgePoint}
      selectedId={bridgePoint.id}
      {unitSystem}
      ariaLabel="Interactive map for selecting the bridge controller target"
      onpick={setCoordinates}
    />
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

  <div class="source-switches">
    <SwitchField checked={scopeBool(channel, 'useFl511', true)} label="FL511 ground truth" description="Confirm target and upstream bridge controller state." onchange={(enabled) => set('useFl511', enabled)} />
    <SwitchField checked={scopeBool(channel, 'useUpstream', true)} label="Upstream progression" description="Use ordered upstream openings as outbound evidence." onchange={(enabled) => set('useUpstream', enabled)} />
  </div>

  <section class:configured={ais.apiKeyConfigured} class:enabled={ais.enabled} class="ais-source">
    <div class="ais-mark" aria-hidden="true">AIS</div>
    <div>
      <span>Predictive vessel source</span>
      <strong>AISStream approach evidence</strong>
      <small>
        {ais.apiKeyConfigured
          ? `${formatDistanceKilometers(ais.radiusKilometers, unitSystem)} bridge-centered watch.`
          : 'No AISStream key is configured.'}
      </small>
    </div>
    {#if ais.apiKeyConfigured}
      <SwitchField checked={ais.enabled} label={ais.enabled ? 'AIS evidence running' : 'AIS evidence parked'} description="This is the same source circuit shown in Outputs." onchange={(enabled) => onaischange({ ...ais, enabled })} />
    {:else}
      <a href="/outputs#aisstream">Configure AISStream in Outputs →</a>
    {/if}
  </section>
</div>

<style>
  .bridge-scope {
    display: grid;
    gap: 16px;
  }

  .identity-fields,
  .coordinate-fields,
  .source-switches {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
  }

  .coordinate-fields {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .bridge-map {
    display: grid;
    overflow: hidden;
    border: 1px solid var(--rule-strong);
  }

  .bridge-map > header {
    padding: 16px;
    color: var(--white);
    background: var(--marine);
  }

  .bridge-map h4,
  .bridge-map p {
    margin: 0;
  }

  .bridge-map h4 {
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .bridge-map p {
    max-width: 70ch;
    margin-top: 5px;
    color: var(--nav-muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .bridge-map details {
    padding: 14px 16px 16px;
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
  }

  .bridge-map summary {
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

  .ais-source {
    display: grid;
    grid-template-columns: 48px minmax(0, 1fr) minmax(230px, 0.78fr);
    align-items: center;
    gap: 15px;
    padding: 16px;
    background: var(--frost);
    border: 1px dashed var(--steel);
  }

  .ais-source.configured {
    background: var(--paper);
    border-style: solid;
    border-color: var(--rule-strong);
  }

  .ais-source.enabled {
    box-shadow: inset 0 -3px 0 var(--success);
  }

  .ais-mark {
    display: grid;
    width: 46px;
    height: 46px;
    place-items: center;
    color: var(--white);
    background: var(--steel);
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    font-weight: 700;
  }

  .ais-source.configured .ais-mark {
    background: var(--marine);
  }

  .ais-source.enabled .ais-mark {
    background: var(--success);
  }

  .ais-source > div:nth-child(2) {
    display: grid;
    min-width: 0;
    gap: 3px;
  }

  .ais-source span,
  .ais-source strong {
    font-family: var(--font-instrument);
    text-transform: uppercase;
  }

  .ais-source span {
    color: var(--muted);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
  }

  .ais-source strong {
    font-size: var(--type-title);
    line-height: 1;
  }

  .ais-source small {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .ais-source > a {
    justify-self: end;
    color: var(--channel);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 700;
    text-align: right;
    text-transform: uppercase;
    text-underline-offset: 4px;
  }

  @media (max-width: 680px) {
    .identity-fields,
    .coordinate-fields,
    .source-switches {
      grid-template-columns: 1fr;
    }

    .ais-source {
      grid-template-columns: 46px minmax(0, 1fr);
    }

    .ais-source > :global(.switch-field),
    .ais-source > a {
      grid-column: 2;
      justify-self: stretch;
      text-align: left;
    }
  }
</style>
