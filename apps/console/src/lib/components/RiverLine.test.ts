import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';

import type { BridgeStateInterval, RiverCorridor, VesselTrack } from '$lib/types';

import RiverLine from './RiverLine.svelte';

function station(
  label: string,
  kind: 'target' | 'bridge' | 'mouth' | 'waypoint',
  sMeters: number,
  bridgeKey?: string
) {
  return { label, kind, bridgeKey, latitude: 25.77, longitude: -80.19, sMeters };
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
        [25.778307, -80.206931]
      ],
      stations: [
        station('River mouth', 'mouth', -520),
        station('Brickell Ave', 'target', 0, 'brickell'),
        station('S Miami Ave', 'bridge', 380),
        station('SW 2 Ave', 'bridge', 780, 'sw_2_ave'),
        station('NW 5 St', 'bridge', 2180, 'nw_5_st'),
        // Beyond the drawn reach: must be dropped, not squeezed in.
        station('Palmer Lake', 'waypoint', 8200)
      ]
    },
    {
      id: 'north_approach',
      label: 'Main Channel approach',
      corridorOffsetMeters: 150,
      centerline: [
        [25.771, -80.1849],
        [25.7635, -80.133]
      ],
      stations: [
        station('River mouth', 'mouth', -520),
        station('Bayfront ICW', 'waypoint', -900),
        station('Government Cut', 'waypoint', -5200)
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
    openingPropensity: 6700,
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

describe('RiverLine', () => {
  it('draws the channel with its bridges and seaward marks named', () => {
    render(RiverLine, { corridor: CORRIDOR, vesselTracks: [], intervals: INTERVALS });

    // Read the drawn labels, not the accessible <title> that repeats them.
    const drawn = Array.from(document.querySelectorAll('.station-label')).map(
      (node) => node.textContent
    );
    expect(drawn).toContain('Brickell Ave');
    expect(drawn).toContain('SW 2 Ave');
    // The bascule FL511 cannot see is still drawn; a blind spot is worth showing.
    expect(drawn).toContain('S Miami Ave');
    // Seaward marks come from the approach channels.
    expect(drawn).toContain('Government Cut');
    // The main channel and the river are one continuous spine, drawn as one
    // line per row; the south approach forks off it as its own dashed leg.
    expect(document.querySelectorAll('.line').length).toBeGreaterThanOrEqual(1);
    expect(document.querySelectorAll('.line.approach').length).toBe(0);
  });

  it('wraps the channel into rows instead of one unreadable line', () => {
    render(RiverLine, { corridor: CORRIDOR, vesselTracks: [], intervals: [] });
    const stations = Array.from(document.querySelectorAll('.station rect, .station circle'));
    // Laid out straight these labels collide; the serpentine is what buys the
    // room, so more than one row must actually exist.
    const rows = new Set(
      Array.from(document.querySelectorAll('.station text')).map((node) =>
        Math.round(Number(node.getAttribute('y')) / 40)
      )
    );
    expect(stations.length).toBeGreaterThan(6);
    expect(rows.size).toBeGreaterThan(1);
  });

  it('carries live bascule state onto the station it belongs to', () => {
    render(RiverLine, { corridor: CORRIDOR, vesselTracks: [], intervals: INTERVALS });
    const states = Array.from(document.querySelectorAll('.station')).map((node) => ({
      title: node.querySelector('title')?.textContent ?? '',
      state: node.getAttribute('data-state')
    }));
    const span = (name: string) => states.find((s) => s.title.startsWith(name));
    expect(span('SW 2 Ave')?.state).toBe('up');
    expect(span('Brickell Ave')?.state).toBe('down');
    // No FL511 selector exists for this span, so it must claim no state.
    expect(span('S Miami Ave')?.state).toBe('none');
    // A span says what it is doing, not merely what it is called.
    expect(span('SW 2 Ave')?.title).toContain('open');
    expect(span('Brickell Ave')?.title).toContain('closed');
  });

  it('places a vessel on the line and reports distance, heading and arrival', () => {
    render(RiverLine, { corridor: CORRIDOR, vesselTracks: [vessel()], intervals: INTERVALS });

    expect(document.querySelectorAll('.vessel').length).toBe(1);
    const transform = document.querySelector('.vessel')?.getAttribute('transform') ?? '';
    expect(transform).not.toContain('NaN');
    expect(screen.getByText('Downriver')).toBeTruthy();
    expect(screen.getByText('1.2 km')).toBeTruthy();
    expect(screen.getByText('6–9 min')).toBeTruthy();
  });

  it('says a commercial hull opens on signal and an ordinary one waits for a slot', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [
        vessel(),
        vessel({
          mmsi: '338215012',
          vesselName: 'BRIGHT SIDE',
          vesselClass: 'pleasure craft',
          openingPropensity: 3300,
          scheduleExempt: false,
          waitsForSlot: true,
          predictedOpeningAt: '2026-08-17T23:00:00Z'
        })
      ],
      intervals: INTERVALS
    });

    expect(screen.getByText('opens on signal')).toBeTruthy();
    expect(screen.getByText('waits for slot')).toBeTruthy();
    // The commercial exemption and the learned opener are separate claims.
    expect(screen.getAllByText('Commercial').length).toBe(1);
    // The opener is tagged twice on purpose: once on the water, once in the
    // list, so it is unmissable whichever the reader is looking at.
    expect(document.querySelectorAll('.opener-tag').length).toBe(1);
    expect(document.querySelectorAll('.manifest .tag.opens').length).toBe(1);
  });

  it('says the source is off rather than showing an empty river', () => {
    render(RiverLine, {
      corridor: { ...CORRIDOR, aisLive: false },
      vesselTracks: [],
      intervals: INTERVALS
    });
    expect(screen.getByText(/AIS source is off/)).toBeTruthy();
    // The channel itself is still drawn: the geometry is true regardless.
    const drawn = Array.from(document.querySelectorAll('.station-label')).map(
      (node) => node.textContent
    );
    expect(drawn).toContain('Brickell Ave');
  });

  it('says nothing about berthed craft at all', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [
        vessel({ mmsi: '111111111', posture: 'moored' }),
        vessel({ mmsi: '222222222', posture: 'off_channel' })
      ],
      intervals: INTERVALS
    });
    // A hull tied up at a pier is not news. It is not drawn, not listed, and
    // not counted — a tally of things to ignore is still something to read.
    expect(document.querySelectorAll('.vessel').length).toBe(0);
    expect(document.querySelectorAll('.manifest li').length).toBe(0);
    const header = document.querySelector('.river-count')?.textContent ?? '';
    expect(header).toMatch(/Nothing under way/);
    expect(header).not.toMatch(/berth/i);
    expect(document.body.textContent).not.toMatch(/berthed/i);
  });

  it('shows identity only where the vessel has broadcast it', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [
        vessel({
          mmsi: '367705810',
          vesselName: 'SARA',
          callSign: 'WDF7318',
          lengthMeters: 30,
          beamMeters: 9,
          destination: 'MIAMI RIVER'
        }),
        // Broadcast nothing but a position: no name, no size, no destination.
        vessel({
          mmsi: '367354090',
          vesselName: undefined,
          vesselClass: undefined,
          lengthMeters: undefined,
          beamMeters: undefined,
          etaMinMinutes: undefined,
          etaMaxMinutes: undefined,
          predictedOpeningAt: undefined,
          openingPropensity: undefined,
          scheduleExempt: false
        })
      ],
      intervals: INTERVALS
    });

    const manifest = document.querySelector('.manifest')?.textContent ?? '';
    expect(manifest).toContain('WDF7318');
    expect(manifest).toContain('30×9 m');
    expect(manifest).toContain('for MIAMI RIVER');
    // The unidentified hull falls back to its MMSI and shows no empty slots.
    expect(manifest).toContain('367354090');
    // No placeholder cells: an unknown field is absent, not a dash.
    expect(manifest).not.toContain('—');
    expect(manifest).not.toMatch(/unreported|not reported/i);
  });

  it('falls back to the call sign before the raw MMSI', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [vessel({ vesselName: undefined, callSign: 'WDF7318' })],
      intervals: INTERVALS
    });
    expect(document.querySelector('.manifest')?.textContent).toContain('WDF7318');
    // And the drawing labels it the same way, never as a bare MMSI.
    expect(document.querySelector('.vessel-tag')?.textContent).toBe('WDF7318');
  });

  it('stacks vessels sharing one spot so their names never overlap', () => {
    render(RiverLine, {
      corridor: CORRIDOR,
      vesselTracks: [
        vessel({ mmsi: '111111111', vesselName: 'SARA', sMeters: 1240 }),
        vessel({ mmsi: '222222222', vesselName: 'COSTA V', sMeters: 1250 }),
        vessel({ mmsi: '333333333', vesselName: 'PEPIN', sMeters: 1262 })
      ],
      intervals: INTERVALS
    });
    const marks = Array.from(document.querySelectorAll('.vessel')).map((node) => {
      const [, x, y] = /translate\((-?[\d.]+) (-?[\d.]+)\)/.exec(
        node.getAttribute('transform') ?? ''
      ) ?? ['', '0', '0'];
      return { x: Number(x), y: Number(y) };
    });
    // One column, distinct rows: a readable list rather than a single blob.
    expect(new Set(marks.map((mark) => mark.x)).size).toBe(1);
    expect(new Set(marks.map((mark) => mark.y)).size).toBe(3);
  });
});
