<!--
THESIS: One question — keep driving, or turn off — answered by the span's state and the traffic coming to change it.
OWN-WORLD: Cool weatherproof paper, marine and graphite rules, condensed instrument type, corridor violet for AIS, one rationed amber mark.
STORY: Read the state, then read the water that will change it. Nothing else.
FIRST VIEWPORT: A decision band over the river, both inside one tile, no scroll.
FORM: Distilled from the Live Time Rail composition — channel roster, signal board and delivery ledger moved to the surfaces that own them.
-->
<script lang="ts">
  import LiveDeck from '$lib/components/LiveDeck.svelte';
  import { loadApp, loadError, loading, snapshot } from '$lib/state';
</script>

<svelte:head>
  <title>Live · Tender’s Log</title>
  <meta
    name="description"
    content="Current bridge prediction, supporting evidence, channel health, and delivery state."
  />
</svelte:head>

{#if $loading && !$snapshot}
  <section class="live-loading" aria-label="Loading live console" aria-busy="true">
    <div class="loading-decision">
      <span class="skeleton-rule"></span><span class="skeleton-state"></span><span class="skeleton-rule"></span>
    </div>
    <div class="loading-rail"><span class="skeleton-rule"></span><span class="skeleton-strip"></span><span class="skeleton-strip"></span></div>
    <div class="loading-ledger"><span class="skeleton-rule"></span><span class="skeleton-rule"></span><span class="skeleton-rule"></span></div>
  </section>
{:else if $snapshot}
  <div class="live-console">
    <!--
      The decision owns the top; the river underneath it is the evidence for
      that decision, shown once. An earlier version also carried a time rail and
      a separate span panel, which restated the same bridge state in two more
      shapes — three readings of one fact, none of which said what was on the
      water.
    -->
    <!-- Decision and river are one reading, so they are one tile, sized to
         the viewport. Everything below is reference the reader scrolls to. -->
    <LiveDeck
      decision={$snapshot.decision}
      corridor={$snapshot.riverCorridor}
      vesselTracks={$snapshot.vesselTracks}
      intervals={$snapshot.bridgeIntervals}
      crossings={$snapshot.bridgeCrossings}
      localTimeZone={$snapshot.localTimeZone}
    />
    <footer class="live-footer">
      <span>All times · {$snapshot.localTimeZone}</span>
      <span>Supports a decision; does not guarantee an opening.</span>
    </footer>
  </div>
{:else}
  <section class="error-sheet" role="alert">
    <p class="registration-label">Console unavailable</p>
    <h1>No complete snapshot</h1>
    <p>{$loadError ?? 'The engine has not produced a trustworthy snapshot.'}</p>
    <button class="primary-action" onclick={() => loadApp()}>Retry engine connection</button>
  </section>
{/if}

<style>
  .live-console {
    min-height: calc(100vh - 72px);
    background: var(--paper);
  }

  .live-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    color: var(--muted);
    background: var(--paper);
    border-top: 1px solid var(--rule-strong);
    padding: 12px clamp(20px, 3vw, 42px);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.045em;
    text-transform: uppercase;
  }

  .live-loading {
    display: grid;
    min-height: calc(100vh - 72px);
    grid-template-columns: 5fr 3.1fr;
    background: var(--paper);
  }

  .loading-decision,
  .loading-rail,
  .loading-ledger {
    display: grid;
    align-content: start;
    gap: 36px;
    padding: 44px;
    border-right: 1px solid var(--rule-strong);
  }

  .loading-decision {
    background: var(--frost);
  }

  .skeleton-state {
    width: 75%;
    height: 180px;
    background: rgba(15, 42, 68, 0.14);
    animation: skeleton-pulse 1.6s ease-in-out infinite alternate;
  }

  .skeleton-strip {
    height: 88px;
    background: var(--frost);
    border: 1px solid var(--rule);
  }

  .loading-ledger .skeleton-rule {
    margin-bottom: 28px;
  }

  .error-sheet p {
    max-width: 65ch;
    color: var(--muted);
    line-height: 1.55;
  }

  @media (max-width: 1180px) {
    .live-loading {
      grid-template-columns: 1fr 1fr;
    }

    .live-loading > :first-child {
      grid-column: 1 / -1;
    }
  }

  @media (max-width: 720px) {
    .live-console {
      min-height: calc(100vh - 134px);
      padding-bottom: 76px;
    }

    .live-loading {
      display: block;
      min-height: 0;
    }

    .loading-decision,
    .loading-rail,
    .loading-ledger {
      min-height: 330px;
      padding: 28px 18px;
      border-right: 0;
      border-bottom: 1px solid var(--rule-strong);
    }

    .live-footer {
      align-items: flex-start;
      flex-direction: column;
      gap: 7px;
      padding: 16px;
    }
  }
</style>
