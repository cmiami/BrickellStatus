<!--
A double-leaf bascule seen from above, crossing the drawn water.

Closed, the two leaves meet at midspan and the roadway reads as one unbroken
band across the channel — a road you can drive. Open, each lifted leaf
foreshortens toward its own pier and the channel reads clear through the gap,
which is what an opened bascule actually looks like from above. Colours name
the road, exactly as everywhere else: green passable, red stopped, steel
unknown — and the broken band carries the state without them.

Drawn in channel-local coordinates: the caller rotates the mark so local y
runs across the water. Everything scales from the ribbon's half-width at the
span, so a mark far upriver stays proportional to its narrower water.
-->
<script lang="ts">
  let {
    state = 'unknown',
    halfWidth,
    target = false
  }: {
    state?: 'up' | 'down' | 'unknown';
    /** Drawn half-width of the water ribbon at this span. */
    halfWidth: number;
    /** The Brickell span draws heavier than the rest. */
    target?: boolean;
  } = $props();

  /** The roadway reaches just past the water onto its abutments. */
  const reach = $derived(halfWidth + (target ? 5 : 3.5));
  const road = $derived(target ? 7 : 4.5);
  const abutment = $derived(target ? 6 : 4);
  /** Each leaf runs from its pier to a hair short of midspan. */
  const leaf = $derived(reach - 0.8);
</script>

<g class="span-mark" class:is-target={target} data-state={state}>
  <rect
    class="abutment"
    x={-abutment / 2 - road / 2}
    y={-reach - abutment}
    width={road + abutment}
    height={abutment}
  />
  <rect
    class="abutment"
    x={-abutment / 2 - road / 2}
    y={reach}
    width={road + abutment}
    height={abutment}
  />
  <!-- Each leaf hinges over its own pier; the group carries the static
       placement so the CSS rotation beneath it can animate cleanly. -->
  <g transform="translate(0 {-reach})">
    <g class="leaf leaf-near">
      <rect x={-road / 2} y="0" width={road} height={leaf} />
    </g>
  </g>
  <g transform="translate(0 {reach})">
    <g class="leaf leaf-far">
      <rect x={-road / 2} y={-leaf} width={road} height={leaf} />
    </g>
  </g>
</g>

<style>
  .abutment {
    fill: var(--steel);
  }

  .leaf rect {
    fill: var(--success);
    stroke: var(--white);
    stroke-width: 0.9;
    transition: fill 500ms ease-out;
  }

  .span-mark[data-state='up'] .leaf rect {
    fill: var(--danger);
  }

  .span-mark[data-state='unknown'] .leaf rect {
    fill: var(--steel);
  }

  .is-target .leaf rect {
    stroke-width: 1.2;
  }

  /* The one authored movement on the chart: seen from above, a lifting leaf
     foreshortens toward its own pier, so the roadway pulls back from midspan
     and the channel reads clear through the gap. */
  .leaf {
    transform-box: fill-box;
    transition: transform 900ms cubic-bezier(0.32, 0.06, 0.2, 1);
  }

  .leaf-near {
    transform-origin: 50% 0%;
  }

  .leaf-far {
    transform-origin: 50% 100%;
  }

  .span-mark[data-state='up'] .leaf-near,
  .span-mark[data-state='up'] .leaf-far {
    transform: scaleY(0.24);
  }

  @media (prefers-reduced-motion: reduce) {
    .leaf,
    .leaf rect {
      transition: none;
    }
  }
</style>
