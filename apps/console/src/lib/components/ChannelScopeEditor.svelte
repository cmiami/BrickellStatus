<script lang="ts">
  import BridgeScopeEditor from './channel-scope/BridgeScopeEditor.svelte';
  import EarthquakeScopeEditor from './channel-scope/EarthquakeScopeEditor.svelte';
  import HurricaneScopeEditor from './channel-scope/HurricaneScopeEditor.svelte';
  import MarketScopeEditor from './channel-scope/MarketScopeEditor.svelte';
  import NewsScopeEditor from './channel-scope/NewsScopeEditor.svelte';
  import OfficialScopeEditor from './channel-scope/OfficialScopeEditor.svelte';
  import SportsScopeEditor from './channel-scope/SportsScopeEditor.svelte';
  import WeatherScopeEditor from './channel-scope/WeatherScopeEditor.svelte';
  import type { ChannelChange } from './channel-scope/scope';
  import type { AisSettings, AlertArea, ChannelPreference, UnitSystem } from '$lib/types';

  let {
    channel,
    areas = [],
    ais,
    unitSystem,
    onchannelchange,
    onaischange,
    onareaadd
  }: {
    channel: ChannelPreference;
    areas?: AlertArea[];
    ais: AisSettings;
    unitSystem: UnitSystem;
    onchannelchange: ChannelChange;
    onaischange: (ais: AisSettings) => void;
    onareaadd?: (area: AlertArea) => void;
  } = $props();
</script>

<div class="scope-editor">
  {#if channel.kind === 'bridge'}
    <BridgeScopeEditor {channel} {ais} {unitSystem} {onchannelchange} {onaischange} />
  {:else if channel.kind === 'weather'}
    <WeatherScopeEditor {channel} {areas} {unitSystem} {onchannelchange} {onareaadd} />
  {:else if channel.kind === 'official'}
    <OfficialScopeEditor {channel} {areas} {unitSystem} {onchannelchange} {onareaadd} />
  {:else if channel.kind === 'hurricane'}
    <HurricaneScopeEditor {channel} {onchannelchange} />
  {:else if channel.kind === 'news'}
    <NewsScopeEditor {channel} {onchannelchange} />
  {:else if channel.kind === 'sports'}
    <SportsScopeEditor {channel} {onchannelchange} />
  {:else if channel.kind === 'earthquake'}
    <EarthquakeScopeEditor {channel} {onchannelchange} />
  {:else if channel.kind === 'markets'}
    <MarketScopeEditor {channel} {onchannelchange} />
  {:else}
    <p>This channel has no editable content scope.</p>
  {/if}
</div>

<style>
  .scope-editor {
    display: grid;
    gap: 14px;
  }

  .scope-editor > p {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }
</style>
