<script lang="ts">
  import {
    BellRing,
    ChartNoAxesCombined,
    CloudRain,
    Landmark,
    Newspaper,
    Waves
  } from '@lucide/svelte';

  import type { ChannelSnapshot } from '$lib/types';

  let { channels }: { channels: ChannelSnapshot[] } = $props();

  // The bridge is the anchor and has the whole page above this. What belongs
  // here is everything else that is true right now, loudest first — and the
  // order comes from the engine's score rather than from a hierarchy invented
  // in the view, because only the engine knows that rain in eight minutes
  // outranks a storm four hundred miles away.
  const active = $derived(
    channels
      .filter((channel) => channel.enabled && channel.active && channel.kind !== 'bridge')
      .filter((channel) => channel.signal)
      .toSorted((left, right) => right.priority.score - left.priority.score)
  );

  const iconFor = (kind: ChannelSnapshot['kind']) => {
    if (kind === 'weather') return CloudRain;
    if (kind === 'official') return Landmark;
    if (kind === 'hurricane') return Waves;
    if (kind === 'news') return Newspaper;
    if (kind === 'markets') return ChartNoAxesCombined;
    return BellRing;
  };

  // The countdown is the reason this channel is where it is in the order, so
  // it is the thing set in type rather than buried in the detail line.
  function countdown(channel: ChannelSnapshot): string | null {
    const minutes = channel.priority.imminenceMinutes ?? channel.signal?.imminenceMinutes;
    if (minutes == null) return null;
    if (minutes === 0) return 'NOW';
    return `T‑${minutes} MIN`;
  }
</script>

{#if active.length}
  <section class="signal-board" aria-label="Everything else happening now">
    <header class="ruled-header">
      <div>
        <p class="registration-label">Also now</p>
        <h2>Soonest first</h2>
      </div>
    </header>

    <div class="signal-cards">
      {#each active as channel (channel.id)}
        {@const Icon = iconFor(channel.kind)}
        {@const timing = countdown(channel)}
        <article class="signal-card" data-urgency={channel.priority.urgency}>
          <div class="signal-kicker">
            <Icon size={17} strokeWidth={1.7} aria-hidden="true" />
            <span>{channel.title}</span>
            {#if timing}<strong class="signal-countdown">{timing}</strong>{/if}
          </div>
          <p class="signal-headline">{channel.signal?.headline}</p>
          <p class="signal-detail">{channel.signal?.detail}</p>
        </article>
      {/each}
    </div>
  </section>
{/if}

<style>
  .signal-board {
    display: grid;
    gap: 16px;
    padding: clamp(18px, 2.4vw, 28px);
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .signal-cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 14px;
  }

  .signal-card {
    display: grid;
    align-content: start;
    gap: 8px;
    padding: 16px 18px;
    background: var(--white);
    border: 1px solid var(--rule);
    border-inline-start: 4px solid var(--steel);
  }

  /* Urgency is a border weight, not a colour riot. Red is reserved for the
     bridge being up, which is the one thing this page exists to shout. */
  .signal-card[data-urgency='action'] {
    border-inline-start-color: var(--channel);
  }

  .signal-card[data-urgency='emergency'] {
    border-inline-start-color: var(--alert);
  }

  .signal-kicker {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--muted);
    font-size: var(--type-caption);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .signal-countdown {
    margin-inline-start: auto;
    padding: 2px 8px;
    color: var(--white);
    font-variant-numeric: tabular-nums;
    background: var(--marine);
  }

  .signal-card[data-urgency='emergency'] .signal-countdown {
    background: var(--alert);
  }

  .signal-headline {
    margin: 0;
    font-size: var(--type-title);
    font-weight: 700;
    line-height: 1.22;
  }

  .signal-detail {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.5;
  }
</style>
