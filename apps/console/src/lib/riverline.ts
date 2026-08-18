/**
 * Laying the corridor out as a schematic line.
 *
 * A chart of the river answers "where exactly"; this answers "how many, which
 * way, and how soon", which is the question a driver actually has. So spacing
 * is even and unscaled, while *order* and *betweenness* stay true to the
 * channel coordinate: a vessel drawn between two stations is genuinely between
 * them on the water.
 *
 * The line serpentines. Laid out straight, fifteen stations across a console
 * leave about seventy pixels each, which is narrower than the word "Government
 * Cut" — so the labels collided and the whole thing read as noise. Wrapping it
 * into rows buys roughly a quarter-kilometre of drawing per station instead.
 */
import type { RiverCorridor, RiverStation, VesselTrack } from './types';
import {
  isOpener,
  makeChannelProjector,
  reachVessels,
  type ReachVessel,
  type TravelDirection
} from './river';

export interface PlacedStation extends RiverStation {
  branchId: string;
  x: number;
  y: number;
  /** Live bascule state, joined by FL511 key. */
  state?: 'up' | 'down' | 'unknown';
  isTarget: boolean;
  /**
   * Retained so a caller can special-case a label, but the diagram now puts
   * every station name in the band above the line and every vessel readout in
   * the band below it. Alternating sides used to drive a station name straight
   * through a vessel's speed and ETA.
   */
  labelAbove: boolean;
}

export interface DiagramVessel extends ReachVessel {
  x: number;
  y: number;
  /** Which way the mark points along the drawn line, in degrees. */
  headingDegrees: number;
  /**
   * Rank within a cluster of vessels sharing a spot. Berthed craft pile onto
   * one another at a marina; fanning them out is the difference between "five
   * tugs" and one illegible blob.
   */
  stackIndex: number;
  stackSize: number;
  /** True when this hull was moved off the line into a shared label column. */
  stackedColumn: boolean;
  /** Where the label sits relative to the mark, after fanning. */
  labelY: number;
  etaMinMinutes?: number;
  etaMaxMinutes?: number;
  lengthMeters?: number;
  beamMeters?: number;
  draughtMeters?: number;
  callSign?: string;
  imoNumber?: number;
  destination?: string;
  /**
   * Drawn hull length in user units, scaled from the reported length.
   *
   * Real dimensions span a 20 m launch to a 100 m coaster, which is a 5x range
   * — drawn linearly the launch vanishes. A square-root scale keeps a small
   * hull legible while a big one still reads as big.
   */
  hullLength: number;
  hullBeam: number;
  /** Whether that size came from a static report or is the neutral default. */
  sizeKnown: boolean;
  scheduleExempt: boolean;
  predictedOpeningAt?: string;
  waitsForSlot: boolean;
}

export interface DiagramLine {
  id: string;
  d: string;
  approach: boolean;
}

export interface RiverDiagram {
  width: number;
  height: number;
  lines: DiagramLine[];
  stations: PlacedStation[];
  /**
   * Only traffic actually on passage. Berthed hulls are not drawn and are not
   * counted anywhere: a vessel tied up at a pier is not news, and saying how
   * many there are is just a number the reader has to decide to ignore.
   */
  vessels: DiagramVessel[];
}

/**
 * Drawing width in user units.
 *
 * This sets the aspect ratio, which is what decides how large the river can be
 * drawn: the tile it lives in is wide and short, so a narrow drawing gets
 * letterboxed into the middle with dead margins either side. Sized to sit
 * close to the tile's own proportions.
 */
const WIDTH = 1500;
const MARGIN_X = 124;
/** Minimum drawing width per station; below this the labels start touching. */
const MIN_STATION_SPACING = 172;
const ROW_GAP = 118;
const TOP = 46;
/** How far the two entrance channels sit apart before they meet at the mouth. */
const APPROACH_GAP = 82;
/** Row height of a stacked cluster sharing one label column. */
const STACK_STEP = 34;
/** Where a stack begins, below the line and clear of the hulls on it. */
const STACK_TOP = 26;
/** Height is rounded up to this, so the drawing does not rescale every refresh. */
const STACK_QUANTUM = 68;
/**
 * Along-path distance within which two vessels share a label column.
 *
 * Sized to the label, not the hull: two boats 60 px apart do not overlap, but
 * "8.0 kn · 14–19 min to Brickell · 96 m" underneath them certainly does.
 */
const CLUSTER_RADIUS = 132;

/** Drawn hull length for a vessel that has reported no dimensions. */
const DEFAULT_HULL_LENGTH = 17;
const DEFAULT_HULL_BEAM = 8;
/** Reference hull: a 30 m river tug draws at the default size. */
const REFERENCE_LENGTH_METERS = 30;

/**
 * Drawn hull size from reported metres.
 *
 * Square-root scaling, clamped: a 12 m launch stays clickable and a 100 m
 * coaster stays inside its lane instead of swallowing two stations.
 */
function hullSize(
  lengthMeters: number | undefined,
  beamMeters: number | undefined
): { hullLength: number; hullBeam: number; sizeKnown: boolean } {
  if (lengthMeters == null || !Number.isFinite(lengthMeters) || lengthMeters <= 0) {
    return {
      hullLength: DEFAULT_HULL_LENGTH,
      hullBeam: DEFAULT_HULL_BEAM,
      sizeKnown: false
    };
  }
  const scale = Math.sqrt(lengthMeters / REFERENCE_LENGTH_METERS);
  const hullLength = Math.min(40, Math.max(14, DEFAULT_HULL_LENGTH * scale));
  // Keep the drawn proportions honest when a beam was reported, and fall back
  // to a plausible ratio when only length is known.
  const ratio = beamMeters && beamMeters > 0 ? beamMeters / lengthMeters : 0.28;
  const hullBeam = Math.min(hullLength * 0.75, Math.max(5, hullLength * ratio * 1.6));
  return { hullLength, hullBeam, sizeKnown: true };
}

function stationsOf(corridor: RiverCorridor, branchId: string): RiverStation[] {
  return corridor.branches.find((branch) => branch.id === branchId)?.stations ?? [];
}

/** Seaward-first: the order a vessel actually travels inbound. */
function seawardFirst(stations: RiverStation[]): RiverStation[] {
  return stations.slice().sort((left, right) => left.sMeters - right.sMeters);
}

interface Node {
  station: RiverStation;
  branchId: string;
}

export function riverDiagram(
  corridor: RiverCorridor,
  vesselTracks: VesselTrack[],
  bridgeStates: Map<string, 'up' | 'down' | 'unknown'>
): RiverDiagram {
  const trunk = seawardFirst(stationsOf(corridor, 'river'));
  const north = seawardFirst(stationsOf(corridor, 'north_approach')).filter(
    (station) => station.kind !== 'mouth'
  );
  const south = seawardFirst(stationsOf(corridor, 'south_approach')).filter(
    (station) => station.kind !== 'mouth'
  );

  // The spine a reader follows: in from the sea on the main channel, through
  // the mouth, then up the river past Brickell.
  const spine: Node[] = [
    ...north.map((station) => ({ station, branchId: 'north_approach' })),
    ...trunk.map((station) => ({ station, branchId: 'river' }))
  ];
  if (spine.length === 0) {
    return { width: WIDTH, height: 320, lines: [], stations: [], vessels: [] };
  }

  const usable = WIDTH - MARGIN_X * 2;
  const maxPerRow = Math.max(2, Math.floor(usable / MIN_STATION_SPACING) + 1);
  const rowCount = Math.ceil(spine.length / maxPerRow);
  // Spread evenly rather than filling rows greedily, which would strand a
  // final row of three stations across the full width.
  const perRow = Math.ceil(spine.length / rowCount);
  // The south approach gets its own lane above the first row, so the fork is
  // visible without either channel crossing the other.
  const spineTop = TOP + APPROACH_GAP;
  // Room below the last row for one vessel's name, readout and tag. Interior
  // rows already have ROW_GAP for the same purpose.
  const baseHeight = spineTop + (rowCount - 1) * ROW_GAP + 70;

  const placed: PlacedStation[] = [];
  const rowPoints: { x: number; y: number }[][] = [];
  spine.forEach((node, index) => {
    const row = Math.floor(index / perRow);
    const column = index % perRow;
    const countInRow = Math.min(perRow, spine.length - row * perRow);
    const step = countInRow > 1 ? usable / (countInRow - 1) : 0;
    // Serpentine: even rows run left to right, odd rows fold back.
    const forward = row % 2 === 0;
    const x = forward ? MARGIN_X + step * column : WIDTH - MARGIN_X - step * column;
    const y = spineTop + row * ROW_GAP;
    if (!rowPoints[row]) rowPoints[row] = [];
    rowPoints[row].push({ x, y });
    placed.push({
      ...node.station,
      branchId: node.branchId,
      x,
      y,
      state: node.station.bridgeKey ? bridgeStates.get(node.station.bridgeKey) : undefined,
      isTarget: node.station.kind === 'target',
      // Alternate so a long name never shares a baseline with its neighbour.
      labelAbove: column % 2 === 0
    });
  });

  const mouth = placed.find((station) => station.kind === 'mouth') ?? placed[north.length];
  const southPlaced: PlacedStation[] = [];
  if (south.length > 0 && mouth) {
    const southSpan = usable * 0.62;
    const southStart = WIDTH - MARGIN_X - southSpan;
    const southStep = southSpan / Math.max(1, south.length);
    south.forEach((station, index) => {
      southPlaced.push({
        ...station,
        branchId: 'south_approach',
        x: southStart + southStep * index,
        y: TOP,
        state: undefined,
        isTarget: false,
        labelAbove: index % 2 === 0
      });
    });
  }

  const lines: DiagramLine[] = rowPoints
    .map((points, row) => ({
      id: `spine-${row}`,
      approach: false,
      d: points.map((point, index) => `${index === 0 ? 'M' : 'L'}${point.x} ${point.y}`).join(' ')
    }))
    .filter((line) => line.d.length > 0);

  // Fold connectors: a rounded turn joining the end of one row to the start of
  // the next, so the line reads as one continuous channel.
  for (let row = 0; row + 1 < rowPoints.length; row += 1) {
    const from = rowPoints[row][rowPoints[row].length - 1];
    const to = rowPoints[row + 1][0];
    const bulge = row % 2 === 0 ? 46 : -46;
    lines.push({
      id: `fold-${row}`,
      approach: false,
      d: `M${from.x} ${from.y} C${from.x + bulge} ${from.y + 10}, ${to.x + bulge} ${to.y - 10}, ${to.x} ${to.y}`
    });
  }

  if (southPlaced.length > 0 && mouth) {
    const points = [...southPlaced.map((station) => ({ x: station.x, y: station.y })), mouth];
    lines.push({
      id: 'south_approach',
      approach: true,
      d: points.map((point, index) => `${index === 0 ? 'M' : 'L'}${point.x} ${point.y}`).join(' ')
    });
  }

  const project = makeChannelProjector(corridor);
  const vessels = layOutVessels(
    reachVessels(vesselTracks).filter((vessel) => vessel.underway),
    placed,
    southPlaced,
    project
  );

  const stations = [...placed, ...southPlaced].map((station) => {
    const crowded = vessels.some(
      (vessel) => vessel.stackSize > 1 && Math.abs(vessel.x - station.x) < MIN_STATION_SPACING / 2
    );
    return crowded ? { ...station, labelAbove: true } : station;
  });

  // A stack on the final row can run past that. Deriving the height straight
  // from the deepest one made the viewBox change every time a boat appeared,
  // rescaling the whole drawing under the reader; quantising it means the box
  // only ever changes when a stack genuinely grows a step deeper.
  const deepest = vessels.reduce((lowest, vessel) => Math.max(lowest, vessel.y + 54), 0);
  const height = Math.max(baseHeight, Math.ceil(deepest / STACK_QUANTUM) * STACK_QUANTUM);
  return { width: WIDTH, height, lines, stations, vessels };
}

function layOutVessels(
  vessels: ReachVessel[],
  spine: PlacedStation[],
  south: PlacedStation[],
  project: ReturnType<typeof makeChannelProjector>
): DiagramVessel[] {
  const placed = vessels.flatMap((vessel) => {
    const line = vessel.track.branch === 'south_approach' && south.length > 0 ? south : spine;
    const spot = interpolate(vessel.sMeters, line);
    if (!spot) return [];
    // Heading comes from the drawn line's own tangent, not from a fixed
    // left/right. The serpentine reverses every row, so "upriver" points right
    // on one row and left on the next; anything else aims half the fleet the
    // wrong way down the river.
    const upriver = (Math.atan2(spot.dy, spot.dx) * 180) / Math.PI;
    const headingDegrees = vessel.direction === 'downriver' ? upriver + 180 : upriver;
    return [{ ...vessel, x: spot.x, y: spot.y, headingDegrees }];
  });

  // Fan anything sharing a spot. Without this the berthed fleet at the port
  // draws as a single mark with five names printed on top of each other.
  const clusters: (typeof placed)[] = [];
  for (const vessel of placed) {
    const cluster = clusters.find((candidate) =>
      candidate.some(
        (other) => Math.hypot(other.x - vessel.x, other.y - vessel.y) <= CLUSTER_RADIUS
      )
    );
    if (cluster) cluster.push(vessel);
    else clusters.push([vessel]);
  }

  return clusters.flatMap((cluster) => {
    // A stack shares one column so its names read as a list rather than as
    // overlapping labels at slightly different x. The berthed fleet at the
    // port is five hulls in one basin; that is what this has to draw.
    const anchorX = cluster.reduce((total, vessel) => total + vessel.x, 0) / cluster.length;
    const anchorY = cluster.reduce((total, vessel) => total + vessel.y, 0) / cluster.length;
    return cluster.map((vessel, index) => {
      const stacked = cluster.length > 1;
      return {
        ...vessel,
        x: stacked ? anchorX : vessel.x,
        // Stacks hang below the line, where no station label ever sits.
        y: stacked ? anchorY + STACK_TOP + index * STACK_STEP : vessel.y,
        stackedColumn: stacked,
        stackIndex: index,
        stackSize: cluster.length,
        // Below the line, clear of the station names above it.
        // Hull sits on the water; everything it says stacks underneath.
        labelY: stacked ? 4 : 22,
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

/**
 * Interpolates a channel position onto the drawn line.
 *
 * The vessel sits between the same two stations it sits between on the water,
 * at the same fraction of the way; only the spacing is schematic.
 */
function interpolate(
  sMeters: number,
  line: PlacedStation[]
): { x: number; y: number; dx: number; dy: number } | null {
  if (line.length === 0) return null;
  const ordered = line.slice().sort((left, right) => left.sMeters - right.sMeters);
  const tangent = (lower: PlacedStation, upper: PlacedStation) => {
    const dx = upper.x - lower.x;
    const dy = upper.y - lower.y;
    const length = Math.hypot(dx, dy) || 1;
    // Unit vector pointing upriver, which is the direction s increases.
    return { dx: dx / length, dy: dy / length };
  };
  for (let index = 0; index + 1 < ordered.length; index += 1) {
    const lower = ordered[index];
    const upper = ordered[index + 1];
    if (sMeters >= lower.sMeters && sMeters <= upper.sMeters) {
      const span = upper.sMeters - lower.sMeters || 1;
      const fraction = (sMeters - lower.sMeters) / span;
      // Straddling a fold: the two stations are on different rows, and a
      // straight interpolation between them cuts diagonally across empty
      // sheet. Ride out to the nearer station instead, which is on the line.
      if (Math.abs(upper.y - lower.y) > 1) {
        const nearer = fraction < 0.5 ? lower : upper;
        const other = fraction < 0.5 ? upper : lower;
        const inward = Math.sign(nearer.x - other.x) || 1;
        return {
          x: nearer.x + inward * 18,
          y: nearer.y,
          ...tangent(lower, upper)
        };
      }
      return {
        x: lower.x + (upper.x - lower.x) * fraction,
        y: lower.y + (upper.y - lower.y) * fraction,
        ...tangent(lower, upper)
      };
    }
  }
  // Past either end: pin to the end it ran off rather than drop the vessel.
  if (ordered.length === 1) return { x: ordered[0].x, y: ordered[0].y, dx: 1, dy: 0 };
  const first = ordered[0];
  const last = ordered[ordered.length - 1];
  return sMeters < first.sMeters
    ? { x: first.x, y: first.y, ...tangent(first, ordered[1]) }
    : { x: last.x, y: last.y, ...tangent(ordered[ordered.length - 2], last) };
}

export { isOpener };
