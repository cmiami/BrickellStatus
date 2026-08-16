<script lang="ts">
  import SwitchField from '$lib/components/SwitchField.svelte';
  import type { AlertArea, ChannelPreference, UnitSystem } from '$lib/types';
  import { windDisplayToMph, windMphForDisplay } from '$lib/units';
  import AreaCoverage from './AreaCoverage.svelte';
  import { scopeBool, scopeNumber, setScope, type ChannelChange } from './scope';

  let { channel, areas, unitSystem, onchannelchange }: {
    channel: ChannelPreference;
    areas: AlertArea[];
    unitSystem: UnitSystem;
    onchannelchange: ChannelChange;
  } = $props();

  const set = (key: string, value: number | boolean) => setScope(channel, onchannelchange, key, value);

  function windInput(event: Event) {
    const displayed = (event.currentTarget as HTMLInputElement).valueAsNumber;
    const mph = windDisplayToMph(displayed, unitSystem);
    set('windGustMph', Number.isFinite(mph) ? Number(mph.toFixed(2)) : 40);
  }
</script>

<div class="weather-scope">
  <AreaCoverage {channel} {areas} {onchannelchange} />
  <div class="rulebook">
    <!-- Rain has no threshold to set. It reads the forecast’s 15-minute
         precipitation amounts and says when rain starts, which is an answer;
         a probability slider only ever asked the reader to guess. -->
    <SwitchField
      checked={scopeBool(channel, 'rainAlertEnabled', true)}
      label="Rain alerts"
      description="Warn when rain is about to start, with the minutes until it does."
      onchange={(enabled) => set('rainAlertEnabled', enabled)}
    />
    <SwitchField
      checked={scopeBool(channel, 'radarEnabled', true)}
      label="Radar"
      description="Show the radar composite on the map and beside rain on the display."
      onchange={(enabled) => set('radarEnabled', enabled)}
    />
    <div class="wind-rule">
      <SwitchField
        checked={scopeBool(channel, 'windAlertEnabled', true)}
        label="Wind alerts"
        description="Warn on forecast gusts above your own threshold."
        onchange={(enabled) => set('windAlertEnabled', enabled)}
      />
      <label class="field gust-field">
        <span>Above</span>
        <input
          type="number"
          min={unitSystem === 'metric' ? 16 : 10}
          max={unitSystem === 'metric' ? 257 : 160}
          value={Number(windMphForDisplay(scopeNumber(channel, 'windGustMph', 40), unitSystem).toFixed(1))}
          disabled={!scopeBool(channel, 'windAlertEnabled', true)}
          oninput={windInput}
        />
        <small class="field-note">{unitSystem === 'metric' ? 'km/h' : 'mph'}</small>
      </label>
    </div>
  </div>
  <p class="scope-note">Forecasts come from Open-Meteo and radar from RainViewer. Official warnings are a separate channel.</p>
</div>

<style>
  .weather-scope,
  .rulebook {
    display: grid;
    gap: 14px;
  }

  .rulebook {
    padding: 16px;
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .wind-rule {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 16px;
    align-items: end;
  }

  .gust-field {
    display: grid;
    gap: 4px;
    width: 130px;
  }

  .scope-note {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  @media (max-width: 720px) {
    .wind-rule {
      grid-template-columns: 1fr;
      align-items: start;
    }
  }
</style>
