/**
 * Reading the AIS corridor for display.
 *
 * The engine already decides what a vessel is doing; this module only turns
 * those decisions into the quantities a reach drawing needs. It deliberately
 * re-derives nothing the engine publishes — a surface that invented its own
 * moored threshold or its own idea of "closing" would eventually disagree with
 * the prediction sitting directly above it on the same page.
 */
import type { RiverCorridor, VesselTrack } from './types';

/**
 * Displacement floor separating a vessel on passage from a hull tied up beside
 * the channel, in metres. This mirrors the collector's own moored test, and is
 * used only as a fallback for a track that reached us without a posture.
 */
export const UNDERWAY_DISPLACEMENT_METERS = 50;

/**
 * Ledger confidence at which a hull is named an opener, in basis points.
 *
 * The engine smooths one observed opening to ~6700 and never to 10000, so this
 * admits a hull seen opening the span once and excludes one seen fitting under.
 */
export const OPENER_PROPENSITY_BASIS_POINTS = 6_000;

/** Channel metres shown either side of the span before the axis is stretched. */
export const REACH_UPRIVER_METERS = 3_500;
export const REACH_SEAWARD_METERS = 2_500;

/**
 * A hull belongs on the Live schematic only while the collector would still
 * call its position current. The Map deliberately keeps the full one-hour
 * discovery trail; this cutoff prevents that archive from masquerading as
 * present movement on the bridge decision surface.
 */
export const LIVE_VESSEL_FRESHNESS_MS = 6 * 60 * 1_000;
const LIVE_VESSEL_FUTURE_SKEW_MS = 30 * 1_000;

export type TravelDirection = 'upriver' | 'downriver' | 'holding';

export interface ReachVessel {
  track: VesselTrack;
  mmsi: string;
  /**
   * Best identity the hull has broadcast: its name, else its call sign, else
   * the MMSI. Never empty, and never a placeholder.
   */
  label: string;
  vesselClass?: string;
  /** Signed channel metres: positive upriver of the span, negative seaward. */
  sMeters: number;
  /** Channel metres to the span, regardless of side. */
  distanceMeters: number;
  speedKnots: number;
  direction: TravelDirection;
  /**
   * Whether this vessel is actually on passage, as opposed to lying at a berth
   * or holding station. Idle traffic is still shown — a river with five hulls
   * on it must never draw as empty — but it is drawn quietly.
   */
  underway: boolean;
  /** True when this hull's own history says it forces an opening. */
  opener: boolean;
  /** The engine's judgement that this course crosses the span. */
  closing: boolean;
  observedAtMs: number;
}

function toMilliseconds(value: string): number {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? 0 : parsed;
}

export function currentVesselTracks(
  tracks: VesselTrack[],
  generatedAt: string | undefined
): VesselTrack[] {
  const generatedAtMs = generatedAt ? toMilliseconds(generatedAt) : 0;
  if (!generatedAtMs) return tracks;
  const cutoff = generatedAtMs - LIVE_VESSEL_FRESHNESS_MS;
  const latest = generatedAtMs + LIVE_VESSEL_FUTURE_SKEW_MS;
  return tracks.filter((track) => {
    const observedAtMs = toMilliseconds(track.observedAt);
    return observedAtMs >= cutoff && observedAtMs <= latest;
  });
}

/** Great-circle metres; the reach spans a few kilometres, so this is exact enough. */
export function metersBetween(
  fromLatitude: number,
  fromLongitude: number,
  toLatitude: number,
  toLongitude: number
): number {
  const EARTH_RADIUS_METERS = 6_371_000;
  const toRadians = (degrees: number) => (degrees * Math.PI) / 180;
  const deltaLatitude = toRadians(toLatitude - fromLatitude);
  const deltaLongitude = toRadians(toLongitude - fromLongitude);
  const haversine =
    Math.sin(deltaLatitude / 2) ** 2 +
    Math.cos(toRadians(fromLatitude)) *
      Math.cos(toRadians(toLatitude)) *
      Math.sin(deltaLongitude / 2) ** 2;
  return 2 * EARTH_RADIUS_METERS * Math.asin(Math.min(1, Math.sqrt(haversine)));
}

/**
 * How far the vessel has ranged from where it now sits, across the retained
 * track. This is the complement of the collector's moored test — that test
 * asks whether every point stayed inside the threshold — so the two agree on
 * the same hull.
 */
export function trackDisplacementMeters(track: VesselTrack): number {
  const latest = track.points.at(-1);
  if (!latest) return 0;
  let widest = 0;
  for (const point of track.points) {
    const spread = metersBetween(
      latest.latitude,
      latest.longitude,
      point.latitude,
      point.longitude
    );
    if (spread > widest) widest = spread;
  }
  return widest;
}

/**
 * Whether this vessel is doing something worth drawing.
 *
 * `underway` and `waiting` are the two postures that bear on an opening: one is
 * a hull on passage, the other a hull stopped at the span for a lift. Moored,
 * off-channel and deep-draft vessels are the noise this filter exists to
 * remove. A track with no posture at all predates the field, so it falls back
 * to the displacement floor the posture itself was derived from.
 */
export function isUnderway(track: VesselTrack): boolean {
  if (track.posture) return track.posture === 'underway' || track.posture === 'waiting';
  return trackDisplacementMeters(track) > UNDERWAY_DISPLACEMENT_METERS;
}

/**
 * Whether this hull is expected to force the span open.
 *
 * Two grounds only: a ledger that has watched it do so, or a sailing rig, whose
 * mast is the reason the bascule exists. An unproven hull is drawn as traffic
 * and never claimed as an opener.
 */
export function isOpener(track: VesselTrack): boolean {
  if (track.vesselClass === 'sailing') return true;
  return (track.openingPropensity ?? 0) >= OPENER_PROPENSITY_BASIS_POINTS;
}

/**
 * Which way along the channel the vessel is travelling.
 *
 * Course over ground bends with the river, so direction is read from the side
 * of the span the vessel is on together with the engine's closing judgement: a
 * vessel upriver that is closing must be running down toward the mouth.
 */
export function travelDirection(track: VesselTrack): TravelDirection {
  const s = track.sMeters;
  if (s == null || track.movement === 'stationary' || track.movement === 'unknown') {
    return 'holding';
  }
  const closing = track.movement === 'approaching';
  const upriverOfSpan = s > 0;
  if (closing) return upriverOfSpan ? 'downriver' : 'upriver';
  return upriverOfSpan ? 'upriver' : 'downriver';
}

/**
 * The vessels a reach drawing should carry, nearest the span first.
 *
 * Only corridor-projected traffic qualifies: a track with no channel
 * coordinate was never placed on the river, and guessing a position for it
 * would put a vessel on the drawing that the model never put on the water.
 *
 * Everything projected is returned, moving or not. An earlier version dropped
 * everything that was not under way, which drew an empty river on an evening
 * when five hulls were being received and read as a broken page rather than as
 * a quiet one. `underway` carries that distinction instead, so the surface can
 * rank and mute rather than hide.
 */
export function reachVessels(tracks: VesselTrack[]): ReachVessel[] {
  return tracks
    .filter((track) => track.sMeters != null)
    .map((track) => {
      const sMeters = track.sMeters as number;
      return {
        track,
        mmsi: track.mmsi,
        label: track.vesselName?.trim() || track.callSign?.trim() || track.mmsi,
        vesselClass: track.vesselClass,
        sMeters,
        distanceMeters: Math.abs(sMeters),
        speedKnots: track.speedKnots,
        direction: travelDirection(track),
        underway: isUnderway(track),
        opener: isOpener(track),
        closing: track.routeIntersects,
        observedAtMs: toMilliseconds(track.observedAt)
      };
    })
    // Traffic on passage first, then by proximity: a tug closing from two
    // kilometres matters more than a yacht tied up at five hundred metres.
    .sort((left, right) => {
      if (left.underway !== right.underway) return left.underway ? -1 : 1;
      return left.distanceMeters - right.distanceMeters;
    });
}

interface PolylineProjection {
  arcMeters: number;
  offsetMeters: number;
}

/**
 * Nearest point on a polyline, in metres along it and metres off it.
 *
 * A local tangent-plane approximation, as in the engine: at city scale it is
 * accurate to well under a metre, which is all a 120 m corridor needs.
 */
function projectPolyline(
  latitude: number,
  longitude: number,
  line: [number, number][]
): PolylineProjection {
  const METERS_PER_DEGREE_LATITUDE = 110_540;
  const metersPerDegreeLongitude = 111_320 * Math.cos((latitude * Math.PI) / 180);
  let best: PolylineProjection = { arcMeters: 0, offsetMeters: Number.POSITIVE_INFINITY };
  let cumulative = 0;
  for (let index = 0; index + 1 < line.length; index += 1) {
    const [aLatitude, aLongitude] = line[index];
    const [bLatitude, bLongitude] = line[index + 1];
    const ax = (aLongitude - longitude) * metersPerDegreeLongitude;
    const ay = (aLatitude - latitude) * METERS_PER_DEGREE_LATITUDE;
    const bx = (bLongitude - longitude) * metersPerDegreeLongitude;
    const by = (bLatitude - latitude) * METERS_PER_DEGREE_LATITUDE;
    const dx = bx - ax;
    const dy = by - ay;
    const lengthSquared = Math.max(1, dx * dx + dy * dy);
    const t = Math.min(1, Math.max(0, -(ax * dx + ay * dy) / lengthSquared));
    const cx = ax + t * dx;
    const cy = ay + t * dy;
    const offsetMeters = Math.sqrt(cx * cx + cy * cy);
    const segmentMeters = metersBetween(aLatitude, aLongitude, bLatitude, bLongitude);
    if (offsetMeters < best.offsetMeters) {
      best = { arcMeters: cumulative + segmentMeters * t, offsetMeters };
    }
    cumulative += segmentMeters;
  }
  return best;
}

export interface ChannelProjection {
  branchId: string;
  /** Signed channel metres: positive upriver of the span, negative seaward. */
  sMeters: number;
  offsetMeters: number;
  /** Whether the fix lies inside the branch's tracked half-width. */
  inCorridor: boolean;
}

/**
 * Builds a projector onto the published corridor, so a historical fix can be
 * placed on the same axis the engine placed the live one on.
 *
 * The arithmetic mirrors the engine's: the trunk runs mouth-first and is
 * shifted so the span reads zero, and each approach continues the coordinate
 * seaward from the mouth. Returns null when the corridor carries no trunk, in
 * which case there is no axis to place anything on.
 */
export function makeChannelProjector(
  corridor: RiverCorridor
): ((latitude: number, longitude: number) => ChannelProjection) | null {
  const trunk = corridor.branches.find((branch) => branch.id === 'river');
  if (!trunk || trunk.centerline.length < 2) return null;
  const [mouth, span] = trunk.centerline;
  const mouthToSpanMeters = metersBetween(mouth[0], mouth[1], span[0], span[1]);
  const approaches = corridor.branches.filter((branch) => branch.id !== 'river');

  return (latitude: number, longitude: number): ChannelProjection => {
    const onTrunk = projectPolyline(latitude, longitude, trunk.centerline);
    let best: ChannelProjection = {
      branchId: trunk.id,
      sMeters: onTrunk.arcMeters - mouthToSpanMeters,
      offsetMeters: onTrunk.offsetMeters,
      inCorridor: onTrunk.offsetMeters <= trunk.corridorOffsetMeters
    };
    for (const branch of approaches) {
      if (branch.centerline.length < 2) continue;
      const projection = projectPolyline(latitude, longitude, branch.centerline);
      if (projection.offsetMeters >= best.offsetMeters) continue;
      best = {
        branchId: branch.id,
        sMeters: -mouthToSpanMeters - projection.arcMeters,
        offsetMeters: projection.offsetMeters,
        inCorridor: projection.offsetMeters <= branch.corridorOffsetMeters
      };
    }
    return best;
  };
}

const METERS_PER_DEGREE_LATITUDE = 110_540;

/**
 * The tracked water as a closed ring, buffered off the centreline by the
 * branch's own half-width.
 *
 * A map has to show tracked *area*, not a line: the corridor test is a
 * perpendicular distance, so the honest drawing of it is the band that
 * distance sweeps. Vertices are offset along the average of their adjacent
 * segment normals, which keeps the band even through the river's bends.
 * Coordinates come back `[longitude, latitude]`, ready for GeoJSON.
 */
export function corridorRing(
  centerline: [number, number][],
  offsetMeters: number
): [number, number][] {
  if (centerline.length < 2) return [];
  const normals: [number, number][] = [];
  for (let index = 0; index + 1 < centerline.length; index += 1) {
    const [aLatitude, aLongitude] = centerline[index];
    const [bLatitude, bLongitude] = centerline[index + 1];
    const metersPerDegreeLongitude =
      111_320 * Math.cos((((aLatitude + bLatitude) / 2) * Math.PI) / 180);
    const dx = (bLongitude - aLongitude) * metersPerDegreeLongitude;
    const dy = (bLatitude - aLatitude) * METERS_PER_DEGREE_LATITUDE;
    const length = Math.hypot(dx, dy) || 1;
    normals.push([-dy / length, dx / length]);
  }

  const left: [number, number][] = [];
  const right: [number, number][] = [];
  for (let index = 0; index < centerline.length; index += 1) {
    const [latitude, longitude] = centerline[index];
    // A vertex belongs to up to two segments; averaging their normals keeps
    // the band continuous instead of notching at every bend.
    const before = normals[index - 1];
    const after = normals[index];
    const nx = ((before?.[0] ?? after[0]) + (after?.[0] ?? before![0])) / 2;
    const ny = ((before?.[1] ?? after[1]) + (after?.[1] ?? before![1])) / 2;
    const length = Math.hypot(nx, ny) || 1;
    const metersPerDegreeLongitude = 111_320 * Math.cos((latitude * Math.PI) / 180) || 1;
    const offsetLongitude = ((nx / length) * offsetMeters) / metersPerDegreeLongitude;
    const offsetLatitude = ((ny / length) * offsetMeters) / METERS_PER_DEGREE_LATITUDE;
    left.push([longitude + offsetLongitude, latitude + offsetLatitude]);
    right.push([longitude - offsetLongitude, latitude - offsetLatitude]);
  }

  const ring = [...left, ...right.reverse()];
  ring.push(ring[0]);
  return ring;
}

/** The whole tracked corridor as a GeoJSON FeatureCollection of polygons. */
export function corridorFeatureCollection(corridor: RiverCorridor) {
  return {
    type: 'FeatureCollection' as const,
    features: corridor.branches
      .map((branch) => ({
        type: 'Feature' as const,
        properties: { id: branch.id, label: branch.label },
        geometry: {
          type: 'Polygon' as const,
          coordinates: [corridorRing(branch.centerline, branch.corridorOffsetMeters)]
        }
      }))
      .filter((feature) => feature.geometry.coordinates[0].length > 3)
  };
}

export interface TrailPoint {
  sMeters: number;
  observedAtMs: number;
  /** 0 at the oldest retained fix, 1 at the newest. */
  freshness: number;
}

/**
 * The vessel's retained track as channel positions, oldest first.
 *
 * Freshness is measured across the track's own span rather than against the
 * wall clock, so a trail always fades from end to end and a vessel reporting
 * every thirty seconds is not drawn as uniformly new.
 */
export function trailFor(
  track: VesselTrack,
  project: (latitude: number, longitude: number) => ChannelProjection
): TrailPoint[] {
  const points = track.points
    .map((point) => ({
      sMeters: project(point.latitude, point.longitude).sMeters,
      observedAtMs: toMilliseconds(point.observedAt)
    }))
    .filter((point) => point.observedAtMs > 0)
    .sort((left, right) => left.observedAtMs - right.observedAtMs);
  if (points.length === 0) return [];
  const oldest = points[0].observedAtMs;
  const newest = points[points.length - 1].observedAtMs;
  const span = Math.max(1, newest - oldest);
  return points.map((point) => ({
    ...point,
    freshness: (point.observedAtMs - oldest) / span
  }));
}

export interface RiverFrame {
  width: number;
  height: number;
  /** Latitude/longitude to SVG user units, north up and west left. */
  project: (latitude: number, longitude: number) => [number, number];
  /** Drawn metres per SVG unit, for the scale bar. */
  metersPerUnit: number;
}

/**
 * Fits a set of coordinates into a drawing box at true shape.
 *
 * Longitude is scaled by the cosine of the frame's own latitude, so the river
 * keeps its real proportions instead of being stretched east-west by the
 * projection. The scale stays uniform on both axes for the same reason: a bend
 * drawn at the wrong angle is a different river.
 */
export function riverFrame(
  coordinates: [number, number][],
  width: number,
  height: number,
  padding: number
): RiverFrame | null {
  if (coordinates.length === 0) return null;
  const latitudes = coordinates.map(([latitude]) => latitude);
  const longitudes = coordinates.map(([, longitude]) => longitude);
  const minLatitude = Math.min(...latitudes);
  const maxLatitude = Math.max(...latitudes);
  const minLongitude = Math.min(...longitudes);
  const maxLongitude = Math.max(...longitudes);
  const cosine = Math.cos((((minLatitude + maxLatitude) / 2) * Math.PI) / 180) || 1;

  const spanX = Math.max(1e-9, (maxLongitude - minLongitude) * cosine);
  const spanY = Math.max(1e-9, maxLatitude - minLatitude);
  const usableWidth = width - padding * 2;
  const usableHeight = height - padding * 2;
  const scale = Math.min(usableWidth / spanX, usableHeight / spanY);
  const offsetX = padding + (usableWidth - spanX * scale) / 2;
  const offsetY = padding + (usableHeight - spanY * scale) / 2;

  return {
    width,
    height,
    project: (latitude: number, longitude: number) => [
      offsetX + (longitude - minLongitude) * cosine * scale,
      // SVG y grows downward; latitude grows north, so the frame is flipped.
      offsetY + (maxLatitude - latitude) * scale
    ],
    metersPerUnit: METERS_PER_DEGREE_LATITUDE / scale
  };
}

export interface ReachScale {
  /** Most upriver channel metre on the axis (drawn at the west edge). */
  upriverMeters: number;
  /** Most seaward channel metre on the axis (drawn at the east edge). */
  seawardMeters: number;
  /** Maps channel metres to a 0–1 position running west (0) to east (1). */
  position: (sMeters: number) => number;
}

/**
 * The axis for the reach, west on the left as on any chart.
 *
 * The window starts at a fixed size so the drawing does not rescale under the
 * reader every time a vessel appears, and only ever grows — a vessel outside
 * the frame stretches it rather than being clamped to an edge, because a mark
 * pinned at the boundary would read as a position.
 */
export function reachScale(sValues: number[]): ReachScale {
  let upriverMeters = REACH_UPRIVER_METERS;
  let seawardMeters = -REACH_SEAWARD_METERS;
  for (const sMeters of sValues) {
    if (sMeters > upriverMeters) upriverMeters = sMeters;
    if (sMeters < seawardMeters) seawardMeters = sMeters;
  }
  // Breathing room so a vessel at the extreme is not drawn half off the sheet.
  const margin = (upriverMeters - seawardMeters) * 0.04;
  upriverMeters += margin;
  seawardMeters -= margin;
  const span = upriverMeters - seawardMeters;
  return {
    upriverMeters,
    seawardMeters,
    position: (sMeters: number) => (upriverMeters - sMeters) / span
  };
}
