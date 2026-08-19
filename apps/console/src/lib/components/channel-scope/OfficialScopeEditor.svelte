<script lang="ts">
  import SwitchField from '$lib/components/SwitchField.svelte';
  import type { AlertArea, ChannelPreference, UnitSystem } from '$lib/types';
  import AreaCoverage from './AreaCoverage.svelte';
  import { scopeBool, scopeList, setScope, toggleScopeList, type ChannelChange } from './scope';

  let { channel, areas, unitSystem, onchannelchange, onareaadd }: {
    channel: ChannelPreference;
    areas: AlertArea[];
    unitSystem: UnitSystem;
    onchannelchange: ChannelChange;
    onareaadd?: (area: AlertArea) => void;
  } = $props();
</script>

<div class="official-scope">
  <AreaCoverage {channel} {areas} {unitSystem} {onchannelchange} {onareaadd} />
  <fieldset class="severity-register">
    <legend>Qualifying severities</legend>
    {#each ['Moderate', 'Severe', 'Extreme'] as severity}
      <label>
        <input
          type="checkbox"
          checked={scopeList(channel, 'severity').includes(severity)}
          onchange={(event) => toggleScopeList(channel, onchannelchange, 'severity', severity, event.currentTarget.checked)}
        />
        <span>
          <strong>{severity}</strong>
          <small>{severity === 'Moderate' ? 'Advisory-level awareness' : severity === 'Severe' ? 'Significant threat to life or property' : 'Extraordinary threat'}</small>
        </span>
      </label>
    {/each}
  </fieldset>
  <SwitchField
    checked={scopeBool(channel, 'includeStatements', false)}
    label="Include statements"
    description="Show non-warning NWS statements in rotation; interrupt policy still applies."
    onchange={(enabled) => setScope(channel, onchannelchange, 'includeStatements', enabled)}
  />
</div>

<style>
  .official-scope {
    display: grid;
    gap: 14px;
  }

  .severity-register {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    margin: 0;
    border: 0;
    padding: 0;
  }

  .severity-register legend {
    width: 100%;
    margin-bottom: 8px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .severity-register label {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 10px;
    padding: 14px;
    background: var(--paper);
    border-block: 1px solid var(--rule);
    border-inline-end: 1px solid var(--rule);
  }

  .severity-register label:last-child {
    border-inline-end: 0;
  }

  .severity-register input {
    width: 18px;
    height: 18px;
    margin: 1px 0 0;
    accent-color: var(--marine);
  }

  .severity-register span {
    display: grid;
    gap: 3px;
  }

  .severity-register strong {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    text-transform: uppercase;
  }

  .severity-register small {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.35;
  }

  @media (max-width: 720px) {
    .severity-register {
      grid-template-columns: 1fr;
    }

    .severity-register label {
      border-inline-end: 0;
      border-bottom: 0;
    }

    .severity-register label:last-child {
      border-bottom: 1px solid var(--rule);
    }
  }
</style>
