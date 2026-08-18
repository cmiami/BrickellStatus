<script lang="ts">
  import type { DecisionSnapshot } from '$lib/types';

  let {
    decision,
    band = false
  }: {
    decision: DecisionSnapshot;
    /**
     * Lay the reading out across the sheet instead of down a column, so the
     * river below it gets the height. Same facts, same type sizes.
     */
    band?: boolean;
  } = $props();

  const confidence = () =>
    decision.confidenceBps == null ? null : Math.round(decision.confidenceBps / 100);

  const timing = () => {
    if (decision.etaMin == null || decision.etaMax == null) return null;
    return decision.etaMin === decision.etaMax
      ? `${decision.etaMax}`
      : `${decision.etaMin} to ${decision.etaMax}`;
  };
</script>

<section
  class="decision"
  class:band
  data-state={decision.state}
  aria-labelledby="decision-state"
  aria-live="polite"
>
  <header class="decision-header">
    <p class="registration-label">{decision.subject}</p>
    {#if decision.availability !== 'fresh'}
      <span class="status-word" data-state={decision.availability}>{decision.availability}</span>
    {/if}
  </header>

  <!--
    The state and the countdown, and as little else as will fit around them.
    Everything the engine consulted to reach this answer is deliberately not
    here: a driver deciding whether to turn needs the answer, not its sources.
  -->
  <h1 id="decision-state">{decision.stateLabel}</h1>

  <div class="decision-timing">
    {#if timing()}
      <strong class="countdown">T&#8209;{timing()}<small>min</small></strong>
      {#if confidence() != null}
        <span class="confidence">{confidence()}% {decision.confidenceLabel ?? 'estimate'}</span>
      {/if}
    {:else}
      <strong class="road">{decision.meaning}</strong>
    {/if}
  </div>

  <div class="band-tail">
    <p class="action">{decision.action}</p>
    <footer>
      {decision.openingAllowedNow
        ? 'Opening permitted on signal now'
        : `Next permitted slot · ${decision.nextLegalSlot ?? 'unavailable'}`}
    </footer>
  </div>
</section>

<style>
  /* The state is the loudest thing on the page, and its colour carries the
     same meaning as the spans below: red the bridge is up or going up, green
     the road is clear. */
  .decision {
    display: grid;
    min-width: 0;
    align-content: start;
    padding: clamp(24px, 3.2vw, 48px);
    background: var(--frost);
    border-right: 1px solid var(--rule-strong);
    border-left: 10px solid var(--success);
  }

  .decision[data-state='open'] {
    background: var(--danger);
    border-left-color: var(--graphite);
    color: var(--white);
  }

  .decision[data-state='likely'] {
    background: var(--amber-sheet);
    border-left-color: var(--danger);
  }

  .decision[data-state='possible'] {
    border-left-color: var(--amber);
  }

  .decision-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding-bottom: 13px;
    border-bottom: 1px solid var(--rule-strong);
  }

  .decision[data-state='open'] .decision-header {
    border-bottom-color: rgba(255, 255, 255, 0.5);
  }

  h1 {
    margin: clamp(24px, 4vh, 46px) 0 0;
    font-family: var(--font-instrument);
    font-size: var(--type-display);
    font-weight: 700;
    line-height: 0.82;
    letter-spacing: -0.03em;
    text-transform: uppercase;
    text-wrap: balance;
  }

  .decision[data-state='likely'] h1 {
    color: var(--danger);
  }

  .decision-timing {
    display: grid;
    gap: 6px;
    margin-top: clamp(18px, 3vh, 34px);
    padding-top: 16px;
    border-top: 1px solid var(--rule-strong);
  }

  .decision[data-state='open'] .decision-timing {
    border-top-color: rgba(255, 255, 255, 0.5);
  }

  .countdown {
    display: flex;
    align-items: baseline;
    gap: 10px;
    font-family: var(--font-instrument);
    font-size: var(--type-headline);
    font-weight: 700;
    line-height: 0.9;
    letter-spacing: -0.02em;
  }

  .countdown small,
  .confidence {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .confidence {
    font-size: var(--type-label);
  }

  .road {
    font-family: var(--font-instrument);
    font-size: var(--type-section);
    font-weight: 700;
    line-height: 1;
    text-transform: uppercase;
  }

  .action {
    margin: 16px 0 0;
    font-size: var(--type-body);
    font-weight: 600;
  }

  footer {
    margin-top: auto;
    padding-top: 20px;
    color: var(--muted);
    font-size: var(--type-caption);
  }

  .decision[data-state='open'] .countdown small,
  .decision[data-state='open'] .confidence,
  .decision[data-state='open'] footer {
    color: rgba(255, 255, 255, 0.82);
  }

  @media (max-width: 900px) {
    .decision {
      border-right: 0;
      border-bottom: 1px solid var(--rule-strong);
    }
  }

  /* Band: the same reading, laid across the sheet. The state keeps the display
     size it earns; timing and consequence sit beside it rather than under it,
     which is what frees the height for the river. */
  .decision.band {
    grid-template-columns: minmax(0, auto) minmax(0, auto) minmax(0, 1fr);
    gap: clamp(20px, 3vw, 52px);
    align-items: center;
    align-content: center;
    padding: clamp(12px, 1.4vw, 18px) clamp(20px, 3vw, 42px);
    border-right: 0;
    border-bottom: 1px solid var(--rule-strong);
  }

  .decision.band .decision-header {
    grid-column: 1 / -1;
    padding-bottom: 6px;
  }

  .decision.band h1 {
    margin: 0;
    font-size: var(--type-display-compact);
    line-height: 0.84;
  }

  .decision.band .decision-timing {
    margin-top: 0;
    padding-top: 0;
    border-top: 0;
    border-left: 1px solid var(--rule-strong);
    padding-left: clamp(18px, 2.4vw, 36px);
  }

  .decision.band[data-state='open'] .decision-timing {
    border-left-color: rgba(255, 255, 255, 0.5);
  }

  /* Consequence and the next permitted slot read as one sentence at the end
     of the band rather than as a footer nobody reaches. */
  .decision.band .action {
    margin: 0;
  }

  .decision.band footer {
    margin: 4px 0 0;
    padding-top: 0;
  }

  .decision.band .band-tail {
    display: grid;
    gap: 2px;
    justify-items: start;
  }

  @media (max-width: 900px) {
    .decision.band {
      grid-template-columns: minmax(0, 1fr);
      gap: 12px;
    }

    .decision.band .decision-timing {
      border-left: 0;
      padding-left: 0;
    }
  }
</style>