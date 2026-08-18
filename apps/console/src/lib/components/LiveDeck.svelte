<!--
THESIS: One tile answers the whole question — what the bridge is doing, and what
on the water is about to change it — inside the first viewport, without a scroll.
FORM: A single ruled sheet. A decision band across the head, and beneath it the
river at full width, which is the element the page exists to show.
-->
<script lang="ts">
  import RiverLine from './RiverLine.svelte';
  import StatusDecision from './StatusDecision.svelte';
  import type {
    BridgeCrossing,
    BridgeStateInterval,
    DecisionSnapshot,
    RiverCorridor,
    VesselTrack
  } from '$lib/types';

  let {
    decision,
    corridor,
    vesselTracks = [],
    intervals = [],
    crossings = [],
    localTimeZone
  }: {
    decision: DecisionSnapshot;
    corridor: RiverCorridor;
    vesselTracks?: VesselTrack[];
    intervals?: BridgeStateInterval[];
    crossings?: BridgeCrossing[];
    localTimeZone?: string;
  } = $props();
</script>

<section class="live-deck" data-state={decision.state}>
  <StatusDecision {decision} band />
  <RiverLine {corridor} {vesselTracks} {intervals} {crossings} {localTimeZone} />
</section>

<style>
  /*
    Sized to the viewport, not to its contents: this tile is the thing a driver
    reads in the seconds before turning, so it must never be the top half of
    something they have to scroll to finish.
  */
  /*
    The river is the subject and takes the room. The decision is one band above
    it — the same words at the same size, laid across instead of down, because
    a column of them was pushing the map into a strip.
  */
  .live-deck {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    height: calc(100vh - 72px);
    min-height: 560px;
    overflow: hidden;
    background: var(--paper);
    border-bottom: 1px solid var(--rule-strong);
  }

  /* The chart component manages its own chart-and-ledger split; the tile only
     hands it the full remaining height and forbids it from growing the page. */
  .live-deck > :global(.river) {
    height: 100%;
    min-height: 0;
    overflow: hidden;
    border-top: 0;
  }

  /* On a phone the tile stops being a viewport and becomes a page again. */
  @media (max-width: 900px) {
    .live-deck {
      height: auto;
      min-height: 0;
    }
  }
</style>
