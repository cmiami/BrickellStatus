import { cleanup, render } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';

import type { BridgeStateInterval, RiverCorridor, VesselTrack } from '$lib/types';

import RiverLine from './RiverLine.svelte';

/**
 * A trimmed cut of the engine's real corridor. The schematic consumes branch
 * identity, station order, and signed channel metres while the component keeps
 * the display focused on routes, moving vessels, and bridge state.
 */
function station(
  label: string,
  kind: 'target' | 'bridge' | 'mouth' | 'waypoint',
  latitude: number,
  longitude: number,
  sMeters: number,
  bridgeKey?: string
) {
  return { label, kind, bridgeKey, latitude, longitude, sMeters };
}

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
        [25.778307, -80.206931]
      ],
      stations: [
        station('River mouth', 'mouth', 25.771, -80.1849, -530),
        station('Brickell Ave', 'target', 25.7699, -80.19005, 0, 'brickell'),
        station('S Miami Ave', 'bridge', 25.7692, -80.1938, 380),
        station('SW 2 Ave', 'bridge', 25.768907, -80.197552, 780, 'sw_2_ave'),
        station('NW 5 St', 'bridge', 25.778307, -80.206931, 2180, 'nw_5_st')
      ]
    },
    {
      id: 'north_approach',
      label: 'Main Channel approach',
      corridorOffsetMeters: 150,
      centerline: [
        [25.771, -80.1849],
        [25.7779, -80.1799],
        [25.7793, -80.1665]
      ],
      stations: [
        station('River mouth', 'mouth', 25.771, -80.1849, -530),
        station('Port entrance', 'waypoint', 25.7779, -80.1799, -1470),
        station('Main Channel', 'waypoint', 25.7793, -80.1665, -3000)
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
        station('River mouth', 'mouth', 25.771, -80.1849, -530),
        station('Dodge Island', 'waypoint', 25.7682, -80.167, -2300),
        station('Government Cut', 'waypoint', 25.7622, -80.129, -7000)
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
        station('River mouth', 'mouth', 25.771, -80.1849, -530),
        station('Brickell Key', 'waypoint', 25.7663, -80.183, -1200),
        station('Rickenbacker', 'waypoint', 25.752, -80.181, -4200)
      ]
    }
  ]
};

function vessel(overrides: Partial<VesselTrack> = {}): VesselTrack {
  return {
    mmsi: '367705810',
    vesselName: 'SARA',
    vesselClass: 'tug',
    movement: 'approaching',
    routeIntersects: true,
    speedKnots: 3.4,
    courseDegrees: 95,
    observedAt: '2026-08-17T17:50:00Z',
    posture: 'underway',
    sMeters: 1200,
    branch: 'river',
    openingPropensity: 6700,
    knownOpener: true,
    likelyToOpenBrickell: true,
    etaMinMinutes: 6,
    etaMaxMinutes: 9,
    scheduleExempt: true,
    predictedOpeningAt: '2026-08-17T21:56:00Z',
    waitsForSlot: false,
    points: [
      { latitude: 25.7783, longitude: -80.2069, observedAt: '2026-08-17T17:45:00Z' },
      { latitude: 25.7731, longitude: -80.2006, observedAt: '2026-08-17T17:50:00Z' }
    ],
    ...overrides
  };
}

const INTERVALS: BridgeStateInterval[] = [
  {
    sourceId: 'fl511.bridge.brickell',
    bridgeKey: 'brickell',
    bridgeName: 'Brickell Avenue Bridge',
    relation: 'target',
    riverOrder: 0,
    state: 'down',
    startedAt: '2026-08-17T17:00:00Z'
  },
  {
    sourceId: 'fl511.bridge.brickell',
    bridgeKey: 'sw_2_ave',
    bridgeName: 'SW 2 Ave',
    relation: 'upstream',
    riverOrder: 1,
    state: 'up',
    startedAt: '2026-08-17T17:44:00Z'
  }
];

afterEach(cleanup);

function compactText(node: Element | null): string {
  return (node?.textContent ?? '').replace(/\s+/g, ' ').trim();
}

function translatedPoint(node: Element): { x: number; y: number } {
  const match = /translate\((-?[\d.]+) (-?[\d.]+)\)/.exec(node.getAttribute('transform') ?? '');
  expect(match).toBeTruthy();
  return { x: Number(match![1]), y: Number(match![2]) };
}

function transformScale(node: Element): number {
  const match = /scale\(([\d.]+)\)/.exec(node.getAttribute('transform') ?? '');
  expect(match).toBeTruthy();
  return Number(match![1]);
}

function glyphScaleX(node: Element): number {
  const match = /scale\((-?[\d.]+)\s+(-?[\d.]+)\)/.exec(node.getAttribute('transform') ?? '');
  expect(match).toBeTruthy();
  return Number(match![1]);
}

function expectOctolinear(path: Element): void {
  const points = Array.from(
    (path.getAttribute('d') ?? '').matchAll(/[ML](-?[\d.]+)\s+(-?[\d.]+)/g),
    (match) => ({ x: Number(match[1]), y: Number(match[2]) })
  );
  expect(points.length).toBeGreaterThan(1);
  for (let index = 1; index < points.length; index += 1) {
    const dx = Math.abs(points[index].x - points[index - 1].x);
    const dy = Math.abs(points[index].y - points[index - 1].y);
    expect(dx === 0 || dy === 0 || Math.abs(dx - dy) < 0.01).toBe(true);
  }
}

describe('RiverLine', () => {
  it('draws octolinear routes but labels only bridges and the shared mouth', () => {
    render(RiverLine, { corridor: CORRIDOR, vesselTracks: [], intervals: INTERVALS });

    const routes = Array.from(document.querySelectorAll('.route'));
    expect(routes).toHaveLength(4);
    expect(routes.map((route) => route.getAttribute('data-role'))).toEqual([
      'river',
      'north',
      'east',
      'south'
    ]);
    for (const route of routes) {
      const paths = Array.from(route.querySelectorAll('path'));
      expect(paths).toHaveLength(3);
      expect(new Set(paths.map((path) => path.getAttribute('d'))).size).toBe(1);
      expectOctolinear(paths[0]);
    }

    const bridgeNames = Array.from(document.querySelectorAll('.station-label')).map(compactText);
    const mouthNames = Array.from(document.querySelectorAll('.junction-label')).map(compactText);
    expect(bridgeNames).toEqual(['S Miami Ave', 'SW 2 Ave', 'NW 5 St']);
    expect(mouthNames).toEqual(['River mouth']);
    expect(document.querySelectorAll('.stations .station')).toHaveLength(4);
    expect(document.querySelector('.target-name')?.textContent).toBe('BRICKELL');

    const visibleText = compactText(document.querySelector('.stations'));
    for (const waypoint of [
      'Port entrance',
      'Main Channel',
      'Dodge Island',
      'Government Cut',
      'Brickell Key',
      'Rickenbacker'
    ]) {
      expect(visibleText).not.toContain(waypoint);
    }
  });

  it('anchors the oversized mechanical Brickell target at 690,320', () => {
    render(RiverLine, { corridor: CORRIDOR, vesselTracks: [], intervals: [] });
    const target = document.querySelector('.target-station');
    const hero = target?.querySelector('.hero-bascule');
    const minis = Array.from(document.querySelectorAll('.stations .mini-bascule'));
    const mini = minis[0];

    expect(target).toHaveAttribute('transform', 'translate(690 320)');
    expect(compactText(target?.querySelector('.target-kicker') ?? null)).toBe('ETA TARGET');
    expect(Number(target?.querySelector('.target-kicker')?.getAttribute('y'))).toBeLessThanOrEqual(
      -240
    );
    expect(compactText(document.querySelector('.route-terminals text'))).toBe('UPRIVER');
    expect(document.querySelector('.map-edge')).toBeNull();
    expect(target?.querySelector('.transit-bascule')).toHaveAttribute('data-scale', 'hero');
    expect(mini?.querySelector('.transit-bascule')).toHaveAttribute('data-scale', 'mini');
    expect(hero).toBeTruthy();
    expect(mini).toBeTruthy();
    expect(transformScale(hero!)).toBeGreaterThan(transformScale(mini!) * 2);

    expect(minis).toHaveLength(3);
    for (const bridge of minis) {
      const routeAngle = Number(bridge.getAttribute('data-route-angle'));
      const bridgeAngle = Number(bridge.getAttribute('data-bridge-angle'));
      expect(((bridgeAngle - routeAngle) % 180 + 180) % 180).toBeCloseTo(90);
      expect(bridgeAngle).toBeGreaterThanOrEqual(-90);
      expect(bridgeAngle).toBeLessThanOrEqual(90);

      // The mechanical mark rotates with the crossing; its readable name and
      // state remain sibling text in the sheet's upright coordinate system.
      const station = bridge.parentElement!;
      expect(station.querySelector('.mini-bascule .station-label')).toBeNull();
      expect(station.querySelector('.mini-bascule .station-state')).toBeNull();
      expect(station.querySelector('.station-label')).not.toHaveAttribute('transform');
      expect(station.querySelector('.station-state')).not.toHaveAttribute('transform');
    }
  });

  it('joins each bridge to its live state and writes the road consequence', () => {
    render(RiverLine, { corridor: CORRIDOR, vesselTracks: [], intervals: INTERVALS });

    const stations = Array.from(document.querySelectorAll('.station'));
    const station = (name: string) =>
      stations.find((node) => node.querySelector('title')?.textContent?.startsWith(name));
    const sw2 = station('SW 2 Ave');
    const southMiami = station('S Miami Ave');
    const brickell = document.querySelector('.target-station');

    expect(sw2).toHaveAttribute('data-state', 'up');
    expect(compactText(sw2?.querySelector('.station-state') ?? null)).toBe('up');
    expect(compactText(sw2?.querySelector('title') ?? null)).toContain('Bridge up');
    expect(southMiami).toHaveAttribute('data-state', 'none');
    expect(compactText(southMiami?.querySelector('.station-state') ?? null)).toBe('no reading');
    expect(brickell).toHaveAttribute('data-state', 'down');
    expect(compactText(brickell?.querySelector('.target-state-word') ?? null)).toBe(
      'BRIDGE down'
    );
    expect(compactText(brickell?.querySelector('.target-road-word') ?? null)).toBe(
      'road moving'
    );
    expect(compactText(brickell?.querySelector('title') ?? null)).toContain(
      'Bridge down. road moving.'
    );
  });

  it('turns the four reversed spans toward the same opening face as SW 2 and NW 5', () => {
    const extraStations = [
      station('SW 1 St', 'bridge', 25.773038, -80.200591, 1112, 'sw_1_st'),
      station('W Flagler', 'bridge', 25.774205, -80.201287, 1350, 'w_flagler'),
      station('NW 17 Ave', 'bridge', 25.785884, -80.222961, 4100, 'nw_17_ave'),
      station('NW 22 Ave', 'bridge', 25.788202, -80.231373, 4900, 'nw_22_ave')
    ];
    const corridor: RiverCorridor = {
      ...CORRIDOR,
      branches: CORRIDOR.branches.map((branch) =>
        branch.id === 'river'
          ? { ...branch, stations: [...branch.stations, ...extraStations] }
          : branch
      )
    };

    render(RiverLine, { corridor, vesselTracks: [], intervals: INTERVALS });

    const byName = (name: string) =>
      Array.from(document.querySelectorAll('.station')).find((node) =>
        node.querySelector('title')?.textContent?.startsWith(name)
      )?.querySelector('.mini-bascule');
    for (const name of ['SW 2 Ave', 'NW 5 St']) {
      const angle = Number(byName(name)?.getAttribute('data-bridge-angle'));
      expect(angle).toBeGreaterThanOrEqual(-90);
      expect(angle).toBeLessThanOrEqual(90);
    }
    for (const name of ['SW 1 St', 'W Flagler', 'NW 17 Ave', 'NW 22 Ave']) {
      const bridge = byName(name);
      const routeAngle = Number(bridge?.getAttribute('data-route-angle'));
      const bridgeAngle = Number(bridge?.getAttribute('data-bridge-angle'));
      expect(((bridgeAngle - routeAngle) % 180 + 180) % 180).toBeCloseTo(90);
      expect(bridgeAngle).toBeGreaterThan(90);
    }
  });

  it('keeps direction, knots, Brickell ETA, and bridge-opening impact with the hull', () => {
    render(RiverLine, { corridor: CORRIDOR, vesselTracks: [vessel()], intervals: INTERVALS });

    const mark = document.querySelector('.vessel');
    const ship = mark?.querySelector('.vessel-ship');
    const railItem = document.querySelector('.manifest li');
    const callout = document.querySelector('.vessel-callout');
    const point = translatedPoint(mark!);

    expect(Number.isFinite(point.x)).toBe(true);
    expect(Number.isFinite(point.y)).toBe(true);
    expect(ship).not.toHaveAttribute('transform');
    expect(glyphScaleX(ship!.querySelector('.vessel-glyph')!)).toBeGreaterThan(0);
    expect(ship?.querySelector('.vessel-glyph')).toHaveAttribute('data-family', 'tug');
    expect(compactText(railItem?.querySelector('.strip-id') ?? null)).toBe('SARA');
    expect(compactText(railItem?.querySelector('.strip-type') ?? null)).toBe(
      'Tug · Shown on route'
    );
    expect(compactText(railItem?.querySelector('.manifest-index') ?? null)).toBe('01');
    expect(compactText(railItem?.querySelector('.strip-movement') ?? null)).toBe(
      'Downriver 3.4 kn'
    );
    expect(compactText(railItem?.querySelector('.strip-eta') ?? null)).toBe(
      '6–9 minTO BRICKELL'
    );
    expect(compactText(railItem?.querySelector('.impact') ?? null)).toContain('KNOWN OPENER');
    expect(compactText(railItem?.querySelector('.impact') ?? null)).toContain(
      'LIKELY TO OPEN BRICKELL'
    );
    expect(compactText(callout?.querySelector('.callout-index-text') ?? null)).toBe('01');
    expect(compactText(callout?.querySelector('.callout-name') ?? null)).toBe('SARA');
    expect(compactText(callout?.querySelector('.callout-motion') ?? null)).toBe(
      'Downriver · 3.4 kn'
    );
    expect(compactText(callout?.querySelector('.callout-eta') ?? null)).toBe('6–9 min');

    const copy = compactText(document.body);
    expect(copy).not.toMatch(/AIS type|Type not broadcast/i);
    expect(document.querySelector('.type-key')).toBeNull();
  });

  it('shows every Brickell-bound vessel beyond three and excludes unrelated AIS contacts', () => {
    const tracks = [
      vessel({ mmsi: '111111111', vesselName: 'SARA' }),
      vessel({ mmsi: '111111112', vesselName: 'COSTA V', sMeters: 1320 }),
      vessel({ mmsi: '111111113', vesselName: 'PEPIN', sMeters: 1440 }),
      vessel({ mmsi: '111111114', vesselName: 'MIA BELLA', sMeters: 1560 }),
      vessel({ mmsi: '111111115', vesselName: 'JEANNINE D', sMeters: 1680 }),
      vessel({
        mmsi: '222222222',
        vesselName: 'PORT WATCH',
        vesselClass: 'passenger',
        posture: 'off_channel',
        branch: 'government_cut',
        sMeters: -2300,
        movement: 'unknown',
        routeIntersects: false,
        speedKnots: 7.3,
        courseDegrees: 87,
        knownOpener: false,
        likelyToOpenBrickell: false,
        etaMinMinutes: undefined,
        etaMaxMinutes: undefined,
        predictedOpeningAt: undefined
      }),
      vessel({
        mmsi: '333333333',
        vesselName: 'PEPIN',
        posture: 'moored',
        sMeters: 640,
        movement: 'stationary',
        routeIntersects: false,
        speedKnots: 0,
        knownOpener: true,
        likelyToOpenBrickell: false,
        etaMinMinutes: undefined,
        etaMaxMinutes: undefined,
        predictedOpeningAt: undefined
      }),
      vessel({
        mmsi: '444444444',
        vesselName: 'BAY HOLD',
        posture: 'holding',
        branch: 'government_cut',
        sMeters: -1900,
        movement: 'stationary',
        routeIntersects: false,
        speedKnots: 0,
        knownOpener: false,
        likelyToOpenBrickell: false,
        etaMinMinutes: undefined,
        etaMaxMinutes: undefined,
        predictedOpeningAt: undefined
      }),
      vessel({
        mmsi: '555555555',
        vesselName: 'OPEN WATER',
        posture: 'off_channel',
        branch: undefined,
        sMeters: undefined,
        movement: 'diverging',
        routeIntersects: false,
        speedKnots: 8.2,
        knownOpener: false,
        likelyToOpenBrickell: false,
        etaMinMinutes: undefined,
        etaMaxMinutes: undefined,
        predictedOpeningAt: undefined
      })
    ];

    render(RiverLine, { corridor: CORRIDOR, vesselTracks: tracks, intervals: INTERVALS });

    const rows = Array.from(document.querySelectorAll('.manifest li'));
    const names = new Set(rows.map((row) => compactText(row.querySelector('.strip-id'))));

    expect(rows).toHaveLength(5);
    expect(document.querySelectorAll('.vessel')).toHaveLength(5);
    expect(document.querySelectorAll('.vessel-callout')).toHaveLength(5);
    expect(document.querySelector('.identifier-rail')).toBeNull();
    expect(names).toEqual(new Set(['SARA', 'COSTA V', 'PEPIN', 'MIA BELLA', 'JEANNINE D']));
    expect(names).not.toContain('PORT WATCH');
    expect(names).not.toContain('BAY HOLD');
    expect(names).not.toContain('OPEN WATER');
    expect(compactText(document.querySelector('.manifest-head'))).toContain(
      'LIVE TRAFFIC · 5 BRICKELL'
    );
  });

  it('keeps side-profile boats upright and mirrors them along every schematic route', () => {
    const branches = [
      { id: 'river', sMeters: 1_200 },
      { id: 'north_approach', sMeters: -1_500 },
      { id: 'government_cut', sMeters: -2_300 },
      { id: 'south_approach', sMeters: -1_200 }
    ] as const;
    const tracks = branches.flatMap(({ id, sMeters }, index) => [
      vessel({
        mmsi: `upriver-${index}`,
        branch: id,
        sMeters,
        movement: id === 'river' ? 'diverging' : 'approaching'
      }),
      vessel({
        mmsi: `downriver-${index}`,
        branch: id,
        sMeters,
        movement: id === 'river' ? 'approaching' : 'diverging'
      })
    ]);

    render(RiverLine, { corridor: CORRIDOR, vesselTracks: tracks, intervals: INTERVALS });

    for (let index = 0; index < branches.length; index += 1) {
      for (const direction of ['upriver', 'downriver'] as const) {
        const mark = document.querySelector(`.vessel[data-mmsi='${direction}-${index}']`)!;
        const ship = mark.querySelector('.vessel-ship')!;
        const chevrons = mark.querySelector('.direction-chevrons')!;
        const glyph = ship.querySelector('.vessel-glyph')!;

        expect(ship).not.toHaveAttribute('transform');
        expect(chevrons.getAttribute('transform')).toMatch(/^rotate\(-?[\d.]+\)$/);
        expect(chevrons.getAttribute('transform')).not.toContain('NaN');
        expect(chevrons.querySelector('path')).toHaveAttribute(
          'd',
          'M38 -8L48 0L38 8 M50 -8L60 0L50 8'
        );
        expect(glyphScaleX(glyph) < 0).toBe(direction === 'upriver');
      }
    }
  });

  it('uses a solid yacht silhouette and the neutral label Vessel for an unknown type', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [
        vessel({
          mmsi: '367354090',
          vesselName: undefined,
          callSign: undefined,
          vesselClass: undefined,
          openingPropensity: undefined,
          predictedOpeningAt: undefined,
          etaMinMinutes: undefined,
          etaMaxMinutes: undefined,
          scheduleExempt: false
        })
      ],
      intervals: INTERVALS
    });

    const mark = document.querySelector(".vessel[data-mmsi='367354090']");
    const glyph = mark?.querySelector('.vessel-glyph');
    const railItem = document.querySelector('.manifest li');

    expect(glyph).toHaveAttribute('data-family', 'generic-motor-yacht');
    expect(glyph?.querySelectorAll('.hull')).toHaveLength(1);
    expect(glyph?.querySelectorAll('.house')).toHaveLength(2);
    expect(glyph?.querySelector('text')).toBeNull();
    expect(glyph?.querySelector('[stroke-dasharray]')).toBeNull();
    expect(compactText(railItem?.querySelector('.strip-id') ?? null)).toBe('367354090');
    expect(compactText(railItem?.querySelector('.strip-type') ?? null)).toBe(
      'Vessel · Shown on route'
    );
    expect(compactText(document.body)).not.toMatch(/unknown type|not broadcast|AIS type/i);
  });

  it('keeps a just-passed vessel through the opening and clears it when Brickell closes', async () => {
    const openIntervals: BridgeStateInterval[] = INTERVALS.map((interval) =>
      interval.bridgeKey === 'brickell' ? { ...interval, state: 'up' } : interval
    );
    const tracks = [
      vessel({
        mmsi: '275543000',
        vesselName: 'VIOLET',
        posture: 'underway',
        branch: 'river',
        sMeters: -40,
        movement: 'diverging',
        routeIntersects: false
      }),
      vessel({
        mmsi: '367705830',
        vesselName: 'PEPIN',
        posture: 'underway',
        branch: 'government_cut',
        sMeters: -168,
        movement: 'diverging',
        routeIntersects: false
      }),
      vessel({ mmsi: '367705810', vesselName: 'SARA' })
    ];
    const { rerender } = render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: tracks,
      intervals: openIntervals
    });

    expect(document.querySelectorAll('.vessel')).toHaveLength(2);
    expect(document.querySelectorAll('.manifest li')).toHaveLength(2);
    expect(compactText(document.body)).toContain('VIOLET');
    expect(compactText(document.body)).toContain('SARA');
    expect(compactText(document.body)).not.toContain('PEPIN');
    expect(compactText(document.querySelector('.manifest-head'))).toContain(
      'LIVE TRAFFIC · 2 BRICKELL'
    );

    await rerender({ corridor: CORRIDOR, vesselTracks: tracks, intervals: INTERVALS });

    expect(document.querySelectorAll('.vessel')).toHaveLength(1);
    expect(document.querySelectorAll('.manifest li')).toHaveLength(1);
    expect(compactText(document.body)).not.toContain('VIOLET');
    expect(compactText(document.body)).toContain('SARA');
  });

  it('omits the vessel sidebar when every current track is outside displayed corridor traffic', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [
        vessel({ mmsi: '111111111', posture: 'moored', routeIntersects: false }),
        vessel({ mmsi: '222222222', posture: 'off_channel', routeIntersects: false })
      ],
      intervals: INTERVALS
    });

    expect(document.querySelectorAll('.vessel')).toHaveLength(0);
    expect(document.querySelector('.manifest-rail')).toBeNull();
    expect(document.querySelector('.manifest')).toBeNull();
    expect(document.querySelector('.river-body')).not.toHaveClass('has-manifest');
  });

  it('states that vessel traffic is unavailable without adding source-provenance chrome', () => {
    const { getByRole, getByText } = render(RiverLine, {
      corridor: { ...CORRIDOR, aisLive: false },
      vesselTracks: [],
      intervals: INTERVALS
    });

    expect(getByText('VESSEL TRAFFIC UNAVAILABLE')).toBeTruthy();
    expect(getByRole('heading', { name: 'Miami River vessel traffic unavailable' })).toBeTruthy();
    expect(compactText(document.body)).not.toMatch(/AIS|broadcast|provenance/i);
    expect(document.querySelector('.manifest-rail')).toBeNull();
  });

  it('falls back to the call sign before the raw MMSI', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [vessel({ vesselName: undefined, callSign: 'WDF7318' })],
      intervals: INTERVALS
    });

    expect(document.querySelector('.strip-id')?.textContent).toBe('WDF7318');
    expect(compactText(document.querySelector('.vessel title'))).toContain('WDF7318');
    expect(compactText(document.body)).not.toContain('367705810');
  });

  it('preserves clustered hull positions without inventing a visual stack', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [
        vessel({ mmsi: '111111111', vesselName: 'SARA', sMeters: 1240 }),
        vessel({
          mmsi: '222222222',
          vesselName: 'COSTA V',
          sMeters: 1250,
          openingPropensity: undefined
        }),
        vessel({
          mmsi: '333333333',
          vesselName: 'PEPIN',
          sMeters: 1262,
          openingPropensity: undefined
        }),
        vessel({
          mmsi: '444444444',
          vesselName: 'MIA BELLA',
          sMeters: 1270,
          openingPropensity: undefined
        })
      ],
      intervals: INTERVALS
    });

    const marks = Array.from(document.querySelectorAll('.vessel'));
    const points = marks.map(translatedPoint);
    expect(new Set(marks.map((mark) => mark.getAttribute('transform'))).size).toBe(4);
    expect(
      Math.max(...points.map((point) => point.x)) - Math.min(...points.map((point) => point.x))
    ).toBeLessThan(6);
    expect(
      Math.max(...points.map((point) => point.y)) - Math.min(...points.map((point) => point.y))
    ).toBeLessThan(6);

    const callouts = Array.from(document.querySelectorAll('.vessel-callout'));
    expect(callouts).toHaveLength(4);
    expect(new Set(callouts.map((callout) => callout.getAttribute('transform'))).size).toBe(4);
    expect(document.querySelector('.vessel-mini-readout')).toBeNull();
    expect(
      new Set(Array.from(document.querySelectorAll('.manifest .strip-id')).map(compactText))
    ).toEqual(new Set(['SARA', 'COSTA V', 'PEPIN', 'MIA BELLA']));
    expect(document.querySelectorAll('.stacked')).toHaveLength(0);
  });
});

describe('RiverLine duplicate hulls', () => {
  // The schematic and the manifest both key on the MMSI. Two rows for one hull
  // aborted the render of the whole live deck rather than drawing it twice.
  it('renders one row per MMSI when a hull arrives twice', () => {
    const { container } = render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [vessel(), vessel({ branch: 'north_approach', sMeters: -800 })],
      intervals: INTERVALS,
      generatedAt: '2026-08-17T17:50:00Z',
      localTimeZone: 'America/New_York'
    });

    expect(compactText(container.querySelector('.manifest-head p'))).toBe('LIVE TRAFFIC · 1');
  });
});
