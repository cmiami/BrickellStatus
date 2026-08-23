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
  import { currentVesselTracks, travelDirection, type TravelDirection } from '$lib/river';
  import { layoutVesselAnnotations } from '$lib/vesselAnnotationLayout';
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

  interface VesselReading {
    label: string;
    vesselClass?: string;
    direction: TravelDirection;
    directionLabel?: string;
    speedKnots: number;
    knownOpener: boolean;
    likelyToOpenBrickell: boolean;
    etaMinMinutes?: number;
    etaMaxMinutes?: number;
    predictedOpeningAt?: string;
  }

  interface IdentifierVessel extends VesselReading {
    mmsi: string;
    track: VesselTrack;
    closing: boolean;
    ordinal: number;
    positioned?: SchematicVessel;
  }

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
  const passageVesselTracks = $derived(
    liveVesselTracks.filter((track) => track.routeIntersects)
  );
  const schematic = $derived(riverSchematic(corridor, passageVesselTracks, bridgeStates));
  const targetState = $derived(schematic.target?.state ?? 'unknown');
  const visibleStations = $derived(
    schematic.stations.filter(
      (station) => !station.isTarget && (station.kind === 'bridge' || station.kind === 'mouth')
    )
  );

  const identifierVessels = $derived.by(() => {
    const positionedByMmsi = new Map(
      schematic.vessels.map((vessel) => [vessel.mmsi, vessel] as const)
    );
    return passageVesselTracks
      .map((track): Omit<IdentifierVessel, 'ordinal'> => {
        const positioned = positionedByMmsi.get(track.mmsi);
        return {
          mmsi: track.mmsi,
          track,
          label:
            positioned?.label ??
            (track.vesselName?.trim() || track.callSign?.trim() || track.mmsi),
          vesselClass: positioned?.vesselClass ?? track.vesselClass,
          direction: positioned?.direction ?? travelDirection(track),
          directionLabel: positioned ? undefined : unpositionedDirection(track),
          speedKnots: positioned?.speedKnots ?? track.speedKnots,
          knownOpener: positioned?.knownOpener ?? track.knownOpener === true,
          likelyToOpenBrickell:
            positioned?.likelyToOpenBrickell ?? track.likelyToOpenBrickell === true,
          etaMinMinutes: positioned?.etaMinMinutes ?? track.etaMinMinutes,
          etaMaxMinutes: positioned?.etaMaxMinutes ?? track.etaMaxMinutes,
          predictedOpeningAt: positioned?.predictedOpeningAt ?? track.predictedOpeningAt,
          closing: positioned?.closing ?? track.routeIntersects,
          positioned
        };
      })
      .sort((left, right) => {
        const impact = (vessel: Omit<IdentifierVessel, 'ordinal'>) =>
          Number(vessel.likelyToOpenBrickell) * 8 + Number(vessel.knownOpener) * 4;
        const distance = (vessel: Omit<IdentifierVessel, 'ordinal'>) =>
          vessel.positioned?.distanceMeters ?? Number.MAX_SAFE_INTEGER;
        return (
          impact(right) - impact(left) ||
          distance(left) - distance(right) ||
          left.mmsi.localeCompare(right.mmsi)
        );
      })
      .map((vessel, index): IdentifierVessel => ({ ...vessel, ordinal: index + 1 }));
  });
  const ordinalByMmsi = $derived(
    new Map(identifierVessels.map((vessel) => [vessel.mmsi, vessel.ordinal] as const))
  );
  const identifierByMmsi = $derived(
    new Map(identifierVessels.map((vessel) => [vessel.mmsi, vessel] as const))
  );
  const CALLOUT_WIDTH = 210;
  const CALLOUT_HEIGHT = 58;
  const annotationLayout = $derived.by(() => {
    const target = schematic.target;
    return layoutVesselAnnotations({
      bounds: { x: 0, y: 0, width: schematic.width, height: schematic.height },
      routes: schematic.routes.map((route) => ({
        points: route.points,
        halfWidth: route.role === 'river' ? 18.5 : 13.5
      })),
      targetExclusion: target
        ? { x: target.x - 234, y: target.y - 274, width: 468, height: 548 }
        : { x: 0, y: 0, width: 0, height: 0 },
      obstacles: visibleStations.map((station) =>
        station.kind === 'bridge'
          ? { x: station.x - 82, y: station.y - 86, width: 164, height: 172 }
          : { x: station.x - 62, y: station.y - 66, width: 124, height: 112 }
      ),
      vessels: schematic.vessels.map((vessel) => ({
        id: vessel.mmsi,
        anchor: { x: vessel.x, y: vessel.y },
        angleDegrees: vessel.angleDegrees,
        hullWidth: Math.max(48, Math.min(68, vessel.hullLength * 2.5)),
        hullHeight: Math.max(30, Math.min(42, vessel.hullLength * 1.48)),
        avoidanceRect: vesselDecorationBounds(vessel),
        cardWidth: CALLOUT_WIDTH,
        cardHeight: CALLOUT_HEIGHT,
        priority:
          Number(vessel.likelyToOpenBrickell) * 8 +
          Number(vessel.knownOpener) * 4 +
          Number(vessel.closing) * 2
      }))
    });
  });

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
  let highlightedVesselMmsi = $state<string | null>(null);
  let previewedVesselMmsi = $state<string | null>(null);
  const activeVesselMmsi = $derived(previewedVesselMmsi ?? highlightedVesselMmsi);

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

  function unpositionedDirection(track: VesselTrack): string {
    switch (track.movement) {
      case 'approaching':
        return 'Toward Brickell';
      case 'diverging':
        return 'Away from Brickell';
      case 'stationary':
        return 'Stationary';
      default:
        return Number.isFinite(track.courseDegrees)
          ? `Course ${track.courseDegrees.toFixed(0)}°`
          : 'Direction unavailable';
    }
  }

  function titleCase(value: string): string {
    return value.replace(/\b\w/g, (letter) => letter.toUpperCase());
  }

  function typeName(vessel: Pick<VesselReading, 'vesselClass'>): string {
    return vessel.vesselClass && vessel.vesselClass !== 'vessel'
      ? titleCase(vessel.vesselClass)
      : 'Vessel';
  }

  function etaReading(
    vessel: Pick<VesselReading, 'etaMinMinutes' | 'etaMaxMinutes'>
  ): string | null {
    if (vessel.etaMinMinutes == null) return null;
    const max = vessel.etaMaxMinutes ?? vessel.etaMinMinutes;
    return max > vessel.etaMinMinutes
      ? `${vessel.etaMinMinutes}–${max} min`
      : `${vessel.etaMinMinutes} min`;
  }

  function openingReading(vessel: Pick<VesselReading, 'predictedOpeningAt'>): string | null {
    if (!vessel.predictedOpeningAt) return null;
    const parsed = new Date(vessel.predictedOpeningAt);
    return Number.isNaN(parsed.getTime()) ? null : clock.format(parsed);
  }

  function stationLabel(station: SchematicStation): string {
    return station.kind === 'bridge' || station.isTarget
      ? `${station.label}. Bridge ${STATE_WORD[station.state ?? 'unknown']}.`
      : station.label;
  }

  function vesselLabel(vessel: VesselReading): string {
    const eta = etaReading(vessel);
    const parts = [
      `${vessel.label}. ${typeName(vessel)}. ${vessel.directionLabel ?? DIRECTION_WORD[vessel.direction]} at ${vessel.speedKnots.toFixed(1)} knots.`
    ];
    if (eta) parts.push(`${eta} to Brickell Avenue Bridge.`);
    if (vessel.knownOpener) parts.push('Known Brickell opener.');
    if (vessel.likelyToOpenBrickell) parts.push('Likely to open Brickell on this passage.');
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

  function identifierProfileFacesLeft(vessel: IdentifierVessel): boolean {
    return vessel.positioned
      ? vesselProfileFacesLeft(vessel.positioned)
      : vessel.direction === 'upriver';
  }

  function vesselDecorationBounds(vessel: SchematicVessel) {
    const hullWidth = Math.max(48, Math.min(68, vessel.hullLength * 2.5));
    const hullHeight = Math.max(30, Math.min(42, vessel.hullLength * 1.48));
    const radians = (vessel.angleDegrees * Math.PI) / 180;
    const rotate = (x: number, y: number) => ({
      x: x * Math.cos(radians) - y * Math.sin(radians),
      y: x * Math.sin(radians) + y * Math.cos(radians)
    });
    const localPoints = [
      { x: -hullWidth / 2, y: -hullHeight / 2 },
      { x: hullWidth / 2, y: hullHeight / 2 },
      { x: -44, y: -38 },
      { x: -18, y: -14 },
      { x: -42, y: -42 },
      { x: 42, y: 42 },
      ...[
        [38, -8],
        [60, -8],
        [38, 8],
        [60, 8]
      ].map(([x, y]) => rotate(x, y))
    ];
    const xs = localPoints.map((point) => point.x);
    const ys = localPoints.map((point) => point.y);
    const left = Math.min(...xs);
    const top = Math.min(...ys);
    return {
      x: vessel.x + left,
      y: vessel.y + top,
      width: Math.max(...xs) - left,
      height: Math.max(...ys) - top
    };
  }

  function placementReading(vessel: IdentifierVessel): string {
    if (vessel.track.posture === 'off_channel' || vessel.track.sMeters == null) {
      return 'Outside route corridor';
    }
    if (vessel.track.posture === 'moored') return 'Moored';
    if (vessel.track.posture === 'holding') return 'Holding position';
    if (vessel.track.posture === 'deep_draft') return 'Too deep for river';
    return vessel.positioned ? 'Shown on route' : 'Position not shown on route';
  }

  function shortVesselName(value: string): string {
    const characters = Array.from(value);
    return characters.length <= 15 ? value : `${characters.slice(0, 14).join('')}…`;
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

  <div class:has-manifest={identifierVessels.length > 0} class="river-body">
    <div class="schematic-stack">
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
          aria-label="{corridor.aisLive ? 'Live Miami River vessel network' : 'Miami River vessel traffic unavailable'} with Brickell Avenue Bridge as the focal target, {visibleStations.length} other bridge or junction stations, and {schematic.vessels.length} vessels shown on the route."
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

          <g class="annotation-leaders" aria-hidden="true">
            {#each annotationLayout.placements as placement (placement.id)}
              {@const vessel = identifierByMmsi.get(placement.id)}
              <line
                class:is-likely-opener={vessel?.likelyToOpenBrickell}
                x1={placement.leader.from.x}
                y1={placement.leader.from.y}
                x2={placement.leader.to.x}
                y2={placement.leader.to.y}
              />
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
              {@const ordinal = ordinalByMmsi.get(vessel.mmsi)}
              <g
                class="vessel"
                class:is-known-opener={vessel.knownOpener}
                class:is-likely-opener={vessel.likelyToOpenBrickell}
                class:is-highlighted={activeVesselMmsi === vessel.mmsi}
                class:is-closing={vessel.closing}
                data-mmsi={vessel.mmsi}
                transform="translate({vessel.x.toFixed(1)} {vessel.y.toFixed(1)})"
              >
                <title>{vesselLabel(vessel)}</title>
                {#if activeVesselMmsi === vessel.mmsi}<circle class="identifier-selection" r="42" />{/if}
                {#if vessel.likelyToOpenBrickell}<circle class="opener-pulse" r="34" />{/if}
                <g
                  class="direction-chevrons"
                  transform="rotate({vessel.angleDegrees.toFixed(1)})"
                  aria-hidden="true"
                >
                  <path d="M38 -8L48 0L38 8 M50 -8L60 0L50 8" />
                </g>
                <g class="vessel-ship">
                  <VesselGlyph
                    kind={vessel.vesselClass}
                    length={Math.max(48, Math.min(68, vessel.hullLength * 2.5))}
                    flip={vesselProfileFacesLeft(vessel)}
                    opener={vessel.likelyToOpenBrickell}
                  />
                </g>
                {#if ordinal != null}
                  <g class="vessel-number" transform="translate(-31 -26)" aria-hidden="true">
                    <rect x="-13" y="-12" width="26" height="24" />
                    <text y="5">{ordinal.toString().padStart(2, '0')}</text>
                  </g>
                {/if}
              </g>
            {/each}
          </g>

          <g
            class="vessel-annotations"
            data-unplaced={annotationLayout.unplacedIds.length}
            aria-hidden="true"
          >
            {#each annotationLayout.placements as placement (placement.id)}
              {@const vessel = identifierByMmsi.get(placement.id)}
              {#if vessel}
                <g
                  class="vessel-callout"
                  class:is-known-opener={vessel.knownOpener}
                  class:is-likely-opener={vessel.likelyToOpenBrickell}
                  class:is-highlighted={activeVesselMmsi === vessel.mmsi}
                  data-mmsi={vessel.mmsi}
                  transform="translate({placement.card.x.toFixed(1)} {placement.card.y.toFixed(1)})"
                >
                  <path class="callout-plate" d="M0 0H{CALLOUT_WIDTH}V{CALLOUT_HEIGHT}H8L0 {CALLOUT_HEIGHT - 8}Z" />
                  <rect class="callout-index" x="8" y="8" width="27" height="24" />
                  <text class="callout-index-text" x="21.5" y="25">
                    {vessel.ordinal.toString().padStart(2, '0')}
                  </text>
                  <text class="callout-name" x="42" y="19">{shortVesselName(vessel.label)}</text>
                  <text class="callout-motion" x="42" y="39">
                    {vessel.directionLabel ?? DIRECTION_WORD[vessel.direction]} · {vessel.speedKnots.toFixed(1)} kn
                  </text>
                  <line class="callout-rule" x1="153" y1="8" x2="153" y2="50" />
                  <text class:unavailable={!etaReading(vessel)} class="callout-eta" x="201" y="23">
                    {etaReading(vessel) ?? '—'}
                  </text>
                  <text class="callout-eta-label" x="201" y="42">
                    {etaReading(vessel) ? 'TO BRICKELL' : 'NO ETA'}
                  </text>
                </g>
              {/if}
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

    </div>

    {#if identifierVessels.length}
      <aside class="manifest-rail" aria-labelledby="manifest-heading">
        <header class="manifest-head">
          <div>
            <p>VESSELS TO · {identifierVessels.length}</p>
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
          aria-label="All vessels expected to pass Brickell Avenue Bridge"
        >
          <ol class="manifest">
          {#each identifierVessels as vessel (vessel.mmsi)}
            <li
              class:is-known-opener={vessel.knownOpener}
              class:is-likely-opener={vessel.likelyToOpenBrickell}
              class:is-closing={vessel.closing}
              class:is-selected={highlightedVesselMmsi === vessel.mmsi}
              title={vesselLabel(vessel)}
            >
              <button
                type="button"
                aria-pressed={highlightedVesselMmsi === vessel.mmsi}
                aria-label={vesselLabel(vessel)}
                onmouseenter={() => (previewedVesselMmsi = vessel.mmsi)}
                onmouseleave={() => {
                  if (previewedVesselMmsi === vessel.mmsi) previewedVesselMmsi = null;
                }}
                onfocus={() => (previewedVesselMmsi = vessel.mmsi)}
                onblur={() => {
                  if (previewedVesselMmsi === vessel.mmsi) previewedVesselMmsi = null;
                }}
                onclick={() => (highlightedVesselMmsi = highlightedVesselMmsi === vessel.mmsi ? null : vessel.mmsi)}
              >
                <span class="manifest-index">{vessel.ordinal.toString().padStart(2, '0')}</span>
                <svg class="profile" viewBox="-28 -23 56 32" aria-hidden="true">
                  <VesselGlyph
                    kind={vessel.vesselClass}
                    length={48}
                    flip={identifierProfileFacesLeft(vessel)}
                    opener={vessel.likelyToOpenBrickell}
                  />
                </svg>

                <div class="strip">
                  <div class="strip-head">
                    <div>
                      <p class="strip-id">{vessel.label}</p>
                      <p class="strip-type">{typeName(vessel)} · {placementReading(vessel)}</p>
                    </div>
                    {#if etaReading(vessel)}
                      <p class="strip-eta">{etaReading(vessel)}<small>TO BRICKELL</small></p>
                    {:else}
                      <p class="strip-eta unavailable">—<small>NO ETA</small></p>
                    {/if}
                  </div>

                  <p class="strip-movement">
                    <strong>{vessel.directionLabel ?? DIRECTION_WORD[vessel.direction]}</strong>
                    <span>{vessel.speedKnots.toFixed(1)} kn</span>
                  </p>

                  {#if vessel.knownOpener || vessel.likelyToOpenBrickell || openingReading(vessel)}
                    <p class="impact">
                      {#if vessel.knownOpener}<strong>KNOWN OPENER</strong>{/if}
                      {#if vessel.likelyToOpenBrickell}<strong>LIKELY TO OPEN BRICKELL</strong>{/if}
                      {#if openingReading(vessel)}<span>{openingReading(vessel)}</span>{/if}
                    </p>
                  {/if}
                </div>
              </button>
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

  .schematic-stack {
    display: grid;
    min-width: 0;
    min-height: 0;
    grid-template-rows: minmax(0, 1fr);
    overflow: hidden;
  }

  .schematic-scroll {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--white);
  }

  .river-body.has-manifest .schematic-stack {
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
  .direction-chevrons {
    transition: transform 680ms cubic-bezier(0.2, 0.72, 0.18, 1);
  }

  .direction-chevrons path {
    fill: none;
    stroke: var(--corridor);
    stroke-width: 3px;
    stroke-linecap: square;
    stroke-linejoin: miter;
  }

  .vessel.is-likely-opener .direction-chevrons path {
    stroke: var(--amber-ink);
  }

  .vessel-number rect {
    fill: var(--marine);
    stroke: var(--white);
    stroke-width: 2px;
  }

  .vessel-number text {
    fill: var(--white);
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 700;
    letter-spacing: 0.03em;
    text-anchor: middle;
  }

  .opener-pulse {
    fill: none;
    stroke: var(--amber-ink);
    stroke-width: 2px;
    transform-box: fill-box;
    transform-origin: center;
    animation: opener-pulse 1.8s ease-out infinite;
  }

  .identifier-selection {
    fill: var(--corridor-wash);
    stroke: var(--marine);
    stroke-width: 3px;
    vector-effect: non-scaling-stroke;
  }

  .annotation-leaders line {
    stroke: var(--marine);
    stroke-width: 1.5px;
    vector-effect: non-scaling-stroke;
  }

  .annotation-leaders line.is-likely-opener {
    stroke: var(--amber-ink);
  }

  .vessel-callout {
    pointer-events: none;
  }

  .callout-plate {
    fill: var(--white);
    stroke: var(--marine);
    stroke-width: 1.5px;
    vector-effect: non-scaling-stroke;
  }

  .vessel-callout.is-known-opener .callout-plate {
    fill: var(--corridor-sheet);
  }

  .vessel-callout.is-likely-opener .callout-plate {
    fill: var(--amber-sheet);
    stroke: var(--amber-ink);
  }

  .vessel-callout.is-highlighted .callout-plate {
    stroke-width: 3px;
  }

  .callout-index {
    fill: var(--marine);
  }

  .vessel-callout.is-likely-opener .callout-index {
    fill: var(--amber-ink);
  }

  .callout-index-text,
  .callout-name,
  .callout-motion,
  .callout-eta,
  .callout-eta-label {
    font-family: var(--font-instrument);
    font-weight: 700;
    text-transform: uppercase;
  }

  .callout-index-text {
    fill: var(--white);
    font-size: 11px;
    letter-spacing: 0.03em;
    text-anchor: middle;
  }

  .callout-name {
    fill: var(--graphite);
    font-size: 12px;
    letter-spacing: 0.025em;
  }

  .callout-motion {
    fill: var(--corridor);
    font-size: 10px;
    letter-spacing: 0.035em;
  }

  .callout-rule {
    stroke: var(--rule-strong);
    stroke-width: 1px;
  }

  .callout-eta {
    fill: var(--corridor);
    font-size: 15px;
    letter-spacing: -0.02em;
    text-anchor: end;
  }

  .callout-eta.unavailable {
    fill: var(--steel);
  }

  .callout-eta-label {
    fill: var(--muted);
    font-size: 8px;
    letter-spacing: 0.07em;
    text-anchor: end;
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
    background: var(--white);
    border-bottom: 1px solid var(--rule-strong);
  }

  .manifest li > button {
    display: grid;
    width: 100%;
    grid-template-columns: 28px 52px minmax(0, 1fr);
    gap: 9px;
    align-items: start;
    padding: 14px 12px;
    color: inherit;
    background: transparent;
    border: 0;
    border-radius: 0;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .manifest li > button:hover {
    box-shadow: inset 4px 0 0 var(--corridor);
  }

  .manifest li > button:focus-visible {
    position: relative;
    z-index: 1;
    outline: var(--focus);
    outline-offset: -3px;
  }

  .manifest li.is-selected > button {
    box-shadow: inset 5px 0 0 var(--marine);
  }

  .manifest-index {
    display: grid;
    width: 26px;
    height: 26px;
    place-items: center;
    color: var(--white);
    background: var(--marine);
    border: 1px solid var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 700;
    letter-spacing: 0.04em;
    line-height: 1;
  }

  .manifest li.is-known-opener {
    background: var(--corridor-sheet);
  }

  .manifest li.is-likely-opener {
    background: var(--amber-sheet);
  }

  .profile {
    width: 52px;
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

    .river-body.has-manifest .schematic-stack {
      border-right: 0;
      border-bottom: 1px solid var(--rule-strong);
    }

    .schematic-scroll {
      height: min(68vh, 620px);
      min-height: 460px;
      overflow-x: auto;
      overflow-y: hidden;
      overscroll-behavior-x: contain;
      border-right: 0;
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
    .manifest li > button {
      grid-template-columns: 26px 46px minmax(0, 1fr);
      padding-inline: 11px;
    }

    .profile {
      width: 48px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .vessel,
    .vessel-ship,
    .direction-chevrons {
      transition: none;
    }

    .opener-pulse {
      animation: none;
    }
  }

  @media (forced-colors: active) {
    .route-color,
    .wakes line,
    .direction-chevrons path,
    .annotation-leaders line,
    .target-disc,
    .target-ticks {
      stroke: CanvasText;
    }

    .target-state-board rect,
    .target-chip,
    .impact strong {
      forced-color-adjust: none;
    }

    .identifier-selection {
      fill: Canvas;
      stroke: Highlight;
    }

    .callout-plate {
      fill: Canvas;
      stroke: CanvasText;
    }

    .callout-index-text,
    .callout-name,
    .callout-motion,
    .callout-eta,
    .callout-eta-label {
      fill: CanvasText;
    }
  }
</style>
