<script lang="ts">
  import SwitchField from '$lib/components/SwitchField.svelte';
  import type { AlertArea, ChannelPreference, UnitSystem } from '$lib/types';
  import { windDisplayToMph, windMphForDisplay } from '$lib/units';
  import AreaCoverage from './AreaCoverage.svelte';
  import { scopeBool, scopeNumber, setScope, type ChannelChange } from './scope';

  let { channel, areas, unitSystem, onchannelchange, onareaadd, onareachange, onarearemove }: {
    channel: ChannelPreference;
    areas: AlertArea[];
    unitSystem: UnitSystem;
    onchannelchange: ChannelChange;
    onareaadd?: (area: AlertArea) => void;
    onareachange?: (area: AlertArea) => void;
    onarearemove?: (area: AlertArea) => void;
  } = $props();

  const set = (key: string, value: number | boolean) => setScope(channel, onchannelchange, key, value);

  // Quarter hours, because that is the resolution the forecast arrives in: the
  // minutely feed is binned to fifteen minutes, so anything finer would be a
  // control that could not change the answer.
  const RAIN_LEAD_CHOICES = [15, 30, 60, 120] as const;

  function leadLabel(minutes: number): string {
    return minutes < 60 ? `${minutes} min` : `${minutes / 60} hr`;
  }

  function windInput(event: Event) {
    const displayed = (event.currentTarget as HTMLInputElement).valueAsNumber;
    const mph = windDisplayToMph(displayed, unitSystem);
    set('windGustMph', Number.isFinite(mph) ? Number(mph.toFixed(2)) : 40);
  }
</script>

<div class="weather-scope">
  <AreaCoverage {channel} {areas} {unitSystem} {onchannelchange} {onareaadd} {onareachange} {onarearemove} />
  <div class="rulebook">
    <!-- Rain has no threshold to set. It reads the forecast’s 15-minute
         precipitation amounts and says when rain starts, which is an answer;
         a probability slider only ever asked the reader to guess. -->
    <SwitchField
      checked={scopeBool(channel, 'rainAlertEnabled', true)}
      label="Rain"
      description="Include rain that is falling now or likely to begin soon."
      onchange={(enabled) => set('rainAlertEnabled', enabled)}
    />
    <div class="rain-lead" role="group" aria-label="How far ahead to warn about rain">
      <span class="rain-lead-label">Warn me up to</span>
      <div class="rain-lead-choices">
        {#each RAIN_LEAD_CHOICES as minutes (minutes)}
          <button
            type="button"
            class="rain-lead-choice"
            aria-pressed={scopeNumber(channel, 'rainWindowMinutes', 30) === minutes}
            disabled={!scopeBool(channel, 'rainAlertEnabled', true)}
            onclick={() => set('rainWindowMinutes', minutes)}
          >
            {leadLabel(minutes)}
          </button>
        {/each}
      </div>
      <small class="field-note">
        Rain expected further out than this waits until it is closer. The panel and
        the alert name the place it is coming to.
      </small>
    </div>
    <SwitchField
      checked={scopeBool(channel, 'radarEnabled', true)}
      label="Radar"
      description="Show the radar composite on the map and beside rain on the display."
      onchange={(enabled) => set('radarEnabled', enabled)}
    />
    <div class="wind-rule">
      <SwitchField
        checked={scopeBool(channel, 'windAlertEnabled', true)}
        label="Strong wind"
        description="Include forecast gusts above this threshold."
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
  .rain-lead {
    display: grid;
    gap: 8px;
    padding: 12px 14px;
    background: var(--paper);
    border: 1px solid var(--rule);
  }

  .rain-lead-label {
    color: var(--muted);
    font-size: var(--type-label);
    text-transform: uppercase;
  }

  .rain-lead-choices {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .rain-lead-choice {
    /* Past the touch minimum: these sit side by side and are the whole reason
       someone opened this panel. */
    min-width: 76px;
    min-height: 44px;
    color: var(--ink);
    background: var(--white);
    border: 1px solid var(--rule-strong);
    cursor: pointer;
  }

  .rain-lead-choice[aria-pressed='true'] {
    color: var(--white);
    background: var(--marine);
    border-color: var(--marine);
  }

  .rain-lead-choice:disabled {
    opacity: 0.45;
    cursor: default;
  }

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
