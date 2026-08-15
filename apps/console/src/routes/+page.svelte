<!--
THESIS: One current decision owns the surface; evidence registers against time instead of dissolving into a generic widget grid.
OWN-WORLD: Cool weatherproof paper, marine and graphite rules, clipped frost strips, condensed instrument type, and one rationed amber interrupt mark.
STORY: Read what matters, when it may happen, why the model believes it, then verify channels and accepted deliveries.
FIRST VIEWPORT: Decision field left, off-centre live time rail and evidence center, enabled-channel and destination ledger right; the primary action is the route instruction inside the decision.
FORM: Grounded direction 3, approved Live Time Rail composition, concept seed 1400f3c1.
-->
<script lang="ts">
  import ChannelLedger from '$lib/components/ChannelLedger.svelte';
  import DispatchLedger from '$lib/components/DispatchLedger.svelte';
  import StatusDecision from '$lib/components/StatusDecision.svelte';
  import TimeRail from '$lib/components/TimeRail.svelte';
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
    <div class="live-grid">
      <StatusDecision decision={$snapshot.decision} />
      <TimeRail evidence={$snapshot.evidence} generatedAt={$snapshot.generatedAt} />
      <ChannelLedger channels={$snapshot.channels} outputs={$snapshot.outputs} />
    </div>
    <DispatchLedger dispatches={$snapshot.dispatches} channels={$snapshot.channels} />
    <footer class="live-footer">
      <span>All times · {$snapshot.localTimeZone}</span>
      <span>Tender’s Log supports decisions; it does not guarantee a bridge opening.</span>
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

  .live-grid {
    display: grid;
    grid-template-columns: minmax(330px, 3.8fr) minmax(390px, 4.7fr) minmax(280px, 3.1fr);
    min-height: 670px;
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
    grid-template-columns: 3.8fr 4.7fr 3.1fr;
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
    .live-grid,
    .live-loading {
      grid-template-columns: 1fr 1fr;
    }

    .live-grid > :global(:first-child),
    .live-loading > :first-child {
      grid-column: 1 / -1;
    }
  }

  @media (max-width: 720px) {
    .live-console {
      min-height: calc(100vh - 134px);
      padding-bottom: 76px;
    }

    .live-grid,
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
