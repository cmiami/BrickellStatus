import { describe, expect, it } from 'vitest';

import {
  BRICKELL_SCHEMATIC_POINT,
  RIVER_SCHEMATIC_HEIGHT,
  RIVER_SCHEMATIC_WIDTH,
  riverSchematic,
  schematicPointAt
} from './riverSchematic';
import type { RiverCorridor, VesselTrack } from './types';

const CORRIDOR: RiverCorridor = {
  bridgeLatitude: 25.7699,
  bridgeLongitude: -80.19005,
  aisLive: true,
  branches: [
    {
      id: 'river',
      label: 'Miami River',
      corridorOffsetMeters: 120,
      centerline: [
        [25.771, -80.1849],
        [25.7699, -80.19005],
        [25.7692, -80.1938],
        [25.768907, -80.197552],
        [25.773038, -80.200591],
        [25.778307, -80.206931],
        [25.79267, -80.23965]
      ],
      stations: [
        {
          label: 'River mouth',
          kind: 'mouth',
          latitude: 25.771,
          longitude: -80.1849,
          sMeters: -530
        },
        {
          label: 'Brickell Ave',
          kind: 'target',
          bridgeKey: 'brickell',
          latitude: 25.7699,
          longitude: -80.19005,
          sMeters: 0
        },
        {
          label: 'S Miami Ave',
          kind: 'bridge',
          latitude: 25.7692,
          longitude: -80.1938,
          sMeters: 380
        },
        {
          label: 'SW 2 Ave',
          kind: 'bridge',
          bridgeKey: 'sw_2_ave',
          latitude: 25.768907,
          longitude: -80.197552,
          sMeters: 780
        },
        {
          label: 'SW 1 St',
          kind: 'bridge',
          bridgeKey: 'sw_1_st',
          latitude: 25.773038,
          longitude: -80.200591,
          sMeters: 1_112
        },
        {
          label: 'NW 5 St',
          kind: 'bridge',
          bridgeKey: 'nw_5_st',
          latitude: 25.778307,
          longitude: -80.206931,
          sMeters: 2_180
        },
        {
          label: 'NW 27 Ave',
          kind: 'bridge',
          bridgeKey: 'nw_27_ave',
          latitude: 25.79267,
          longitude: -80.23965,
          sMeters: 5_860
        }
      ]
    },
    {
      id: 'north_approach',
      label: 'Main Channel',
      corridorOffsetMeters: 150,
      centerline: [
        [25.771, -80.1849],
        [25.7779, -80.1799],
        [25.7793, -80.1665]
      ],
      stations: [
        {
          label: 'River mouth',
          kind: 'mouth',
          latitude: 25.771,
          longitude: -80.1849,
          sMeters: -530
        },
        {
          label: 'Port entrance',
          kind: 'waypoint',
          latitude: 25.7779,
          longitude: -80.1799,
          sMeters: -1_470
        },
        {
          label: 'Main Channel',
          kind: 'waypoint',
          latitude: 25.7793,
          longitude: -80.1665,
          sMeters: -3_000
        }
      ]
    },
    {
      id: 'government_cut',
      label: 'Government Cut',
      corridorOffsetMeters: 220,
      centerline: [
        [25.771, -80.1849],
        [25.7682, -80.167],
        [25.7622, -80.129]
      ],
      stations: [
        {
          label: 'River mouth',
          kind: 'mouth',
          latitude: 25.771,
          longitude: -80.1849,
          sMeters: -530
        },
        {
          label: 'Dodge Island',
          kind: 'waypoint',
          latitude: 25.7682,
          longitude: -80.167,
          sMeters: -2_300
        },
        {
          label: 'Government Cut',
          kind: 'waypoint',
          latitude: 25.7622,
          longitude: -80.129,
          sMeters: -7_000
        }
      ]
    },
    {
      id: 'south_approach',
      label: 'South approach',
      corridorOffsetMeters: 150,
      centerline: [
        [25.771, -80.1849],
        [25.7663, -80.183],
        [25.752, -80.181]
      ],
      stations: [
        {
          label: 'River mouth',
          kind: 'mouth',
          latitude: 25.771,
          longitude: -80.1849,
          sMeters: -530
        },
        {
          label: 'Brickell Key',
          kind: 'waypoint',
          latitude: 25.7663,
          longitude: -80.183,
          sMeters: -1_200
        },
        {
          label: 'Rickenbacker',
          kind: 'waypoint',
          latitude: 25.752,
          longitude: -80.181,
          sMeters: -4_200
        }
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
    vesselClass: 'tug',
    posture: 'underway',
    sMeters: 1_200,
    branch: 'river',
    callSign: 'WDF7318',
    imoNumber: 9_123_456,
    destination: 'MIAMI RIVER',
    lengthMeters: 30,
    beamMeters: 9,
    draughtMeters: 3.1,
    openingPropensity: 6_700,
    knownOpener: true,
    likelyToOpenBrickell: true,
    etaMinMinutes: 6,
    etaMaxMinutes: 9,
    scheduleExempt: true,
    predictedOpeningAt: '2026-08-17T17:56:00Z',
    waitsForSlot: false,
    points: [
      { latitude: 25.778307, longitude: -80.206931, observedAt: '2026-08-17T17:40:00Z' },
      { latitude: 25.773038, longitude: -80.200591, observedAt: '2026-08-17T17:45:00Z' },
      { latitude: 25.768907, longitude: -80.197552, observedAt: '2026-08-17T17:50:00Z' }
    ],
    ...overrides
  };
}

const STATES = new Map([
  ['brickell', 'down' as const],
  ['sw_2_ave', 'up' as const],
  ['nw_27_ave', 'unknown' as const]
]);

describe('riverSchematic', () => {
  it('fixes the one target at Brickell and joins bridge state by key', () => {
    const schematic = riverSchematic(CORRIDOR, [], STATES);
    expect(schematic.target).toBeTruthy();
    expect(schematic.target?.label).toBe('Brickell Ave');
    expect(schematic.target?.x).toBe(BRICKELL_SCHEMATIC_POINT.x);
    expect(schematic.target?.y).toBe(BRICKELL_SCHEMATIC_POINT.y);
    expect(schematic.target?.state).toBe('down');
    expect(schematic.stations.find((station) => station.label === 'SW 2 Ave')?.state).toBe('up');
    expect(schematic.stations.find((station) => station.label === 'NW 27 Ave')?.state).toBe(
      'unknown'
    );
    // The FL511-invisible span remains a bridge but gains no invented reading.
    expect(schematic.stations.find((station) => station.label === 'S Miami Ave')?.state).toBeUndefined();
  });

  it('keeps every route, station, vessel and wake point finite and on the sheet', () => {
    const schematic = riverSchematic(CORRIDOR, [track()], STATES);
    expect(schematic.width).toBe(RIVER_SCHEMATIC_WIDTH);
    expect(schematic.height).toBe(RIVER_SCHEMATIC_HEIGHT);

    const points = [
      ...schematic.routes.flatMap((route) => route.points),
      ...schematic.stations,
      ...schematic.vessels,
      ...schematic.vessels.flatMap((vessel) => vessel.wake)
    ];
    for (const point of points) {
      expect(Number.isFinite(point.x)).toBe(true);
      expect(Number.isFinite(point.y)).toBe(true);
      expect(point.x).toBeGreaterThanOrEqual(0);
      expect(point.x).toBeLessThanOrEqual(schematic.width);
      expect(point.y).toBeGreaterThanOrEqual(0);
      expect(point.y).toBeLessThanOrEqual(schematic.height);
    }
    for (const route of schematic.routes) {
      expect(route.d).not.toContain('NaN');
      for (let index = 0; index + 1 < route.points.length; index += 1) {
        const dx = Math.abs(route.points[index + 1].x - route.points[index].x);
        const dy = Math.abs(route.points[index + 1].y - route.points[index].y);
        expect(
          dx < 0.001 || dy < 0.001 || Math.abs(dx - dy) < 0.001,
          `${route.id} segment ${index}: dx=${dx}, dy=${dy}`
        ).toBe(true);
      }
    }
  });

  it('preserves river order while running upriver to the left', () => {
    const schematic = riverSchematic(CORRIDOR, [], STATES);
    const river = schematic.stations
      .filter((station) => station.branchId === 'river')
      .slice()
      .sort((left, right) => left.sMeters - right.sMeters);
    const targetIndex = river.findIndex((station) => station.isTarget);
    expect(river[targetIndex - 1].kind).toBe('mouth');
    for (let index = targetIndex + 1; index < river.length; index += 1) {
      expect(river[index].sMeters).toBeGreaterThan(river[index - 1].sMeters);
      expect(river[index].x).toBeLessThan(river[index - 1].x);
    }
  });

  it('clears the Brickell hero zone, then gives upstream stations even route spacing', () => {
    const schematic = riverSchematic(CORRIDOR, [], STATES);
    const route = schematic.routes.find((candidate) => candidate.id === 'river')!;
    const upstream = schematic.stations
      .filter((station) => station.branchId === 'river' && station.sMeters >= 0)
      .slice()
      .sort((left, right) => left.sMeters - right.sMeters);

    const target = upstream[0];
    const upstreamStops = upstream.slice(1);
    expect(target.isTarget).toBe(true);
    expect(upstreamStops[0].x).toBeLessThanOrEqual(470);
    expect(target.x - upstreamStops[0].x).toBeGreaterThanOrEqual(220);

    const routeIndexes = upstreamStops.map((station) =>
      route.points.findIndex((point) => point.sMeters === station.sMeters)
    );
    expect(routeIndexes.every((index) => index >= 0)).toBe(true);

    const stationGaps = routeIndexes.slice(1).map((routeIndex, stationIndex) => {
      const previousRouteIndex = routeIndexes[stationIndex];
      let distance = 0;
      for (let index = previousRouteIndex; index < routeIndex; index += 1) {
        distance += Math.hypot(
          route.points[index + 1].x - route.points[index].x,
          route.points[index + 1].y - route.points[index].y
        );
      }
      return distance;
    });
    expect(Math.max(...stationGaps) - Math.min(...stationGaps)).toBeLessThan(0.001);

    for (const station of [target, ...upstreamStops]) {
      expect(schematicPointAt(route, station.sMeters)).toMatchObject({
        x: station.x,
        y: station.y
      });
    }
  });

  it('fans the three approach branches from one mouth interchange', () => {
    const schematic = riverSchematic(CORRIDOR, [], STATES);
    const byId = new Map(schematic.routes.map((route) => [route.id, route]));
    const north = byId.get('north_approach')!;
    const east = byId.get('government_cut')!;
    const south = byId.get('south_approach')!;
    const interchange = ({ x, y, sMeters }: (typeof east.points)[number]) => ({ x, y, sMeters });
    expect(interchange(north.points[0])).toEqual(interchange(east.points[0]));
    expect(interchange(south.points[0])).toEqual(interchange(east.points[0]));
    expect(east.points[0].x).toBe(990);
    expect(north.points.at(-1)!.x).toBeGreaterThan(BRICKELL_SCHEMATIC_POINT.x);
    expect(north.points.at(-1)!.y).toBeLessThan(BRICKELL_SCHEMATIC_POINT.y);
    expect(east.points.at(-1)!.y).toBe(BRICKELL_SCHEMATIC_POINT.y);
    expect(south.points.at(-1)!.x).toBeGreaterThan(BRICKELL_SCHEMATIC_POINT.x);
    expect(south.points.at(-1)!.y).toBeGreaterThan(BRICKELL_SCHEMATIC_POINT.y);
    expect(schematic.stations.filter((station) => station.kind === 'mouth')).toHaveLength(1);
  });

  it('draws a nearer vessel spatially nearer the target and preserves MMSI keys', () => {
    const schematic = riverSchematic(
      CORRIDOR,
      [
        track({ mmsi: '111111111', sMeters: 600 }),
        track({ mmsi: '999999999', sMeters: 4_500 })
      ],
      STATES
    );
    const byMmsi = new Map(schematic.vessels.map((vessel) => [vessel.mmsi, vessel]));
    const target = schematic.target!;
    const distance = (vessel: { x: number; y: number }) =>
      Math.hypot(vessel.x - target.x, vessel.y - target.y);
    expect(distance(byMmsi.get('111111111')!)).toBeLessThan(
      distance(byMmsi.get('999999999')!)
    );
    expect([...byMmsi.keys()].sort()).toEqual(['111111111', '999999999']);
  });

  it('passes engine identity, movement, timing, opening and schedule fields through unchanged', () => {
    const source = track();
    const vessel = riverSchematic(CORRIDOR, [source], STATES).vessels[0];
    expect(vessel.track).toBe(source);
    expect(vessel).toMatchObject({
      mmsi: '367705810',
      vesselName: 'SARA',
      vesselClass: 'tug',
      movement: 'approaching',
      routeIntersects: true,
      speedKnots: 4.2,
      courseDegrees: 95,
      posture: 'underway',
      sMeters: 1_200,
      branch: 'river',
      callSign: 'WDF7318',
      imoNumber: 9_123_456,
      destination: 'MIAMI RIVER',
      lengthMeters: 30,
      beamMeters: 9,
      draughtMeters: 3.1,
      openingPropensity: 6_700,
      etaMinMinutes: 6,
      etaMaxMinutes: 9,
      scheduleExempt: true,
      predictedOpeningAt: '2026-08-17T17:56:00Z',
      waitsForSlot: false,
      knownOpener: true,
      likelyToOpenBrickell: true
    });
  });

  it('maps a recent wake onto the schematic and ends it at the current hull', () => {
    const vessel = riverSchematic(CORRIDOR, [track()], STATES).vessels[0];
    expect(vessel.wake.length).toBeGreaterThanOrEqual(2);
    const last = vessel.wake.at(-1)!;
    expect(last.x).toBeCloseTo(vessel.x);
    expect(last.y).toBeCloseTo(vessel.y);
    expect(last.freshness).toBe(1);
    for (let index = 1; index < vessel.wake.length; index += 1) {
      expect(vessel.wake[index].freshness).toBeGreaterThanOrEqual(
        vessel.wake[index - 1].freshness
      );
    }
  });

  it('places a legacy branchless vessel on the river without inventing a new target', () => {
    const vessel = riverSchematic(
      CORRIDOR,
      [track({ branch: undefined, sMeters: 700 })],
      STATES
    ).vessels[0];
    expect(vessel.routeId).toBe('river');
    expect(vessel.sMeters).toBe(700);
  });

  it('returns a fixed, empty fallback when no river trunk exists', () => {
    const schematic = riverSchematic({ ...CORRIDOR, branches: [] }, [track()], STATES);
    expect(schematic).toEqual({
      width: RIVER_SCHEMATIC_WIDTH,
      height: RIVER_SCHEMATIC_HEIGHT,
      routes: [],
      stations: [],
      vessels: [],
      target: null
    });
  });
});
