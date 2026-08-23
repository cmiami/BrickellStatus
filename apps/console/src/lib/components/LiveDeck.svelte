<!--
THESIS: Brickell is the interchange every live route resolves against; this
surface refuses the category's top-down map and detached dashboard cards.
OWN-WORLD: Cool dispatch paper, marine rules, muscular violet transit lines,
condensed civic lettering, mechanical bascule marks, and one rationed amber hull.
STORY: Read Brickell's road state, follow the vessel that can change it, then
read that vessel's plain type, direction, knots, and ETA without leaving its route.
FIRST VIEWPORT: A compact decision band above a full-field schematic; a traffic
rail docks only when boats exist. Brickell is 3–4× every other bridge.
FORM: First-ranked Brickell Interchange fused with visible-transit-network
staging and the approved map/ETA/mechanism comp combination; seed 55e9df82.
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
    generatedAt,
    localTimeZone
  }: {
    decision: DecisionSnapshot;
    corridor: RiverCorridor;
    vesselTracks?: VesselTrack[];
    intervals?: BridgeStateInterval[];
    crossings?: BridgeCrossing[];
    generatedAt?: string;
    localTimeZone?: string;
  } = $props();
</script>

<section class="live-deck" data-state={decision.state}>
  <StatusDecision {decision} band />
  <RiverLine {corridor} {vesselTracks} {intervals} {crossings} {generatedAt} {localTimeZone} />
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
    height: 100%;
    min-height: 0;
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
  @media (max-width: 1180px) {
    .live-deck {
      height: auto;
      min-height: 0;
    }
  }
</style>
