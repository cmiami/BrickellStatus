<!--
Upright side profiles, drawn bow-right on one 44 × 26 marine register. The
route owns the true heading; this component only mirrors horizontally, keeping
wheelhouses, masts, people decks, and working gear legible in either direction.

The family shares a crisp filled silhouette, white cut lines, and fine external
hardware. Individual classes earn their distinction from credible anatomy—not
from a label or a decorative badge. Unknown classes use the same solid Miami
motor-yacht as a generic vessel.
-->
<script lang="ts">
  let {
    kind = 'vessel',
    length = 26,
    flip = false,
    opener = false
  }: {
    /** AIS class word from the engine, or `vessel` when none was broadcast. */
    kind?: string;
    /** Drawn hull length in user units; height follows the box ratio. */
    length?: number;
    /** Mirror to face the other way along the line. */
    flip?: boolean;
    opener?: boolean;
  } = $props();

  /** Families that share a silhouette. */
  const family = $derived.by(() => {
    switch (kind) {
      case 'tug':
      case 'tug + tow':
        return 'tug';
      case 'cargo':
        return 'cargo';
      case 'tanker':
        return 'tanker';
      case 'sailing':
        return 'sailing';
      case 'pleasure craft':
        return 'yacht';
      case 'passenger':
        return 'passenger';
      case 'fishing':
        return 'fishing';
      case 'pilot':
        return 'pilot';
      default:
        return 'generic-motor-yacht';
    }
  });
</script>

<g
  class="vessel-glyph"
  class:is-opener={opener}
  data-family={family}
  transform="scale({(flip ? -1 : 1) * (length / 44)} {length / 44})"
>
  {#if family === 'tug'}
    <!-- Deep workboat hull, high bow fender, tall pilothouse, towing bitt. -->
    <path class="hull" d="M-21.5 -3.2H13.8Q18.5 -3.1 22 0L18.2 4.8H-17.5Q-21.5 3 -21.5 -3.2Z" />
    <path class="cut" d="M-16.8 2.1Q0 3.4 18.3 1.1" />
    <path class="house" d="M-6 -3V-11.5L-2.5 -15H10L14.2 -10.5V-3Z" />
    <path class="house" d="M-2.5 -15V-18H7.7L10 -15Z" />
    <path class="window" d="M-1.1 -13.7H3.1V-10.7H-3.8ZM4.4 -13.7H7.6L10.2 -10.7H4.4Z" />
    <path class="stack" d="M-11.8 -3V-11H-7.5V-3Z" />
    <path class="ink-line" d="M3 -18V-20.2M3 -19.3H8M-16 -3V-7H-12V-3" />
    <path class="fender" d="M-21.7 -1.7H-18.6V3.1H-21.2Z" />
    <circle class="fender" cx="18.2" cy="-0.2" r="2.3" />
  {:else if family === 'cargo'}
    <!-- Boxship: aft accommodation block and three broad container tiers. -->
    <path class="hull" d="M-22 -1.5H16.5Q20 -1.2 22 0.5L16.8 4.8H-19Q-22 3.1 -22 -1.5Z" />
    <path class="cut" d="M-17.5 2.3H17.9" />
    <path class="house" d="M-21 -1.5V-13H-13.4V-1.5Z" />
    <path class="window" d="M-19.4 -11.5H-17.2V-9.5H-19.4ZM-16.1 -11.5H-14.2V-9.5H-16.1ZM-19.4 -8.1H-17.2V-6.1H-19.4ZM-16.1 -8.1H-14.2V-6.1H-16.1Z" />
    <path class="stack" d="M-18.8 -13V-17H-16V-13" />
    <path class="cargo-box" d="M-11.7 -1.5V-6.6H16.7V-1.5Z" />
    <path class="cargo-box" d="M-11.7 -6.6V-11.7H16.7V-6.6Z" />
    <path class="cargo-box" d="M-4.6 -11.7V-16.8H9.6V-11.7Z" />
    <path class="container-seam" d="M-4.6 -11.5V-1.7M2.5 -16.6V-1.7M9.6 -11.5V-1.7" />
  {:else if family === 'tanker'}
    <!-- Long flush deck, aft house, manifold and repeated tank hatches. -->
    <path class="hull" d="M-22 -1H16.5Q20 -0.9 22 1L17.2 4.3Q4 5 -18.6 4.3Q-22 2.8 -22 -1Z" />
    <path class="cut" d="M-17.8 2.2Q1 3.1 18.2 1.6" />
    <path class="house" d="M-21 -1V-10.8H-13.2V-1Z" />
    <path class="window" d="M-19.5 -9.1H-17.2V-7H-19.5ZM-16.1 -9.1H-13.9V-7H-16.1Z" />
    <path class="stack" d="M-18.8 -10.8V-15H-15.3V-10.8Z" />
    <path class="deck" d="M-11 -3.8H17V-1H-11Z" />
    <path class="ink-line" d="M-8.5 -3.8V-7M-8.5 -6H12.8M3.2 -6V-2.8M12.8 -6V-2.8" />
    <path class="equipment" d="M-2.5 -7.4H6.5V-4.2H-2.5Z" />
    <circle class="tank-fitting" cx="-7" cy="-5.8" r="1.2" />
    <circle class="tank-fitting" cx="9.5" cy="-5.8" r="1.2" />
    <path class="rail" d="M13 -3.8V-7.5H18.2M15.7 -7.5V-3.8" />
  {:else if family === 'sailing'}
    <!-- Deep keel profile and a legible sloop rig, even at manifest size. -->
    <path class="hull" d="M-18 -1.3Q0 0.3 19 -1.3Q17.2 3.1 11.5 4.8H-10.8Q-16.2 3.4 -18 -1.3Z" />
    <path class="keel" d="M-2.4 4.2L-0.5 6.4L3 4.2Z" />
    <path class="cut" d="M-13.8 1.6Q1 3.1 15 1.2" />
    <path class="house" d="M-8 -1L-5.6 -4.4H3.8L7 -1Z" />
    <path class="window" d="M-4.8 -3.4H-1V-1.7H-6ZM0 -3.4H3.1L4.8 -1.7H0Z" />
    <path class="sail" d="M-1.8 -19.8V-5.1H-14.4Z" />
    <path class="sail" d="M0.8 -18.9V-5.1H13.1Z" />
    <path class="mast" d="M-0.5 -20.2V-1M-13.8 -5.2H9.8" />
    <path class="rigging" d="M-0.5 -19.8L-16.5 -1M-0.5 -19.8L16 -1" />
  {:else if family === 'yacht'}
    <!-- Express yacht: low curved screen and one uninterrupted planing sheer. -->
    <path class="hull" d="M-21 -2Q-7 -1.1 6 -1.5H21Q18.8 2.1 13 4.1Q2 5.1 -14.5 4.3Q-19.4 2.9 -21 -2Z" />
    <path class="cut" d="M-15.8 2Q0 3.2 16.8 0.9" />
    <path class="house" d="M-13 -1.6L-8.4 -7.2Q-4.4 -10 1.7 -9.4Q6.2 -8.2 10.5 -1.6Z" />
    <path class="window" d="M-7.5 -6.8Q-5.2 -8.1 -2.3 -8V-3H-10.6ZM-1 -8Q1.3 -8.1 3.7 -7L8 -3H-1Z" />
    <path class="rail" d="M10 -1.8L13.2 -4.8H19.5M16 -4.8V-2" />
    <path class="ink-line" d="M-8.1 -8Q-3.7 -10.9 2.4 -10.1L5.8 -8.7M-0.2 -10.3V-12.4" />
  {:else if family === 'passenger'}
    <!-- Streamlined two-deck ferry with long glazing and a raked bridge. -->
    <path class="hull" d="M-22 -1.2H16.5Q20 -1 22 1.2L16.8 4.6H-18.5Q-22 3 -22 -1.2Z" />
    <path class="cut" d="M-17.8 2.3Q1 3.1 18 1.6" />
    <path class="house" d="M-18 -1.2V-6.8H-13V-12H7.5Q12 -10.7 16.5 -6V-1.2Z" />
    <path class="window" d="M-11.3 -10.4H6.8Q10 -9.5 13.1 -6.8H-11.3ZM-15.9 -5.4H13.7V-2.7H-15.9Z" />
    <path class="mullion" d="M-5.5 -10.3V-6.9M0.4 -10.3V-6.9M6.2 -10.1V-6.9M-10 -5.3V-2.8M-4.1 -5.3V-2.8M1.8 -5.3V-2.8M7.7 -5.3V-2.8" />
    <path class="rail" d="M-18 -6.8H-13M-16 -6.8V-9" />
    <path class="mast" d="M1.5 -12V-16M-2 -14H5M4 -14L6 -12" />
  {:else if family === 'fishing'}
    <!-- Trawler: raised bow, forward house, open work deck and outriggers. -->
    <path class="hull" d="M-20 -1.2H5Q11 -3 17.5 -2.5L21 0Q18.5 3.6 12.5 4.8H-14.5Q-19 3 -20 -1.2Z" />
    <path class="cut" d="M-15.5 2Q1 3.4 17.5 0.8" />
    <path class="house" d="M2.5 -2.1V-9.5H11.5L16 -5V-2.6Z" />
    <path class="window" d="M4.3 -7.9H8V-4.2H4.3ZM9.2 -7.9H11L14.3 -4.2H9.2Z" />
    <path class="mast" d="M-1.5 -1.4V-16M-5.2 -12.5H2.2M-3.5 -12.5L-1.5 -16L0.5 -12.5" />
    <path class="boom" d="M-1.5 -12.7L-18 -7M-1.5 -12.7L16.5 -8.5" />
    <path class="rigging" d="M-18 -7V-3.8M-1.5 -12.7L12.5 -2.8" />
    <path class="equipment" d="M-15 -4.3H-8V-1.2H-15Z" />
  {:else if family === 'pilot'}
    <!-- Pilot launch: enclosed house, raised spear bow and hard planing chine. -->
    <path class="hull" d="M-21 -1.5H6Q14 -1.2 21 -4L18 0.7Q13 4.2 4 5H-15Q-19 3.5 -21 -1.5Z" />
    <path class="cut" d="M-16 2Q0 3.6 16.8 0.2" />
    <path class="house" d="M-14 -1.6L-10.5 -9.8H1L7 -1.5Z" />
    <path class="window" d="M-9.7 -8H-5V-3.4H-11.7ZM-3.7 -8H0.1L4.2 -3.4H-3.7Z" />
    <path class="mast" d="M-5.6 -9.8V-15M-9 -12.5H-1.6M-7.7 -15H-3.5" />
    <path class="cut pilot-slash" d="M-11 0.1L-7.2 2.7M-5.4 0.6L-1.8 3" />
  {:else if family === 'generic-motor-yacht'}
    <!-- Miami flybridge yacht: curved sheer, stepped stern and two real decks. -->
    <path class="hull" d="M-20 -1.7Q-5 -0.9 7 -1.4H21.5Q19 2 13.2 4.3H-14.5Q-18.2 3.9 -19.4 2.5H-22V0.7H-19.8Z" />
    <path class="cut" d="M-15.7 2Q1 3.4 17.2 0.8" />
    <path class="house" d="M-14 -1.5L-9.5 -8.3Q-3.5 -9.6 4.8 -8.3L11.5 -1.5Z" />
    <path class="window" d="M-8.7 -7Q-6.3 -7.7 -3.5 -7.7V-3H-11.7ZM-2.1 -7.7H3.9L8.5 -3H-2.1Z" />
    <path class="house" d="M-6 -8.7Q-1.2 -12 5.2 -11.2L8 -8.3Q0.6 -9.3 -6 -8.7Z" />
    <path class="window" d="M-1.9 -10.8Q1.2 -11.4 4.5 -10.5L5.8 -9.4Q1 -9.8 -3.2 -9.4Z" />
    <path class="rail" d="M10.6 -1.8L13.8 -4.8H20M17 -4.8V-2" />
    <path class="mast" d="M0.9 -13.3V-17M-2.5 -15.4H5M3.6 -15.4L6 -13.4" />
    <ellipse class="equipment" cx="0.9" cy="-17.6" rx="2.8" ry="0.8" />
  {/if}
</g>

<style>
  .vessel-glyph {
    color: var(--corridor);
    filter: drop-shadow(0 1px 0 var(--marine));
    overflow: visible;
    shape-rendering: geometricPrecision;
  }

  .vessel-glyph.is-opener {
    color: var(--amber-ink);
  }

  .hull,
  .cargo-box,
  .deck,
  .sail,
  .equipment,
  .fender,
  .tank-fitting,
  .keel {
    fill: currentColor;
    stroke: var(--white);
    stroke-width: 1.05;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }

  .hull {
    stroke-width: 1.35;
  }

  .house {
    fill: var(--white);
    stroke: currentColor;
    stroke-width: 1.2;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }

  .cargo-box {
    stroke-width: 0.9;
  }

  .window {
    fill: var(--marine);
  }

  .vessel-glyph.is-opener .window {
    fill: currentColor;
  }

  .cut,
  .container-seam,
  .mullion,
  .ink-line,
  .rail,
  .mast,
  .boom,
  .rigging {
    fill: none;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }

  .cut {
    stroke: var(--white);
    stroke-width: 1;
  }

  .container-seam,
  .mullion {
    stroke: var(--white);
    stroke-width: 0.9;
  }

  .ink-line,
  .rail,
  .mast,
  .boom {
    stroke: currentColor;
    stroke-width: 1.05;
  }

  .rigging {
    stroke: currentColor;
    stroke-width: 0.9;
  }

  .stack {
    fill: currentColor;
    stroke: var(--white);
    stroke-width: 0.9;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }

  .fender {
    stroke-width: 1;
  }

  .pilot-slash {
    stroke-width: 1.35;
  }
</style>
