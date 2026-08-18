<!--
A double-leaf bascule at a glance: each leaf is hinged over its own pier and the
free ends meet at midspan when closed. Opening lifts both away from the centre,
which is the movement a driver actually sees.

Green closed, red open — the colours name the road, not the river: a closed span
is a road you can drive, an open one has stopped you. State is carried by the
shape as well as the colour, so it survives a monochrome or colour-blind read.
-->
<script lang="ts">
  let {
    state = 'unknown',
    size = 26,
    title,
    inline = false
  }: {
    state?: 'up' | 'down' | 'unknown';
    size?: number;
    /** Omit inside a labelled row; supply it where the mark stands alone. */
    title?: string;
    /**
     * Draw as a bare group for nesting inside an existing SVG, centred on the
     * origin, rather than as a standalone element with its own viewport.
     */
    inline?: boolean;
  } = $props();
</script>

{#if inline}
  <!-- Nested in a parent SVG: a group centred on the origin, so the caller
       positions it like any other mark on the line. -->
  <g class="bascule-mark" data-state={state} transform="translate(-17 -12)">
    {#if title}<title>{title}</title>{/if}
  <line class="mark-water" x1="0" y1="16.5" x2="34" y2="16.5" />
    <rect class="mark-approach" x="0" y="8" width="6" height="3" />
    <rect class="mark-approach" x="28" y="8" width="6" height="3" />
    <rect class="mark-pier" x="5" y="10" width="2.4" height="6.5" />
    <rect class="mark-pier" x="26.6" y="10" width="2.4" height="6.5" />
    <rect class="mark-leaf mark-leaf-left" x="6" y="8" width="11" height="3" />
    <rect class="mark-leaf mark-leaf-right" x="17" y="8" width="11" height="3" />
  </g>
{:else}
  <svg
    class="bascule-mark"
    data-state={state}
    viewBox="0 0 34 20"
    width={size}
    height={(size / 34) * 20}
    role={title ? 'img' : 'presentation'}
    aria-hidden={title ? undefined : 'true'}
    aria-label={title}
  >
    {#if title}<title>{title}</title>{/if}
  <line class="mark-water" x1="0" y1="16.5" x2="34" y2="16.5" />
    <rect class="mark-approach" x="0" y="8" width="6" height="3" />
    <rect class="mark-approach" x="28" y="8" width="6" height="3" />
    <rect class="mark-pier" x="5" y="10" width="2.4" height="6.5" />
    <rect class="mark-pier" x="26.6" y="10" width="2.4" height="6.5" />
    <rect class="mark-leaf mark-leaf-left" x="6" y="8" width="11" height="3" />
    <rect class="mark-leaf mark-leaf-right" x="17" y="8" width="11" height="3" />
  </svg>
{/if}

<style>
  svg.bascule-mark {
    display: block;
    flex: none;
  }

  .mark-water {
    stroke: var(--corridor);
    stroke-width: 1.1;
    opacity: 0.45;
  }

  .mark-approach,
  .mark-pier {
    fill: var(--steel);
  }

  .mark-leaf {
    fill: var(--success);
    transform-box: view-box;
    transition:
      transform 700ms cubic-bezier(0.32, 0.06, 0.2, 1),
      fill 500ms ease-out;
  }

  .bascule-mark[data-state='up'] .mark-leaf {
    fill: var(--danger);
  }

  .bascule-mark[data-state='unknown'] .mark-leaf {
    fill: var(--steel);
  }

  /* Each leaf turns about its own pier, so the free end at midspan rises. */
  .mark-leaf-left {
    transform-origin: 6px 9.5px;
  }

  .mark-leaf-right {
    transform-origin: 28px 9.5px;
  }

  .bascule-mark[data-state='up'] .mark-leaf-left {
    transform: rotate(-62deg);
  }

  .bascule-mark[data-state='up'] .mark-leaf-right {
    transform: rotate(62deg);
  }

  @media (prefers-reduced-motion: reduce) {
    .mark-leaf {
      transition: none;
    }
  }
</style>
