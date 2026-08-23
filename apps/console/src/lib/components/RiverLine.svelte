<!--
THESIS: The river is a live transit network whose interchange is Brickell, not
a geographic chart that asks a driver to decode bearings and range rings.
OWN-WORLD: One muscular violet route, widely spaced bridge stations, confident
boat silhouettes, clipped ETA plates, and a monumental mechanical Brickell.
STORY: See Brickell's road state first, then follow any hull's direction,
speed, and arrival time along the line that will bring it to the bridge.
FIRST VIEWPORT: The schematic fills the surface. Brickell owns the middle of
the river; a vessel rail appears only when there are actually boats to show.
FORM: Brickell Interchange using visible-transit-network staging; seed 55e9df82.
-->
<script lang="ts">
  import TransitBascule from './TransitBascule.svelte';
  import VesselGlyph from './VesselGlyph.svelte';
  import {
    riverSchematic,
    type SchematicRoute,
    type SchematicStation,
    type SchematicVessel
  } from '$lib/riverSchematic';
  import { currentVesselTracks } from '$lib/river';
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
    generatedAt,
    localTimeZone
  }: {
    corridor: RiverCorridor;
    vesselTracks?: VesselTrack[];
    intervals?: BridgeStateInterval[];
    crossings?: BridgeCrossing[];
    generatedAt?: string;
    localTimeZone?: string;
  } = $props();

  const bridgeStates = $derived.by(() => {
    const latest = new Map<string, BridgeStateInterval>();
    for (const interval of intervals) {
      const held = latest.get(interval.bridgeKey);
      if (
        !held ||
        (!interval.endedAt && !!held.endedAt) ||
        (!!interval.endedAt === !!held.endedAt && interval.startedAt > held.startedAt)
      ) {
        latest.set(interval.bridgeKey, interval);
      }
    }

    const states = new Map<string, 'up' | 'down' | 'unknown'>();
    for (const [key, interval] of latest) {
      states.set(key, interval.endedAt ? 'down' : interval.state);
    }
    return states;
  });

  const liveVesselTracks = $derived(currentVesselTracks(vesselTracks, generatedAt));
  const schematic = $derived(riverSchematic(corridor, liveVesselTracks, bridgeStates));
  const targetState = $derived(schematic.target?.state ?? 'unknown');
  const visibleStations = $derived(
    schematic.stations.filter(
      (station) => !station.isTarget && (station.kind === 'bridge' || station.kind === 'mouth')
    )
  );

  const manifest = $derived.by(() =>
    schematic.vessels.slice().sort((left, right) => {
      const impact = (vessel: SchematicVessel) =>
        Number(vessel.opener) * 4 + Number(vessel.closing) * 2;
      return impact(right) - impact(left) || left.distanceMeters - right.distanceMeters;
    })
  );

  const calloutMmsis = $derived(
    new Set(manifest.slice(0, Math.min(3, manifest.length)).map((vessel) => vessel.mmsi))
  );

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

  let schematicScroll: HTMLElement | null = $state(null);
  let manifestScroll: HTMLElement | null = $state(null);

  const STATE_WORD = { up: 'up', down: 'down', unknown: 'no reading' } as const;
  const ROAD_WORD = {
    up: 'road stopped',
    down: 'road moving',
    unknown: 'road state unknown'
  } as const;
  const DIRECTION_WORD = {
    upriver: 'Upriver',
    downriver: 'Downriver',
    holding: 'Holding'
  } as const;

  function titleCase(value: string): string {
    return value.replace(/\b\w/g, (letter) => letter.toUpperCase());
  }

  function typeName(vessel: SchematicVessel): string {
    return vessel.vesselClass && vessel.vesselClass !== 'vessel'
      ? titleCase(vessel.vesselClass)
      : 'Vessel';
  }

  function etaReading(vessel: SchematicVessel): string | null {
    if (vessel.etaMinMinutes == null) return null;
    const max = vessel.etaMaxMinutes ?? vessel.etaMinMinutes;
    return max > vessel.etaMinMinutes
      ? `${vessel.etaMinMinutes}–${max} min`
      : `${vessel.etaMinMinutes} min`;
  }

  function openingReading(vessel: SchematicVessel): string | null {
    if (!vessel.predictedOpeningAt) return null;
    const parsed = new Date(vessel.predictedOpeningAt);
    return Number.isNaN(parsed.getTime()) ? null : clock.format(parsed);
  }

  function stationLabel(station: SchematicStation): string {
    return station.kind === 'bridge' || station.isTarget
      ? `${station.label}. Bridge ${STATE_WORD[station.state ?? 'unknown']}.`
      : station.label;
  }

  function vesselLabel(vessel: SchematicVessel): string {
    const eta = etaReading(vessel);
    const parts = [
      `${vessel.label}. ${typeName(vessel)}. ${DIRECTION_WORD[vessel.direction]} at ${vessel.speedKnots.toFixed(1)} knots.`
    ];
    if (eta) parts.push(`${eta} to Brickell Avenue Bridge.`);
    if (vessel.opener) parts.push('Expected to open the bridge.');
    return parts.join(' ');
  }

  function routeTerminal(route: SchematicRoute) {
    return route.points.at(-1) ?? { x: 0, y: 0 };
  }

  function routeTitle(route: SchematicRoute): string {
    // "MIAMI RIVER" is already registered in the lower-left map title. Keep
    // the tight upstream terminus to one directional word so it cannot run
    // into the final two bridge labels.
    return route.role === 'river' ? 'UPRIVER' : route.label.toUpperCase();
  }

  function stationNameY(station: SchematicStation): number {
    return station.y + station.labelSide * 58;
  }

  function stationStateY(station: SchematicStation): number {
    return stationNameY(station) + station.labelSide * 18;
  }

  function bridgeOrientation(station: SchematicStation): number {
    // A crossing has the same axis after a half-turn. Keep that perpendicular
    // axis in the readable half of the circle so an upriver tangent (usually
    // 180–225deg) cannot turn the asymmetric bridge artwork upside down.
    const perpendicular = (((station.angleDegrees + 90) % 180) + 180) % 180;
    return perpendicular > 90 ? perpendicular - 180 : perpendicular;
  }

  function vesselProfileFacesLeft(vessel: SchematicVessel): boolean {
    // VesselGlyph is an upright side profile, not a plan-view hull. The
    // runway carries the exact route angle; the silhouette only mirrors when
    // that heading has a leftward screen component.
    const heading = ((vessel.angleDegrees % 360) + 360) % 360;
    return heading > 90 && heading < 270;
  }

  function calloutX(vessel: SchematicVessel): number {
    if (vessel.x > schematic.width - 280) return -250;
    if (schematic.target && vessel.x < schematic.target.x && vessel.x > schematic.target.x - 300) {
      return -260;
    }
    return 34;
  }

  function calloutY(vessel: SchematicVessel, index: number): number {
    const slot = index % 3;
    if (vessel.y < 160) return [32, 128, 224][slot];
    if (vessel.y > schematic.height - 150) return [-108, -204, -300][slot];
    return [-110, 34, 130][slot];
  }

  function miniCalloutX(vessel: SchematicVessel, index: number): number {
    if (vessel.x > schematic.width - 240) return -216;
    return index % 2 === 0 ? 30 : -216;
  }

  function miniCalloutY(vessel: SchematicVessel, index: number): number {
    const slot = (index - 3) % 4;
    if (vessel.y < 130) return [26, 76, 126, 176][slot];
    if (vessel.y > schematic.height - 110) return [-64, -114, -164, -214][slot];
    return [28, -64, 80, -116][slot];
  }

  function centerOnTarget(node: HTMLElement) {
    const center = () => {
      if (node.scrollWidth <= node.clientWidth || !schematic.target) return;
      node.scrollLeft =
        (schematic.target.x / schematic.width) * node.scrollWidth - node.clientWidth / 2;
    };

    center();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(center);
    observer.observe(node);
    return {
      destroy() {
        observer.disconnect();
      }
    };
  }

  function panMap(direction: -1 | 1) {
    if (!schematicScroll) return;
    schematicScroll.scrollLeft += direction * Math.max(180, schematicScroll.clientWidth * 0.7);
  }

  function panManifest(direction: -1 | 1) {
    if (!manifestScroll) return;
    manifestScroll.scrollTop += direction * Math.max(140, manifestScroll.clientHeight * 0.7);
  }

  // Kept at the render boundary for future passage attribution; it never
  // changes or relabels the engine's Brickell-only ETA.
  const recordedPassages = $derived(crossings.length);
</script>

<section class="river" aria-labelledby="river-heading" data-passages={recordedPassages}>
  <h2 id="river-heading" class="visually-hidden">
    {corridor.aisLive ? 'Miami River live vessel traffic' : 'Miami River vessel traffic unavailable'}
  </h2>

  <div class:has-manifest={manifest.length > 0} class="river-body">
    <div
      class="schematic-scroll"
      bind:this={schematicScroll}
      use:centerOnTarget
      role="region"
      aria-label="Scrollable Miami River transit schematic centered on Brickell Avenue Bridge"
    >
      {#if schematic.routes.length}
        <svg
          class="schematic-plot"
          viewBox="0 0 {schematic.width} {schematic.height}"
          role="img"
          aria-label="{corridor.aisLive ? 'Live Miami River vessel network' : 'Miami River vessel traffic unavailable'} with Brickell Avenue Bridge as the focal target, {visibleStations.length} other bridge or junction stations, and {manifest.length} vessels under way."
        >
          <defs>
            <pattern id="transit-grid" width="72" height="72" patternUnits="userSpaceOnUse">
              <path d="M72 0H0V72" fill="none" />
              <circle cx="0" cy="0" r="1.4" />
            </pattern>
          </defs>

          <rect class="map-paper" width={schematic.width} height={schematic.height} />
          <rect class="map-grid" width={schematic.width} height={schematic.height} />

          <g class="map-title" aria-hidden="true">
            <text class="map-kicker" x="42" y={schematic.height - 82}>
              {corridor.aisLive ? 'LIVE VESSEL TRAFFIC' : 'VESSEL TRAFFIC UNAVAILABLE'}
            </text>
            <text class="map-name" x="42" y={schematic.height - 38}>MIAMI RIVER</text>
            <path d="M42 {schematic.height - 22}H270" />
          </g>

          <g class="bay-title" aria-hidden="true">
            <text x={schematic.width - 42} y="50" text-anchor="end">BISCAYNE BAY</text>
            <path d="M{schematic.width - 260} 66H{schematic.width - 42}" />
          </g>

          <g class="routes" aria-hidden="true">
            {#each schematic.routes as route (route.id)}
              <g class="route" data-role={route.role}>
                <path class="route-casing" fill="none" d={route.d} />
                <path class="route-color" fill="none" d={route.d} />
                <path class="route-register" fill="none" d={route.d} />
              </g>
            {/each}
          </g>

          <g class="route-terminals" aria-hidden="true">
            {#each schematic.routes as route (route.id)}
              {@const terminal = routeTerminal(route)}
              <circle cx={terminal.x} cy={terminal.y} r="10" />
              <text
                x={terminal.x}
                y={terminal.y - 27}
                text-anchor={terminal.x > schematic.width / 2 ? 'end' : 'start'}
              >{routeTitle(route)}</text>
            {/each}
          </g>

          <g class="wakes" aria-hidden="true">
            {#each schematic.vessels as vessel (vessel.mmsi)}
              {#each vessel.wake.slice(0, -1) as point, index (point.observedAt + index)}
                {@const next = vessel.wake[index + 1]}
                <line
                  x1={point.x}
                  y1={point.y}
                  x2={next.x}
                  y2={next.y}
                  style="opacity: {(0.08 + next.freshness * 0.42).toFixed(2)}; stroke-width: {(
                    2 +
                    next.freshness * 3
                  ).toFixed(1)}"
                />
              {/each}
            {/each}
          </g>

          <g class="stations">
            {#each visibleStations as station (station.routeId + station.label)}
              <g
                class="station"
                data-kind={station.kind}
                data-state={station.state ?? 'none'}
              >
                <title>{stationLabel(station)}</title>
                {#if station.kind === 'bridge'}
                  <g
                    class="mini-bascule"
                    data-route-angle={station.angleDegrees.toFixed(1)}
                    data-bridge-angle={bridgeOrientation(station).toFixed(1)}
                    transform="translate({station.x} {station.y}) rotate({bridgeOrientation(
                      station
                    ).toFixed(1)}) scale(0.84)"
                  >
                    <TransitBascule
                      state={station.state ?? 'unknown'}
                      title={stationLabel(station)}
                    />
                  </g>
                  <text class="station-label" x={station.x} y={stationNameY(station)}>
                    {station.label}
                  </text>
                  <text class="station-state" x={station.x} y={stationStateY(station)}>
                    {STATE_WORD[station.state ?? 'unknown']}
                  </text>
                {:else}
                  <circle class="junction-ring" cx={station.x} cy={station.y} r="20" />
                  <circle class="junction-core" cx={station.x} cy={station.y} r="8" />
                  <text class="junction-label" x={station.x} y={station.y - 38}>
                    {station.label}
                  </text>
                {/if}
              </g>
            {/each}
          </g>

          {#if schematic.target}
            <g
              class="target-station"
              data-state={targetState}
              transform="translate({schematic.target.x} {schematic.target.y})"
            >
              <title>Brickell Avenue Bridge. Bridge {STATE_WORD[targetState]}. {ROAD_WORD[targetState]}.</title>
              <circle class="target-disc-shadow" r="198" />
              <circle class="target-disc" r="188" />
              <path class="target-ticks" d="M-214 0H-182 M214 0H182 M0 -214V-182 M0 214V182" />

              <text class="target-kicker" y="-252">ETA TARGET</text>
              <text class="target-name" y="-175">BRICKELL</text>
              <text class="target-subname" y="-143">AVENUE BRIDGE</text>

              <g class="hero-bascule" transform="translate(0 -48) scale(3.2)">
                <TransitBascule
                  state={targetState}
                  hero
                  title={`Brickell Avenue Bridge ${STATE_WORD[targetState]}; ${ROAD_WORD[targetState]}`}
                />
              </g>

              <g class="target-state-board" transform="translate(0 132)">
                <rect x="-136" y="-29" width="272" height="58" />
                <text class="target-road-word" y="-4">{ROAD_WORD[targetState]}</text>
                <text class="target-state-word" y="17">BRIDGE {STATE_WORD[targetState]}</text>
              </g>
            </g>
          {/if}

          <g class="vessels">
            {#each schematic.vessels as vessel (vessel.mmsi)}
              {@const calloutIndex = manifest.findIndex((item) => item.mmsi === vessel.mmsi)}
              {@const expanded = calloutMmsis.has(vessel.mmsi)}
              <g
                class="vessel"
                class:is-opener={vessel.opener}
                class:is-closing={vessel.closing}
                data-mmsi={vessel.mmsi}
                transform="translate({vessel.x.toFixed(1)} {vessel.y.toFixed(1)})"
              >
                <title>{vesselLabel(vessel)}</title>
                {#if vessel.opener}<circle class="opener-pulse" r="34" />{/if}
                <g
                  class="heading-runway"
                  transform="rotate({vessel.angleDegrees.toFixed(1)})"
                  aria-hidden="true"
                >
                  <line x1="-48" y1="0" x2="53" y2="0" />
                  <path d="M53 0L42 -7V7Z" />
                </g>
                <g class="vessel-ship">
                  <VesselGlyph
                    kind={vessel.vesselClass}
                    length={Math.max(48, Math.min(68, vessel.hullLength * 2.5))}
                    flip={vesselProfileFacesLeft(vessel)}
                    opener={vessel.opener}
                  />
                </g>

                {#if expanded}
                  <line
                    class="callout-leader"
                    x1="0"
                    y1="0"
                    x2={calloutX(vessel)}
                    y2={calloutY(vessel, calloutIndex) + 43}
                  />
                  <g
                    class="vessel-callout"
                    data-opener={vessel.opener}
                    transform="translate({calloutX(vessel)} {calloutY(vessel, calloutIndex)})"
                  >
                    <path class="callout-plate" d="M0 0H238V86H10L0 76Z" />
                    <text class="vessel-tag" x="13" y="23">{vessel.label}</text>
                    <text class="vessel-type" x="13" y="43">{typeName(vessel)}</text>
                    <text class="vessel-direction" x="13" y="68">
                      {DIRECTION_WORD[vessel.direction]} · {vessel.speedKnots.toFixed(1)} kn
                    </text>
                    <line class="callout-rule" x1="146" y1="11" x2="146" y2="75" />
                    {#if etaReading(vessel)}
                      <text class="vessel-eta" x="226" y="43">{etaReading(vessel)}</text>
                      <text class="vessel-eta-label" x="226" y="65">TO BRICKELL</text>
                    {:else}
                      <text class="vessel-eta no-eta" x="226" y="42">—</text>
                      <text class="vessel-eta-label" x="226" y="64">NO ETA</text>
                    {/if}
                    {#if vessel.opener}
                      <g class="callout-opener" transform="translate(10 84)">
                        <rect width="102" height="19" />
                        <text x="51" y="13">EXPECTED OPENER</text>
                      </g>
                    {/if}
                  </g>
                {:else}
                  <line
                    class="callout-leader compact-leader"
                    x1="0"
                    y1="0"
                    x2={miniCalloutX(vessel, calloutIndex)}
                    y2={miniCalloutY(vessel, calloutIndex) + 22}
                  />
                  <g
                    class="vessel-mini-readout"
                    transform="translate({miniCalloutX(vessel, calloutIndex)} {miniCalloutY(vessel, calloutIndex)})"
                  >
                    <rect width="210" height="44" />
                    <text class="mini-name" x="9" y="15">{vessel.label}</text>
                    <text class="mini-type" x="9" y="32">{typeName(vessel)}</text>
                    <text class="mini-movement" x="201" y="15">
                      {DIRECTION_WORD[vessel.direction]} · {vessel.speedKnots.toFixed(1)} kn
                    </text>
                    <text class="mini-eta" x="201" y="33">
                      {etaReading(vessel) ?? 'NO ETA'} · BRICKELL
                    </text>
                  </g>
                {/if}
              </g>
            {/each}
          </g>

        </svg>
      {:else}
        <p class="map-unavailable">River corridor unavailable.</p>
      {/if}

      <div class="map-pan-controls" role="group" aria-label="Pan the river schematic">
        <button type="button" onclick={() => panMap(-1)} aria-label="Pan schematic upriver">←</button>
        <button type="button" onclick={() => panMap(1)} aria-label="Pan schematic toward the bay">→</button>
      </div>
    </div>

    {#if manifest.length}
      <aside class="manifest-rail" aria-labelledby="manifest-heading">
        <header class="manifest-head">
          <div>
            <p>VESSELS TO</p>
            <h3 id="manifest-heading">BRICKELL</h3>
          </div>
          <div class="manifest-actions">
            <span class="target-chip" data-state={targetState}>{STATE_WORD[targetState]}</span>
            <span class="manifest-scroll-controls" role="group" aria-label="Scroll the vessel list">
              <button type="button" onclick={() => panManifest(-1)} aria-label="Earlier vessels">↑</button>
              <button type="button" onclick={() => panManifest(1)} aria-label="Later vessels">↓</button>
            </span>
          </div>
        </header>

        <div
          class="manifest-scroll"
          bind:this={manifestScroll}
          role="region"
          aria-label="Vessels approaching Brickell Avenue Bridge"
        >
          <ol class="manifest">
          {#each manifest as vessel (vessel.mmsi)}
            <li
              class:is-opener={vessel.opener}
              class:is-closing={vessel.closing}
              title={vesselLabel(vessel)}
            >
              <svg class="profile" viewBox="-28 -23 56 32" aria-hidden="true">
                <VesselGlyph
                  kind={vessel.vesselClass}
                  length={48}
                  flip={vesselProfileFacesLeft(vessel)}
                  opener={vessel.opener}
                />
              </svg>

              <div class="strip">
                <div class="strip-head">
                  <div>
                    <p class="strip-id">{vessel.label}</p>
                    <p class="strip-type">{typeName(vessel)}</p>
                  </div>
                  {#if etaReading(vessel)}
                    <p class="strip-eta">{etaReading(vessel)}<small>TO BRICKELL</small></p>
                  {:else}
                    <p class="strip-eta unavailable">—<small>NO ETA</small></p>
                  {/if}
                </div>

                <p class="strip-movement">
                  <strong>{DIRECTION_WORD[vessel.direction]}</strong>
                  <span>{vessel.speedKnots.toFixed(1)} kn</span>
                </p>

                {#if vessel.opener || openingReading(vessel)}
                  <p class="impact">
                    {#if vessel.opener}<strong>EXPECTED OPENER</strong>{/if}
                    {#if openingReading(vessel)}<span>{openingReading(vessel)}</span>{/if}
                  </p>
                {/if}
              </div>
            </li>
          {/each}
          </ol>
        </div>
      </aside>
    {/if}
  </div>

  <ul class="visually-hidden" aria-label="Bridge readings">
    {#each schematic.stations.filter((station) => station.kind === 'bridge' || station.isTarget) as station (station.routeId + station.label)}
      <li>{stationLabel(station)}</li>
    {/each}
  </ul>
</section>

<style>
  .river {
    min-height: 0;
    padding: clamp(10px, 1.4vw, 20px);
    color: var(--graphite);
    background: var(--frost);
  }

  .river-body {
    display: grid;
    height: 100%;
    min-height: 0;
    background: var(--white);
    border: 1px solid var(--rule-strong);
    box-shadow: 0 8px 24px rgba(11, 42, 69, 0.08);
  }

  .river-body.has-manifest {
    grid-template-columns: minmax(0, 1fr) clamp(286px, 23vw, 344px);
  }

  .schematic-scroll {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--white);
  }

  .river-body.has-manifest .schematic-scroll {
    border-right: 1px solid var(--rule-strong);
  }

  .map-pan-controls {
    display: none;
  }

  .map-pan-controls button,
  .manifest-scroll-controls button {
    display: grid;
    width: 32px;
    height: 32px;
    place-items: center;
    color: var(--marine);
    background: var(--white);
    border: 1px solid var(--rule-strong);
    padding: 0;
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    font-weight: 700;
    line-height: 1;
  }

  .map-pan-controls button + button,
  .manifest-scroll-controls button + button {
    border-left: 0;
  }

  .map-pan-controls button:focus-visible,
  .manifest-scroll-controls button:focus-visible {
    outline: var(--focus);
    outline-offset: 2px;
  }

  .schematic-plot {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
  }

  .map-paper {
    fill: var(--white);
  }

  .map-grid {
    fill: url('#transit-grid');
    opacity: 0.3;
  }

  :global(#transit-grid path) {
    stroke: var(--rule);
    stroke-width: 0.75px;
  }

  :global(#transit-grid circle) {
    fill: var(--rule-strong);
  }

  .map-title text,
  .bay-title text,
  .route-terminals text {
    fill: var(--marine);
    font-family: var(--font-instrument);
    font-weight: 700;
    text-transform: uppercase;
  }

  .map-title path,
  .bay-title path {
    fill: none;
    stroke: var(--marine);
    stroke-width: 1.2px;
  }

  .map-kicker {
    font-size: var(--type-caption);
    letter-spacing: 0.18em;
  }

  .map-name {
    font-size: var(--type-section);
    letter-spacing: 0.05em;
  }

  .bay-title text {
    font-size: var(--type-title);
    letter-spacing: 0.18em;
  }

  .route-casing,
  .route-color,
  .route-register {
    stroke-linecap: square;
    stroke-linejoin: bevel;
  }

  .route-casing {
    stroke: var(--white);
    stroke-width: 37px;
  }

  .route-color {
    stroke: var(--corridor);
    stroke-width: 23px;
  }

  .route-register {
    stroke: rgba(255, 255, 255, 0.76);
    stroke-dasharray: 1 15;
    stroke-width: 3px;
  }

  .route[data-role='north'] .route-color,
  .route[data-role='east'] .route-color,
  .route[data-role='south'] .route-color,
  .route[data-role='approach'] .route-color {
    stroke: var(--channel);
    stroke-width: 15px;
    opacity: 0.88;
  }

  .route[data-role='north'] .route-casing,
  .route[data-role='east'] .route-casing,
  .route[data-role='south'] .route-casing,
  .route[data-role='approach'] .route-casing {
    stroke-width: 27px;
  }

  .route-terminals circle {
    fill: var(--white);
    stroke: var(--marine);
    stroke-width: 3px;
  }

  .route-terminals text {
    fill: var(--muted);
    font-size: var(--type-label);
    letter-spacing: 0.075em;
  }

  .wakes line {
    stroke: var(--corridor);
    stroke-dasharray: 3 9;
    stroke-linecap: square;
  }

  .junction-ring {
    fill: var(--white);
    stroke: var(--marine);
    stroke-width: 4px;
  }

  .junction-core {
    fill: var(--marine);
  }

  .station-label,
  .station-state,
  .junction-label {
    fill: var(--graphite);
    stroke: var(--white);
    stroke-width: 5px;
    paint-order: stroke;
    font-family: var(--font-instrument);
    font-size: var(--type-body);
    font-weight: 700;
    letter-spacing: 0.045em;
    text-anchor: middle;
    text-transform: uppercase;
  }

  .station-state {
    fill: var(--success);
    font-size: var(--type-label);
    letter-spacing: 0.11em;
  }

  .station[data-state='up'] .station-state {
    fill: var(--danger);
  }

  .station[data-state='unknown'] .station-state {
    fill: var(--muted);
  }

  .junction-label {
    fill: var(--marine);
    font-size: var(--type-caption);
  }

  .target-disc-shadow {
    fill: none;
    stroke: var(--marine);
    stroke-width: 14px;
    opacity: 0.08;
  }

  .target-disc {
    fill: var(--white);
    stroke: var(--marine);
    stroke-width: 3px;
  }

  .target-ticks {
    fill: none;
    stroke: var(--marine);
    stroke-width: 3px;
  }

  .target-kicker,
  .target-name,
  .target-subname,
  .target-road-word,
  .target-state-word {
    fill: var(--marine);
    font-family: var(--font-instrument);
    font-weight: 700;
    text-anchor: middle;
    text-transform: uppercase;
  }

  .target-kicker {
    fill: var(--muted);
    font-size: var(--type-caption);
    letter-spacing: 0.2em;
  }

  .target-name {
    font-size: var(--type-headline);
    letter-spacing: 0.035em;
  }

  .target-subname {
    font-size: var(--type-title);
    letter-spacing: 0.16em;
  }

  .target-state-board rect {
    fill: var(--success);
    stroke: var(--marine);
    stroke-width: 2px;
  }

  .target-road-word,
  .target-state-word {
    fill: var(--white);
  }

  .target-road-word {
    font-size: var(--type-title);
    letter-spacing: 0.08em;
  }

  .target-state-word {
    font-size: var(--type-caption);
    letter-spacing: 0.15em;
  }

  .target-station[data-state='up'] .target-state-board rect {
    fill: var(--danger);
  }

  .target-station[data-state='unknown'] .target-state-board rect {
    fill: var(--steel);
  }

  .vessel {
    transition: transform 8.6s linear;
  }

  .vessel-ship,
  .heading-runway {
    transition: transform 680ms cubic-bezier(0.2, 0.72, 0.18, 1);
  }

  .heading-runway {
    opacity: 0.58;
  }

  .heading-runway line {
    stroke: var(--corridor);
    stroke-dasharray: 3 7;
    stroke-width: 2px;
  }

  .heading-runway path {
    fill: var(--corridor);
  }

  .vessel.is-opener .heading-runway line,
  .vessel.is-opener .callout-leader {
    stroke: var(--amber-ink);
  }

  .vessel.is-opener .heading-runway path {
    fill: var(--amber-ink);
  }

  .opener-pulse {
    fill: none;
    stroke: var(--amber-ink);
    stroke-width: 2px;
    transform-box: fill-box;
    transform-origin: center;
    animation: opener-pulse 1.8s ease-out infinite;
  }

  .callout-leader {
    stroke: var(--marine);
    stroke-width: 1.5px;
  }

  .callout-plate {
    fill: var(--white);
    stroke: var(--marine);
    stroke-width: 1.5px;
  }

  .vessel-mini-readout rect {
    fill: var(--white);
    stroke: var(--marine);
    stroke-width: 1.2px;
  }

  .mini-name,
  .mini-type,
  .mini-movement,
  .mini-eta {
    fill: var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 700;
    letter-spacing: 0.045em;
    text-transform: uppercase;
  }

  .mini-name {
    font-size: var(--type-label);
  }

  .mini-type {
    fill: var(--muted);
    font-weight: 600;
  }

  .mini-movement,
  .mini-eta {
    fill: var(--corridor);
    text-anchor: end;
  }

  .mini-eta {
    fill: var(--muted);
  }

  .vessel-callout[data-opener='true'] .callout-plate {
    fill: var(--amber-sheet);
    stroke: var(--amber-ink);
  }

  .vessel-tag,
  .vessel-type,
  .vessel-direction,
  .vessel-eta,
  .vessel-eta-label {
    fill: var(--graphite);
    font-family: var(--font-instrument);
    text-transform: uppercase;
  }

  .vessel-tag {
    font-size: var(--type-label);
    font-weight: 700;
    letter-spacing: 0.035em;
  }

  .vessel-type {
    fill: var(--muted);
    font-size: var(--type-caption);
    font-weight: 600;
    letter-spacing: 0.07em;
  }

  .vessel-direction {
    fill: var(--corridor);
    font-size: var(--type-caption);
    font-weight: 700;
    letter-spacing: 0.075em;
  }

  .callout-rule {
    stroke: var(--rule-strong);
    stroke-width: 1px;
  }

  .vessel-eta {
    font-size: var(--type-title);
    font-weight: 700;
    letter-spacing: -0.015em;
    text-anchor: end;
  }

  .vessel-eta.no-eta {
    fill: var(--steel);
  }

  .vessel-eta-label {
    fill: var(--muted);
    font-size: var(--type-caption);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-anchor: end;
  }

  .callout-opener rect {
    fill: var(--amber);
    stroke: var(--amber-ink);
    stroke-width: 0.8px;
  }

  .callout-opener text {
    fill: var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 700;
    letter-spacing: 0.07em;
    text-anchor: middle;
  }

  .manifest-rail {
    display: flex;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    background: var(--frost);
  }

  .manifest-head {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 86px;
    padding: 14px 16px;
    color: var(--white);
    background: var(--marine);
  }

  .manifest-actions,
  .manifest-scroll-controls {
    display: flex;
    align-items: center;
  }

  .manifest-actions {
    gap: 8px;
  }

  .manifest-scroll-controls button {
    width: 27px;
    height: 27px;
    color: var(--white);
    background: transparent;
    border-color: rgba(255, 255, 255, 0.58);
    font-size: var(--type-body);
  }

  .manifest-head p,
  .manifest-head h3 {
    margin: 0;
    color: inherit;
    font-family: var(--font-instrument);
    font-weight: 700;
    line-height: 0.95;
    letter-spacing: 0.055em;
  }

  .manifest-head p {
    font-size: var(--type-caption);
    letter-spacing: 0.14em;
  }

  .manifest-head h3 {
    margin-top: 5px;
    font-size: var(--type-section);
  }

  .target-chip {
    display: inline-grid;
    place-items: center;
    min-width: 72px;
    min-height: 34px;
    color: var(--marine);
    background: var(--success-sheet);
    border: 1px solid currentColor;
    padding: 5px 8px;
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 700;
    letter-spacing: 0.09em;
    text-transform: uppercase;
  }

  .target-chip[data-state='up'] {
    color: var(--white);
    background: var(--danger);
  }

  .target-chip[data-state='unknown'] {
    color: var(--graphite);
    background: var(--paper);
  }

  .manifest-scroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .manifest {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .manifest li {
    display: grid;
    grid-template-columns: 58px minmax(0, 1fr);
    gap: 11px;
    align-items: start;
    padding: 16px 15px;
    background: var(--white);
    border-bottom: 1px solid var(--rule-strong);
  }

  .manifest li.is-opener {
    background: var(--amber-sheet);
  }

  .profile {
    width: 56px;
    height: 32px;
    margin-top: 2px;
  }

  .strip {
    display: grid;
    gap: 9px;
    min-width: 0;
  }

  .strip-head {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: start;
  }

  .strip-id,
  .strip-type,
  .strip-eta,
  .strip-movement,
  .impact {
    margin: 0;
  }

  .strip-id,
  .strip-type,
  .strip-eta,
  .strip-movement {
    font-family: var(--font-instrument);
    text-transform: uppercase;
  }

  .strip-id {
    overflow: hidden;
    color: var(--graphite);
    font-size: var(--type-label);
    font-weight: 700;
    letter-spacing: 0.035em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .strip-type {
    margin-top: 4px;
    color: var(--muted);
    font-size: var(--type-caption);
    font-weight: 600;
    letter-spacing: 0.075em;
  }

  .strip-eta {
    color: var(--corridor);
    font-size: var(--type-title);
    font-weight: 700;
    line-height: 0.9;
    text-align: right;
    white-space: nowrap;
  }

  .strip-eta small {
    display: block;
    margin-top: 5px;
    color: var(--muted);
    font-size: var(--type-caption);
    letter-spacing: 0.08em;
  }

  .strip-eta.unavailable {
    color: var(--steel);
  }

  .strip-movement {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    color: var(--corridor);
    font-size: var(--type-label);
    font-weight: 700;
    letter-spacing: 0.06em;
  }

  .strip-movement span {
    color: var(--graphite);
  }

  .impact {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    color: var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    letter-spacing: 0.07em;
  }

  .impact strong {
    display: inline-block;
    background: var(--amber);
    padding: 3px 6px;
  }

  .map-unavailable {
    margin: 20px;
    color: var(--muted);
    font-size: var(--type-body);
  }

  @keyframes opener-pulse {
    from {
      opacity: 0.75;
      transform: scale(0.7);
    }
    to {
      opacity: 0;
      transform: scale(1.45);
    }
  }

  @media (max-width: 1180px) {
    .river {
      padding: 10px;
    }

    .river-body.has-manifest {
      grid-template-columns: minmax(0, 1fr);
    }

    .schematic-scroll {
      height: min(68vh, 620px);
      min-height: 460px;
      overflow-x: auto;
      overflow-y: hidden;
      overscroll-behavior-x: contain;
      border-right: 0;
      border-bottom: 1px solid var(--rule-strong);
    }

    .schematic-plot {
      position: static;
      width: 1120px;
      min-width: 1120px;
      height: 100%;
    }

    .map-pan-controls {
      position: sticky;
      bottom: 12px;
      left: calc(100% - 90px);
      z-index: 4;
      display: flex;
      width: max-content;
      margin: -44px 12px 12px auto;
    }

    .manifest-scroll {
      overflow-y: visible;
    }

    .manifest-scroll-controls {
      display: none;
    }
  }

  @media (max-width: 560px) {
    .manifest li {
      grid-template-columns: 50px minmax(0, 1fr);
      padding-inline: 11px;
    }

    .profile {
      width: 48px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .vessel,
    .vessel-ship,
    .heading-runway {
      transition: none;
    }

    .opener-pulse {
      animation: none;
    }
  }

  @media (forced-colors: active) {
    .route-color,
    .wakes line,
    .heading-runway line,
    .callout-leader,
    .target-disc,
    .target-ticks {
      stroke: CanvasText;
    }

    .callout-plate,
    .vessel-mini-readout rect {
      fill: Canvas;
      stroke: CanvasText;
    }

    .vessel-tag,
    .vessel-type,
    .vessel-direction,
    .vessel-eta,
    .vessel-eta-label,
    .mini-name,
    .mini-type,
    .mini-movement,
    .mini-eta {
      fill: CanvasText;
    }

    .target-state-board rect,
    .target-chip,
    .impact strong {
      forced-color-adjust: none;
    }
  }
</style>
