/**
 * THESIS: Read the Miami River as a transit network whose one interchange is
 * Brickell, preserving water order while refusing geographic clutter.
 * OWN-WORLD: Fixed octolinear routes, a shared mouth junction, and one oversized
 * target coordinate; live facts remain the engine's facts.
 * STORY: Follow a hull from an approach, through the mouth, to the span, then
 * upriver past every bascule in the order it will meet them.
 * FIRST VIEWPORT: Brickell sits at (690, 320), upriver runs left, and three bay
 * approaches fan right into distinct upper, middle, and lower routes.
 * FORM: A pure, fixed-size projection for semantic SVG rendering.
 */
import type { RiverCorridor, RiverCorridorBranch, RiverStation, VesselTrack } from './types';
import { makeChannelProjector, reachVessels, type ReachVessel } from './river';
import { separateRouteHulls } from './route-hulls';
import type { PublicVesselGroup } from './tow-inference';

export const RIVER_SCHEMATIC_WIDTH = 1_320;
export const RIVER_SCHEMATIC_HEIGHT = 640;
export const BRICKELL_SCHEMATIC_POINT = { x: 690, y: 320 } as const;

export type SchematicBridgeState = 'up' | 'down' | 'unknown';

export interface SchematicRoutePoint {
  x: number;
  y: number;
  /** The engine's signed channel coordinate at this point. */
  sMeters: number;
  /** Tangent toward increasing `sMeters`, in SVG degrees. */
  angleDegrees: number;
}

export interface SchematicRoute {
  /** The engine branch id; stable enough to key an SVG route. */
  id: string;
  branchId: string;
  label: string;
  approach: boolean;
  role: 'river' | 'north' | 'east' | 'south' | 'approach';
  /** Geometry-order vertices. Every segment is horizontal or 45 degrees. */
  points: SchematicRoutePoint[];
  /** Ready for an SVG path's `d` attribute. */
  d: string;
}

export interface SchematicStation extends RiverStation {
  branchId: string;
  routeId: string;
  x: number;
  y: number;
  /** Tangent toward upriver/increasing `sMeters`. */
  angleDegrees: number;
  state?: SchematicBridgeState;
  isTarget: boolean;
  /** Suggested normal side for a renderer's label, never a data claim. */
  labelSide: -1 | 1;
}

export interface SchematicTrailPoint {
  x: number;
  y: number;
  observedAt: string;
  /** Zero at the oldest retained schematic fix, one at the hull. */
  freshness: number;
}

export interface SchematicVessel extends ReachVessel {
  routeId: string;
  x: number;
  y: number;
  /** Heading along the drawn route, already flipped for downriver travel. */
  angleDegrees: number;
  wake: SchematicTrailPoint[];
  /** Original route anchor and a separate visual placement. */
  reportedX: number;
  reportedY: number;
  displaySMeters: number;
  estimated: boolean;
  estimatePath: string;
  towGroupId?: string;

  // Engine fields repeated at the rendering boundary so a component never has
  // to reconstruct identity, movement, timing, or schedule meaning.
  vesselName?: string;
  movement: VesselTrack['movement'];
  routeIntersects: boolean;
  courseDegrees: number;
  observedAt: string;
  posture?: VesselTrack['posture'];
  branch?: VesselTrack['branch'];
  callSign?: string;
  imoNumber?: number;
  destination?: string;
  lengthMeters?: number;
  beamMeters?: number;
  draughtMeters?: number;
  openingPropensity?: number;
  etaMinMinutes?: number;
  etaMaxMinutes?: number;
  scheduleExempt: boolean;
  predictedOpeningAt?: string;
  waitsForSlot: boolean;

  /** Display sizing only; real dimensions above remain unchanged. */
  hullLength: number;
  hullBeam: number;
  sizeKnown: boolean;
}

export interface RiverSchematic {
  width: number;
  height: number;
  routes: SchematicRoute[];
  stations: SchematicStation[];
  vessels: SchematicVessel[];
  /** The one engine-published target, or null when the corridor has none. */
  target: SchematicStation | null;
}

interface RouteSeedPoint {
  x: number;
  y: number;
  sMeters: number;
}

export interface SchematicRoutePosition {
  x: number;
  y: number;
  /** Unit tangent toward increasing `sMeters`. */
  dx: number;
  dy: number;
  angleDegrees: number;
}

const MOUTH_POINT = { x: 990, y: 320 } as const;
const UPRIVER_GUIDE = [
  BRICKELL_SCHEMATIC_POINT,
  { x: 620, y: 320 },
  { x: 560, y: 260 },
  { x: 470, y: 260 },
  { x: 410, y: 200 },
  { x: 330, y: 200 },
  { x: 270, y: 140 },
  { x: 190, y: 140 },
  { x: 130, y: 80 },
  { x: 50, y: 80 }
] as const;
const FIRST_UPRIVER_STATION_GUIDE_INDEX = 3;
const UPRIVER_TAIL_FRACTION = 0.1;
const DEFAULT_UPRIVER_END_METERS = 6_000;
const DEFAULT_MOUTH_METERS = -530;
const DEFAULT_APPROACH_END_METERS = -7_000;
const WAKE_MAX_AGE_MS = 30 * 60_000;
const WAKE_MAX_POINTS = 18;

const DEFAULT_HULL_LENGTH = 17;
const DEFAULT_HULL_BEAM = 6.5;
const REFERENCE_LENGTH_METERS = 30;

function finite(values: number[]): number[] {
  return values.filter(Number.isFinite);
}

function normalizeDegrees(degrees: number): number {
  return ((degrees % 360) + 360) % 360;
}

function tangent(lower: RouteSeedPoint, upper: RouteSeedPoint) {
  const dx = upper.x - lower.x;
  const dy = upper.y - lower.y;
  const length = Math.hypot(dx, dy) || 1;
  const unitX = dx / length;
  const unitY = dy / length;
  return {
    dx: unitX,
    dy: unitY,
    angleDegrees: normalizeDegrees((Math.atan2(unitY, unitX) * 180) / Math.PI)
  };
}

function pointOnSeeds(points: RouteSeedPoint[], sMeters: number): SchematicRoutePosition | null {
  if (points.length === 0 || !Number.isFinite(sMeters)) return null;
  if (points.length === 1) {
    return { x: points[0].x, y: points[0].y, dx: 1, dy: 0, angleDegrees: 0 };
  }

  const ordered = points.slice().sort((left, right) => left.sMeters - right.sMeters);
  for (let index = 0; index + 1 < ordered.length; index += 1) {
    const lower = ordered[index];
    const upper = ordered[index + 1];
    if (sMeters < lower.sMeters || sMeters > upper.sMeters) continue;
    const span = upper.sMeters - lower.sMeters || 1;
    const fraction = (sMeters - lower.sMeters) / span;
    return {
      x: lower.x + (upper.x - lower.x) * fraction,
      y: lower.y + (upper.y - lower.y) * fraction,
      ...tangent(lower, upper)
    };
  }

  const first = ordered[0];
  const last = ordered[ordered.length - 1];
  return sMeters < first.sMeters
    ? { x: first.x, y: first.y, ...tangent(first, ordered[1]) }
    : { x: last.x, y: last.y, ...tangent(ordered[ordered.length - 2], last) };
}

/** Place a signed channel coordinate on an already-built schematic route. */
export function schematicPointAt(
  route: SchematicRoute,
  sMeters: number
): SchematicRoutePosition | null {
  return pointOnSeeds(route.points, sMeters);
}

function exposeRoute(
  branch: RiverCorridorBranch,
  approach: boolean,
  role: SchematicRoute['role'],
  seeds: RouteSeedPoint[]
): SchematicRoute {
  const points = seeds.map((point) => ({
    ...point,
    angleDegrees: pointOnSeeds(seeds, point.sMeters)?.angleDegrees ?? 0
  }));
  return {
    id: branch.id,
    branchId: branch.id,
    label: branch.label,
    approach,
    role,
    points,
    d: points
      .map((point, index) => `${index === 0 ? 'M' : 'L'}${point.x} ${point.y}`)
      .join(' ')
  };
}

function branchCoordinates(
  branch: RiverCorridorBranch,
  project: ReturnType<typeof makeChannelProjector>
): number[] {
  const values = branch.stations.map((station) => station.sMeters);
  if (project) {
    for (const [latitude, longitude] of branch.centerline) {
      const fix = project(latitude, longitude);
      if (fix.branchId === branch.id) values.push(fix.sMeters);
    }
  }
  return finite(values);
}

interface MeasuredGuidePoint {
  x: number;
  y: number;
  distance: number;
}

function measureGuide(points: readonly { x: number; y: number }[]): MeasuredGuidePoint[] {
  let distance = 0;
  return points.map((point, index) => {
    if (index > 0) {
      distance += Math.hypot(point.x - points[index - 1].x, point.y - points[index - 1].y);
    }
    return { ...point, distance };
  });
}

function interpolateGuide(
  guide: MeasuredGuidePoint[],
  distance: number
): Pick<RouteSeedPoint, 'x' | 'y'> {
  const clamped = Math.min(Math.max(distance, 0), guide.at(-1)?.distance ?? 0);
  for (let index = 0; index + 1 < guide.length; index += 1) {
    const lower = guide[index];
    const upper = guide[index + 1];
    if (clamped > upper.distance) continue;
    const span = upper.distance - lower.distance || 1;
    const fraction = (clamped - lower.distance) / span;
    return {
      x: lower.x + (upper.x - lower.x) * fraction,
      y: lower.y + (upper.y - lower.y) * fraction
    };
  }
  const end = guide.at(-1) ?? BRICKELL_SCHEMATIC_POINT;
  return { x: end.x, y: end.y };
}

function interpolateCoordinate(
  anchors: Array<{ distance: number; sMeters: number }>,
  distance: number
): number {
  for (let index = 0; index + 1 < anchors.length; index += 1) {
    const lower = anchors[index];
    const upper = anchors[index + 1];
    if (distance > upper.distance) continue;
    const span = upper.distance - lower.distance || 1;
    return lower.sMeters + (upper.sMeters - lower.sMeters) * ((distance - lower.distance) / span);
  }
  return anchors.at(-1)?.sMeters ?? 0;
}

/**
 * Give each published upstream station the same amount of visual river. Guide
 * corners are inserted between station anchors, with their channel coordinate
 * interpolated, so every exposed segment remains horizontal or 45 degrees and
 * `schematicPointAt` still interpolates continuously in real river order.
 */
function upstreamRouteSeeds(
  branch: RiverCorridorBranch,
  targetS: number,
  upperS: number
): RouteSeedPoint[] {
  const stationCoordinates = [
    ...new Set(
      branch.stations
        .map((station) => station.sMeters)
        .filter((sMeters) => Number.isFinite(sMeters) && sMeters > targetS)
    )
  ].sort((left, right) => left - right);
  const guide = measureGuide(UPRIVER_GUIDE);
  const guideLength = guide.at(-1)?.distance ?? 0;
  const firstStationDistance = guide[FIRST_UPRIVER_STATION_GUIDE_INDEX]?.distance ?? 0;
  const lastStationS = stationCoordinates.at(-1);
  const hasTail = lastStationS == null || upperS - lastStationS > 1;
  const stationEndDistance = hasTail
    ? firstStationDistance +
      (guideLength - firstStationDistance) * (1 - UPRIVER_TAIL_FRACTION)
    : guideLength;
  const anchors: Array<{ distance: number; sMeters: number }> = [
    { distance: 0, sMeters: targetS }
  ];

  stationCoordinates.forEach((sMeters, index) => {
    const stationFraction = stationCoordinates.length === 1 ? 0 : index / (stationCoordinates.length - 1);
    anchors.push({
      distance:
        firstStationDistance + (stationEndDistance - firstStationDistance) * stationFraction,
      sMeters
    });
  });
  if (hasTail) {
    anchors.push({ distance: guideLength, sMeters: Math.max(upperS, targetS + 1) });
  }

  // Station distances win if one nearly coincides with a bend: this keeps the
  // station an exact route vertex while avoiding microscopic route segments.
  const distances = anchors.map((anchor) => anchor.distance);
  for (const point of guide) {
    if (!distances.some((distance) => Math.abs(distance - point.distance) < 0.001)) {
      distances.push(point.distance);
    }
  }

  return distances
    .sort((left, right) => left - right)
    .map((distance) => ({
      ...interpolateGuide(guide, distance),
      sMeters: interpolateCoordinate(anchors, distance)
    }));
}

function riverRoute(
  branch: RiverCorridorBranch,
  project: ReturnType<typeof makeChannelProjector>,
  targetS: number,
  mouthS: number
): SchematicRoute {
  const values = branchCoordinates(branch, project);
  const upper = Math.max(targetS + DEFAULT_UPRIVER_END_METERS, ...values, targetS + 1);
  return exposeRoute(branch, false, 'river', [
    { ...MOUTH_POINT, sMeters: mouthS },
    ...upstreamRouteSeeds(branch, targetS, upper)
  ]);
}

function knownApproachTemplate(branchId: string): {
  role: SchematicRoute['role'];
  points: Array<{ x: number; y: number }>;
} | null {
  if (branchId === 'north_approach') {
    return {
      role: 'north',
      points: [MOUTH_POINT, { x: 1_060, y: 250 }, { x: 1_160, y: 250 }, { x: 1_280, y: 130 }]
    };
  }
  if (branchId === 'government_cut') {
    return {
      role: 'east',
      points: [MOUTH_POINT, { x: 1_100, y: 320 }, { x: 1_280, y: 320 }]
    };
  }
  if (branchId === 'south_approach') {
    return {
      role: 'south',
      points: [MOUTH_POINT, { x: 1_060, y: 390 }, { x: 1_160, y: 390 }, { x: 1_280, y: 510 }]
    };
  }
  return null;
}

function genericApproachTemplate(index: number): {
  role: 'approach';
  points: Array<{ x: number; y: number }>;
} {
  // Unknown published branches still get a stable octolinear lane. The finite
  // slot list avoids deriving a visual claim from geographic bearing.
  const slots = [180, 460, 110, 530];
  const endY = slots[index % slots.length];
  const direction = Math.sign(endY - MOUTH_POINT.y) || 1;
  const laneY = MOUTH_POINT.y + direction * 70;
  return {
    role: 'approach',
    points: [
      MOUTH_POINT,
      { x: 1_060, y: laneY },
      { x: 1_160, y: laneY },
      { x: 1_280, y: laneY + direction * 120 }
    ]
  };
}

function approachRoute(
  branch: RiverCorridorBranch,
  project: ReturnType<typeof makeChannelProjector>,
  mouthS: number,
  genericIndex: number
): SchematicRoute {
  const template = knownApproachTemplate(branch.id) ?? genericApproachTemplate(genericIndex);
  const values = branchCoordinates(branch, project);
  const seawardS = Math.min(
    mouthS - 1,
    DEFAULT_APPROACH_END_METERS,
    ...(values.length ? values : [mouthS - 1])
  );
  const last = template.points.length - 1;
  const seeds = template.points.map((point, index) => ({
    ...point,
    // Geometry runs away from the mouth while the engine coordinate decreases.
    sMeters: mouthS + (seawardS - mouthS) * (index / Math.max(1, last))
  }));
  return exposeRoute(branch, true, template.role, seeds);
}

function hullSize(
  lengthMeters: number | undefined,
  beamMeters: number | undefined
): { hullLength: number; hullBeam: number; sizeKnown: boolean } {
  if (lengthMeters == null || !Number.isFinite(lengthMeters) || lengthMeters <= 0) {
    return { hullLength: DEFAULT_HULL_LENGTH, hullBeam: DEFAULT_HULL_BEAM, sizeKnown: false };
  }
  const scale = Math.sqrt(lengthMeters / REFERENCE_LENGTH_METERS);
  const hullLength = Math.min(40, Math.max(13, DEFAULT_HULL_LENGTH * scale));
  const ratio = beamMeters && beamMeters > 0 ? beamMeters / lengthMeters : 0.3;
  const hullBeam = Math.min(hullLength * 0.42, Math.max(4.5, hullLength * ratio * 1.15));
  return { hullLength, hullBeam, sizeKnown: true };
}

function wakeFor(
  vessel: ReachVessel,
  current: SchematicRoutePosition,
  routeById: Map<string, SchematicRoute>,
  project: ReturnType<typeof makeChannelProjector>
): SchematicTrailPoint[] {
  if (!project || vessel.direction === 'holding') return [];
  const stamped = vessel.track.points
    .map((point) => ({ point, atMs: Date.parse(point.observedAt) }))
    .filter(({ atMs }) => Number.isFinite(atMs))
    .sort((left, right) => left.atMs - right.atMs);
  const newestMs = stamped.at(-1)?.atMs ?? 0;
  const fixes = stamped
    .filter(({ atMs }) => newestMs - atMs <= WAKE_MAX_AGE_MS)
    .slice(-WAKE_MAX_POINTS);
  if (fixes.length < 2) return [];
  const oldestMs = fixes[0].atMs;
  const span = Math.max(1, newestMs - oldestMs);
  const wake = fixes.flatMap(({ point, atMs }) => {
    const projected = project(point.latitude, point.longitude);
    const route = routeById.get(projected.branchId);
    const position = route ? schematicPointAt(route, projected.sMeters) : null;
    return position
      ? [
          {
            x: position.x,
            y: position.y,
            observedAt: point.observedAt,
            freshness: (atMs - oldestMs) / span
          }
        ]
      : [];
  });
  if (wake.length === 0) return [];
  const last = wake.at(-1)!;
  if (Math.hypot(last.x - current.x, last.y - current.y) < 0.5) {
    Object.assign(last, {
      x: current.x,
      y: current.y,
      observedAt: vessel.track.observedAt,
      freshness: 1
    });
  } else {
    wake.push({
      x: current.x,
      y: current.y,
      observedAt: vessel.track.observedAt,
      freshness: 1
    });
  }
  return wake.length >= 2 ? wake : [];
}

function placeVessels(
  tracks: VesselTrack[],
  routes: SchematicRoute[],
  project: ReturnType<typeof makeChannelProjector>
): SchematicVessel[] {
  const routeById = new Map(routes.map((route) => [route.id, route]));
  return reachVessels(tracks)
    .filter((vessel) => vessel.underway)
    .flatMap((vessel) => {
      const routeId = vessel.track.branch ?? 'river';
      const route = routeById.get(routeId);
      if (!route) return [];
      const position = schematicPointAt(route, vessel.sMeters);
      if (!position) return [];
      const angleDegrees = normalizeDegrees(
        position.angleDegrees + (vessel.direction === 'downriver' ? 180 : 0)
      );
      const track = vessel.track;
      return [
        {
          ...vessel,
          routeId,
          x: position.x,
          y: position.y,
          angleDegrees,
          wake: wakeFor(vessel, position, routeById, project),
          reportedX: position.x, reportedY: position.y,
          displaySMeters: vessel.sMeters, estimated: false, estimatePath: '',
          vesselName: track.vesselName,
          movement: track.movement,
          routeIntersects: track.routeIntersects,
          courseDegrees: track.courseDegrees,
          observedAt: track.observedAt,
          posture: track.posture,
          branch: track.branch,
          callSign: track.callSign,
          imoNumber: track.imoNumber,
          destination: track.destination,
          lengthMeters: track.lengthMeters,
          beamMeters: track.beamMeters,
          draughtMeters: track.draughtMeters,
          openingPropensity: track.openingPropensity,
          etaMinMinutes: track.etaMinMinutes,
          etaMaxMinutes: track.etaMaxMinutes,
          scheduleExempt: track.scheduleExempt === true,
          predictedOpeningAt: track.predictedOpeningAt,
          waitsForSlot: track.waitsForSlot === true,
          ...hullSize(track.lengthMeters, track.beamMeters)
        }
      ];
    });
}

/**
 * Project the engine's fixed Brickell corridor onto an octolinear transit map.
 * This function is deterministic and has no clock: a snapshot in produces a
 * schematic out, with no forecasting or dead reckoning added by the surface.
 */
export function riverSchematic(
  corridor: RiverCorridor,
  vesselTracks: VesselTrack[],
  bridgeStates: Map<string, SchematicBridgeState>,
  towGroups: PublicVesselGroup[] = []
): RiverSchematic {
  const trunk = corridor.branches.find((branch) => branch.id === 'river');
  if (!trunk) {
    return {
      width: RIVER_SCHEMATIC_WIDTH,
      height: RIVER_SCHEMATIC_HEIGHT,
      routes: [],
      stations: [],
      vessels: [],
      target: null
    };
  }

  const project = makeChannelProjector(corridor);
  const targetSource = trunk.stations.find((station) => station.kind === 'target') ?? null;
  const targetS = targetSource?.sMeters ?? 0;
  const mouthSource = trunk.stations.find((station) => station.kind === 'mouth') ?? null;
  const mouthS = mouthSource?.sMeters ?? DEFAULT_MOUTH_METERS;

  const routes = [riverRoute(trunk, project, targetS, mouthS)];
  let genericIndex = 0;
  for (const branch of corridor.branches) {
    if (branch.id === 'river') continue;
    routes.push(approachRoute(branch, project, mouthS, genericIndex));
    if (!knownApproachTemplate(branch.id)) genericIndex += 1;
  }

  const routeById = new Map(routes.map((route) => [route.id, route]));
  const stations: SchematicStation[] = [];
  for (const branch of corridor.branches) {
    const route = routeById.get(branch.id);
    if (!route) continue;
    branch.stations.forEach((station, index) => {
      // Every approach repeats the mouth; the trunk owns the one interchange.
      if (branch.id !== 'river' && station.kind === 'mouth') return;
      const isTarget = branch.id === 'river' && station === targetSource;
      const position = isTarget
        ? schematicPointAt(route, targetS)
        : schematicPointAt(route, station.sMeters);
      if (!position) return;
      stations.push({
        ...station,
        branchId: branch.id,
        routeId: route.id,
        x: isTarget ? BRICKELL_SCHEMATIC_POINT.x : position.x,
        y: isTarget ? BRICKELL_SCHEMATIC_POINT.y : position.y,
        angleDegrees: position.angleDegrees,
        state: station.bridgeKey ? bridgeStates.get(station.bridgeKey) : undefined,
        isTarget,
        labelSide: branch.id === 'river' ? (index % 2 === 0 ? -1 : 1) : route.role === 'south' ? 1 : -1
      });
    });
  }

  const target = stations.find((station) => station.isTarget) ?? null;
  return {
    width: RIVER_SCHEMATIC_WIDTH,
    height: RIVER_SCHEMATIC_HEIGHT,
    routes,
    stations,
    vessels: layoutSchematicHulls(placeVessels(vesselTracks, routes, project), routes, towGroups),
    target
  };
}

/** Footprint spacing is cartographic displacement, never an AIS coordinate. */
function layoutSchematicHulls(vessels: SchematicVessel[], routes: SchematicRoute[], groups: PublicVesselGroup[]): SchematicVessel[] {
  const byId = new Map(vessels.map((vessel) => [vessel.mmsi, vessel]));
  const membership = new Map<string, { formation: string; order: number; desired: number }>();
  for (const group of groups) {
    const members = [...group.tugIds, ...group.towIds].map((id) => byId.get(id)).filter((v): v is SchematicVessel => !!v);
    if (members.length < 2) continue;
    const freshest = members.reduce((a, b) => a.observedAtMs >= b.observedAtMs ? a : b);
    const offsets = group.memberOffsetsMeters;
    members.sort((a, b) => (offsets?.[a.mmsi] ?? a.sMeters) - (offsets?.[b.mmsi] ?? b.sMeters) || a.mmsi.localeCompare(b.mmsi));
    members.forEach((member, order) => membership.set(member.mmsi, { formation: group.id, order,
      desired: offsets?.[member.mmsi] !== undefined && offsets[freshest.mmsi] !== undefined
        ? freshest.sMeters + offsets[member.mmsi]! - offsets[freshest.mmsi]! : member.sMeters }));
  }
  const byRoute = new Map(routes.map((route) => [route.id, route]));
  const sorted = [...vessels].sort((a, b) => b.observedAtMs - a.observedAtMs || a.mmsi.localeCompare(b.mmsi));
  const placed = separateRouteHulls(sorted.flatMap((vessel) => {
    const route = byRoute.get(vessel.routeId);
    if (!route) return [];
    let minimum = Math.min(...route.points.map((point) => point.sMeters));
    let maximum = Math.max(...route.points.map((point) => point.sMeters));
    if (vessel.sMeters < 0) maximum = Math.min(maximum, -2);
    else minimum = Math.max(minimum, 2);
    return [{ id: vessel.mmsi, sMeters: membership.get(vessel.mmsi)?.desired ?? vessel.sMeters,
      minimum, maximum, radius: Math.hypot(Math.max(48, Math.min(68, vessel.hullLength * 2.5)), 42) / 2 + 12,
      ...membership.get(vessel.mmsi), pointAt: (s: number) => schematicPointAt(route, s)! }];
  }), 5);
  return vessels.map((vessel) => {
    const route = byRoute.get(vessel.routeId)!;
    const s = placed.get(vessel.mmsi) ?? vessel.sMeters;
    const point = schematicPointAt(route, s)!;
    const estimated = Math.hypot(point.x - vessel.reportedX, point.y - vessel.reportedY) > 0.5;
    const points = [vessel.sMeters, ...route.points.map((p) => p.sMeters).filter((measure) =>
      measure > Math.min(s, vessel.sMeters) && measure < Math.max(s, vessel.sMeters)), s].sort((a, b) => a - b);
    return { ...vessel, x: point.x, y: point.y, displaySMeters: s, estimated,
      angleDegrees: normalizeDegrees(point.angleDegrees + (vessel.direction === 'downriver' ? 180 : 0)),
      estimatePath: points.map((measure, index) => { const p = schematicPointAt(route, measure)!;
        return `${index === 0 ? 'M' : 'L'}${p.x} ${p.y}`; }).join(' '),
      towGroupId: membership.get(vessel.mmsi)?.formation };
  });
}
