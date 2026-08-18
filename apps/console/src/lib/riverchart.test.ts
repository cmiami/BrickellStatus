import { describe, expect, it } from 'vitest';

import { riverChart } from './riverchart';
import type { RiverCorridor, VesselTrack } from './types';

/** A trimmed cut of the engine's real geometry, stations on the water. */
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
        [25.768907, -80.197552],
        [25.778307, -80.206931],
        [25.79267, -80.23965]
      ],
      stations: [
        { label: 'River mouth', kind: 'mouth', latitude: 25.771, longitude: -80.1849, sMeters: -530 },
        {
          label: 'Brickell Ave',
          kind: 'target',
          bridgeKey: 'brickell',
          latitude: 25.7699,
          longitude: -80.19005,
          sMeters: 0
        },
        {
          label: 'NW 27 Ave',
          kind: 'bridge',
          bridgeKey: 'nw_27_ave',
          latitude: 25.79267,
          longitude: -80.23965,
          sMeters: 5860
        }
      ]
    },
    {
      id: 'north_approach',
      label: 'Main Channel approach',
      corridorOffsetMeters: 150,
      centerline: [
        [25.771, -80.1849],
        [25.7779, -80.1799],
        [25.7645, -80.133]
      ],
      stations: [
        { label: 'River mouth', kind: 'mouth', latitude: 25.771, longitude: -80.1849, sMeters: -530 },
        {
          label: 'Government Cut',
          kind: 'waypoint',
          latitude: 25.7645,
          longitude: -80.133,
          sMeters: -6600
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
    posture: 'underway',
    sMeters: 1200,
    branch: 'river',
    points: [
      { latitude: 25.774205, longitude: -80.201287, observedAt: '2026-08-17T17:40:00Z' },
      { latitude: 25.773038, longitude: -80.200591, observedAt: '2026-08-17T17:45:00Z' },
      { latitude: 25.768907, longitude: -80.197552, observedAt: '2026-08-17T17:50:00Z' }
    ],
    ...overrides
  };
}

const NO_STATES = new Map<string, 'up' | 'down' | 'unknown'>();

describe('riverChart', () => {
  it('puts the target span at the centre of the range rings', () => {
    const chart = riverChart(CORRIDOR, [], NO_STATES);
    const target = chart.stations.find((station) => station.isTarget);
    expect(target).toBeTruthy();
    expect(Math.abs(target!.x - chart.bridgeX)).toBeLessThan(1.5);
    expect(Math.abs(target!.y - chart.bridgeY)).toBeLessThan(1.5);
    // Rings grow with range but sub-linearly: the compression is real.
    const [inner, outer] = [chart.rings[0], chart.rings[chart.rings.length - 1]];
    expect(outer.radius).toBeGreaterThan(inner.radius);
    expect(outer.radius / inner.radius).toBeLessThan(outer.kilometers / inner.kilometers);
  });

  it('keeps every drawn coordinate finite and on the sheet', () => {
    const chart = riverChart(CORRIDOR, [track()], NO_STATES);
    for (const station of chart.stations) {
      expect(Number.isFinite(station.x)).toBe(true);
      expect(Number.isFinite(station.y)).toBe(true);
      expect(station.x).toBeGreaterThanOrEqual(0);
      expect(station.x).toBeLessThanOrEqual(chart.width);
      expect(station.y).toBeGreaterThanOrEqual(0);
      expect(station.y).toBeLessThanOrEqual(chart.height);
    }
    for (const vessel of chart.vessels) {
      expect(Number.isFinite(vessel.x)).toBe(true);
      expect(Number.isFinite(vessel.y)).toBe(true);
      expect(Number.isFinite(vessel.angleDegrees)).toBe(true);
    }
    for (const branch of chart.branches) {
      expect(branch.ribbon).not.toContain('NaN');
      expect(branch.centerline).not.toContain('NaN');
    }
  });

  it('preserves channel order: a nearer vessel draws nearer the span', () => {
    const chart = riverChart(
      CORRIDOR,
      [
        track({ mmsi: '111111111', sMeters: 600 }),
        track({ mmsi: '222222222', sMeters: 2400 })
      ],
      NO_STATES
    );
    const byMmsi = new Map(chart.vessels.map((vessel) => [vessel.mmsi, vessel]));
    const near = byMmsi.get('111111111')!;
    const far = byMmsi.get('222222222')!;
    const range = (v: { x: number; y: number }) =>
      Math.hypot(v.x - chart.bridgeX, v.y - chart.bridgeY);
    expect(range(near)).toBeLessThan(range(far));
  });

  it('pins a vessel past the drawn water to its end rather than dropping it', () => {
    const chart = riverChart(
      CORRIDOR,
      [track({ mmsi: '999999999', sMeters: 55_000 })],
      NO_STATES
    );
    expect(chart.vessels.length).toBe(1);
    expect(Number.isFinite(chart.vessels[0].x)).toBe(true);
    expect(chart.vessels[0].x).toBeGreaterThanOrEqual(0);
    expect(chart.vessels[0].x).toBeLessThanOrEqual(chart.width);
  });

  it('draws a wake behind a moving hull that fades toward the oldest fix', () => {
    const chart = riverChart(CORRIDOR, [track()], NO_STATES);
    const wake = chart.vessels[0].wake;
    expect(wake.length).toBeGreaterThanOrEqual(3);
    for (let index = 1; index < wake.length; index += 1) {
      expect(wake[index].freshness).toBeGreaterThanOrEqual(wake[index - 1].freshness);
    }
    // The wake ends at the hull itself.
    const last = wake[wake.length - 1];
    expect(Math.abs(last.x - chart.vessels[0].x)).toBeLessThan(0.5);
    expect(Math.abs(last.y - chart.vessels[0].y)).toBeLessThan(0.5);
  });

  it('gives a holding vessel no wake and aligns it with the channel', () => {
    const chart = riverChart(
      CORRIDOR,
      [track({ movement: 'stationary', posture: 'waiting' })],
      NO_STATES
    );
    expect(chart.vessels.length).toBe(1);
    expect(chart.vessels[0].wake.length).toBe(0);
    expect(Number.isFinite(chart.vessels[0].angleDegrees)).toBe(true);
  });

  it('carries live bascule state onto its station by FL511 key', () => {
    const chart = riverChart(
      CORRIDOR,
      [],
      new Map([
        ['brickell', 'down' as const],
        ['nw_27_ave', 'up' as const]
      ])
    );
    const byLabel = new Map(chart.stations.map((station) => [station.label, station]));
    expect(byLabel.get('Brickell Ave')?.state).toBe('down');
    expect(byLabel.get('NW 27 Ave')?.state).toBe('up');
    expect(byLabel.get('River mouth')?.state).toBeUndefined();
  });

  it('tapers the ribbon: the water draws narrower far from the span', () => {
    const chart = riverChart(CORRIDOR, [], NO_STATES);
    const byLabel = new Map(chart.stations.map((station) => [station.label, station]));
    const near = byLabel.get('Brickell Ave')!;
    const far = byLabel.get('NW 27 Ave')!;
    expect(far.halfWidth).toBeLessThan(near.halfWidth);
  });

  it('returns an empty chart rather than throwing when the trunk is missing', () => {
    const chart = riverChart({ ...CORRIDOR, branches: [] }, [track()], NO_STATES);
    expect(chart.branches).toEqual([]);
    expect(chart.vessels).toEqual([]);
    expect(chart.width).toBeGreaterThan(0);
    expect(chart.height).toBeGreaterThan(0);
  });
});
