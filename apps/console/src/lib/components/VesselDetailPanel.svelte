<script lang="ts">
  import { Clock3, Navigation, RotateCcw, X } from '@lucide/svelte';

  import VesselGlyph from '$lib/components/VesselGlyph.svelte';
  import type { BridgeCrossing, VesselDetail, VesselTrack } from '$lib/types';

  let {
    track,
    detail = null,
    loading = false,
    error = null,
    localTimeZone,
    onclose,
    onretry = () => {}
  }: {
    track: VesselTrack;
    detail?: VesselDetail | null;
    loading?: boolean;
    error?: string | null;
    localTimeZone?: string;
    onclose: () => void;
    onretry?: () => void;
  } = $props();

  let panel = $state<HTMLElement | null>(null);
  let focusedMmsi = '';

  $effect(() => {
    const mmsi = track.mmsi;
    if (mmsi === focusedMmsi) return;
    focusedMmsi = mmsi;
    queueMicrotask(() => panel?.focus());
  });

  const vesselName = $derived(track.vesselName?.trim() || `Vessel ${track.mmsi}`);
  const resolvedPassages = $derived(
    (detail?.transitsOpened ?? 0) + (detail?.transitsFitsUnder ?? 0)
  );
  const recordedPassages = $derived(
    resolvedPassages + (detail?.transitsUnknown ?? 0) + (detail?.transitsPending ?? 0)
  );

  const movementWords: Record<VesselTrack['movement'], string> = {
    approaching: 'Toward Brickell',
    diverging: 'Away from Brickell',
    stationary: 'Holding position',
    unknown: 'Direction not clear'
  };

  const postureWords: Record<NonNullable<VesselTrack['posture']>, string> = {
    underway: 'Under way',
    waiting: 'Waiting near the bridge',
    holding: 'Holding',
    moored: 'Moored',
    off_channel: 'Outside the tracked channel',
    deep_draft: 'Too deep for the river'
  };

  const branchWords: Record<NonNullable<VesselTrack['branch']>, string> = {
    river: 'Miami River',
    north_approach: 'Main Channel approach',
    government_cut: 'Government Cut',
    south_approach: 'South channel approach'
  };

  function formatDateTime(value: string): string {
    const date = new Date(value);
    if (!Number.isFinite(date.getTime())) return 'Time unavailable';
    try {
      return new Intl.DateTimeFormat(undefined, {
        month: 'short',
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
        timeZone: localTimeZone || undefined
      }).format(date);
    } catch {
      return date.toLocaleString([], {
        month: 'short',
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit'
      });
    }
  }

  function formatDimension(value: number | undefined): string | null {
    return value != null && Number.isFinite(value) && value > 0 ? `${value.toFixed(1)} m` : null;
  }

  function dimensions(): string | null {
    const length = formatDimension(track.lengthMeters);
    const beam = formatDimension(track.beamMeters);
    const draught = formatDimension(track.draughtMeters);
    if (!length && !beam && !draught) return null;
    return [
      length ? `${length} long` : null,
      beam ? `${beam} beam` : null,
      draught ? `${draught} draught` : null
    ]
      .filter(Boolean)
      .join(' · ');
  }

  function etaWords(): string | null {
    if (track.etaMinMinutes == null) return null;
    if (track.etaMaxMinutes == null || track.etaMaxMinutes === track.etaMinMinutes) {
      return `${track.etaMinMinutes} min to Brickell`;
    }
    return `${track.etaMinMinutes}–${track.etaMaxMinutes} min to Brickell`;
  }

  function impactSummary(): string {
    if (track.routeIntersects) {
      return etaWords()
        ? `Latest reading: on a Brickell-bound path · ${etaWords()}`
        : 'Latest reading: on a Brickell-bound path. Arrival time is not clear yet.';
    }
    if (track.movement === 'approaching') {
      return 'Latest reading: moving toward Brickell, but too far out to count as an expected passage.';
    }
    if (track.movement === 'diverging') return 'Latest reading: moving away from Brickell.';
    if (track.movement === 'stationary') return 'The latest reading showed no movement toward Brickell.';
    return 'The latest reading does not show a clear effect on Brickell.';
  }

  function historySummary(): string {
    if (!detail || recordedPassages === 0) {
      return 'No recorded Brickell passage for this vessel yet.';
    }
    if (resolvedPassages === 0) {
      return `Brickell has recorded ${recordedPassages} ${recordedPassages === 1 ? 'passage' : 'passages'}, but the bridge result is not confirmed yet.`;
    }
    if (detail.transitsOpened > 0) {
      return `Brickell went up for this vessel on ${detail.transitsOpened} of ${resolvedPassages} confirmed ${resolvedPassages === 1 ? 'passage' : 'passages'}.`;
    }
    return `This vessel passed Brickell ${resolvedPassages} ${resolvedPassages === 1 ? 'time' : 'times'} with the bridge down.`;
  }

  function crossingResult(crossing: BridgeCrossing): string {
    switch (crossing.outcome) {
      case 'opened':
        return 'Bridge went up';
      case 'fits_under':
        return 'Bridge stayed down';
      case 'unknown':
        return 'Bridge result not confirmed';
      default:
        return 'Still checking the bridge result';
    }
  }

  function crossingDirection(crossing: BridgeCrossing): string {
    return crossing.direction === 'upriver' ? 'Upriver' : 'Downriver';
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose();
    }
  }}
/>

<aside
  bind:this={panel}
  class="vessel-detail"
  aria-labelledby="vessel-detail-title"
  aria-describedby="vessel-detail-impact"
  tabindex="-1"
>
  <header class="detail-header">
    <svg class="vessel-mark" viewBox="-42 -27 84 54" role="img" aria-label={`${vesselName} vessel profile`}>
      <VesselGlyph kind={track.vesselClass} length={54} />
    </svg>
    <div>
      <p class="registration-label">Vessel details</p>
      <h2 id="vessel-detail-title">{vesselName}</h2>
      <p>MMSI {track.mmsi}{track.vesselClass ? ` · ${track.vesselClass}` : ''}</p>
    </div>
    <button class="detail-close" type="button" aria-label="Close vessel details" onclick={onclose}>
      <X size={18} aria-hidden="true" />
    </button>
  </header>

  <section class="current-reading" aria-labelledby="current-reading-title">
    <h3 id="current-reading-title">Latest AIS reading</h3>
    <div class="reading-strip">
      <div><strong>{track.speedKnots.toFixed(1)}</strong><span>knots</span></div>
      <div><strong>{track.courseDegrees.toFixed(0)}°</strong><span>course</span></div>
      <div>
        <strong>{movementWords[track.movement]}</strong>
        <span>{track.posture ? postureWords[track.posture] : 'Standing unavailable'}</span>
      </div>
    </div>

    <dl class="identity-grid">
      {#if track.callSign}<div><dt>Call sign</dt><dd>{track.callSign}</dd></div>{/if}
      {#if track.imoNumber}<div><dt>IMO</dt><dd>{track.imoNumber}</dd></div>{/if}
      {#if track.destination}<div class="wide"><dt>Destination</dt><dd>{track.destination}</dd></div>{/if}
      {#if dimensions()}<div class="wide"><dt>Dimensions</dt><dd>{dimensions()}</dd></div>{/if}
      <div><dt>Waterway</dt><dd>{track.branch ? branchWords[track.branch] : 'Not placed on the channel'}</dd></div>
      <div><dt>Latest position</dt><dd><time datetime={track.observedAt}>{formatDateTime(track.observedAt)}</time></dd></div>
    </dl>
  </section>

  <section class:expected={track.routeIntersects} class="brickell-impact" aria-labelledby="brickell-impact-title">
    <div class="section-heading">
      <Navigation size={18} strokeWidth={1.7} aria-hidden="true" />
      <h3 id="brickell-impact-title">Brickell impact</h3>
    </div>
    <p id="vessel-detail-impact">{impactSummary()}</p>
    {#if track.predictedOpeningAt}
      <p class="passage-time">
        <Clock3 size={15} aria-hidden="true" />
        Possible passage {formatDateTime(track.predictedOpeningAt)}{track.waitsForSlot ? ' · waits for the next opening time' : ''}
      </p>
    {/if}
  </section>

  <section class="passage-history" aria-labelledby="passage-history-title" aria-busy={loading}>
    <div class="section-heading">
      <h3 id="passage-history-title">Brickell passage history</h3>
      {#if detail?.lastCrossingAt}
        <time datetime={detail.lastCrossingAt}>Last crossing {formatDateTime(detail.lastCrossingAt)}</time>
      {/if}
    </div>

    {#if loading}
      <p class="history-state" role="status">Loading this vessel’s Brickell history…</p>
    {:else if error}
      <div class="history-state history-error" role="alert">
        <p>Brickell history could not be loaded. The latest AIS reading above is still available.</p>
        <button type="button" onclick={onretry}><RotateCcw size={15} aria-hidden="true" /> Try again</button>
      </div>
    {:else}
      <p class="history-summary">{historySummary()}</p>
      {#if detail}
        <div class="history-register">
          <span>First seen <time datetime={detail.firstSeenAt}>{formatDateTime(detail.firstSeenAt)}</time></span>
          {#if detail.lastOpenedAt}
            <span>Last bridge-up passage <time datetime={detail.lastOpenedAt}>{formatDateTime(detail.lastOpenedAt)}</time></span>
          {/if}
          {#if detail.openingPropensity != null && resolvedPassages > 0}
            <span>{Math.round(detail.openingPropensity / 100)}% estimated opening chance from confirmed passages</span>
          {/if}
        </div>
      {/if}
      {#if detail && recordedPassages > 0}
        <dl class="passage-totals">
          <div><dt>Bridge up</dt><dd>{detail.transitsOpened}</dd></div>
          <div><dt>Bridge down</dt><dd>{detail.transitsFitsUnder}</dd></div>
          <div><dt>Not confirmed</dt><dd>{detail.transitsUnknown + detail.transitsPending}</dd></div>
        </dl>
      {/if}

      {#if detail?.recentCrossings.length}
        <ol class="crossing-list" aria-label="Recent Brickell crossings">
          {#each detail.recentCrossings as crossing (crossing.crossedAt)}
            <li>
              <span><strong>{crossingResult(crossing)}</strong><small>{crossingDirection(crossing)}{crossing.speedKnots != null ? ` · ${crossing.speedKnots.toFixed(1)} kn` : ''}</small></span>
              <time datetime={crossing.crossedAt}>{formatDateTime(crossing.crossedAt)}</time>
            </li>
          {/each}
        </ol>
      {/if}
    {/if}
  </section>
</aside>

<style>
  .vessel-detail {
    display: grid;
    max-height: min(720px, calc(100vh - 176px));
    overflow: auto;
    color: var(--graphite);
    background: var(--paper);
    border: 1px solid var(--marine);
    border-radius: 2px;
    box-shadow: var(--strip-shadow);
    scrollbar-color: var(--steel) transparent;
  }

  .detail-header {
    position: sticky;
    top: 0;
    z-index: 1;
    display: grid;
    grid-template-columns: 82px minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    padding: 16px 16px 15px;
    color: var(--white);
    background: var(--marine);
    border-bottom: 1px solid var(--marine);
  }

  .vessel-mark {
    width: 82px;
    height: 54px;
    overflow: visible;
    color: var(--corridor);
    background: var(--white);
    border: 1px solid var(--nav-subdued);
  }

  .detail-header .registration-label {
    margin: 0 0 4px;
    color: var(--nav-muted);
  }

  .detail-header h2 {
    margin: 0;
    overflow-wrap: anywhere;
    font-size: var(--type-section);
    line-height: 0.95;
    text-transform: uppercase;
  }

  .detail-header p:last-child {
    margin: 5px 0 0;
    color: var(--nav-muted);
    font-size: var(--type-caption);
  }

  .detail-close {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    color: var(--white);
    background: transparent;
    border: 1px solid var(--nav-subdued);
    border-radius: 2px;
    cursor: pointer;
  }

  .detail-close:hover {
    color: var(--marine);
    background: var(--white);
  }

  .current-reading,
  .brickell-impact,
  .passage-history {
    padding: 18px;
    border-bottom: 1px solid var(--rule-strong);
  }

  .passage-history {
    border-bottom: 0;
  }

  h3 {
    margin: 0;
    color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    letter-spacing: 0.06em;
    line-height: 1;
    text-transform: uppercase;
  }

  .reading-strip {
    display: grid;
    grid-template-columns: 0.72fr 0.72fr 1.5fr;
    margin-top: 12px;
    color: var(--marine);
    border-block: 1px solid var(--rule-strong);
  }

  .reading-strip > div {
    display: grid;
    align-content: center;
    min-width: 0;
    min-height: 68px;
    gap: 2px;
    padding: 10px 12px;
    border-right: 1px solid var(--rule);
  }

  .reading-strip > div:last-child {
    border-right: 0;
  }

  .reading-strip strong {
    overflow-wrap: anywhere;
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    line-height: 1;
    text-transform: uppercase;
  }

  .reading-strip span,
  .identity-grid dt,
  .passage-totals dt,
  .crossing-list small,
  .section-heading time {
    color: var(--muted);
    font-size: var(--type-micro);
    line-height: 1.35;
  }

  .reading-strip span,
  .identity-grid dt,
  .passage-totals dt {
    font-family: var(--font-instrument);
    letter-spacing: 0.055em;
    text-transform: uppercase;
  }

  .identity-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    margin: 14px 0 0;
    border-top: 1px solid var(--rule);
  }

  .identity-grid > div {
    display: grid;
    align-content: start;
    gap: 4px;
    padding: 11px 10px 9px 0;
    border-bottom: 1px solid var(--rule);
  }

  .identity-grid > div:nth-child(even):not(.wide) {
    padding-left: 10px;
    border-left: 1px solid var(--rule);
  }

  .identity-grid .wide {
    grid-column: 1 / -1;
  }

  .identity-grid dd,
  .passage-totals dd {
    margin: 0;
    overflow-wrap: anywhere;
    font-size: var(--type-caption);
    font-weight: 600;
    line-height: 1.4;
  }

  .brickell-impact {
    background: var(--frost);
  }

  .brickell-impact.expected {
    background: var(--corridor-sheet);
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    color: var(--marine);
  }

  .brickell-impact .section-heading {
    justify-content: start;
  }

  .brickell-impact > p,
  .history-summary,
  .history-state p {
    margin: 10px 0 0;
    font-size: var(--type-body-small);
    line-height: 1.5;
  }

  .passage-time {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--marine);
    font-weight: 600;
  }

  .history-state {
    margin: 12px 0 0;
    padding: 14px;
    color: var(--muted);
    background: var(--frost);
    border: 1px dashed var(--rule-strong);
    font-size: var(--type-body-small);
  }

  .history-error {
    color: var(--danger);
    background: var(--paper);
    border-style: solid;
  }

  .history-error p {
    margin-top: 0;
  }

  .history-error button {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    margin-top: 11px;
    color: var(--danger);
    background: transparent;
    border: 1px solid currentColor;
    padding: 8px 10px;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .history-register {
    display: flex;
    flex-wrap: wrap;
    gap: 5px 14px;
    margin-top: 10px;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .history-register span + span {
    padding-left: 14px;
    border-left: 1px solid var(--rule-strong);
  }

  .passage-totals {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    margin: 14px 0 0;
    border-block: 1px solid var(--rule-strong);
  }

  .passage-totals > div {
    display: grid;
    gap: 3px;
    padding: 10px;
    border-right: 1px solid var(--rule);
  }

  .passage-totals > div:last-child {
    border-right: 0;
  }

  .passage-totals dd {
    color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-title);
  }

  .crossing-list {
    display: grid;
    margin: 14px 0 0;
    padding: 0;
    border-top: 1px solid var(--rule);
    list-style: none;
  }

  .crossing-list li {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 9px 0;
    border-bottom: 1px solid var(--rule);
  }

  .crossing-list span {
    display: grid;
    gap: 3px;
  }

  .crossing-list strong {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    text-transform: uppercase;
  }

  .crossing-list time {
    color: var(--muted);
    font-size: var(--type-caption);
    white-space: nowrap;
  }

  @media (max-width: 560px) {
    .detail-header {
      grid-template-columns: 64px minmax(0, 1fr) auto;
      padding: 13px;
    }

    .vessel-mark {
      width: 64px;
      height: 48px;
    }

    .current-reading,
    .brickell-impact,
    .passage-history {
      padding: 15px;
    }

    .reading-strip {
      grid-template-columns: 1fr 1fr;
    }

    .reading-strip > div:nth-child(2) {
      border-right: 0;
    }

    .reading-strip > div:last-child {
      grid-column: 1 / -1;
      border-top: 1px solid var(--rule);
    }

    .identity-grid,
    .passage-totals {
      grid-template-columns: 1fr;
    }

    .identity-grid > div,
    .identity-grid > div:nth-child(even):not(.wide) {
      padding-left: 0;
      border-left: 0;
    }

    .passage-totals > div {
      grid-template-columns: 1fr auto;
      align-items: center;
      border-right: 0;
      border-bottom: 1px solid var(--rule);
    }

    .passage-totals > div:last-child {
      border-bottom: 0;
    }

    .crossing-list li {
      grid-template-columns: 1fr;
      gap: 4px;
    }

    .history-register {
      display: grid;
    }

    .history-register span + span {
      padding-left: 0;
      border-left: 0;
    }
  }
</style>
