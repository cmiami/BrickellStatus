<script lang="ts">
  import SwitchField from '$lib/components/SwitchField.svelte';
  import type { ChannelPreference } from '$lib/types';
  import { scopeBool, setScope, type ChannelChange } from './scope';

  let { channel, onchannelchange }: { channel: ChannelPreference; onchannelchange: ChannelChange } = $props();
</script>

<div class="hurricane-scope">
  <section class="source-register" aria-labelledby={`${channel.id}-source-heading`}>
    <header>
      <div>
        <p class="registration-label">Official sources</p>
        <h4 id={`${channel.id}-source-heading`}>National Hurricane Center</h4>
      </div>
      <span>2 live feeds</span>
    </header>
    <div class="source-rows">
      <div>
        <strong>CurrentStorms</strong>
        <span>Active systems, position, intensity, movement, and official products.</span>
      </div>
      <div>
        <strong>Atlantic outlook</strong>
        <span>Official basin-wide development context.</span>
      </div>
    </div>
  </section>

  <section class:enabled={scopeBool(channel, 'allAtlanticSystems', false)} class="activation-rule">
    <div>
      <p class="registration-label">Activation</p>
      <strong>{scopeBool(channel, 'allAtlanticSystems', false) ? 'Every active Atlantic cyclone' : 'Context only'}</strong>
      <span>{scopeBool(channel, 'allAtlanticSystems', false)
        ? 'Any active Atlantic cyclone may activate this channel.'
        : 'Official products remain visible without taking over the display.'}</span>
    </div>
    <SwitchField
      checked={scopeBool(channel, 'allAtlanticSystems', false)}
      label="Activate for every Atlantic cyclone"
      description="Basin-wide status, not a local threat calculation."
      onchange={(enabled) => setScope(channel, onchannelchange, 'allAtlanticSystems', enabled)}
    />
  </section>

  <p class="scope-note">NWS remains the authority for location-specific warnings.</p>
</div>

<style>
  .hurricane-scope {
    display: grid;
    gap: 14px;
  }

  .source-register,
  .activation-rule {
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .source-register > header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 20px;
    padding: 16px;
    color: var(--white);
    background: var(--marine);
  }

  .source-register h4,
  .source-register p,
  .activation-rule p {
    margin: 0;
  }

  .source-register h4 {
    margin-top: 4px;
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .source-register header > span {
    color: var(--nav-muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .source-rows {
    display: grid;
    grid-template-columns: 1fr 1fr;
  }

  .source-rows > div {
    display: grid;
    gap: 4px;
    padding: 16px;
    border-inline-end: 1px solid var(--rule);
  }

  .source-rows > div:last-child {
    border-inline-end: 0;
  }

  .source-rows strong,
  .activation-rule strong {
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    text-transform: uppercase;
  }

  .source-rows span,
  .activation-rule span,
  .scope-note {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  .activation-rule {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(260px, 0.8fr);
    align-items: center;
    gap: 22px;
    padding: 16px;
  }

  .activation-rule.enabled {
    background: var(--amber-sheet);
  }

  .activation-rule > div {
    display: grid;
    gap: 5px;
  }

  .scope-note {
    margin: 0;
  }

  @media (max-width: 720px) {
    .source-rows,
    .activation-rule {
      grid-template-columns: 1fr;
    }

    .source-rows > div {
      border-inline-end: 0;
      border-bottom: 1px solid var(--rule);
    }

    .source-rows > div:last-child {
      border-bottom: 0;
    }
  }
</style>
