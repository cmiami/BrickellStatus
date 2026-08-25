<script lang="ts">
  import {
    BellRing,
    ChartNoAxesCombined,
    CloudRain,
    ExternalLink,
    Landmark,
    Newspaper,
    Trophy,
    Waves
  } from '@lucide/svelte';
  import { onMount } from 'svelte';

  import type { ChannelPriority, ChannelSignal, ChannelSnapshot } from '$lib/types';
  import { displayStatus } from '$lib/state';

  let { channels, generatedAt }: { channels: ChannelSnapshot[]; generatedAt: string } = $props();
  let noticeRail = $state<HTMLDivElement>();

  interface CurrentNotice {
    key: string;
    channel: ChannelSnapshot;
    signal: ChannelSignal;
    priority: ChannelPriority;
    sourceUrl?: string;
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

    const ranked = notices.toSorted((left, right) =>
      right.priority.score - left.priority.score ||
      left.channel.id.localeCompare(right.channel.id)
    );

    // The rail below is keyed, and a keyed list is not merely untidy when two
    // rows share a key -- Svelte aborts the render, which takes the decision,
    // the river, and this rail off the screen together and leaves the loading
    // skeleton standing with nothing to say why. The engine collapses an alert
    // that reached one channel through two area collectors, so this is the
    // second line of defence; it exists because the cost of a duplicate here
    // is the whole page rather than one wrong row.
    const unique = new Map<string, CurrentNotice>();
    for (const notice of ranked) {
      const key = noticeKey(notice);
      if (!unique.has(key)) unique.set(key, notice);
    }
    return [...unique.values()];
  });

  function noticeKey(notice: CurrentNotice): string {
    return `${notice.channel.id}:${notice.key}`;
  }

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

  function isOnPanel(notice: CurrentNotice): boolean {
    return (
      $displayStatus?.state === 'connected' &&
      Boolean($displayStatus.lastAckAt) &&
      $displayStatus.activeChannelId === notice.channel.id &&
      $displayStatus.activeNoticeKey === notice.key
    );
  }

  function sourceHref(notice: CurrentNotice): string {
    if (notice.sourceUrl) {
      try {
        const source = new URL(notice.sourceUrl);
        if (source.protocol === 'https:') return source.href;
      } catch {
        // A malformed provider URL is not allowed to become a broken external
        // navigation target. The channel remains a useful inspectable fallback.
      }
    }
    return `/channels?channel=${encodeURIComponent(notice.channel.id)}`;
  }

  function hasExternalSource(notice: CurrentNotice): boolean {
    return sourceHref(notice).startsWith('https://');
  }
</script>

{#if current.length}
  <section
    class="signal-board"
    aria-label={`${current.length} active ${current.length === 1 ? 'alert' : 'alerts'}, highest priority first`}
    aria-live="polite"
    aria-relevant="additions text"
  >
    <header class="board-heading">
      <h2>Alerts</h2>
      <p>{current.length}</p>
    </header>

    <div
      id="current-notice-rail"
      bind:this={noticeRail}
      class="notice-rail"
      role="region"
      aria-label="Active alerts, horizontally scrollable"
    >
      {#each current as notice, index (noticeKey(notice))}
        {@const Icon = iconFor(notice.channel.kind)}
        {@const onPanel = isOnPanel(notice)}
        {@const external = hasExternalSource(notice)}
        <article
          class="notice"
          data-urgency={notice.priority.urgency}
          data-leading={index === 0 && commandsAttention(notice)}
          data-on-panel={onPanel}
        >
          <a
            class="notice-link"
            href={sourceHref(notice)}
            target={external ? '_blank' : undefined}
            rel={external ? 'noopener noreferrer' : undefined}
            aria-current={onPanel ? 'true' : undefined}
            aria-label={`${onPanel ? 'On panel. ' : ''}${notice.channel.title}: ${notice.signal.headline}. ${external ? 'Open source content' : 'Open channel'}`}
            title={notice.signal.headline}
          >
            <span class="notice-channel"><Icon size={15} strokeWidth={1.8} aria-hidden="true" />{notice.channel.title}</span>
            <strong class="notice-headline">{notice.signal.headline}</strong>
            <span class="notice-state">{onPanel ? 'On panel' : stateLabel(notice)}</span>
            {#if external}<ExternalLink class="source-mark" size={13} strokeWidth={1.8} aria-hidden="true" />{/if}
          </a>
        </article>
      {/each}
    </div>

    {#if current.length > 1}
      <div class="rail-controls" aria-label="Scroll active alerts">
        <button type="button" aria-label="Previous alerts" aria-controls="current-notice-rail" onclick={() => moveNoticeRail(-1)}>←</button>
        <button type="button" aria-label="Next alerts" aria-controls="current-notice-rail" onclick={() => moveNoticeRail(1)}>→</button>
      </div>
    {/if}
  </section>
{/if}

<style>
  .signal-board {
    display: grid;
    height: 52px;
    grid-template-columns: auto minmax(0, 1fr) auto;
    min-width: 0;
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
    border-bottom: 1px solid var(--rule-strong);
  }

  .board-heading {
    display: flex;
    min-width: 78px;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 12px;
    color: var(--marine);
    background: var(--frost);
    border-inline-end: 1px solid var(--rule-strong);
  }

  .board-heading h2,
  .board-heading p {
    margin: 0;
  }

  .board-heading h2 {
    font-size: var(--type-label);
    line-height: 1;
    letter-spacing: 0.075em;
    text-transform: uppercase;
  }

  .board-heading p {
    min-width: 19px;
    padding: 3px 5px;
    color: var(--white);
    background: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 700;
    line-height: 1;
    text-align: center;
  }

  .notice-rail {
    display: grid;
    min-width: 0;
    grid-auto-columns: minmax(230px, 1fr);
    grid-auto-flow: column;
    overflow-x: auto;
    overflow-y: hidden;
    overscroll-behavior-inline: contain;
    scroll-snap-type: inline proximity;
    scrollbar-width: none;
  }

  .notice-rail::-webkit-scrollbar {
    display: none;
  }

  .notice {
    min-width: 0;
    scroll-snap-align: start;
    background: var(--white);
    border-inline-end: 1px solid var(--rule);
  }

  .notice-link {
    display: grid;
    height: 50px;
    min-width: 0;
    grid-template-columns: auto minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    color: var(--graphite);
    text-decoration: none;
    transition:
      color 120ms ease-out,
      background-color 120ms ease-out;
  }

  .notice-link:hover {
    background: var(--paper);
  }

  .notice-link[aria-current='true'] {
    color: var(--white);
    background: var(--marine);
    box-shadow: inset 0 -1px 0 var(--white);
  }

  .notice-channel {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.055em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .notice-channel :global(svg) {
    flex: 0 0 auto;
  }

  .notice-link[aria-current='true'] .notice-channel {
    color: var(--nav-muted);
  }

  .notice-state {
    white-space: nowrap;
    color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 700;
    letter-spacing: 0.045em;
    line-height: 1;
    text-transform: uppercase;
  }

  .notice[data-leading='true'] .notice-link:not([aria-current='true']) .notice-state {
    color: var(--amber-ink);
  }

  .notice[data-leading='true'][data-urgency='emergency'] .notice-link:not([aria-current='true']) .notice-state {
    color: var(--danger);
  }

  .notice-headline {
    min-width: 0;
    overflow: hidden;
    font-family: var(--font-instrument);
    font-size: var(--type-body-small);
    font-weight: 700;
    line-height: 1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .notice-link :global(.source-mark) {
    flex: 0 0 auto;
    color: var(--channel);
  }

  .notice-link[aria-current='true'] .notice-state,
  .notice-link[aria-current='true'] :global(.source-mark) {
    color: var(--white);
  }

  .rail-controls {
    display: flex;
    background: var(--frost);
    border-inline-start: 1px solid var(--rule-strong);
  }

  .rail-controls button {
    display: grid;
    width: 44px;
    height: 50px;
    place-items: center;
    color: var(--marine);
    background: transparent;
    font-family: var(--font-instrument);
    font-size: var(--type-body);
    font-weight: 700;
    line-height: 1;
    cursor: pointer;
  }

  .rail-controls button + button {
    border-inline-start: 1px solid var(--rule);
  }

  .rail-controls button:hover {
    background: var(--paper);
  }

  @media (max-width: 720px) {
    .board-heading {
      min-width: 62px;
      padding-inline: 9px;
    }

    .board-heading h2 {
      font-size: var(--type-micro);
    }

    .notice-rail {
      grid-auto-columns: minmax(220px, 82vw);
    }

    .notice-link {
      padding-inline: 10px;
    }

    .notice-channel {
      max-width: 74px;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .rail-controls button {
      width: 44px;
    }
  }
</style>
