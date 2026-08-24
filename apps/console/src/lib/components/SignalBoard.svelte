<script lang="ts">
  import {
    BellRing,
    ChartNoAxesCombined,
    CloudRain,
    Landmark,
    Newspaper,
    Trophy,
    Waves
  } from '@lucide/svelte';
  import { onMount } from 'svelte';

  import type { ChannelPriority, ChannelSignal, ChannelSnapshot } from '$lib/types';

  let { channels, generatedAt }: { channels: ChannelSnapshot[]; generatedAt: string } = $props();
  let noticeRail = $state<HTMLDivElement>();

  interface CurrentNotice {
    key: string;
    channel: ChannelSnapshot;
    signal: ChannelSignal;
    priority: ChannelPriority;
  }

  const generatedAtMs = $derived(Date.parse(generatedAt));
  let wallClockMs = $state<number | null>(null);
  const relevanceTimeMs = $derived(
    wallClockMs === null
      ? generatedAtMs
      : Number.isFinite(generatedAtMs)
        ? Math.max(generatedAtMs, wallClockMs)
        : wallClockMs
  );

  onMount(() => {
    const advanceClock = () => {
      wallClockMs = Date.now();
    };
    advanceClock();
    const timer = setInterval(advanceClock, 5_000);
    return () => clearInterval(timer);
  });

  function isCurrent(signal: ChannelSignal): boolean {
    const expiresAt = signal.expiresAt;
    if (!expiresAt) return true;
    const expiresAtMs = Date.parse(expiresAt);
    // The runtime normally clears expired signals. This second boundary keeps a
    // delayed snapshot from putting a notice back on screen at or after expiry.
    return !Number.isFinite(expiresAtMs) || !Number.isFinite(relevanceTimeMs) || expiresAtMs > relevanceTimeMs;
  }

  // The bridge is the anchor and has the whole page above this. What belongs
  // here is everything else that is true right now, loudest first — and the
  // order comes from the engine's score rather than from a hierarchy invented
  // in the view, because only the engine knows that rain in eight minutes
  // outranks a storm four hundred miles away.
  const current = $derived.by(() => {
    const notices = channels
      .filter((channel) => channel.enabled && channel.active && channel.kind !== 'bridge')
      .flatMap<CurrentNotice>((channel) => {
        const channelNotices = channel.notices ?? (channel.signal
          ? [{ key: channel.materialKey, signal: channel.signal, priority: channel.priority }]
          : []);
        return channelNotices.map((notice) => ({ ...notice, channel }));
      })
      .filter((notice) => isCurrent(notice.signal));

    return notices.toSorted((left, right) =>
      right.priority.score - left.priority.score ||
      left.channel.id.localeCompare(right.channel.id)
    );
  });

  const iconFor = (kind: ChannelSnapshot['kind']) => {
    if (kind === 'weather') return CloudRain;
    if (kind === 'official') return Landmark;
    if (kind === 'hurricane') return Waves;
    if (kind === 'news') return Newspaper;
    if (kind === 'sports') return Trophy;
    if (kind === 'markets') return ChartNoAxesCombined;
    return BellRing;
  };

  // The countdown is the reason this channel is where it is in the order, so
  // it is the thing set in type rather than buried in the detail line.
  function countdown(notice: CurrentNotice): string | null {
    const minutes = notice.priority.imminenceMinutes ?? notice.signal.imminenceMinutes;
    if (minutes == null) return null;
    if (minutes === 0) return 'NOW';
    return `T‑${minutes} MIN`;
  }

  function stateLabel(notice: CurrentNotice): string {
    const timing = countdown(notice);
    if (timing) return timing;
    if (notice.signal.severity) return notice.signal.severity.toUpperCase();
    if (notice.priority.urgency === 'emergency') return 'EMERGENCY';
    if (notice.priority.urgency === 'action') return 'ACT NOW';
    if (notice.priority.confirmed) return 'CONFIRMED';
    if (notice.priority.urgency === 'heads_up') return 'HEADS UP';
    return 'CURRENT';
  }

  function commandsAttention(notice: CurrentNotice): boolean {
    const minutes = notice.priority.imminenceMinutes ?? notice.signal.imminenceMinutes;
    return (
      (minutes !== undefined && minutes <= 15) ||
      notice.priority.urgency === 'action' ||
      notice.priority.urgency === 'emergency'
    );
  }

  function moveNoticeRail(direction: -1 | 1): void {
    if (!noticeRail) return;
    const page = Math.max(noticeRail.clientWidth * 0.8, 280);
    noticeRail.scrollBy({ left: page * direction, behavior: 'smooth' });
  }
</script>

{#if current.length}
  <section
    class="signal-board"
    aria-label={`${current.length} current ${current.length === 1 ? 'notice' : 'notices'}, highest priority first`}
    aria-live="polite"
    aria-relevant="additions text"
  >
    <header class="board-heading">
      <h2>Current notices</h2>
      <p>{current.length} active · urgent first</p>
      {#if current.length > 1}
        <div class="rail-controls" aria-label="Scroll current notices">
          <button type="button" aria-label="Previous notices" aria-controls="current-notice-rail" onclick={() => moveNoticeRail(-1)}>←</button>
          <button type="button" aria-label="Next notices" aria-controls="current-notice-rail" onclick={() => moveNoticeRail(1)}>→</button>
        </div>
      {/if}
    </header>

    <div
      id="current-notice-rail"
      bind:this={noticeRail}
      class="notice-rail"
      role="region"
      aria-label="Current notices, horizontally scrollable"
    >
      {#each current as notice, index (`${notice.channel.id}:${notice.key}`)}
        {@const Icon = iconFor(notice.channel.kind)}
        <article
          class="notice"
          data-urgency={notice.priority.urgency}
          data-leading={index === 0 && commandsAttention(notice)}
        >
          <header>
            <span class="notice-channel"><Icon size={16} strokeWidth={1.7} aria-hidden="true" />{notice.channel.title}</span>
            <strong class="notice-state">{stateLabel(notice)}</strong>
          </header>
          <p class="notice-headline">{notice.signal.headline}</p>
          <p class="notice-detail">{notice.signal.detail}</p>
        </article>
      {/each}
    </div>
  </section>
{/if}

<style>
  .signal-board {
    display: grid;
    grid-template-columns: minmax(150px, 0.55fr) minmax(0, 4fr);
    min-width: 0;
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
  }

  .board-heading {
    display: grid;
    align-content: center;
    gap: 5px;
    padding: 14px clamp(16px, 2vw, 26px);
    background: var(--paper);
    border-inline-end: 1px solid var(--rule-strong);
  }

  .board-heading h2,
  .board-heading p {
    margin: 0;
  }

  .board-heading h2 {
    color: var(--marine);
    font-size: var(--type-title);
    line-height: 1;
    text-transform: uppercase;
  }

  .rail-controls {
    display: flex;
    gap: 6px;
    margin-top: 4px;
  }

  .rail-controls button {
    display: grid;
    width: 30px;
    height: 28px;
    place-items: center;
    color: var(--marine);
    background: var(--white);
    border: 1px solid var(--rule-strong);
    font-family: var(--font-instrument);
    font-size: var(--type-body);
    line-height: 1;
    cursor: pointer;
  }

  .board-heading p {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.055em;
    text-transform: uppercase;
  }

  .notice-rail {
    display: grid;
    min-width: 0;
    grid-auto-columns: minmax(270px, 1fr);
    grid-auto-flow: column;
    overflow-x: auto;
    overscroll-behavior-inline: contain;
    scrollbar-color: var(--steel) var(--frost);
    scrollbar-width: thin;
  }

  .notice-rail::-webkit-scrollbar {
    height: 8px;
  }

  .notice-rail::-webkit-scrollbar-track {
    background: var(--frost);
  }

  .notice-rail::-webkit-scrollbar-thumb {
    background: var(--steel);
    border: 2px solid var(--frost);
  }

  .notice {
    display: grid;
    min-width: 0;
    align-content: center;
    gap: 5px;
    padding: 12px 16px 13px;
    background: var(--white);
    border-inline-end: 1px solid var(--rule);
  }

  .notice > header,
  .notice-channel {
    display: flex;
    align-items: center;
  }

  .notice > header {
    min-width: 0;
    justify-content: space-between;
    gap: 12px;
  }

  .notice-channel {
    min-width: 0;
    gap: 7px;
    overflow: hidden;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 600;
    letter-spacing: 0.055em;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .notice-channel :global(svg) {
    flex: 0 0 auto;
  }

  .notice-state {
    flex: 0 0 auto;
    padding: 3px 6px;
    color: var(--marine);
    background: var(--frost);
    border: 1px solid var(--rule);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    letter-spacing: 0.045em;
    line-height: 1;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .notice[data-leading='true'] .notice-state {
    color: var(--graphite);
    background: var(--amber);
    border-color: var(--amber-ink);
  }

  .notice[data-leading='true'][data-urgency='emergency'] .notice-state {
    color: var(--white);
    background: var(--danger);
    border-color: var(--danger);
  }

  .notice-headline {
    margin: 0;
    color: var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-body);
    font-weight: 700;
    line-height: 1.12;
    overflow-wrap: anywhere;
  }

  .notice-detail {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  @media (max-width: 720px) {
    .signal-board {
      grid-template-columns: 1fr;
    }

    .board-heading {
      border-inline-end: 0;
      border-bottom: 1px solid var(--rule-strong);
    }

    .rail-controls {
      display: none;
    }

    .notice-rail {
      grid-auto-flow: row;
      grid-auto-columns: auto;
      overflow-x: visible;
    }

    .notice {
      border-inline-end: 0;
      border-bottom: 1px solid var(--rule);
    }

    .notice:last-child {
      border-bottom: 0;
    }

  }
</style>
