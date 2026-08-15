<script lang="ts">
  import type { DecisionSnapshot } from '$lib/types';

  let { decision }: { decision: DecisionSnapshot } = $props();

  const confidence = () =>
    decision.confidenceBps == null ? null : Math.round(decision.confidenceBps / 100);

  const timing = () => {
    if (decision.etaMin == null || decision.etaMax == null) return null;
    return `${decision.etaMin}–${decision.etaMax}`;
  };
</script>

<section class="decision" aria-labelledby="decision-subject" aria-live="polite">
  <header class="decision-header">
    <p class="registration-label">Current status</p>
    <span class="status-word" data-state={decision.availability}>{decision.availability}</span>
  </header>

  <div class="decision-subject">
    <span class="registration-label">Bridge</span>
    <h1 id="decision-subject">{decision.subject}</h1>
  </div>

  <div class="decision-outcome" data-state={decision.state}>
    <span class="registration-label">Status</span>
    <strong>{decision.stateLabel}</strong>
  </div>

  <div class="decision-readings">
    <div>
      <span class="registration-label">ETA window</span>
      {#if timing()}
        <strong>{timing()} <small>min</small></strong>
      {:else}
        <strong class="unavailable">Unavailable</strong>
      {/if}
    </div>
    <div>
      <span class="registration-label">Confidence</span>
      {#if confidence() != null}
        <strong>{confidence()}<small>% · {decision.confidenceLabel ?? 'estimate'}</small></strong>
      {:else}
        <strong class="unavailable">Not applicable</strong>
      {/if}
    </div>
  </div>

  <div class="decision-explanation">
    <p>{decision.meaning}</p>
    <div class="status-detail">
      <span class="registration-label">Status detail</span>
      <strong>{decision.action}</strong>
    </div>
  </div>

  <footer>
    <div>
      <span class="registration-label">Schedule context</span>
      <strong>
        {decision.openingAllowedNow
          ? 'Opening permitted on signal now'
          : `Next permitted slot · ${decision.nextLegalSlot ?? 'unavailable'}`}
      </strong>
    </div>
    <p>{decision.confidenceBasis ?? 'No predictive score is currently available.'}</p>
  </footer>
</section>

<style>
  .decision {
    display: grid;
    min-width: 0;
    align-content: start;
    padding: clamp(24px, 3.2vw, 48px);
    background: var(--frost);
    border-right: 1px solid var(--rule-strong);
  }

  .decision-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding-bottom: 13px;
    border-bottom: 1px solid var(--rule-strong);
  }

  .decision-subject {
    display: grid;
    gap: 12px;
    margin-top: clamp(34px, 5vh, 58px);
  }

  h1 {
    max-width: 9ch;
    margin: 0;
    color: var(--marine);
    font-size: var(--type-display);
    font-weight: 700;
    line-height: 0.78;
    letter-spacing: -0.025em;
    text-transform: uppercase;
    text-wrap: balance;
  }

  .decision-outcome {
    display: grid;
    gap: 8px;
    margin-top: clamp(36px, 6vh, 64px);
    padding-bottom: 17px;
    border-bottom: 1px solid var(--rule-strong);
  }

  .decision-outcome strong {
    font-family: var(--font-instrument);
    font-size: var(--type-display-compact);
    font-weight: 700;
    line-height: 0.84;
    letter-spacing: -0.02em;
    text-transform: uppercase;
  }

  .decision-outcome[data-state='open'] {
    color: var(--white);
    background: var(--graphite);
    margin-inline: -18px;
    padding: 18px;
    border-bottom-color: var(--graphite);
  }

  .decision-outcome[data-state='likely'] {
    margin-inline: -18px;
    padding: 18px;
    color: var(--graphite);
    background: var(--amber-sheet);
    border-bottom-color: var(--amber-ink);
  }

  .decision-readings {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    border-bottom: 1px solid var(--rule-strong);
  }

  .decision-readings > div {
    display: grid;
    min-width: 0;
    gap: 8px;
    padding: 18px 14px 18px 0;
  }

  .decision-readings > div + div {
    border-left: 1px solid var(--rule);
    padding-left: 20px;
  }

  .decision-readings strong {
    display: flex;
    align-items: baseline;
    gap: 9px;
    font-family: var(--font-instrument);
    font-size: var(--type-display-compact);
    font-weight: 600;
    line-height: 0.9;
    letter-spacing: -0.02em;
  }

  .decision-readings small {
    max-width: 8ch;
    color: var(--muted);
    font-size: var(--type-body-small);
    font-weight: 600;
    line-height: 1;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .decision-readings .unavailable {
    align-items: center;
    min-height: 68px;
    color: var(--muted);
    font-size: var(--type-title);
    letter-spacing: 0;
  }

  .decision-explanation {
    display: grid;
    gap: 10px;
    padding: 22px 0 6px;
  }

  .decision-explanation p {
    max-width: 58ch;
    margin: 0;
    color: var(--muted);
    font-size: var(--type-body-small);
    line-height: 1.5;
  }

  .status-detail {
    display: grid;
    gap: 6px;
    padding-top: 4px;
  }

  .status-detail strong {
    color: var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-body);
    font-weight: 600;
    line-height: 1.25;
    text-transform: uppercase;
  }

  footer {
    display: grid;
    gap: 12px;
    margin-top: 24px;
    padding-top: 16px;
    border-top: 1px solid var(--rule);
  }

  footer > div {
    display: grid;
    gap: 6px;
  }

  footer strong {
    font-family: var(--font-instrument);
    font-size: var(--type-body);
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }

  footer p {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  @media (max-width: 1180px) {
    .decision {
      border-right: 0;
      border-bottom: 1px solid var(--rule-strong);
    }

    h1 {
      max-width: 13ch;
    }
  }

  @media (max-width: 540px) {
    .decision {
      padding: 22px 18px 28px;
    }

    .decision-subject {
      margin-top: 30px;
    }

    h1 {
      font-size: var(--type-display-compact);
    }

    .decision-outcome {
      margin-top: 38px;
    }

    .decision-readings {
      grid-template-columns: 1fr;
    }

    .decision-readings > div + div {
      border-top: 1px solid var(--rule);
      border-left: 0;
      padding-left: 0;
    }
  }
</style>
