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
    <section class:disabled={!scopeBool(channel, 'rainAlertEnabled', true)}>
      <SwitchField
        checked={scopeBool(channel, 'rainAlertEnabled', true)}
        label="Rain heads-up"
        description="Evaluate forecast rain only while this rule is enabled."
        onchange={(enabled) => set('rainAlertEnabled', enabled)}
      />
      <div class="rule-fields">
        <label class="field">
          <span>Probability</span>
          <input type="number" min="1" max="100" value={scopeNumber(channel, 'rainProbabilityThreshold', 60)} disabled={!scopeBool(channel, 'rainAlertEnabled', true)} oninput={(event) => set('rainProbabilityThreshold', event.currentTarget.valueAsNumber)} />
          <small class="field-note">Percent required.</small>
        </label>
        <label class="field">
          <span>Lead window</span>
          <input type="number" min="0" max="1440" value={scopeNumber(channel, 'rainLeadMinutes', 90)} disabled={!scopeBool(channel, 'rainAlertEnabled', true)} oninput={(event) => set('rainLeadMinutes', event.currentTarget.valueAsNumber)} />
          <small class="field-note">Minutes ahead.</small>
        </label>
      </div>
    </section>
    <section class:disabled={!scopeBool(channel, 'windAlertEnabled', true)}>
      <SwitchField
        checked={scopeBool(channel, 'windAlertEnabled', true)}
        label="Wind-gust heads-up"
        description="Evaluate forecast gusts only while this rule is enabled."
        onchange={(enabled) => set('windAlertEnabled', enabled)}
      />
      <label class="field gust-field">
        <span>Gust threshold</span>
        <input
          type="number"
          min={unitSystem === 'metric' ? 16 : 10}
          max={unitSystem === 'metric' ? 257 : 160}
          value={Number(windMphForDisplay(scopeNumber(channel, 'windGustMph', 40), unitSystem).toFixed(1))}
          disabled={!scopeBool(channel, 'windAlertEnabled', true)}
          oninput={windInput}
        />
        <small class="field-note">{unitSystem === 'metric' ? 'Kilometers per hour.' : 'Miles per hour.'}</small>
      </label>
    </section>
  </div>
  <p class="scope-note">Open-Meteo supplies forecasts. NWS warnings remain a separate official channel.</p>
</div>

<style>
  .weather-scope,
  .rulebook {
    display: grid;
    gap: 14px;
  }

  .rulebook {
    grid-template-columns: 1fr 1fr;
  }

  .rulebook section {
    display: grid;
    align-content: start;
    gap: 14px;
    padding: 16px;
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .rulebook section.disabled {
    opacity: 0.58;
  }

  .rule-fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  .gust-field {
    max-width: 260px;
  }

  .scope-note {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  @media (max-width: 720px) {
    .rulebook,
    .rule-fields {
      grid-template-columns: 1fr;
    }
  }
</style>
