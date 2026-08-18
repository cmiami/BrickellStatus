<!--
Vessel silhouettes, one per AIS ship-type family.

Side profiles rather than plan views: at twenty pixels a hull seen from above is
a wedge whatever it is, while a profile keeps the one feature that identifies
each class — a tug's tall wheelhouse, a container stack, a mast. Every glyph is
drawn bow-right on the same waterline in the same 44×26 box, so a fleet reads as
one family and a hull can be mirrored to face its heading without redrawing.

A class the vessel never broadcast gets the plain boat. It is not a guess.
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

  const HEIGHT_RATIO = 26 / 44;

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
        return 'boat';
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
    <!-- Stubby hull, tall wheelhouse well forward, fendered bow. -->
    <path class="hull" d="M-22 4 L18 4 L22 0 L18 -4 L-22 -4 Z" />
    <rect class="house" x="-2" y="-13" width="13" height="9" />
    <rect class="house" x="1" y="-17" width="7" height="4" />
    <path class="stack" d="M-8 -4 L-8 -11 L-4 -11 L-4 -4 Z" />
  {:else if family === 'cargo'}
    <!-- Long low hull under a container stack, house right aft. -->
    <path class="hull" d="M-22 4 L17 4 L22 -1 L-22 -1 Z" />
    <rect class="house" x="-21" y="-11" width="7" height="10" />
    <rect class="cargo-box" x="-12" y="-6" width="7" height="5" />
    <rect class="cargo-box" x="-4" y="-6" width="7" height="5" />
    <rect class="cargo-box" x="4" y="-6" width="7" height="5" />
    <rect class="cargo-box" x="-12" y="-11" width="7" height="5" />
    <rect class="cargo-box" x="-4" y="-11" width="7" height="5" />
  {:else if family === 'tanker'}
    <!-- Flush deck, midships manifold, house aft. -->
    <path class="hull" d="M-22 4 L17 4 L22 -1 L-22 -1 Z" />
    <rect class="house" x="-21" y="-10" width="8" height="9" />
    <rect class="deck" x="-10" y="-4" width="20" height="3" />
    <path class="stack" d="M-1 -4 L-1 -9 L2 -9 L2 -4 Z" />
  {:else if family === 'sailing'}
    <!-- The mast is why the bascule exists, so it dominates the glyph. -->
    <path class="hull" d="M-16 4 L14 4 L18 0 L-16 0 Z" />
    <path class="mast" d="M-1 0 L-1 -22" />
    <path class="sail" d="M-2 -21 L-2 -3 L-14 -3 Z" />
    <path class="sail" d="M1 -20 L1 -3 L11 -3 Z" />
  {:else if family === 'yacht'}
    <!-- Sleek sheer, raked screen, low cabin. -->
    <path class="hull" d="M-18 4 L14 4 L20 -2 L-18 -2 Z" />
    <path class="house" d="M-9 -2 L4 -2 L1 -9 L-7 -9 Z" />
  {:else if family === 'passenger'}
    <!-- Stacked decks: the profile that says "carries people". -->
    <path class="hull" d="M-21 4 L16 4 L21 -1 L-21 -1 Z" />
    <rect class="house" x="-16" y="-7" width="28" height="6" />
    <rect class="house" x="-12" y="-13" width="20" height="6" />
    <path class="stack" d="M0 -13 L0 -18 L4 -18 L4 -13 Z" />
  {:else if family === 'fishing'}
    <!-- Small hull, wheelhouse forward, working boom aft. -->
    <path class="hull" d="M-17 4 L14 4 L19 -1 L-17 -1 Z" />
    <rect class="house" x="1" y="-9" width="9" height="8" />
    <path class="mast" d="M-4 -1 L-4 -14" />
    <path class="boom" d="M-4 -13 L-16 -4" />
  {:else if family === 'pilot'}
    <!-- Fast launch: long foredeck, house aft, low freeboard. -->
    <path class="hull" d="M-18 3 L15 3 L20 -2 L-18 -2 Z" />
    <rect class="house" x="-13" y="-9" width="11" height="7" />
    <path class="mast" d="M-8 -9 L-8 -14" />
  {:else}
    <!-- No class broadcast: a plain boat, claiming nothing about the hull. -->
    <path class="hull" d="M-17 4 L14 4 L19 -1 L-17 -1 Z" />
    <rect class="house" x="-6" y="-8" width="11" height="7" />
  {/if}
</g>

<style>
  .hull,
  .house,
  .cargo-box,
  .deck,
  .sail {
    fill: var(--corridor);
    stroke: var(--white);
    stroke-width: 1.1;
    stroke-linejoin: round;
  }

  /* Container blocks read as a stack only if their edges survive; a shared
     fill with a white seam does that at any size. */
  .cargo-box {
    stroke-width: 0.9;
  }

  .mast,
  .boom,
  .stack {
    fill: var(--corridor);
    stroke: var(--corridor);
    stroke-width: 1.7;
    stroke-linecap: round;
  }

  /* A hull the ledger has watched lift the span is the one thing on this
     drawing that changes a driver's plan, so it alone takes amber. */
  .vessel-glyph.is-opener .hull,
  .vessel-glyph.is-opener .house,
  .vessel-glyph.is-opener .cargo-box,
  .vessel-glyph.is-opener .deck,
  .vessel-glyph.is-opener .sail {
    fill: var(--amber-ink);
  }

  .vessel-glyph.is-opener .mast,
  .vessel-glyph.is-opener .boom,
  .vessel-glyph.is-opener .stack {
    fill: var(--amber-ink);
    stroke: var(--amber-ink);
  }
</style>
