import { describe, expect, it } from 'vitest';

import {
  OPENER_PROPENSITY_BASIS_POINTS,
  corridorRing,
  currentVesselTracks,
  isOpener,
  isUnderway,
  makeChannelProjector,
  reachScale,
  reachVessels,
  trailFor,
  travelDirection
} from './river';
import type { RiverCorridor, VesselTrack } from './types';

/** The engine's published geometry, trimmed to what these tests exercise. */
const CORRIDOR: RiverCorridor = {
  bridgeLatitude: 25.7699,
  bridgeLongitude: -80.19005,
  aisLive: true,
  branches: [
    {
      id: 'river',
      label: 'Miami River',
      corridorOffsetMeters: 120,
      stations: [],
      centerline: [
        [25.771, -80.1849],
        [25.7699, -80.19005],
        [25.7692, -80.1938],
        [25.768907, -80.197552],
        [25.773038, -80.200591],
        [25.774205, -80.201287],
        [25.778307, -80.206931]
      ]
    },
    {
      id: 'north_approach',
      label: 'Main Channel approach',
      corridorOffsetMeters: 150,
      stations: [],
      centerline: [
        [25.771, -80.1849],
        [25.769, -80.1824],
        [25.7663, -80.183],
        [25.7725, -80.1795],
        [25.7705, -80.17],
        [25.7635, -80.133]
      ]
    }
  ]
};

function track(overrides: Partial<VesselTrack> = {}): VesselTrack {
  return {
    mmsi: '367705810',
    vesselName: 'SARA',
    movement: 'approaching',
    routeIntersects: true,
    speedKnots: 4.2,
    courseDegrees: 95,
    observedAt: '2026-08-17T17:50:00Z',
    posture: 'underway',
    sMeters: 1200,
    points: [
      { latitude: 25.774205, longitude: -80.201287, observedAt: '2026-08-17T17:40:00Z' },
      { latitude: 25.773038, longitude: -80.200591, observedAt: '2026-08-17T17:45:00Z' },
      { latitude: 25.768907, longitude: -80.197552, observedAt: '2026-08-17T17:50:00Z' }
    ],
    ...overrides
  };
}

describe('channel projection', () => {
  it('places the span at zero and orders the bascules upriver', () => {
    const project = makeChannelProjector(CORRIDOR);
    expect(project).toBeTruthy();
    const span = project!(25.7699, -80.19005);
    expect(Math.abs(span.sMeters)).toBeLessThan(1);
    expect(span.inCorridor).toBe(true);

    // Matches the engine's own assertion: SW 2 Ave lands between 700 and 850 m
    // upriver. If the two ever disagree the reach would draw vessels at
    // positions the prediction was never made from.
    const sw2 = project!(25.768907, -80.197552);
    expect(sw2.sMeters).toBeGreaterThan(700);
    expect(sw2.sMeters).toBeLessThan(850);
  });

  it('continues the coordinate seaward along the approach channel', () => {
    const project = makeChannelProjector(CORRIDOR);
    const entering = project!(25.7676, -80.1827);
    expect(entering.branchId).toBe('north_approach');
    expect(entering.sMeters).toBeLessThan(-600);
    expect(entering.inCorridor).toBe(true);
  });

  it('rejects a berth off the channel even though it is near the span', () => {
    const project = makeChannelProjector(CORRIDOR);
    const docked = project!(25.7687, -80.1895);
    expect(docked.inCorridor).toBe(false);
    expect(Math.abs(docked.sMeters)).toBeLessThan(250);
  });

  it('returns null when the corridor carries no trunk to measure against', () => {
    expect(makeChannelProjector({ ...CORRIDOR, branches: [] })).toBeNull();
  });
});

describe('what counts as under way', () => {
  it('keeps vessels on passage and vessels waiting at the span', () => {
    expect(isUnderway(track({ posture: 'underway' }))).toBe(true);
    expect(isUnderway(track({ posture: 'waiting' }))).toBe(true);
  });

  it('drops the moored, off-channel and deep-draft fleet', () => {
    expect(isUnderway(track({ posture: 'moored' }))).toBe(false);
    expect(isUnderway(track({ posture: 'off_channel' }))).toBe(false);
    expect(isUnderway(track({ posture: 'deep_draft' }))).toBe(false);
  });

  it('falls back to the displacement floor when no posture was published', () => {
    // A hull swinging on its lines: every fix within tens of metres.
    const drifting = track({
      posture: undefined,
      points: [
        { latitude: 25.7692, longitude: -80.1938, observedAt: '2026-08-17T17:40:00Z' },
        { latitude: 25.76922, longitude: -80.19383, observedAt: '2026-08-17T17:45:00Z' },
        { latitude: 25.76919, longitude: -80.19378, observedAt: '2026-08-17T17:50:00Z' }
      ]
    });
    expect(isUnderway(drifting)).toBe(false);
    // The same vessel, actually moving down the river.
    expect(isUnderway(track({ posture: undefined }))).toBe(true);
  });
});

describe('naming an opener', () => {
  it('marks a hull the ledger has watched open the span', () => {
    expect(isOpener(track({ openingPropensity: OPENER_PROPENSITY_BASIS_POINTS }))).toBe(true);
    expect(isOpener(track({ openingPropensity: 6700 }))).toBe(true);
  });

  it('marks a sailing rig on sight', () => {
    expect(isOpener(track({ vesselClass: 'sailing', openingPropensity: undefined }))).toBe(true);
  });

  it('leaves an unproven hull unclaimed, and one seen fitting under', () => {
    expect(isOpener(track({ openingPropensity: undefined }))).toBe(false);
    // Beta-smoothed: one crossing under a closed span reads 3300, not zero.
    expect(isOpener(track({ openingPropensity: 3300 }))).toBe(false);
  });
});

describe('travel direction', () => {
  it('reads a closing vessel upriver of the span as running down to it', () => {
    expect(travelDirection(track({ sMeters: 1200, movement: 'approaching' }))).toBe('downriver');
  });

  it('reads a closing vessel seaward of the span as running up to it', () => {
    expect(travelDirection(track({ sMeters: -900, movement: 'approaching' }))).toBe('upriver');
  });

  it('flips when the vessel is opening the gap instead of closing it', () => {
    expect(travelDirection(track({ sMeters: 1200, movement: 'diverging' }))).toBe('upriver');
    expect(travelDirection(track({ sMeters: -900, movement: 'diverging' }))).toBe('downriver');
  });

  it('claims no heading for a vessel on station', () => {
    expect(travelDirection(track({ movement: 'stationary' }))).toBe('holding');
    expect(travelDirection(track({ sMeters: undefined }))).toBe('holding');
  });
});

describe('live vessel freshness', () => {
  it('keeps the one-hour archive intact while selecting only current tracks for Live', () => {
    const generatedAt = '2026-08-23T16:10:00Z';
    const fresh = track({ mmsi: '111111111', observedAt: '2026-08-23T16:04:00Z' });
    const stale = track({ mmsi: '222222222', observedAt: '2026-08-23T16:03:59Z' });
    const future = track({ mmsi: '333333333', observedAt: '2026-08-23T16:10:31Z' });
    const archive = [fresh, stale, future];

    expect(currentVesselTracks(archive, generatedAt).map((item) => item.mmsi)).toEqual([
      '111111111'
    ]);
    expect(archive).toHaveLength(3);
  });
});

describe('the reach', () => {
  it('carries every corridor-projected vessel, traffic on passage first', () => {
    const vessels = reachVessels([
      track({ mmsi: '111111111', sMeters: 2400 }),
      track({ mmsi: '222222222', sMeters: -300 }),
      // Berthed: still carried, so a river with hulls on it never draws empty.
      track({ mmsi: '333333333', sMeters: -120, posture: 'moored' }),
      // No channel coordinate: never placed on the river, so never drawn.
      track({ mmsi: '444444444', sMeters: undefined })
    ]);
    // Under way beats proximity: the moored hull is nearest but sorts last.
    expect(vessels.map((vessel) => vessel.mmsi)).toEqual([
      '222222222',
      '111111111',
      '333333333'
    ]);
    expect(vessels[0].distanceMeters).toBe(300);
    expect(vessels[0].underway).toBe(true);
    expect(vessels[2].underway).toBe(false);
  });

  it('labels a vessel by its MMSI when it has broadcast no name', () => {
    const [vessel] = reachVessels([track({ vesselName: undefined })]);
    expect(vessel.label).toBe('367705810');
    const [blank] = reachVessels([track({ vesselName: '   ' })]);
    expect(blank.label).toBe('367705810');
  });

  it('runs west to east, and grows rather than clipping a distant vessel', () => {
    const scale = reachScale([]);
    // Upriver is west, so it sits left of the span, which sits left of seaward.
    expect(scale.position(3000)).toBeLessThan(scale.position(0));
    expect(scale.position(0)).toBeLessThan(scale.position(-2000));

    const stretched = reachScale([9000]);
    expect(stretched.upriverMeters).toBeGreaterThan(9000);
    expect(stretched.position(9000)).toBeGreaterThanOrEqual(0);
    expect(stretched.position(9000)).toBeLessThanOrEqual(1);
  });

  it('fades a trail from its oldest fix to its newest', () => {
    const project = makeChannelProjector(CORRIDOR);
    const trail = trailFor(track(), project!);
    expect(trail).toHaveLength(3);
    expect(trail[0].freshness).toBe(0);
    expect(trail[2].freshness).toBe(1);
    // Oldest fix furthest upriver, newest nearest the span: running downriver.
    expect(trail[0].sMeters).toBeGreaterThan(trail[2].sMeters);
  });

  it('survives a track whose timestamps are unusable', () => {
    const project = makeChannelProjector(CORRIDOR);
    const broken = trailFor(
      track({
        points: [{ latitude: 25.7692, longitude: -80.1938, observedAt: 'not a time' }]
      }),
      project!
    );
    expect(broken).toEqual([]);
  });
});

describe('corridor area', () => {
  it('buffers the centreline into a closed ring of the right width', () => {
    const ring = corridorRing(CORRIDOR.branches[0].centerline, 120);
    // Both sides of the line plus the closing vertex.
    expect(ring).toHaveLength(CORRIDOR.branches[0].centerline.length * 2 + 1);
    expect(ring[0]).toEqual(ring[ring.length - 1]);

    // The band is drawn about the line, so the span stays inside it: measured
    // north-south at the span, the ring should straddle 25.7699.
    const latitudes = ring.map(([, latitude]) => latitude);
    expect(Math.min(...latitudes)).toBeLessThan(25.7699);
    expect(Math.max(...latitudes)).toBeGreaterThan(25.7699);
  });

  it('declines to draw a branch that has no line', () => {
    expect(corridorRing([], 120)).toEqual([]);
    expect(corridorRing([[25.77, -80.19]], 120)).toEqual([]);
  });
});
