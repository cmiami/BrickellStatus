<script lang="ts">
  import type { BridgeStateInterval } from '$lib/types';

  let {
    intervals = [],
    localTimeZone
  }: { intervals?: BridgeStateInterval[]; localTimeZone?: string } = $props();

  type SpanState = 'up' | 'down' | 'unknown';

  interface Span {
    key: string;
    name: string;
    relation: 'target' | 'upstream';
    state: SpanState;
    since?: string;
  }

  /// Openings are reported in the bridge's own zone, not the viewer's. A user
  /// watching from another timezone still needs the time a driver at Brickell
  /// would read off a clock. An unusable zone falls back to the local one
  /// rather than throwing and blanking the panel.
  const clock = $derived.by(() => {
    try {
      return new Intl.DateTimeFormat('en-US', {
        hour: 'numeric',
        minute: '2-digit',
        timeZone: localTimeZone || undefined
      });
    } catch {
      return new Intl.DateTimeFormat('en-US', { hour: 'numeric', minute: '2-digit' });
    }
  });

  /// A tick clock so the "open for N min" reading advances without a new
  /// snapshot. Bridge openings are minutes long, so a minute of resolution is
  /// all the precision this reading can honestly claim.
  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  /// The newest interval per bridge is the live one. An interval still missing
  /// `endedAt` is in progress, so it wins outright; otherwise fall back to the
  /// most recently started, which is the last thing we actually observed.
  const spans = $derived.by<Span[]>(() => {
    const latest = new Map<string, BridgeStateInterval>();
    for (const interval of intervals) {
      const held = latest.get(interval.bridgeKey);
      if (!held) {
        latest.set(interval.bridgeKey, interval);
        continue;
      }
      const heldOpen = !held.endedAt;
      const nextOpen = !interval.endedAt;
      if (nextOpen && !heldOpen) {
        latest.set(interval.bridgeKey, interval);
      } else if (nextOpen === heldOpen && interval.startedAt > held.startedAt) {
        latest.set(interval.bridgeKey, interval);
      }
    }

    return [...latest.values()]
      .map((interval) => ({
        key: interval.bridgeKey,
        name: interval.bridgeName,
        relation: interval.relation,
        // A closed-out interval describes history, not the present. Only an
        // open-ended one lets us claim the span is up right now.
        state: (interval.endedAt ? 'down' : interval.state) as SpanState,
        since: interval.endedAt ? undefined : interval.startedAt
      }))
      .sort((left, right) =>
        left.relation === right.relation
          ? left.name.localeCompare(right.name)
          : left.relation === 'target'
            ? -1
            : 1
      );
  });

  const target = $derived(spans.find((span) => span.relation === 'target') ?? null);
  const upstream = $derived(spans.filter((span) => span.relation === 'upstream'));

  function openedAt(span: Span): string | null {
    if (span.state !== 'up' || !span.since) return null;
    const parsed = new Date(span.since);
    return Number.isNaN(parsed.getTime()) ? null : clock.format(parsed);
  }

  function openMinutes(span: Span): number | null {
    if (span.state !== 'up' || !span.since) return null;
    const parsed = new Date(span.since).getTime();
    if (Number.isNaN(parsed)) return null;
    return Math.max(0, Math.floor((now - parsed) / 60_000));
  }

  function stateWord(state: SpanState): string {
    return state === 'up' ? 'Open' : state === 'down' ? 'Closed' : 'Unknown';
  }

  function spanLabel(span: Span): string {
    const opened = openedAt(span);
    const minutes = openMinutes(span);
    if (span.state === 'up' && opened) {
      return `${span.name}. Open since ${opened}, ${minutes} minutes.`;
    }
    return `${span.name}. ${stateWord(span.state)}.`;
  }
</script>

<section class="spans" aria-label="Miami River bascule spans">
  <header class="spans-header">
    <p class="registration-label">River spans</p>
    <span class="spans-legend" aria-hidden="true">
      <em data-state="up"></em> Open
      <em data-state="down"></em> Closed
    </span>
  </header>

  {#if target}
    <figure class="target" data-state={target.state} aria-label={spanLabel(target)}>
      <!--
        Double-leaf bascule: each leaf is hinged over its own pier and the free
        ends meet at midspan when closed. Opening lifts both leaves away from
        the centre, which is the movement drivers actually see from Brickell.
      -->
      <svg viewBox="0 0 200 92" role="img" aria-hidden="true" class="bascule">
        <line class="water" x1="0" y1="74" x2="200" y2="74" />
        <line class="water water-low" x1="0" y1="80" x2="200" y2="80" />

        <rect class="approach" x="0" y="41" width="28" height="6" />
        <rect class="approach" x="172" y="41" width="28" height="6" />
        <rect class="pier" x="20" y="47" width="12" height="27" />
        <rect class="pier" x="168" y="47" width="12" height="27" />

        <g class="leaf leaf-left">
          <rect x="28" y="41" width="72" height="6" />
          <path class="rail" d="M32 41 L32 34 M52 41 L52 34 M72 41 L72 34 M92 41 L92 34" />
        </g>
        <g class="leaf leaf-right">
          <rect x="100" y="41" width="72" height="6" />
          <path class="rail" d="M108 41 L108 34 M128 41 L128 34 M148 41 L148 34 M168 41 L168 34" />
        </g>
      </svg>

      <figcaption>
        <span class="registration-label">{target.name}</span>
        <strong>{stateWord(target.state)}</strong>
        {#if openedAt(target)}
          <small>Opened {openedAt(target)} · {openMinutes(target)} min</small>
        {:else if target.state === 'down'}
          <small>Deck down · traffic moving</small>
        {:else}
          <small>No confirmed reading</small>
        {/if}
      </figcaption>
    </figure>
  {/if}

  {#if upstream.length}
    <ul class="upstream">
      {#each upstream as span (span.key)}
        <li data-state={span.state} title={spanLabel(span)}>
          <span class="pip" aria-hidden="true">
            <svg viewBox="0 0 24 14" class="mini-bascule">
              <line class="water" x1="0" y1="11" x2="24" y2="11" />
              <rect class="approach" x="0" y="5" width="4" height="2.4" />
              <rect class="approach" x="20" y="5" width="4" height="2.4" />
              <rect class="leaf mini-left" x="4" y="5" width="8" height="2.4" />
              <rect class="leaf mini-right" x="12" y="5" width="8" height="2.4" />
            </svg>
          </span>
          <span class="pip-text">
            <strong>{span.name}</strong>
            {#if openedAt(span)}
              <small>Open {openedAt(span)} · {openMinutes(span)} min</small>
            {:else}
              <small>{stateWord(span.state)}</small>
            {/if}
          </span>
        </li>
      {/each}
    </ul>
    <p class="upstream-note">
      An upstream span opening minutes before or after Brickell is the same vessel
      working the river.
    </p>
  {/if}
</section>

<style>
  .spans {
    display: grid;
    grid-template-columns: minmax(230px, 330px) minmax(0, 1fr);
    gap: 16px clamp(20px, 3vw, 40px);
    align-items: start;
    padding: clamp(18px, 2.4vw, 30px) clamp(20px, 3vw, 42px);
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
  }

  .spans-header {
    display: flex;
    grid-column: 1 / -1;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding-bottom: 11px;
    border-bottom: 1px solid var(--rule-strong);
  }

  .spans-legend {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .spans-legend em + em {
    margin-left: 9px;
  }

  .spans-legend em {
    width: 10px;
    height: 10px;
    border: 1px solid var(--graphite);
  }

  .spans-legend em[data-state='up'] {
    background: var(--danger);
  }

  .spans-legend em[data-state='down'] {
    background: var(--success);
  }

  .target {
    display: grid;
    gap: 12px;
    margin: 0;
  }

  .bascule {
    display: block;
    width: 100%;
    height: auto;
    background: var(--white);
    border: 1px solid var(--rule);
  }

  .water {
    stroke: var(--channel);
    stroke-width: 1.4;
    opacity: 0.5;
  }

  .water-low {
    opacity: 0.26;
  }

  .approach,
  .pier {
    fill: var(--steel);
  }

  .leaf rect,
  .leaf.mini-left,
  .leaf.mini-right {
    fill: var(--success);
  }

  .leaf .rail {
    stroke: var(--marine);
    stroke-width: 1.6;
    fill: none;
  }

  /* Each leaf turns about its own pier, so the free end at midspan is what
     rises. view-box keeps the origin in the same units as the geometry. */
  .leaf-left,
  .leaf-right {
    transform-box: view-box;
    transition: transform 900ms cubic-bezier(0.32, 0.06, 0.2, 1);
  }

  .leaf-left {
    transform-origin: 28px 44px;
  }

  .leaf-right {
    transform-origin: 172px 44px;
  }

  .target[data-state='up'] .leaf-left {
    transform: rotate(-64deg);
  }

  .target[data-state='up'] .leaf-right {
    transform: rotate(64deg);
  }

  .target[data-state='up'] .leaf rect {
    fill: var(--danger);
  }

  .target[data-state='unknown'] .leaf rect {
    fill: var(--steel);
  }

  .target[data-state='unknown'] .leaf .rail {
    stroke: var(--paper);
  }

  figcaption {
    display: grid;
    gap: 5px;
  }

  figcaption strong {
    font-family: var(--font-instrument);
    font-size: var(--type-section);
    font-weight: 700;
    line-height: 1;
    text-transform: uppercase;
  }

  .target[data-state='up'] figcaption strong {
    color: var(--danger);
  }

  .target[data-state='down'] figcaption strong {
    color: var(--success);
  }

  .target[data-state='unknown'] figcaption strong {
    color: var(--muted);
  }

  figcaption small {
    color: var(--muted);
    font-size: var(--type-caption);
  }

  .upstream {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
    gap: 1px;
    margin: 0;
    padding: 0;
    background: var(--rule);
    border: 1px solid var(--rule);
    list-style: none;
  }

  .upstream li {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 9px 11px;
    background: var(--white);
    border-left: 4px solid var(--steel);
  }

  .upstream li[data-state='up'] {
    background: var(--amber-sheet);
    border-left-color: var(--danger);
  }

  .upstream li[data-state='down'] {
    border-left-color: var(--success);
  }

  .pip {
    display: block;
    flex: none;
    width: 42px;
  }

  .mini-bascule {
    display: block;
    width: 100%;
    height: auto;
  }

  .mini-bascule .leaf {
    transform-box: view-box;
    transition: transform 700ms cubic-bezier(0.32, 0.06, 0.2, 1);
  }

  .mini-left {
    transform-origin: 4px 6.2px;
  }

  .mini-right {
    transform-origin: 20px 6.2px;
  }

  li[data-state='up'] .mini-left {
    transform: rotate(-62deg);
  }

  li[data-state='up'] .mini-right {
    transform: rotate(62deg);
  }

  li[data-state='up'] .leaf {
    fill: var(--danger);
  }

  li[data-state='unknown'] .leaf {
    fill: var(--steel);
  }

  .pip-text {
    display: grid;
    min-width: 0;
    gap: 2px;
  }

  .pip-text strong {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }

  .pip-text small {
    color: var(--muted);
    font-size: var(--type-micro);
  }

  li[data-state='up'] .pip-text small {
    color: var(--amber-ink);
    font-weight: 700;
  }

  .upstream-note {
    grid-column: 2;
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
  }

  @media (max-width: 900px) {
    .spans {
      grid-template-columns: minmax(0, 1fr);
    }

    .upstream-note {
      grid-column: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .leaf-left,
    .leaf-right,
    .mini-bascule .leaf {
      transition: none;
    }
  }
</style>
