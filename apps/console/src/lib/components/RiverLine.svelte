<!--
THESIS: The river as a chart — where every hull actually is on the water that
leads to Brickell, and whether the span will let it through.
FORM: A plan chart drawn from the engine's own geometry, Brickell Avenue
Bridge at its centre with range rings around it. Corridor violet is the
tracked water and its traffic; amber marks only a hull that will lift the
span. The manifest docks beside the chart as a ledger column.
-->
<script lang="ts">
  import BasculeMark from './BasculeMark.svelte';
  import SpanPlanMark from './SpanPlanMark.svelte';
  import VesselGlyph from './VesselGlyph.svelte';
  import { riverChart, type ChartVessel } from '$lib/riverchart';
  import type {
    BridgeCrossing,
    BridgeStateInterval,
    RiverCorridor,
    VesselTrack
  } from '$lib/types';

  let {
    corridor,
    vesselTracks = [],
    intervals = [],
    crossings = [],
    localTimeZone
  }: {
    corridor: RiverCorridor;
    vesselTracks?: VesselTrack[];
    intervals?: BridgeStateInterval[];
    crossings?: BridgeCrossing[];
    localTimeZone?: string;
  } = $props();

  /// Live bascule state per FL511 key. A closed-out interval is history, so
  /// only an open-ended one lets us say a span is up right now.
  const bridgeStates = $derived.by(() => {
    const latest = new Map<string, BridgeStateInterval>();
    for (const interval of intervals) {
      const held = latest.get(interval.bridgeKey);
      if (!held) {
        latest.set(interval.bridgeKey, interval);
        continue;
      }
      const heldOpen = !held.endedAt;
      const nextOpen = !interval.endedAt;
      if (nextOpen && !heldOpen) latest.set(interval.bridgeKey, interval);
      else if (nextOpen === heldOpen && interval.startedAt > held.startedAt) {
        latest.set(interval.bridgeKey, interval);
      }
    }
    const states = new Map<string, 'up' | 'down' | 'unknown'>();
    for (const [key, interval] of latest) {
      states.set(key, interval.endedAt ? 'down' : interval.state);
    }
    return states;
  });

  const chart = $derived(riverChart(corridor, vesselTracks, bridgeStates));

  /// The span's own live interval: what is happening at Brickell right now.
  const targetInterval = $derived.by(() => {
    const own = intervals.filter((interval) => interval.relation === 'target');
    const live = own.filter((interval) => !interval.endedAt);
    const pool = live.length ? live : own;
    return pool.reduce<BridgeStateInterval | null>(
      (newest, interval) =>
        !newest || interval.startedAt > newest.startedAt ? interval : newest,
      null
    );
  });

  const spanIsUp = $derived(
    !!targetInterval && targetInterval.state === 'up' && !targetInterval.endedAt
  );

  /// Which hull the current opening is for.
  ///
  /// An opening with nothing attributed to it is the normal case rather than a
  /// fault: the span lifts for whatever is on the water, and only a vessel that
  /// was broadcasting and was seen on both sides of the bridge line leaves a
  /// crossing behind. Saying "no crossing recorded" is the honest reading, and
  /// it is different from claiming nothing went through.
  const openingCrossings = $derived.by(() => {
    if (!spanIsUp || !targetInterval) return [];
    const from = Date.parse(targetInterval.startedAt);
    if (Number.isNaN(from)) return [];
    // A vessel can clear the line shortly before the span is recorded up.
    const window = from - 4 * 60_000;
    return crossings
      .filter((crossing) => {
        const at = Date.parse(crossing.crossedAt);
        return !Number.isNaN(at) && at >= window;
      })
      .sort((left, right) => right.crossedAt.localeCompare(left.crossedAt));
  });

  /// Vessels nearest the span first: that is the order they matter in.
  const manifest = $derived(
    chart.vessels.slice().sort((left, right) => left.distanceMeters - right.distanceMeters)
  );

  const openerCount = $derived(manifest.filter((vessel) => vessel.opener).length);
  const closingCount = $derived(manifest.filter((vessel) => vessel.closing).length);

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

  /// Speed and time to the span, which is what the mark is for.
  function chartReading(vessel: ChartVessel): string {
    const parts = [`${vessel.speedKnots.toFixed(1)} kn`];
    const eta = etaReading(vessel);
    // Not "to Brickell": every reading on this chart is to Brickell, and
    // repeating it on each hull is what pushed the labels into each other.
    if (eta) parts.push(eta);
    return parts.join(' · ');
  }

  /// Everything known about this hull's identity beyond its label, in the
  /// order a reader cares about. Absent fields are absent, not blank slots.
  function identity(vessel: ChartVessel): string[] {
    const parts: string[] = [];
    if (vessel.vesselClass) parts.push(vessel.vesselClass);
    // The call sign only earns space when it is not already the label.
    if (vessel.callSign && vessel.callSign !== vessel.label) parts.push(vessel.callSign);
    if (vessel.lengthMeters) {
      const beam = vessel.beamMeters ? `×${Math.round(vessel.beamMeters)}` : '';
      parts.push(`${Math.round(vessel.lengthMeters)}${beam} m`);
    }
    return parts;
  }

  const STATE_WORD = { up: 'open', down: 'closed', unknown: 'no reading' } as const;

  /// A span reads as its name and what it is doing; a channel mark has no
  /// state to report and says only what it is.
  function stationLabel(station: {
    label: string;
    kind: string;
    state?: 'up' | 'down' | 'unknown';
  }): string {
    if (station.kind !== 'bridge' && station.kind !== 'target') return station.label;
    return `${station.label} — ${STATE_WORD[station.state ?? 'unknown']}`;
  }

  function distanceReading(meters: number): string {
    return meters >= 1_000 ? `${(meters / 1_000).toFixed(1)} km` : `${Math.round(meters / 10) * 10} m`;
  }

  function etaReading(vessel: ChartVessel): string | null {
    if (vessel.etaMinMinutes == null) return null;
    const max = vessel.etaMaxMinutes ?? vessel.etaMinMinutes;
    return max > vessel.etaMinMinutes
      ? `${vessel.etaMinMinutes}–${max} min`
      : `${vessel.etaMinMinutes} min`;
  }

  function openingReading(vessel: ChartVessel): string | null {
    if (!vessel.predictedOpeningAt) return null;
    const parsed = new Date(vessel.predictedOpeningAt);
    return Number.isNaN(parsed.getTime()) ? null : clock.format(parsed);
  }

  const DIRECTION_WORD = {
    upriver: 'Upriver',
    downriver: 'Downriver',
    holding: 'Holding'
  } as const;

  function vesselLabel(vessel: ChartVessel): string {
    const side = vessel.sMeters >= 0 ? 'upriver of' : 'seaward of';
    const kind = vessel.vesselClass ? `${vessel.vesselClass}, ` : '';
    const eta = etaReading(vessel);
    const opening = openingReading(vessel);
    const parts = [
      `${vessel.label}. ${kind}${DIRECTION_WORD[vessel.direction].toLowerCase()} at ${vessel.speedKnots.toFixed(1)} knots, ${distanceReading(vessel.distanceMeters)} ${side} the span.`
    ];
    if (vessel.destination) parts.push(`Bound for ${vessel.destination}.`);
    if (vessel.lengthMeters) {
      const beam = vessel.beamMeters ? ` by ${Math.round(vessel.beamMeters)} metres` : '';
      parts.push(`Hull ${Math.round(vessel.lengthMeters)} metres${beam}.`);
    }
    if (eta) parts.push(`Reaches Brickell in ${eta}.`);
    if (opening) {
      parts.push(
        vessel.waitsForSlot
          ? `Waits for the ${opening} slot.`
          : vessel.scheduleExempt
            ? `Passed on arrival at ${opening}; commercial traffic is not held by the schedule.`
            : `Could open at ${opening}.`
      );
    }
    if (vessel.opener) parts.push('Known to open the span.');
    return parts.join(' ');
  }

  /// On a phone the sheet pans instead of shrinking, and it wakes centred on
  /// the span — the chart's whole subject — rather than on its west edge.
  function centerOnSpan(node: HTMLElement) {
    if (node.scrollWidth > node.clientWidth) {
      node.scrollLeft =
        (chart.bridgeX / chart.width) * node.scrollWidth - node.clientWidth / 2;
    }
  }

  /// A hull seen from above: pointed bow, parallel body, rounded stern.
  function hullPath(length: number, beam: number): string {
    const l = length / 2;
    const b = beam / 2;
    const s = Math.min(2.4, b * 0.6);
    return (
      `M${l.toFixed(1)} 0 ` +
      `Q${(l * 0.42).toFixed(1)} ${(-b).toFixed(1)} ${(-l * 0.28).toFixed(1)} ${(-b).toFixed(1)} ` +
      `L${(-l + s).toFixed(1)} ${(-b).toFixed(1)} Q${(-l).toFixed(1)} ${(-b).toFixed(1)} ${(-l).toFixed(1)} ${(-b + s).toFixed(1)} ` +
      `L${(-l).toFixed(1)} ${(b - s).toFixed(1)} Q${(-l).toFixed(1)} ${b.toFixed(1)} ${(-l + s).toFixed(1)} ${b.toFixed(1)} ` +
      `L${(-l * 0.28).toFixed(1)} ${b.toFixed(1)} Q${(l * 0.42).toFixed(1)} ${b.toFixed(1)} ${l.toFixed(1)} 0 Z`
    );
  }
</script>

<section class="river" aria-labelledby="river-heading">
  <header class="river-head">
    <div>
      <p class="registration-label" id="river-heading">Miami River · plan of the tracked water</p>
      <p class="river-count" aria-live="polite">
        {#if !corridor.aisLive}
          <span class="ais-off">AIS source is off — no vessels are being received</span>
        {:else if manifest.length}
          <strong>{manifest.length}</strong> under way{#if closingCount},
            <strong>{closingCount}</strong> closing on Brickell{/if}{#if openerCount}<span
              class="opener-count"
            >&nbsp;· {openerCount} known opener{openerCount === 1 ? '' : 's'}</span
            >{/if}
        {:else}
          Nothing under way
        {/if}
      </p>
    </div>
    <span class="river-legend" aria-hidden="true">
      <em data-state="up"></em> Open
      <em data-state="down"></em> Closed
      <em data-state="unknown"></em> No reading
    </span>
  </header>

  {#if spanIsUp && targetInterval}
    <p class="opening-attribution" data-known={openingCrossings.length > 0}>
      <BasculeMark state="up" size={34} />
      <strong>Open since {clock.format(new Date(targetInterval.startedAt))}</strong>
      {#if openingCrossings.length}
        ·
        {#each openingCrossings as crossing, index (crossing.mmsi + crossing.crossedAt)}
          {index > 0 ? ', ' : ''}<span class="crossed"
            >{crossing.vesselName ?? `MMSI ${crossing.mmsi}`}</span
          >{#if crossing.vesselClass}&nbsp;({crossing.vesselClass}){/if} crossed
          {crossing.direction} at {clock.format(new Date(crossing.crossedAt))}
        {/each}
      {:else}
        · no AIS crossing recorded for this opening
      {/if}
    </p>
  {/if}

  <div class="river-body">
    <div class="river-scroll" use:centerOnSpan>
      <svg
        class="river-plot"
        viewBox="0 0 {chart.width} {chart.height}"
        role="img"
        aria-label="Plan chart of the Miami River and its bay approaches, Brickell Avenue Bridge at the centre with range rings at {chart.rings
          .map((ring) => ring.kilometers)
          .join(', ')} kilometres, and {manifest.length} vessels under way."
      >
        <!-- Range rings first: they sit beneath the water they measure. -->
        <g class="rings">
          {#each chart.rings as ring (ring.kilometers)}
            <circle cx={chart.bridgeX} cy={chart.bridgeY} r={ring.radius} />
            <text x={ring.labelX} y={ring.labelY}>{ring.kilometers} km</text>
          {/each}
        </g>

        <!-- The tracked water. Bank rules first, then every fill on top, so
             the three channels merge seamlessly where they meet at the mouth
             instead of drawing their edges across one another. -->
        {#each chart.branches as branch (branch.id)}
          <path
            class="bank"
            class:approach={branch.approach}
            d={branch.ribbon}
          />
        {/each}
        {#each chart.branches as branch (branch.id)}
          <g class="water" class:approach={branch.approach}>
            <path class="ribbon" d={branch.ribbon} />
            <path class="channel-rule" fill="none" d={branch.centerline} />
          </g>
        {/each}

        {#if chart.bayLabel}
          <text class="bay-label" x={chart.bayLabel.x} y={chart.bayLabel.y}
            >Biscayne Bay</text
          >
        {/if}

        <!-- Wakes ride the water beneath every mark. -->
        <g class="wakes">
          {#each chart.vessels as vessel (vessel.mmsi)}
            {#each vessel.wake.slice(0, -1) as point, index (index)}
              {@const next = vessel.wake[index + 1]}
              <line
                x1={point.x.toFixed(1)}
                y1={point.y.toFixed(1)}
                x2={next.x.toFixed(1)}
                y2={next.y.toFixed(1)}
                style="opacity: {(0.08 + 0.3 * next.freshness).toFixed(2)}; stroke-width: {(
                  1 +
                  1.8 * next.freshness
                ).toFixed(1)}"
              />
            {/each}
          {/each}
        </g>

        {#each chart.stations as station (station.branchId + station.label)}
          <g
            class="station"
            class:is-target={station.isTarget}
            data-kind={station.kind}
            data-state={station.state ?? 'none'}
          >
            <title>{stationLabel(station)}</title>
            {#if station.kind === 'bridge' || station.isTarget}
              <g transform="translate({station.x} {station.y}) rotate({station.angleDegrees})">
                <SpanPlanMark
                  state={station.state ?? 'unknown'}
                  halfWidth={station.halfWidth}
                  target={station.isTarget}
                />
              </g>
            {:else}
              <circle
                class="channel-mark"
                class:is-mouth={station.kind === 'mouth'}
                cx={station.x}
                cy={station.y}
                r={station.kind === 'mouth' ? 4.4 : 3}
              />
            {/if}
            <text
              class="station-label"
              class:anchor-start={station.labelAnchor === 'start'}
              class:anchor-end={station.labelAnchor === 'end'}
              x={station.labelX}
              y={station.labelY}
            >{station.label}</text>
            {#if station.isTarget}
              <text
                class="target-state"
                x={station.x}
                y={station.y - station.halfWidth - 13}
              >{STATE_WORD[station.state ?? 'unknown']}</text>
            {/if}
          </g>
        {/each}

        {#each chart.vessels as vessel (vessel.mmsi)}
          <g
            class="vessel"
            class:is-opener={vessel.opener}
            transform="translate({vessel.x.toFixed(1)} {vessel.y.toFixed(1)})"
          >
            <title>{vesselLabel(vessel)}</title>
            <g class="hull-group" transform="rotate({vessel.angleDegrees.toFixed(1)})">
              <path class="hull" d={hullPath(vessel.hullLength, vessel.hullBeam)} />
              {#if vessel.hullLength >= 20}
                <rect
                  class="house"
                  x={-vessel.hullLength * 0.32}
                  y={-vessel.hullBeam * 0.22}
                  width={vessel.hullLength * 0.3}
                  height={vessel.hullBeam * 0.44}
                  rx="0.8"
                />
              {/if}
            </g>

            <!-- A presentation attribute loses to the stylesheet, so the
                 stacked anchor is a class rather than text-anchor="start". -->
            <text
              class="vessel-tag"
              class:stacked={vessel.stackedColumn}
              x={vessel.stackedColumn ? 16 : 0}
              y={vessel.labelY}>{vessel.label}</text>
            <text
              class="vessel-read"
              class:stacked={vessel.stackedColumn}
              x={vessel.stackedColumn ? 16 : 0}
              y={vessel.labelY + 11}>{chartReading(vessel)}</text>
            {#if vessel.opener}
              <!-- The tag rides with the hull, not only in the ledger: this is
                   the one mark on the chart that changes a driver's plan. -->
              <g
                class="opener-tag"
                transform="translate({vessel.stackedColumn ? 46 : 0} {vessel.labelY + 22})"
              >
                <rect x="-30" y="-9" width="60" height="13" rx="1" />
                <text y="0.5">Opens span</text>
              </g>
            {/if}
          </g>
        {/each}

        <!-- Chart furniture: north, and what the rings mean. -->
        <g class="compass" transform="translate({chart.width - 34} 34)">
          <line x1="0" y1="12" x2="0" y2="-10" />
          <path d="M0 -14 L4.5 -4 L0 -7 L-4.5 -4 Z" />
          <text y="26">N</text>
        </g>
        <text class="scale-note" x="16" y={chart.height - 14}
          >Rings · distance to the span — water beyond 1 km drawn compressed</text
        >
      </svg>
    </div>

    <aside class="manifest-rail" aria-label="Vessels under way, nearest the span first">
      {#if manifest.length}
        <ul class="manifest">
          {#each manifest as vessel (vessel.mmsi)}
            <li class:is-opener={vessel.opener} title={vesselLabel(vessel)}>
              <svg class="profile" viewBox="-24 -21 48 27" aria-hidden="true">
                <VesselGlyph
                  kind={vessel.vesselClass}
                  length={40}
                  flip={vessel.direction === 'downriver'}
                  opener={vessel.opener}
                />
              </svg>
              <div class="strip">
                <p class="strip-id">
                  <strong>{vessel.label}</strong>
                  {#if vessel.opener}<em class="tag opens">Opens span</em>{/if}
                  {#if vessel.scheduleExempt}<em class="tag exempt">Commercial</em>{/if}
                </p>
                {#if identity(vessel).length}
                  <p class="strip-identity">{identity(vessel).join(' · ')}</p>
                {/if}
                {#if vessel.destination}
                  <!-- The skipper said where they are going. Nothing inferred
                       from course competes with that. -->
                  <p class="strip-identity destination">for {vessel.destination}</p>
                {/if}
                <p class="strip-readings">
                  <span
                    ><strong>{DIRECTION_WORD[vessel.direction]}</strong>
                    <small>{vessel.speedKnots.toFixed(1)} kn</small></span
                  >
                  <span
                    ><strong>{distanceReading(vessel.distanceMeters)}</strong>
                    <small>{vessel.sMeters >= 0 ? 'upriver' : 'seaward'}</small></span
                  >
                  {#if etaReading(vessel)}
                    <span
                      ><strong>{etaReading(vessel)}</strong>
                      <small>to Brickell</small></span
                    >
                  {/if}
                  {#if openingReading(vessel)}
                    <span class="opening" class:waits={vessel.waitsForSlot}
                      ><strong>{openingReading(vessel)}</strong>
                      <small>
                        {#if vessel.waitsForSlot}waits for slot
                        {:else if vessel.scheduleExempt}opens on signal
                        {:else}earliest opening{/if}
                      </small></span
                    >
                  {/if}
                </p>
              </div>
            </li>
          {/each}
        </ul>
      {:else if corridor.aisLive}
        <p class="river-empty">Nothing is moving toward Brickell.</p>
      {:else}
        <p class="river-empty">
          The chart is drawn from the geometry this app tracks, but the AIS
          source is switched off, so no vessel positions are arriving. Enable it
          in Channels to populate the water.
        </p>
      {/if}
    </aside>
  </div>
</section>

<style>
  .river {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: clamp(14px, 2vw, 22px) clamp(20px, 3vw, 42px) clamp(16px, 2.2vw, 24px);
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
  }

  .river-head {
    display: flex;
    flex: none;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 11px;
    border-bottom: 1px solid var(--rule-strong);
  }

  .river-count {
    margin: 5px 0 0;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .river-count strong {
    color: var(--graphite);
    font-size: var(--type-label);
  }

  .opener-count {
    color: var(--amber-ink);
    font-weight: 700;
  }

  .ais-off {
    color: var(--danger);
    font-weight: 700;
  }

  .river-legend {
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

  .river-legend em + em {
    margin-left: 9px;
  }

  .river-legend em {
    width: 10px;
    height: 10px;
    border: 1px solid var(--graphite);
  }

  .river-legend em[data-state='up'] {
    background: var(--danger);
  }

  .river-legend em[data-state='down'] {
    background: var(--success);
  }

  .river-legend em[data-state='unknown'] {
    background: var(--steel);
  }

  /* What the span is up for. Amber, because this is the one line here that
     explains an event the reader is currently looking at. */
  .opening-attribution {
    display: flex;
    flex: none;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 5px;
    margin: 10px 0 0;
    padding: 8px 11px;
    color: var(--graphite);
    background: var(--amber-sheet);
    border-left: 1px solid var(--amber-ink);
    font-size: var(--type-caption);
  }

  .opening-attribution strong {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  /* Nothing attributed is not a warning; it is an ordinary reading. */
  .opening-attribution[data-known='false'] {
    color: var(--muted);
    background: var(--frost);
    border-left-color: var(--rule-strong);
  }

  .crossed {
    font-weight: 700;
  }

  /* The chart is the broad field; the ledger docks beside it and scrolls so
     the water never moves under the reader. */
  .river-body {
    display: grid;
    flex: 1 1 auto;
    grid-template-columns: minmax(0, 1fr) clamp(264px, 24vw, 344px);
    gap: 14px;
    min-height: 0;
    margin-top: 12px;
  }

  .river-scroll {
    position: relative;
    min-height: 0;
    overflow: hidden;
    background: var(--white);
    border: 1px solid var(--rule);
  }

  .river-plot {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
  }

  .rings circle {
    fill: none;
    stroke: var(--steel);
    stroke-dasharray: 1.5 5.5;
    stroke-linecap: round;
    stroke-width: 1.1;
    opacity: 0.65;
  }

  /* Ring readings sit over whatever the ring crosses, so they carry their own
     paper behind the letterforms. */
  .rings text {
    fill: var(--muted);
    stroke: var(--white);
    stroke-width: 3;
    paint-order: stroke;
    font-family: var(--font-instrument);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-anchor: middle;
    text-transform: uppercase;
  }

  /* Bank rules live on their own layer beneath every fill, so the junction at
     the mouth reads as one body of water rather than three outlines. */
  .bank {
    fill: none;
    stroke: var(--corridor-rule);
    stroke-width: 0.9;
    stroke-linejoin: round;
  }

  .bank.approach {
    opacity: 0.7;
  }

  .water .ribbon {
    fill: var(--corridor-wash);
    stroke: none;
  }

  /* The entrance channels are the same water, drawn a shade lighter so the
     river the bridge sits on stays the primary line. */
  .water.approach .ribbon {
    opacity: 0.72;
  }

  /* The bay is context, set like water names on a chart: spaced, quiet. */
  .bay-label {
    fill: var(--steel);
    font-family: var(--font-instrument);
    font-size: 15px;
    font-style: italic;
    font-weight: 600;
    letter-spacing: 0.34em;
    opacity: 0.62;
    text-anchor: middle;
    text-transform: uppercase;
  }

  .water .channel-rule {
    stroke: var(--corridor);
    stroke-dasharray: 5 7;
    stroke-width: 1;
    opacity: 0.35;
  }

  .wakes line {
    stroke: var(--corridor);
    stroke-linecap: round;
  }

  .station-label {
    fill: var(--muted);
    stroke: var(--white);
    stroke-width: 3;
    paint-order: stroke;
    font-family: var(--font-instrument);
    font-size: 12.5px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-anchor: middle;
    text-transform: uppercase;
  }

  .station-label.anchor-start {
    text-anchor: start;
  }

  .station-label.anchor-end {
    text-anchor: end;
  }

  .station.is-target .station-label {
    fill: var(--marine);
    font-size: 19px;
    font-weight: 700;
    letter-spacing: 0.02em;
  }

  /* The state word rides with the target's name: the one span whose reading
     must survive without the legend, in words as well as colour. */
  .target-state {
    fill: var(--muted);
    stroke: var(--white);
    stroke-width: 3;
    paint-order: stroke;
    font-family: var(--font-instrument);
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.09em;
    text-anchor: middle;
    text-transform: uppercase;
  }

  .station.is-target[data-state='up'] .target-state {
    fill: var(--danger);
  }

  .channel-mark {
    fill: var(--white);
    stroke: var(--corridor);
    stroke-width: 1.3;
    opacity: 0.8;
  }

  .channel-mark.is-mouth {
    stroke-width: 1.8;
  }

  /* The opener tag rides on the chart beside its hull. */
  .opener-tag rect {
    fill: var(--amber-sheet);
    stroke: var(--amber-ink);
    stroke-width: 1;
  }

  .opener-tag text {
    fill: var(--amber-ink);
    font-family: var(--font-instrument);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-anchor: middle;
    text-transform: uppercase;
  }

  /* Ten seconds between snapshots, so a vessel that jumps reads as a glitch
     rather than as movement. Easing the transform lets it run the distance it
     actually ran. */
  .vessel {
    transition: transform 1100ms cubic-bezier(0.22, 0.61, 0.36, 1);
  }

  .hull-group {
    transition: transform 1100ms cubic-bezier(0.22, 0.61, 0.36, 1);
  }

  .hull {
    fill: var(--corridor);
    stroke: var(--white);
    stroke-width: 1.1;
    stroke-linejoin: round;
  }

  .house {
    fill: none;
    stroke: var(--white);
    stroke-width: 0.9;
    opacity: 0.85;
  }

  .vessel.is-opener .hull {
    fill: var(--amber-ink);
  }

  .vessel-tag {
    fill: var(--graphite);
    stroke: var(--white);
    stroke-width: 3;
    paint-order: stroke;
    font-family: var(--font-instrument);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-anchor: middle;
    text-transform: uppercase;
  }

  .vessel-tag.stacked {
    text-anchor: start;
  }

  /* The two numbers a driver is actually after, under the name. */
  .vessel-read {
    fill: var(--muted);
    stroke: var(--white);
    stroke-width: 3;
    paint-order: stroke;
    font-family: var(--font-body);
    font-size: 10px;
    font-weight: 500;
    text-anchor: middle;
  }

  .vessel-read.stacked {
    text-anchor: start;
  }

  .compass line {
    stroke: var(--steel);
    stroke-width: 1.2;
  }

  .compass path {
    fill: var(--steel);
  }

  .compass text {
    fill: var(--muted);
    font-family: var(--font-instrument);
    font-size: 11px;
    font-weight: 700;
    text-anchor: middle;
  }

  .scale-note {
    fill: var(--muted);
    font-family: var(--font-instrument);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  /* The ledger: every hull as a clipped evidence strip, nearest first. */
  .manifest-rail {
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    border-left: 1px solid var(--rule);
    padding-left: 14px;
  }

  .manifest {
    display: grid;
    gap: 0;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .manifest li {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr);
    gap: 10px;
    align-items: start;
    padding: 10px 0;
    border-bottom: 1px solid var(--rule);
  }

  .manifest li:last-child {
    border-bottom: 0;
  }

  .profile {
    width: 44px;
    height: 25px;
    margin-top: 2px;
  }

  .strip {
    display: grid;
    gap: 3px;
    min-width: 0;
  }

  .strip-id {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: baseline;
    margin: 0;
  }

  .strip-id strong {
    overflow: hidden;
    color: var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 700;
    letter-spacing: 0.03em;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .strip-identity {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-micro);
  }

  .destination {
    color: var(--corridor);
    font-weight: 600;
  }

  .strip-readings {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    margin: 2px 0 0;
  }

  .strip-readings span {
    display: grid;
    gap: 0;
  }

  .strip-readings strong {
    color: var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }

  .strip-readings small {
    color: var(--muted);
    font-size: var(--type-micro);
  }

  .strip-readings .opening.waits strong {
    color: var(--amber-ink);
  }

  .tag {
    display: inline-block;
    padding: 1px 4px;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-style: normal;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .tag.opens {
    color: var(--amber-ink);
    background: var(--amber-sheet);
  }

  .tag.exempt {
    color: var(--corridor);
    background: var(--corridor-sheet);
  }

  .river-empty {
    margin: 8px 0 2px;
    max-width: 34ch;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.55;
  }

  @media (max-width: 900px) {
    .river-body {
      grid-template-columns: minmax(0, 1fr);
    }

    /* A phone pans the sheet rather than shrinking its lettering away; the
       action above wakes the scroll centred on the Brickell span. */
    .river-scroll {
      overflow-x: auto;
      overflow-y: hidden;
      overscroll-behavior-x: contain;
    }

    .river-plot {
      position: static;
      width: 760px;
      height: auto;
    }

    .manifest-rail {
      border-left: 0;
      padding-left: 0;
      overflow-y: visible;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .vessel,
    .hull-group {
      transition: none;
    }
  }
</style>
