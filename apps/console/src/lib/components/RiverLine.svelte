<!--
THESIS: The river as a transit line — how many vessels, which way, how soon,
and whether the span will actually let them through.
FORM: Corridor violet is the line and its traffic; the target keeps marine ink;
amber marks only a hull that will lift the span.
-->
<script lang="ts">
  import BasculeMark from './BasculeMark.svelte';
  import ChannelMark from './ChannelMark.svelte';
  import VesselGlyph from './VesselGlyph.svelte';
  import { riverDiagram, type DiagramVessel } from '$lib/riverline';
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

  const diagram = $derived(riverDiagram(corridor, vesselTracks, bridgeStates));

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
    diagram.vessels.slice().sort((left, right) => left.distanceMeters - right.distanceMeters)
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

  /// Speed and time to the span, which is what the mark is for. Size joins it
  /// once a static report has actually reported one.
  function diagramReading(vessel: DiagramVessel): string {
    const parts = [`${vessel.speedKnots.toFixed(1)} kn`];
    const eta = etaReading(vessel);
    // Not "to Brickell": every reading on this drawing is to Brickell, and
    // repeating it on each hull is what pushed the labels into each other.
    if (eta) parts.push(eta);
    if (vessel.lengthMeters) parts.push(`${Math.round(vessel.lengthMeters)} m`);
    return parts.join(' · ');
  }

  /// Everything known about this hull's identity beyond its label, in the
  /// order a reader cares about. Absent fields are absent, not blank slots.
  function identity(vessel: DiagramVessel): string[] {
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
  function stationLabel(station: { label: string; kind: string; state?: 'up' | 'down' | 'unknown' }): string {
    if (station.kind !== 'bridge' && station.kind !== 'target') return station.label;
    return `${station.label} — ${STATE_WORD[station.state ?? 'unknown']}`;
  }

  function distanceReading(meters: number): string {
    return meters >= 1_000 ? `${(meters / 1_000).toFixed(1)} km` : `${Math.round(meters / 10) * 10} m`;
  }

  function etaReading(vessel: DiagramVessel): string | null {
    if (vessel.etaMinMinutes == null) return null;
    const max = vessel.etaMaxMinutes ?? vessel.etaMinMinutes;
    return max > vessel.etaMinMinutes
      ? `${vessel.etaMinMinutes}–${max} min`
      : `${vessel.etaMinMinutes} min`;
  }

  function openingReading(vessel: DiagramVessel): string | null {
    if (!vessel.predictedOpeningAt) return null;
    const parsed = new Date(vessel.predictedOpeningAt);
    return Number.isNaN(parsed.getTime()) ? null : clock.format(parsed);
  }

  const DIRECTION_WORD = {
    upriver: 'Upriver',
    downriver: 'Downriver',
    holding: 'Holding'
  } as const;

  function vesselLabel(vessel: DiagramVessel): string {
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

</script>

<section class="river" aria-labelledby="river-heading">
  <header class="river-head">
    <div>
      <p class="registration-label" id="river-heading">Miami River · tracked channel</p>
      <p class="river-count" aria-live="polite">
        {#if !corridor.aisLive}
          <span class="ais-off">AIS source is off — no vessels are being received</span>
        {:else if manifest.length}
          <strong>{manifest.length}</strong> under way{#if closingCount},
            <strong>{closingCount}</strong> closing on Brickell{/if}{#if openerCount}<span
              class="opener-count"
            >
              · {openerCount} known opener{openerCount === 1 ? '' : 's'}</span
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

  <div class="river-scroll">
    <svg
      class="river-plot"
      viewBox="0 0 {diagram.width} {diagram.height}"
      role="img"
      aria-label="Transit diagram of the Miami River channel: bascule bridges upriver of Brickell, marked entrance channels seaward, and {manifest.length} vessels under way."
    >
      {#each diagram.lines as line (line.id)}
        <path class="line" class:approach={line.approach} fill="none" d={line.d} />
      {/each}

      {#each diagram.stations as station (station.branchId + station.label)}
        <g
          class="station"
          class:is-target={station.isTarget}
          data-kind={station.kind}
          data-state={station.state ?? 'none'}
        >
          <title>{stationLabel(station)}</title>
          {#if station.kind === 'bridge' || station.isTarget}
            <!-- A bascule drawn as a bascule: leaves down when the road is
                 open, lifted when the span is. -->
            <g
              transform="translate({station.x} {station.y}) scale({station.isTarget ? 1.9 : 1.2})"
            >
              <BasculeMark state={station.state ?? 'unknown'} inline />
            </g>
          {:else}
            <g transform="translate({station.x} {station.y})">
              <ChannelMark junction={station.kind === 'mouth'} />
            </g>
          {/if}
          <text
            x={station.x}
            y={station.y - (station.isTarget ? 34 : 24)}
            class="station-label"
          >{station.label}</text>
        </g>
      {/each}

      {#each diagram.vessels as vessel (vessel.mmsi)}
        <g
          class="vessel"
          class:is-opener={vessel.opener}
          transform="translate({vessel.x.toFixed(1)} {vessel.y.toFixed(1)})"
        >
          <title>{vesselLabel(vessel)}</title>
          <VesselGlyph
            kind={vessel.vesselClass}
            length={vessel.hullLength * 2.1}
            flip={Math.abs(vessel.headingDegrees) > 90}
            opener={vessel.opener}
          />

          <!-- A presentation attribute loses to the stylesheet, so the stacked
               anchor is a class rather than text-anchor="start". -->
          <text
            class="vessel-tag"
            class:stacked={vessel.stackedColumn}
            x={vessel.stackedColumn ? 20 : 0}
            y={vessel.labelY}>{vessel.label}</text>
          <text
            class="vessel-read"
            class:stacked={vessel.stackedColumn}
            x={vessel.stackedColumn ? 20 : 0}
            y={vessel.labelY + 12}>{diagramReading(vessel)}</text>
          {#if vessel.opener}
            <!-- The tag rides with the hull, not only in the table: this is the
                 one mark on the drawing that changes a driver's plan. -->
            <g
              class="opener-tag"
              transform="translate({vessel.stackedColumn ? 50 : 0} {vessel.labelY + 24})"
            >
              <rect x="-30" y="-9" width="60" height="13" rx="1" />
              <text y="0.5">Opens span</text>
            </g>
          {/if}
        </g>
      {/each}
    </svg>
  </div>

  {#if manifest.length}
    <ul class="manifest">
      {#each manifest as vessel (vessel.mmsi)}
        <li class:is-opener={vessel.opener} title={vesselLabel(vessel)}>
          <span class="cell id">
            <strong>{vessel.label}</strong>
            <small>
              {#if vessel.opener}<em class="tag opens">Opens span</em>{/if}
              {#if vessel.scheduleExempt}<em class="tag exempt">Commercial</em>{/if}
              {identity(vessel).join(' · ')}
            </small>
            {#if vessel.destination}
              <!-- The skipper said where they are going. Nothing inferred from
                   course competes with that. -->
              <small class="destination">for {vessel.destination}</small>
            {/if}
          </span>
          <span class="cell">
            <strong>{DIRECTION_WORD[vessel.direction]}</strong>
            <small>{vessel.speedKnots.toFixed(1)} kn</small>
          </span>
          <span class="cell">
            <strong>{distanceReading(vessel.distanceMeters)}</strong>
            <small>{vessel.sMeters >= 0 ? 'upriver' : 'seaward'}</small>
          </span>
          <span class="cell">
            {#if etaReading(vessel)}
              <strong>{etaReading(vessel)}</strong>
              <small>to Brickell</small>
            {/if}
          </span>
          <span class="cell opening" class:waits={vessel.waitsForSlot}>
            {#if openingReading(vessel)}
              <strong>{openingReading(vessel)}</strong>
              <small>
                {#if vessel.waitsForSlot}waits for slot
                {:else if vessel.scheduleExempt}opens on signal
                {:else}earliest opening{/if}
              </small>
            {/if}
          </span>
        </li>
      {/each}
    </ul>
  {:else if corridor.aisLive}
    <p class="river-empty">Nothing is moving toward Brickell.</p>
  {:else}
    <p class="river-empty">
      The channel is drawn from the geometry this app tracks, but the AIS source
      is switched off, so no vessel positions are arriving. Enable it in
      Channels to populate the line.
    </p>
  {/if}
</section>

<style>
  .river {
    padding: clamp(16px, 2.2vw, 26px) clamp(20px, 3vw, 42px);
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
  }

  .river-head {
    display: flex;
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

  /* Wide diagrams scroll inside their own box rather than pushing the page. */
  .river-scroll {
    overflow-x: auto;
    margin-top: 12px;
  }

  .river-plot {
    display: block;
    width: 100%;
    min-width: 760px;
    height: auto;
    background: var(--white);
    border: 1px solid var(--rule);
  }

  .line {
    fill: none;
    stroke: var(--corridor);
    stroke-width: 7;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.32;
  }

  /* The entrance channels are the same water, drawn lighter so the trunk the
     bridge sits on stays the primary line. */
  .line.approach {
    stroke-width: 5.5;
    opacity: 0.2;
    stroke-dasharray: 13 7;
  }







  .station {
    transition: opacity 400ms ease-out;
  }

  .station-label {
    fill: var(--muted);
    font-family: var(--font-instrument);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-anchor: middle;
    text-transform: uppercase;
  }

  .station.is-target .station-label {
    fill: var(--marine);
    font-size: 17px;
    font-weight: 700;
  }



  /* The opener tag rides on the drawing beside its hull. */
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
     actually ran. Everything else that changes fades rather than swaps. */
  .vessel {
    transition: transform 1100ms cubic-bezier(0.22, 0.61, 0.36, 1);
  }

  .vessel-tag {
    fill: var(--graphite);
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
    font-family: var(--font-body);
    font-size: 10.5px;
    font-weight: 500;
    text-anchor: middle;
  }

  .vessel-read.stacked {
    text-anchor: start;
  }

  .destination {
    color: var(--corridor);
    font-weight: 600;
  }

  .manifest {
    display: grid;
    gap: 0;
    margin: 14px 0 0;
    padding: 0;
    list-style: none;
  }

  .manifest li {
    display: grid;
    grid-template-columns: minmax(170px, 2.4fr) repeat(4, minmax(74px, 1fr));
    gap: clamp(8px, 1.4vw, 18px);
    align-items: center;
    padding: 8px 0;
    border-bottom: 1px solid var(--rule);
  }

  .manifest li:last-child {
    border-bottom: 0;
  }

  .cell {
    display: grid;
    min-width: 0;
    gap: 1px;
  }

  .cell strong {
    overflow: hidden;
    color: var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.03em;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .cell small {
    color: var(--muted);
    font-size: var(--type-micro);
  }

  .cell.opening.waits strong {
    color: var(--amber-ink);
  }

  .tag {
    display: inline-block;
    margin-right: 4px;
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
    margin: 14px 0 2px;
    max-width: 66ch;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.55;
  }

  @media (max-width: 720px) {
    .manifest li {
      grid-template-columns: minmax(0, 1.6fr) minmax(0, 1fr) minmax(0, 1fr);
      row-gap: 4px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .vessel,
    .station {
      transition: none;
    }
  }
</style>