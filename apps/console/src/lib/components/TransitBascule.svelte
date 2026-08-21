<!--
An elevation-style double-leaf bascule for the transit schematic.

The group is centred on the bridge lock at (0, 0), so callers can translate
or rotate it as one station mark. The geometry does the semantic work: a
continuous roadway is closed to water traffic, raised leaves expose the
channel, and an unknown reading breaks both leaves into dashed, shortened
halves. Colour reinforces that reading but never owns it.
-->
<script lang="ts">
  let {
    state,
    hero = false,
    title
  }: {
    state: 'up' | 'down' | 'unknown';
    hero?: boolean;
    title?: string;
  } = $props();

  const unit = $derived(hero ? 1 : 0.62);
  const edge = $derived(58 * unit);
  const pivot = $derived(29 * unit);
  const deck = $derived(7 * unit);
  const pier = $derived(7.5 * unit);
  const water = $derived(15 * unit);
  const foundation = $derived(11 * unit);
  const pivotRadius = $derived(4.2 * unit);
  const barrierOffset = $derived(10.5 * unit);
  const barrierLength = $derived(13 * unit);
  const barrierY = $derived(-deck / 2 - 3.2 * unit);

  const stateTitle = $derived(
    state === 'up'
      ? 'Bridge up; both leaves are raised and the road is stopped.'
      : state === 'down'
        ? 'Bridge down; both leaves meet and the road is moving.'
        : 'Bridge state unknown; no confirmed up or down reading.'
  );
  const accessibleTitle = $derived(title?.trim() || stateTitle);

  const leftLeaf = $derived(
    `M0 ${(-deck / 2).toFixed(2)} H${(pivot - 1.3 * unit).toFixed(2)} ` +
      `L${pivot.toFixed(2)} ${(-deck * 0.18).toFixed(2)} V${(deck * 0.18).toFixed(2)} ` +
      `L${(pivot - 1.3 * unit).toFixed(2)} ${(deck / 2).toFixed(2)} H0 Z`
  );
  const rightLeaf = $derived(
    `M0 ${(-deck / 2).toFixed(2)} H${(-pivot + 1.3 * unit).toFixed(2)} ` +
      `L${(-pivot).toFixed(2)} ${(-deck * 0.18).toFixed(2)} V${(deck * 0.18).toFixed(2)} ` +
      `L${(-pivot + 1.3 * unit).toFixed(2)} ${(deck / 2).toFixed(2)} H0 Z`
  );
</script>

<g
  class="transit-bascule"
  class:is-hero={hero}
  data-state={state}
  data-scale={hero ? 'hero' : 'mini'}
  role="img"
  aria-label={accessibleTitle}
>
  <title>{accessibleTitle}</title>

  <!-- The tracked route is always present. Only a confirmed open span gives
       its centre dashes motion, and even then the movement stays quiet. -->
  <line class="water-bed" x1={-edge} y1={water} x2={edge} y2={water} />
  <line class="channel-flow" x1={-edge} y1={water} x2={edge} y2={water} />

  <!-- Concrete piers and wider footings keep the bridge mechanically legible
       when the road leaves are in motion. -->
  <g class="civil-work" aria-hidden="true">
    <rect
      class="pier"
      x={-pivot - pier / 2}
      y={deck / 2}
      width={pier}
      height={water + foundation * 0.74 - deck / 2}
    />
    <rect
      class="pier"
      x={pivot - pier / 2}
      y={deck / 2}
      width={pier}
      height={water + foundation * 0.74 - deck / 2}
    />
    <rect
      class="footing"
      x={-pivot - foundation / 2}
      y={water + foundation * 0.56}
      width={foundation}
      height={foundation * 0.34}
    />
    <rect
      class="footing"
      x={pivot - foundation / 2}
      y={water + foundation * 0.56}
      width={foundation}
      height={foundation * 0.34}
    />
  </g>

  {#if hero}
    <!-- Brickell earns architectural mass at hero scale: broad concrete
         abutments, its single bay-side operator house, parapets and windows.
         Mini stations keep the distilled mechanical mark. -->
    <g class="hero-architecture" aria-hidden="true">
      <path class="hero-abutment" d="M-58 4H-35V25H-54L-58 19Z" />
      <path class="hero-abutment" d="M58 4H35V25H54L58 19Z" />
      <path class="hero-parapet" d="M-58 2V-2H-35V2 M58 2V-2H35V2" />

      <g class="control-house control-house-bay" transform="translate(37 -18)">
        <path class="house-roof" d="M-2 3L7 -3L16 3Z" />
        <rect class="house-body" x="0" y="3" width="14" height="18" />
        <rect class="house-window" x="3" y="7" width="3" height="5" />
        <rect class="house-window" x="8" y="7" width="3" height="5" />
      </g>

      <g class="approach-rail">
        <path d="M-58 -6H-30 M30 -6H58" />
        <path d="M-54 -6V-2 M-48 -6V-2 M-42 -6V-2 M-36 -6V-2" />
        <path d="M54 -6V-2 M48 -6V-2 M42 -6V-2 M36 -6V-2" />
      </g>
    </g>
  {/if}

  <!-- Fixed roadway approaches. Their upper and lower rules continue through
       the leaves only in the confirmed-down geometry. -->
  <path
    class="approach approach-left"
    d={`M${(-edge).toFixed(2)} ${(-deck / 2).toFixed(2)} H${(-pivot).toFixed(2)} V${(
      deck / 2
    ).toFixed(2)} H${(-edge).toFixed(2)} Z`}
  />
  <path
    class="approach approach-right"
    d={`M${pivot.toFixed(2)} ${(-deck / 2).toFixed(2)} H${edge.toFixed(2)} V${(
      deck / 2
    ).toFixed(2)} H${pivot.toFixed(2)} Z`}
  />
  <line class="road-rule" x1={-edge} y1={0} x2={-pivot} y2={0} />
  <line class="road-rule" x1={pivot} y1={0} x2={edge} y2={0} />

  <!-- Each leaf owns a local hinge at the origin. CSS rotates the inner group,
       while the outer translation permanently registers it to its pier. -->
  <g transform={`translate(${-pivot} 0)`}>
    <g class="leaf leaf-left">
      <path class="leaf-face" d={leftLeaf} />
      <line class="leaf-road" x1={0} y1={0} x2={pivot} y2={0} />
      <path
        class="leaf-brace"
        d={`M${(pivot * 0.12).toFixed(2)} ${(deck * 0.34).toFixed(2)} L${(
          pivot * 0.48
        ).toFixed(2)} ${(-deck * 0.34).toFixed(2)} L${(pivot * 0.84).toFixed(2)} ${(
          deck * 0.34
        ).toFixed(2)}`}
      />
    </g>
  </g>
  <g transform={`translate(${pivot} 0)`}>
    <g class="leaf leaf-right">
      <path class="leaf-face" d={rightLeaf} />
      <line class="leaf-road" x1={-pivot} y1={0} x2={0} y2={0} />
      <path
        class="leaf-brace"
        d={`M${(-pivot * 0.12).toFixed(2)} ${(deck * 0.34).toFixed(2)} L${(
          -pivot * 0.48
        ).toFixed(2)} ${(-deck * 0.34).toFixed(2)} L${(-pivot * 0.84).toFixed(2)} ${(
          deck * 0.34
        ).toFixed(2)}`}
      />
    </g>
  </g>

  <!-- Pivot rings sit above the moving leaves, so the hinge remains obvious
       at mini scale instead of reading as a decorative rotation. -->
  <g class="pivots" aria-hidden="true">
    <circle class="pivot-outer" cx={-pivot} cy="0" r={pivotRadius} />
    <circle class="pivot-inner" cx={-pivot} cy="0" r={pivotRadius * 0.42} />
    <circle class="pivot-outer" cx={pivot} cy="0" r={pivotRadius} />
    <circle class="pivot-inner" cx={pivot} cy="0" r={pivotRadius * 0.42} />
  </g>

  <!-- The lock exists only when both leaves physically meet. Unknown leaves
       shorten away from it; open leaves lift clear of it. -->
  <rect
    class="center-lock"
    x={-2.15 * unit}
    y={-2.15 * unit}
    width={4.3 * unit}
    height={4.3 * unit}
    transform="rotate(45)"
  />
  <path
    class="unknown-register"
    d={`M${(-5.2 * unit).toFixed(2)} ${(-5.2 * unit).toFixed(2)} L0 ${(
      -8.1 * unit
    ).toFixed(2)} L${(5.2 * unit).toFixed(2)} ${(-5.2 * unit).toFixed(2)}`}
  />

  <!-- Road barriers lead the leaves when opening and rise only after the
       centre lock has settled when closing. Signal lamps reinforce the road
       condition without becoming the only state cue. -->
  <g
    class="barrier barrier-left"
    transform={`translate(${-pivot - barrierOffset} ${barrierY})`}
    aria-hidden="true"
  >
    <line class="signal-post" x1="0" y1={-4.4 * unit} x2="0" y2={5.8 * unit} />
    <circle class="signal-case" cx="0" cy={-5.8 * unit} r={2.7 * unit} />
    <circle class="signal-lamp" cx="0" cy={-5.8 * unit} r={1.35 * unit} />
    <g class="barrier-arm arm-left">
      <rect x="0" y={-1.2 * unit} width={barrierLength} height={2.4 * unit} />
      <line x1={barrierLength * 0.34} y1={-1.1 * unit} x2={barrierLength * 0.34} y2={1.1 * unit} />
      <line x1={barrierLength * 0.68} y1={-1.1 * unit} x2={barrierLength * 0.68} y2={1.1 * unit} />
    </g>
  </g>
  <g
    class="barrier barrier-right"
    transform={`translate(${pivot + barrierOffset} ${barrierY})`}
    aria-hidden="true"
  >
    <line class="signal-post" x1="0" y1={-4.4 * unit} x2="0" y2={5.8 * unit} />
    <circle class="signal-case" cx="0" cy={-5.8 * unit} r={2.7 * unit} />
    <circle class="signal-lamp" cx="0" cy={-5.8 * unit} r={1.35 * unit} />
    <g class="barrier-arm arm-right">
      <rect x={-barrierLength} y={-1.2 * unit} width={barrierLength} height={2.4 * unit} />
      <line x1={-barrierLength * 0.34} y1={-1.1 * unit} x2={-barrierLength * 0.34} y2={1.1 * unit} />
      <line x1={-barrierLength * 0.68} y1={-1.1 * unit} x2={-barrierLength * 0.68} y2={1.1 * unit} />
    </g>
  </g>
</g>

<style>
  .transit-bascule {
    color: var(--graphite);
  }

  .water-bed {
    stroke: var(--corridor-wash);
    stroke-linecap: square;
    stroke-width: 7px;
  }

  .transit-bascule:not(.is-hero) .water-bed {
    opacity: 0;
  }

  .transit-bascule:not(.is-hero) .channel-flow {
    opacity: 0;
    animation: none;
  }

  .channel-flow {
    fill: none;
    stroke: var(--corridor);
    stroke-dasharray: 3 6;
    stroke-linecap: square;
    stroke-width: 1.2px;
    opacity: 0.38;
  }

  .transit-bascule[data-state='up'] .channel-flow {
    opacity: 0.86;
    animation: channel-passage 1.35s linear infinite;
  }

  .pier,
  .footing {
    fill: var(--steel);
  }

  .footing {
    opacity: 0.86;
  }

  .hero-abutment,
  .house-body {
    fill: var(--frost);
    stroke: var(--marine);
    stroke-linejoin: bevel;
    stroke-width: 1px;
  }

  .hero-abutment {
    fill: var(--paper);
  }

  .hero-parapet,
  .approach-rail path {
    fill: none;
    stroke: var(--marine);
    stroke-linecap: square;
    stroke-linejoin: bevel;
    stroke-width: 0.9px;
  }

  .house-roof {
    fill: var(--marine);
  }

  .house-window {
    fill: var(--channel);
    stroke: var(--marine);
    stroke-width: 0.55px;
  }

  .approach,
  .leaf-face {
    fill: var(--success);
    stroke: var(--white);
    stroke-linejoin: bevel;
    stroke-width: 1.05px;
  }

  .road-rule,
  .leaf-road {
    stroke: var(--white);
    stroke-dasharray: 3 3;
    stroke-linecap: butt;
    stroke-width: 0.8px;
    opacity: 0.82;
  }

  .leaf-brace {
    fill: none;
    stroke: var(--marine);
    stroke-linecap: square;
    stroke-linejoin: bevel;
    stroke-width: 0.82px;
    opacity: 0.56;
  }

  .leaf {
    transform-box: fill-box;
    transition:
      transform 1.08s cubic-bezier(0.2, 0.72, 0.18, 1),
      opacity 180ms ease-out,
      fill 260ms ease-out;
  }

  .leaf-left {
    transform-origin: left center;
  }

  .leaf-right {
    transform-origin: right center;
  }

  .transit-bascule[data-state='up'] .leaf {
    transition-delay: 150ms, 150ms, 150ms;
  }

  .transit-bascule[data-state='up'] .leaf-left {
    transform: rotate(-58deg);
  }

  .transit-bascule[data-state='up'] .leaf-right {
    transform: rotate(58deg);
  }

  .transit-bascule[data-state='up'] .approach,
  .transit-bascule[data-state='up'] .leaf-face {
    fill: var(--danger);
  }

  .pivot-outer {
    fill: var(--frost);
    stroke: var(--marine);
    stroke-width: 1.15px;
  }

  .pivot-inner {
    fill: var(--marine);
  }

  .center-lock {
    fill: var(--graphite);
    stroke: var(--white);
    stroke-width: 0.8px;
    transform-box: fill-box;
    transform-origin: center;
    transition:
      opacity 100ms ease-out 900ms,
      scale 100ms ease-out 900ms;
  }

  .transit-bascule[data-state='up'] .center-lock,
  .transit-bascule[data-state='unknown'] .center-lock {
    opacity: 0;
    scale: 0.35;
    transition-delay: 0ms;
  }

  .unknown-register {
    fill: none;
    stroke: var(--steel);
    stroke-dasharray: 2 2;
    stroke-linecap: square;
    stroke-width: 1.2px;
    opacity: 0;
  }

  .signal-post {
    stroke: var(--marine);
    stroke-width: 1.2px;
  }

  .signal-case {
    fill: var(--frost);
    stroke: var(--marine);
    stroke-width: 0.9px;
  }

  .signal-lamp {
    fill: var(--success);
    transition: fill 180ms ease-out;
  }

  .barrier-arm {
    transform-box: fill-box;
    transition: transform 260ms cubic-bezier(0.2, 0.72, 0.18, 1) 900ms;
  }

  .barrier-arm rect {
    fill: var(--steel);
    stroke: var(--marine);
    stroke-width: 0.7px;
  }

  .barrier-arm line {
    stroke: var(--white);
    stroke-width: 0.72px;
  }

  .arm-left {
    transform: rotate(-84deg);
    transform-origin: left center;
  }

  .arm-right {
    transform: rotate(84deg);
    transform-origin: right center;
  }

  .transit-bascule[data-state='up'] .barrier-arm {
    transform: rotate(0deg);
    transition-delay: 0ms;
  }

  .transit-bascule[data-state='up'] .signal-lamp {
    fill: var(--danger);
  }

  .transit-bascule[data-state='unknown'] .approach,
  .transit-bascule[data-state='unknown'] .leaf-face {
    fill: transparent;
    stroke: var(--steel);
    stroke-dasharray: 2.4 2.4;
  }

  .transit-bascule[data-state='unknown'] .road-rule,
  .transit-bascule[data-state='unknown'] .leaf-road,
  .transit-bascule[data-state='unknown'] .leaf-brace {
    opacity: 0;
  }

  .transit-bascule[data-state='unknown'] .leaf-left,
  .transit-bascule[data-state='unknown'] .leaf-right {
    transform: scaleX(0.82);
  }

  .transit-bascule[data-state='unknown'] .unknown-register {
    opacity: 1;
  }

  .transit-bascule[data-state='unknown'] .barrier-arm {
    transform: rotate(0deg);
  }

  .transit-bascule[data-state='unknown'] .signal-lamp {
    fill: transparent;
    stroke: var(--steel);
    stroke-dasharray: 1 1;
    stroke-width: 0.7px;
  }

  .transit-bascule[data-state='unknown'] .leaf,
  .transit-bascule[data-state='unknown'] .barrier-arm,
  .transit-bascule[data-state='unknown'] .center-lock,
  .transit-bascule[data-state='unknown'] .signal-lamp {
    transition: none;
  }

  @keyframes channel-passage {
    to {
      stroke-dashoffset: -18;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .leaf,
    .center-lock,
    .barrier-arm,
    .signal-lamp {
      transition: none;
    }

    .transit-bascule[data-state='up'] .channel-flow {
      animation: none;
    }
  }

  @media (forced-colors: active) {
    .transit-bascule {
      color: CanvasText;
      forced-color-adjust: auto;
    }

    .water-bed,
    .channel-flow {
      stroke: LinkText;
    }

    .pier,
    .footing,
    .approach,
    .leaf-face,
    .barrier-arm rect,
    .signal-case,
    .pivot-outer {
      fill: Canvas;
      stroke: CanvasText;
    }

    .road-rule,
    .leaf-road,
    .leaf-brace,
    .barrier-arm line,
    .signal-post,
    .unknown-register {
      stroke: CanvasText;
    }

    .pivot-inner,
    .center-lock,
    .signal-lamp {
      fill: CanvasText;
      stroke: Canvas;
    }

    .transit-bascule[data-state='unknown'] .approach,
    .transit-bascule[data-state='unknown'] .leaf-face,
    .transit-bascule[data-state='unknown'] .signal-lamp {
      fill: Canvas;
      stroke: GrayText;
    }
  }
</style>
