<script lang="ts">
  import { CircleDashed, Ship, Waypoints } from '@lucide/svelte';
  import type { EvidenceStrip } from '$lib/types';

  let {
    evidence,
    generatedAt
  }: {
    evidence: EvidenceStrip[];
    generatedAt: string;
  } = $props();

  const formatAge = (seconds: number) => {
    if (seconds < 60) return `${Math.max(0, Math.round(seconds))} sec`;
    return `${Math.round(seconds / 60)} min`;
  };

  const formatTime = (iso: string) =>
    new Intl.DateTimeFormat('en-US', { hour: 'numeric', minute: '2-digit' }).format(new Date(iso));

  const iconFor = (strip: EvidenceStrip) => {
    if (strip.sourceId.includes('ais')) return Ship;
    if (strip.sourceId.includes('upstream') || strip.sourceId.includes('fl511')) return Waypoints;
    return CircleDashed;
  };
</script>

<section class="rail-section" aria-labelledby="rail-heading">
  <header class="rail-header">
    <div>
      <p class="registration-label">Evidence register</p>
      <h2 id="rail-heading">Live time rail</h2>
    </div>
    <span class="auto-register">Auto register</span>
  </header>

  <div class="rail" aria-label="Evidence ordered by observation time">
    <div class="rail-axis" aria-hidden="true">
      <span class="now-pin"></span>
      <span class="rail-rule"></span>
    </div>

    <div class="rail-now">
      <span>Now</span>
      <strong>{formatTime(generatedAt)}</strong>
    </div>

    <div class="strip-stack">
      {#each evidence as strip, index (strip.id)}
        {@const StripIcon = iconFor(strip)}
        <article
          class="dispatch-strip"
          class:corroborated={strip.corroborated}
          class:interrupt={strip.interrupt}
          data-state={strip.state}
          style={`--strip-index: ${index}`}
        >
          <span class="strip-notch" aria-hidden="true"></span>
          <div class="strip-icon"><StripIcon size={25} strokeWidth={1.5} aria-hidden="true" /></div>
          <div class="strip-copy">
            <div class="strip-title-row">
              <h3>{strip.title}</h3>
              <span class="status-word" data-state={strip.availability}>{strip.availability}</span>
            </div>
            <p>{strip.detail}</p>
            <div class="strip-meta">
              <span>{strip.sourceLabel}</span>
              <time datetime={strip.observedAt}>{formatTime(strip.observedAt)}</time>
              <span>Age {formatAge(strip.ageSeconds)}</span>
              {#if strip.contributionBps != null}
                <span>+{Math.round(strip.contributionBps / 100)} evidence</span>
              {/if}
            </div>
          </div>
        </article>
      {/each}

      {#if evidence.length === 0}
        <div class="empty-register">
          <CircleDashed size={22} aria-hidden="true" />
          <strong>No current evidence</strong>
          <span>Fresh observations will register here. Schedule context alone does not create a warning.</span>
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  .rail-section {
    min-width: 0;
    padding: clamp(24px, 3vw, 42px);
    background: var(--paper);
    border-right: 1px solid var(--rule-strong);
  }

  .rail-header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 18px;
    padding-bottom: 13px;
    border-bottom: 1px solid var(--rule-strong);
  }

  .rail-header > div {
    display: grid;
    gap: 7px;
  }

  h2 {
    margin: 0;
    color: var(--marine);
    font-size: var(--type-section);
    font-weight: 700;
    line-height: 0.95;
    text-transform: uppercase;
  }

  .auto-register {
    color: var(--marine);
    border: 1px solid var(--rule-strong);
    border-radius: 2px;
    padding: 6px 8px;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .rail {
    position: relative;
    min-height: 510px;
    padding: 38px 0 28px 88px;
  }

  .rail-axis {
    position: absolute;
    inset: 34px auto 12px 52px;
    display: grid;
    width: 22px;
    justify-items: center;
    grid-template-rows: auto 1fr;
  }

  .now-pin {
    z-index: 1;
    width: 20px;
    height: 20px;
    border: 3px solid var(--marine);
    border-radius: 50%;
    background: var(--frost);
  }

  .rail-rule {
    width: 2px;
    height: 100%;
    background: var(--marine);
  }

  .rail-rule::after {
    display: block;
    width: 2px;
    height: 42px;
    margin-top: 100%;
    background: var(--steel);
    content: '';
  }

  .rail-now {
    position: absolute;
    top: 30px;
    left: 0;
    display: grid;
    width: 46px;
    gap: 2px;
    justify-items: end;
    color: var(--channel);
    font-family: var(--font-instrument);
    text-transform: uppercase;
  }

  .rail-now span {
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.06em;
  }

  .rail-now strong {
    font-size: var(--type-body-small);
    font-weight: 700;
  }

  .strip-stack {
    display: grid;
    gap: 22px;
  }

  .dispatch-strip {
    --state-rule: var(--marine);
    position: relative;
    display: grid;
    min-width: 0;
    grid-template-columns: 44px minmax(0, 1fr);
    color: var(--graphite);
    background: var(--frost);
    border: 1px solid rgba(90, 107, 124, 0.72);
    border-radius: 2px;
    box-shadow: var(--strip-shadow);
    animation: register-strip 480ms cubic-bezier(0.16, 1, 0.3, 1) both;
    animation-delay: calc(var(--strip-index) * 70ms);
  }

  .dispatch-strip::before {
    position: absolute;
    top: 50%;
    right: 100%;
    width: 36px;
    border-top: 1px solid var(--state-rule);
    content: '';
  }

  .dispatch-strip::after {
    position: absolute;
    top: calc(50% - 5px);
    right: calc(100% + 30px);
    width: 10px;
    height: 10px;
    border: 2px solid var(--state-rule);
    border-radius: 50%;
    background: var(--paper);
    content: '';
  }

  .dispatch-strip[data-state='pending'] {
    --state-rule: var(--steel);
    border-style: dashed;
    box-shadow: none;
  }

  .dispatch-strip[data-state='stale'] {
    --state-rule: var(--steel);
    opacity: 0.72;
  }

  .dispatch-strip[data-state='disabled'] {
    --state-rule: var(--steel);
    opacity: 0.48;
    text-decoration: line-through;
    box-shadow: none;
  }

  .dispatch-strip.interrupt {
    border-top-color: var(--amber);
  }

  .dispatch-strip.corroborated::after {
    box-shadow: 0 0 0 3px var(--paper), 0 0 0 4px var(--state-rule);
  }

  .strip-notch {
    position: absolute;
    top: calc(50% - 9px);
    left: -1px;
    width: 9px;
    height: 18px;
    background: var(--paper);
    border-top: 1px solid rgba(90, 107, 124, 0.72);
    border-right: 1px solid rgba(90, 107, 124, 0.72);
    border-bottom: 1px solid rgba(90, 107, 124, 0.72);
  }

  .strip-icon {
    display: grid;
    place-items: center;
    color: var(--marine);
    border-right: 1px solid var(--rule);
  }

  .strip-copy {
    min-width: 0;
    padding: 14px 15px 12px;
  }

  .strip-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  h3 {
    margin: 0;
    overflow: hidden;
    font-size: var(--type-title);
    font-weight: 700;
    line-height: 1;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .strip-title-row .status-word {
    flex: 0 0 auto;
  }

  .strip-copy > p {
    margin: 6px 0 10px;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.35;
  }

  .strip-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 12px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.045em;
    text-transform: uppercase;
  }

  .strip-meta span:last-child {
    margin-left: auto;
    color: var(--channel);
  }

  .empty-register {
    display: grid;
    min-height: 160px;
    place-items: center;
    align-content: center;
    gap: 8px;
    color: var(--muted);
    border: 1px dashed var(--steel);
    padding: 24px;
    text-align: center;
  }

  .empty-register span {
    max-width: 42ch;
    font-size: var(--type-label);
    line-height: 1.45;
  }

  @keyframes register-strip {
    from {
      clip-path: inset(0 100% 0 0);
      transform: translateX(-18px);
      box-shadow: none;
    }
    to {
      clip-path: inset(0 0 0 0);
      transform: translateX(0);
      box-shadow: var(--strip-shadow);
    }
  }

  @media (max-width: 1180px) {
    .rail-section {
      border-right: 0;
      border-bottom: 1px solid var(--rule-strong);
    }
  }

  @media (max-width: 540px) {
    .rail-section {
      padding: 24px 16px 30px;
    }

    .rail {
      min-height: auto;
      padding: 72px 0 0;
    }

    .rail-axis {
      inset: 30px 6px auto;
      display: grid;
      width: auto;
      height: 22px;
      grid-template-columns: auto 1fr;
      grid-template-rows: none;
      align-items: center;
    }

    .rail-rule {
      width: 100%;
      height: 2px;
    }

    .rail-rule::after {
      display: none;
    }

    .rail-now {
      top: 22px;
      left: auto;
      right: 8px;
      width: auto;
      justify-items: end;
    }

    .strip-stack {
      gap: 14px;
    }

    .dispatch-strip::before,
    .dispatch-strip::after {
      display: none;
    }

    .strip-meta span:last-child {
      margin-left: 0;
    }
  }
</style>
