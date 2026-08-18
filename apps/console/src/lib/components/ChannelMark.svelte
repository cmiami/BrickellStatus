<!--
A channel mark: the buoy a seaward waypoint actually is.

Drawn as a lit buoy on its waterline — body, cage, topmark — so the entrance
channels read as marked water rather than as a row of dots. The mouth, where
both approaches meet the river, gets the larger junction mark.
-->
<script lang="ts">
  let { junction = false }: { junction?: boolean } = $props();
</script>

<g class="channel-mark" class:junction>
  {#if junction}
    <!-- Junction: taller, with a doubled topmark for the fork. -->
    <path class="body" d="M-4.5 6 L4.5 6 L3 -2 L-3 -2 Z" />
    <path class="cage" d="M-3 -2 L3 -2 L1.8 -7 L-1.8 -7 Z" />
    <circle class="topmark" cx="0" cy="-9.5" r="2.4" />
  {:else}
    <path class="body" d="M-3.6 5 L3.6 5 L2.4 -1.5 L-2.4 -1.5 Z" />
    <path class="cage" d="M-2.4 -1.5 L2.4 -1.5 L1.4 -5.5 L-1.4 -5.5 Z" />
    <circle class="topmark" cx="0" cy="-7.4" r="1.9" />
  {/if}
</g>

<style>
  .body {
    fill: var(--corridor);
    stroke: var(--white);
    stroke-width: 1;
    stroke-linejoin: round;
  }

  .cage {
    fill: none;
    stroke: var(--corridor);
    stroke-width: 1.3;
    stroke-linejoin: round;
  }

  .topmark {
    fill: var(--white);
    stroke: var(--corridor);
    stroke-width: 1.4;
  }

  /* The mouth is where both entrance channels meet the river; it carries the
     weight of a junction mark so the fork is legible at a glance. */
  .channel-mark.junction .topmark {
    fill: var(--corridor);
  }
</style>
