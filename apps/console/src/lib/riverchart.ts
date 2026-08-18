/**
 * The corridor as a plan-view chart, drawn from the engine's own geometry.
 *
 * This replaces the serpentine transit diagram: the river is drawn in its real
 * shape — the actual bends of the Miami River, the fork at Brickell Point, the
 * Main Channel reaching for Government Cut — with Brickell Avenue Bridge at
 * the origin of the sheet. Every centerline the engine publishes is smoothed
 * through its own waypoints, so there are no hard corners that the water
 * itself does not have.
 *
 * Scale is honest but not uniform. Within `FOCUS_METERS` of the span the chart
 * is true to scale; beyond it, distance compresses logarithmically toward the
 * edges. The compression is *radial about the bridge*, which keeps three
 * things true at once: every bearing from the span is real, the range rings
 * stay perfect circles, and no amount of compression can fold one channel
 * across another. The rings are labeled in real kilometres, so the compression
 * is visible rather than hidden.
 */
import type { RiverCorridor, RiverStation, VesselTrack } from './types';
import {
  isOpener,
  makeChannelProjector,
  reachVessels,
  type ReachVessel
} from './river';

/** ViewBox width in user units; height follows the warped geometry. */
const CHART_WIDTH = 1000;
/** Sheet margin so edge labels are never clipped. */
const PADDING = 66;
/** Radius of true scale about the span, in real metres. */
const FOCUS_METERS = 700;
/** Softness of the logarithmic compression beyond the focus, in metres. */
const COMPRESS_METERS = 1150;
/** Real metres between smoothed samples; small enough that bends stay round. */
const SAMPLE_METERS = 35;
/** Range rings, in real kilometres from the span. */
const RING_KILOMETERS = [1, 2, 4];

/** Drawn-space distance within which vessels share one stacked label column. */
const CLUSTER_RADIUS = 62;
/** Row height of a stacked cluster sharing one label column. */
const STACK_STEP = 30;
/** Where a stack's first row begins below its anchor. */
const STACK_TOP = 24;

/** Drawn hull length for a vessel that has reported no dimensions. */
const DEFAULT_HULL_LENGTH = 17;
const DEFAULT_HULL_BEAM = 6.5;
/** Reference hull: a 30 m river tug draws at the default size. */
const REFERENCE_LENGTH_METERS = 30;

/** Trail fixes older than this are not drawn as wake. */
const WAKE_MAX_AGE_MS = 30 * 60_000;
const WAKE_MAX_POINTS = 18;

export interface ChartStation extends RiverStation {
  branchId: string;
  x: number;
  y: number;
  /** Drawn channel tangent at the station, in degrees. */
  angleDegrees: number;
  /** Live bascule state, joined by FL511 key. */
  state?: 'up' | 'down' | 'unknown';
  isTarget: boolean;
  /** Where the name sits, offset off the water along the channel's normal. */
  labelX: number;
  labelY: number;
  labelAnchor: 'start' | 'middle' | 'end';
  /** Drawn half-width of the ribbon here, for sizing a span mark. */
  halfWidth: number;
}

export interface WakePoint {
  x: number;
  y: number;
  /** 0 at the oldest drawn fix, 1 at the hull. */
  freshness: number;
}

export interface ChartVessel extends ReachVessel {
  x: number;
  y: number;
  /** Direction of travel along the drawn channel, in degrees. */
  angleDegrees: number;
  /** Rank within a cluster sharing one label column. */
  stackIndex: number;
  stackSize: number;
  /** True when this hull shares a label column with its neighbours. */
  stackedColumn: boolean;
  /** Where the label block starts relative to the mark, after fanning. */
  labelY: number;
  /** Recent fixes drawn as a fading wake, oldest first, hull position last. */
  wake: WakePoint[];
  etaMinMinutes?: number;
  etaMaxMinutes?: number;
  lengthMeters?: number;
  beamMeters?: number;
  draughtMeters?: number;
  callSign?: string;
  imoNumber?: number;
  destination?: string;
  /** Drawn hull length in user units, square-root scaled from real metres. */
  hullLength: number;
  hullBeam: number;
  /** Whether that size came from a static report or is the neutral default. */
  sizeKnown: boolean;
  scheduleExempt: boolean;
  predictedOpeningAt?: string;
  waitsForSlot: boolean;
}

export interface ChartRing {
  kilometers: number;
  /** Drawn radius about the span. */
  radius: number;
  labelX: number;
  labelY: number;
}

export interface ChartBranch {
  id: string;
  /** Closed ribbon outline of the tracked water. */
  ribbon: string;
  /** The channel centerline, for the dashed chart rule. */
  centerline: string;
  approach: boolean;
}

export interface RiverChart {
  width: number;
  height: number;
  /** Drawn position of the target span — the origin everything rings around. */
  bridgeX: number;
  bridgeY: number;
  branches: ChartBranch[];
  rings: ChartRing[];
  stations: ChartStation[];
  /** Only traffic on passage; berthed hulls are not drawn and not counted. */
  vessels: ChartVessel[];
  /** Where to set the bay's name, in the open water between the approaches. */
  bayLabel: { x: number; y: number } | null;
}

interface DensePoint {
  /** Warped position in metre space, east/north of the span (north positive). */
  x: number;
  y: number;
  /** Engine channel coordinate at this point. */
  sMeters: number;
  /** Drawn ribbon half-width in warped metres. */
  halfWidth: number;
}

interface DenseBranch {
  id: string;
  approach: boolean;
  points: DensePoint[];
  /** Dense index of each original waypoint, for exact station placement. */
  waypointIndex: number[];
}

/** Radial compression: true inside the focus, logarithmic beyond it. */
function warpDistance(meters: number): number {
  if (meters <= FOCUS_METERS) return meters;
  return FOCUS_METERS + COMPRESS_METERS * Math.log1p((meters - FOCUS_METERS) / COMPRESS_METERS);
}

/** d(warp)/d(meters): how much the chart shrinks the water at this range. */
function warpGain(meters: number): number {
  if (meters <= FOCUS_METERS) return 1;
  return COMPRESS_METERS / (COMPRESS_METERS + meters - FOCUS_METERS);
}

/**
 * Centripetal Catmull-Rom through the waypoints, sampled densely.
 *
 * Centripetal parameterisation is the variant that cannot loop or overshoot
 * between unevenly spaced control points, which the downtown cluster is.
 * The curve passes through every waypoint, so stations — which are waypoints —
 * sit exactly on the drawn water.
 */
function smoothPolyline(
  waypoints: { x: number; y: number }[]
): { points: { x: number; y: number }[]; waypointIndex: number[] } {
  if (waypoints.length < 2) {
    return { points: waypoints.slice(), waypointIndex: waypoints.map((_, i) => i) };
  }
  const pts = [waypoints[0], ...waypoints, waypoints[waypoints.length - 1]];
  const points: { x: number; y: number }[] = [];
  const waypointIndex: number[] = [];

  const alpha = 0.5;
  const knot = (a: { x: number; y: number }, b: { x: number; y: number }) =>
    Math.max(1e-6, Math.hypot(b.x - a.x, b.y - a.y) ** alpha);

  for (let i = 1; i + 2 < pts.length; i += 1) {
    const [p0, p1, p2, p3] = [pts[i - 1], pts[i], pts[i + 1], pts[i + 2]];
    waypointIndex.push(points.length);
    const t0 = 0;
    const t1 = t0 + knot(p0, p1);
    const t2 = t1 + knot(p1, p2);
    const t3 = t2 + knot(p2, p3);
    const segmentMeters = Math.hypot(p2.x - p1.x, p2.y - p1.y);
    const steps = Math.max(1, Math.ceil(segmentMeters / SAMPLE_METERS));
    for (let step = 0; step < steps; step += 1) {
      const t = t1 + ((t2 - t1) * step) / steps;
      const lerp = (
        a: { x: number; y: number },
        b: { x: number; y: number },
        ta: number,
        tb: number
      ) => {
        const u = tb - ta < 1e-9 ? 0 : (t - ta) / (tb - ta);
        return { x: a.x + (b.x - a.x) * u, y: a.y + (b.y - a.y) * u };
      };
      const a1 = lerp(p0, p1, t0, t1);
      const a2 = lerp(p1, p2, t1, t2);
      const a3 = lerp(p2, p3, t2, t3);
      const b1 = lerp(a1, a2, t0, t2);
      const b2 = lerp(a2, a3, t1, t3);
      points.push(lerp(b1, b2, t1, t2));
    }
  }
  waypointIndex.push(points.length);
  points.push(pts[pts.length - 1]);
  return { points, waypointIndex };
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

export function riverChart(
  corridor: RiverCorridor,
  vesselTracks: VesselTrack[],
  bridgeStates: Map<string, 'up' | 'down' | 'unknown'>
): RiverChart {
  const trunkBranch = corridor.branches.find((branch) => branch.id === 'river');
  if (!trunkBranch || trunkBranch.centerline.length < 2) {
    return {
      width: CHART_WIDTH,
      height: 320,
      bridgeX: CHART_WIDTH / 2,
      bridgeY: 160,
      branches: [],
      rings: [],
      stations: [],
      vessels: [],
      bayLabel: null
    };
  }

  const bridgeLat = corridor.bridgeLatitude;
  const bridgeLon = corridor.bridgeLongitude;
  const metersPerDegLat = 110_540;
  const metersPerDegLon = 111_320 * Math.cos((bridgeLat * Math.PI) / 180);
  /** Local tangent plane: metres east and north of the span. */
  const toLocal = (latitude: number, longitude: number) => ({
    x: (longitude - bridgeLon) * metersPerDegLon,
    y: (latitude - bridgeLat) * metersPerDegLat
  });
  const warpPoint = (p: { x: number; y: number }) => {
    const d = Math.hypot(p.x, p.y);
    if (d < 1e-9) return { x: 0, y: 0 };
    const w = warpDistance(d) / d;
    return { x: p.x * w, y: p.y * w };
  };

  const [mouth, span] = trunkBranch.centerline;
  const mouthLocal = toLocal(mouth[0], mouth[1]);
  const spanLocal = toLocal(span[0], span[1]);
  const mouthToSpanMeters = Math.hypot(spanLocal.x - mouthLocal.x, spanLocal.y - mouthLocal.y);

  // Smooth, measure, and warp every branch the engine publishes.
  const dense: DenseBranch[] = corridor.branches
    .filter((branch) => branch.centerline.length >= 2)
    .map((branch) => {
      const locals = branch.centerline.map(([lat, lon]) => toLocal(lat, lon));
      const smoothed = smoothPolyline(locals);
      // Real arc metres along the smoothed line, for the channel coordinate.
      const arcs: number[] = [0];
      for (let i = 1; i < smoothed.points.length; i += 1) {
        const a = smoothed.points[i - 1];
        const b = smoothed.points[i];
        arcs.push(arcs[i - 1] + Math.hypot(b.x - a.x, b.y - a.y));
      }
      const isTrunk = branch.id === 'river';
      // Trunk: s = arc − (arc at the span waypoint), so the span reads zero.
      // Approaches: the mouth already sits mouth-to-span short of the bridge
      // and going seaward moves further away — the engine's own convention.
      const spanArc = isTrunk ? arcs[smoothed.waypointIndex[1] ?? 0] : 0;
      const points: DensePoint[] = smoothed.points.map((point, i) => {
        const range = Math.hypot(point.x, point.y);
        return {
          ...warpPoint(point),
          sMeters: isTrunk ? arcs[i] - spanArc : -mouthToSpanMeters - arcs[i],
          halfWidth: branch.corridorOffsetMeters * warpGain(range)
        };
      });
      return {
        id: branch.id,
        approach: !isTrunk,
        points,
        waypointIndex: smoothed.waypointIndex
      };
    });

  // Fit the water onto the sheet at one uniform scale. Rings are not part of
  // the fit: where a ring runs off the drawn water it is clipped by the sheet
  // edge, exactly as on a paper chart.
  const ringRadii = RING_KILOMETERS.map((km) => warpDistance(km * 1000));
  let minX = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  for (const branch of dense) {
    for (const point of branch.points) {
      const reach = point.halfWidth;
      if (point.x - reach < minX) minX = point.x - reach;
      if (point.x + reach > maxX) maxX = point.x + reach;
      if (point.y - reach < minY) minY = point.y - reach;
      if (point.y + reach > maxY) maxY = point.y + reach;
    }
  }
  const scale = (CHART_WIDTH - PADDING * 2) / Math.max(1, maxX - minX);
  const height = Math.ceil((maxY - minY) * scale + PADDING * 2);
  /** Warped metres → sheet units. North is up, so y flips. */
  const place = (point: { x: number; y: number }) => ({
    x: PADDING + (point.x - minX) * scale,
    y: PADDING + (maxY - point.y) * scale
  });
  const bridgeSheet = place({ x: 0, y: 0 });

  const branches: ChartBranch[] = dense.map((branch) => {
    const sheet = branch.points.map((point) => ({ ...place(point), hw: point.halfWidth * scale }));
    // Average adjacent segment normals so the ribbon stays even through bends.
    const normals: { x: number; y: number }[] = [];
    for (let i = 0; i + 1 < sheet.length; i += 1) {
      const dx = sheet[i + 1].x - sheet[i].x;
      const dy = sheet[i + 1].y - sheet[i].y;
      const len = Math.hypot(dx, dy) || 1;
      normals.push({ x: -dy / len, y: dx / len });
    }
    const left: string[] = [];
    const right: string[] = [];
    for (let i = 0; i < sheet.length; i += 1) {
      const before = normals[i - 1];
      const after = normals[i];
      let nx = (before?.x ?? after?.x ?? 0) + (after?.x ?? before?.x ?? 0);
      let ny = (before?.y ?? after?.y ?? 0) + (after?.y ?? before?.y ?? 0);
      const len = Math.hypot(nx, ny) || 1;
      nx /= len;
      ny /= len;
      const hw = Math.max(3.5, sheet[i].hw);
      left.push(`${(sheet[i].x + nx * hw).toFixed(1)} ${(sheet[i].y + ny * hw).toFixed(1)}`);
      right.push(`${(sheet[i].x - nx * hw).toFixed(1)} ${(sheet[i].y - ny * hw).toFixed(1)}`);
    }
    right.reverse();
    return {
      id: branch.id,
      approach: branch.approach,
      ribbon: `M${left.join(' L')} L${right.join(' L')} Z`,
      centerline: sheet
        .map((point, i) => `${i === 0 ? 'M' : 'L'}${point.x.toFixed(1)} ${point.y.toFixed(1)}`)
        .join(' ')
    };
  });

  // Labels ride each ring toward the sheet's open lower-left water.
  const rings: ChartRing[] = ringRadii.map((radius, index) => {
    const r = radius * scale;
    return {
      kilometers: RING_KILOMETERS[index],
      radius: r,
      labelX: bridgeSheet.x - r * Math.SQRT1_2,
      labelY: bridgeSheet.y + r * Math.SQRT1_2
    };
  });

  /** Nearest dense point to a channel coordinate on one branch. */
  const branchById = new Map(dense.map((branch) => [branch.id, branch]));
  const pointAt = (
    branchId: string | undefined,
    sMeters: number
  ): { x: number; y: number; angle: number } | null => {
    const branch = branchById.get(branchId ?? 'river') ?? branchById.get('river');
    if (!branch || branch.points.length < 2) return null;
    const pts = branch.points;
    const ascending = pts[pts.length - 1].sMeters >= pts[0].sMeters;
    let lo = 0;
    let hi = pts.length - 1;
    const clamped = Math.min(
      Math.max(sMeters, Math.min(pts[0].sMeters, pts[hi].sMeters)),
      Math.max(pts[0].sMeters, pts[hi].sMeters)
    );
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1;
      if (ascending === (clamped >= pts[mid].sMeters)) lo = mid;
      else hi = mid;
    }
    const a = pts[lo];
    const b = pts[hi];
    const span = b.sMeters - a.sMeters || 1;
    const u = Math.min(1, Math.max(0, (clamped - a.sMeters) / span));
    const pos = place({ x: a.x + (b.x - a.x) * u, y: a.y + (b.y - a.y) * u });
    const pa = place(a);
    const pb = place(b);
    // Drawn tangent toward increasing s — upriver on every branch.
    const upriver = ascending
      ? Math.atan2(pb.y - pa.y, pb.x - pa.x)
      : Math.atan2(pa.y - pb.y, pa.x - pb.x);
    return { ...pos, angle: (upriver * 180) / Math.PI };
  };

  // Stations sit exactly on their waypoint, which the smoothing passes through.
  const stations: ChartStation[] = [];
  for (const branch of corridor.branches) {
    const denseBranch = branchById.get(branch.id);
    if (!denseBranch) continue;
    branch.stations.forEach((station) => {
      // Every branch names the mouth; the trunk draws it once for all three.
      if (station.kind === 'mouth' && branch.id !== 'river') return;
      const at = pointAt(branch.id, station.sMeters);
      if (!at) return;
      const local = toLocal(station.latitude, station.longitude);
      const sheet = place(warpPoint(local));
      const halfWidth = Math.max(
        3.5,
        branch.corridorOffsetMeters * warpGain(Math.hypot(local.x, local.y)) * scale
      );
      const isTarget = station.kind === 'target';
      // The label clears the water along the channel's own normal, so a name
      // beside a vertical reach steps sideways rather than into the channel.
      const normalRadians = ((at.angle + 90) * Math.PI) / 180;
      let nx = Math.cos(normalRadians);
      let ny = Math.sin(normalRadians);
      const side = labelSideFor(station, branch.id, { x: nx, y: ny }, sheet, bridgeSheet);
      nx *= side;
      ny *= side;
      const distance = halfWidth + (isTarget ? 24 : 13);
      const labelX = sheet.x + nx * distance;
      const labelY = sheet.y + ny * distance + (ny > 0.3 ? 9 : ny > -0.3 ? 4 : 0);
      const labelAnchor: 'start' | 'middle' | 'end' =
        isTarget || Math.abs(nx) < 0.45 ? 'middle' : nx > 0 ? 'start' : 'end';
      stations.push({
        ...station,
        branchId: branch.id,
        x: sheet.x,
        y: sheet.y,
        angleDegrees: at.angle,
        state: station.bridgeKey ? bridgeStates.get(station.bridgeKey) : undefined,
        isTarget,
        labelX,
        labelY,
        labelAnchor,
        halfWidth
      });
    });
  }

  const project = makeChannelProjector(corridor);
  const vessels = layOutVessels(
    reachVessels(vesselTracks).filter((vessel) => vessel.underway),
    pointAt,
    project
  );

  // The bay's name sits in the open water between the two entrance channels,
  // pulled a little toward the span so it stays clear of the sheet edge.
  const seawardMost = (branchId: string) => {
    const branch = branchById.get(branchId);
    return branch ? branch.points[branch.points.length - 1] : null;
  };
  const northEnd = seawardMost('north_approach');
  const southEnd = seawardMost('south_approach');
  const bayLabel =
    northEnd && southEnd
      ? (() => {
          const mid = place({
            x: ((northEnd.x + southEnd.x) / 2) * 0.72,
            y: ((northEnd.y + southEnd.y) / 2) * 0.72
          });
          return { x: mid.x, y: mid.y };
        })()
      : null;

  return {
    width: CHART_WIDTH,
    height,
    bridgeX: bridgeSheet.x,
    bridgeY: bridgeSheet.y,
    branches,
    rings,
    stations,
    vessels,
    bayLabel
  };
}

/**
 * Which side of the channel a station's label sits on, as a sign applied to
 * the drawn normal.
 *
 * Chosen from the real geography rather than alternation: the bay approaches
 * label on their outer (seaward) side where the sheet is open, and the tight
 * downtown pairs are curated apart so no two names share a baseline.
 */
function labelSideFor(
  station: RiverStation,
  branchId: string,
  normal: { x: number; y: number },
  sheet: { x: number; y: number },
  bridge: { x: number; y: number }
): -1 | 1 {
  // The north approach labels on its northern bank, where the sheet is empty;
  // outboard alone would push Bayfront into the water the traffic occupies.
  if (branchId === 'north_approach') return normal.y > 0 ? -1 : 1;
  if (branchId !== 'river') {
    // Outboard of the channel: away from the span, into open water or paper.
    const outward = normal.x * (sheet.x - bridge.x) + normal.y * (sheet.y - bridge.y);
    return outward >= 0 ? 1 : -1;
  }
  const below = new Set(['S Miami Ave', 'SW 1 St', 'NW 22 Ave', 'NW 27 Ave']);
  const wantBelow = below.has(station.label);
  // The normal's own orientation varies along the river; pick the sign that
  // actually lands the label on the wanted bank.
  const sign: -1 | 1 = normal.y > 0 ? 1 : -1;
  return wantBelow ? sign : ((-sign) as -1 | 1);
}

function layOutVessels(
  vessels: ReachVessel[],
  pointAt: (branchId: string | undefined, s: number) => { x: number; y: number; angle: number } | null,
  project: ReturnType<typeof makeChannelProjector>
): ChartVessel[] {
  const placed = vessels.flatMap((vessel) => {
    const spot = pointAt(vessel.track.branch, vessel.sMeters);
    if (!spot) return [];
    // The mark points the way the hull is travelling along the drawn water.
    const angleDegrees =
      vessel.direction === 'downriver' ? spot.angle + 180 : spot.angle;
    const wake: WakePoint[] = [];
    if (project && vessel.direction !== 'holding') {
      // Age is measured against the track's own newest fix, not the wall
      // clock, so the drawing is a pure function of the snapshot it was given.
      const stamped = vessel.track.points
        .map((point) => ({ point, atMs: Date.parse(point.observedAt) }))
        .filter(({ atMs }) => Number.isFinite(atMs))
        .sort((a, b) => a.atMs - b.atMs);
      const newestMs = stamped[stamped.length - 1]?.atMs ?? 0;
      const fixes = stamped
        .filter(({ atMs }) => newestMs - atMs <= WAKE_MAX_AGE_MS)
        .slice(-WAKE_MAX_POINTS);
      if (fixes.length >= 2) {
        const oldest = fixes[0].atMs;
        const newest = fixes[fixes.length - 1].atMs;
        const ageSpan = Math.max(1, newest - oldest);
        for (const { point, atMs } of fixes) {
          const fix = project(point.latitude, point.longitude);
          const at = pointAt(fix.branchId, fix.sMeters);
          if (at) wake.push({ x: at.x, y: at.y, freshness: (atMs - oldest) / ageSpan });
        }
        // The wake ends at the hull itself, wherever the last fix projected.
        wake.push({ x: spot.x, y: spot.y, freshness: 1 });
      }
    }
    return [{ ...vessel, x: spot.x, y: spot.y, angleDegrees, wake }];
  });

  // Fan anything sharing a spot, so five hulls in one basin read as a list.
  const clusters: (typeof placed)[] = [];
  for (const vessel of placed) {
    const cluster = clusters.find((candidate) =>
      candidate.some((other) => Math.hypot(other.x - vessel.x, other.y - vessel.y) <= CLUSTER_RADIUS)
    );
    if (cluster) cluster.push(vessel);
    else clusters.push([vessel]);
  }

  return clusters.flatMap((cluster) => {
    const anchorX = cluster.reduce((total, vessel) => total + vessel.x, 0) / cluster.length;
    const anchorY = cluster.reduce((total, vessel) => total + vessel.y, 0) / cluster.length;
    return cluster.map((vessel, index) => {
      const stacked = cluster.length > 1;
      return {
        ...vessel,
        x: stacked ? anchorX : vessel.x,
        y: stacked ? anchorY + STACK_TOP + index * STACK_STEP : vessel.y,
        wake: stacked ? [] : vessel.wake,
        stackedColumn: stacked,
        stackIndex: index,
        stackSize: cluster.length,
        labelY: stacked ? 4 : 19,
        etaMinMinutes: vessel.track.etaMinMinutes,
        etaMaxMinutes: vessel.track.etaMaxMinutes,
        lengthMeters: vessel.track.lengthMeters,
        beamMeters: vessel.track.beamMeters,
        draughtMeters: vessel.track.draughtMeters,
        callSign: vessel.track.callSign,
        imoNumber: vessel.track.imoNumber,
        destination: vessel.track.destination,
        ...hullSize(vessel.track.lengthMeters, vessel.track.beamMeters),
        scheduleExempt: vessel.track.scheduleExempt === true,
        predictedOpeningAt: vessel.track.predictedOpeningAt,
        waitsForSlot: vessel.track.waitsForSlot === true
      };
    });
  });
}

export { isOpener };
