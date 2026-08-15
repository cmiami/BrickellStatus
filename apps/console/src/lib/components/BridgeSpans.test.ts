import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';

import type { BridgeStateInterval } from '$lib/types';

import BridgeSpans from './BridgeSpans.svelte';

const ZONE = 'America/New_York';

function interval(overrides: Partial<BridgeStateInterval> = {}): BridgeStateInterval {
  return {
    sourceId: 'fl511.bridge.brickell',
    bridgeKey: 'brickell',
    bridgeName: 'Brickell Avenue Bridge',
    relation: 'target',
    riverOrder: 0,
    state: 'down',
    startedAt: '2026-08-15T17:00:00Z',
    ...overrides
  };
}

const RIVER_ORDER: Record<string, number> = {
  brickell: 0,
  sw_2_ave: 1,
  sw_1_st: 2,
  w_flagler: 3,
  nw_5_st: 4,
  nw_12_ave: 5
};

function upstream(key: string, name: string, extra: Partial<BridgeStateInterval> = {}) {
  return interval({
    bridgeKey: key,
    bridgeName: name,
    relation: 'upstream',
    riverOrder: RIVER_ORDER[key] ?? 9,
    ...extra
  });
}

afterEach(cleanup);

describe('BridgeSpans', () => {
  it('marks an open-ended up interval as open and a closed-out one as closed', () => {
    render(BridgeSpans, {
      intervals: [
        interval({ state: 'up', startedAt: '2026-08-15T18:00:00Z' }),
        upstream('sw_2_ave', 'SW 2 Ave', {
          state: 'up',
          startedAt: '2026-08-15T17:30:00Z',
          endedAt: '2026-08-15T17:40:00Z'
        })
      ],
      localTimeZone: ZONE
    });

    const target = screen.getByLabelText(/Brickell Avenue Bridge\. Open since/);
    expect(target.getAttribute('data-state')).toBe('up');

    // The upstream span's interval ended, so it describes history. Reporting it
    // as still up would be the exact failure this panel exists to avoid.
    const closed = screen.getByTitle(/SW 2 Ave\. Closed\./);
    expect(closed.getAttribute('data-state')).toBe('down');
  });

  it('shows the opening time in the bridge time zone, not the viewer time zone', () => {
    render(BridgeSpans, {
      // 18:20Z in August is 14:20 in Miami.
      intervals: [interval({ state: 'up', startedAt: '2026-08-15T18:20:00Z' })],
      localTimeZone: ZONE
    });
    expect(screen.getByText(/Opened 2:20 PM/)).toBeTruthy();
  });

  it('prefers an in-progress interval over a newer completed one', () => {
    render(BridgeSpans, {
      intervals: [
        interval({ state: 'up', startedAt: '2026-08-15T18:00:00Z' }),
        interval({
          state: 'down',
          startedAt: '2026-08-15T18:05:00Z',
          endedAt: '2026-08-15T18:06:00Z'
        })
      ],
      localTimeZone: ZONE
    });
    expect(
      screen.getByLabelText(/Brickell Avenue Bridge\. Open since/).getAttribute('data-state')
    ).toBe('up');
  });

  it('lists every upstream span so an opening can be lined up against Brickell', () => {
    render(BridgeSpans, {
      intervals: [
        interval({ state: 'down' }),
        upstream('sw_2_ave', 'SW 2 Ave', { state: 'up', startedAt: '2026-08-15T18:10:00Z' }),
        upstream('sw_1_st', 'SW 1 St', { state: 'down' }),
        upstream('w_flagler', 'W Flagler', { state: 'unknown' })
      ],
      localTimeZone: ZONE
    });
    expect(screen.getByTitle(/SW 2 Ave\. Open since/)).toBeTruthy();
    expect(screen.getByTitle(/SW 1 St\. Closed\./)).toBeTruthy();
    expect(screen.getByTitle(/W Flagler\. Unknown\./)).toBeTruthy();
  });

  it('renders without a target span rather than throwing', () => {
    render(BridgeSpans, {
      intervals: [upstream('sw_1_st', 'SW 1 St', { state: 'down' })],
      localTimeZone: ZONE
    });
    expect(screen.getByTitle(/SW 1 St\. Closed\./)).toBeTruthy();
  });

  it('renders with no intervals at all', () => {
    render(BridgeSpans, { intervals: [], localTimeZone: ZONE });
    expect(screen.getByLabelText('Miami River bascule spans')).toBeTruthy();
  });

  it('lists upstream spans in river order, not alphabetical order', () => {
    // Alphabetically NW 12 Ave precedes SW 2 Ave, which would scramble the one
    // property that makes the row readable: an opening walking along it is a
    // vessel under way.
    render(BridgeSpans, {
      intervals: [
        interval({ state: 'down' }),
        upstream('nw_12_ave', 'NW 12 Ave'),
        upstream('sw_2_ave', 'SW 2 Ave'),
        upstream('nw_5_st', 'NW 5 St'),
        upstream('sw_1_st', 'SW 1 St')
      ],
      localTimeZone: ZONE
    });
    const names = Array.from(document.querySelectorAll('.upstream li strong')).map(
      (node) => node.textContent
    );
    expect(names).toEqual(['SW 2 Ave', 'SW 1 St', 'NW 5 St', 'NW 12 Ave']);
  });

  it('nests the upstream spans inside Brickell rather than beside it', () => {
    // Brickell is the subject; every other span is context for it. If the
    // upstream list is a sibling it competes for the same attention, which is
    // the opposite of what this panel is for.
    render(BridgeSpans, {
      intervals: [
        interval({ state: 'down' }),
        upstream('sw_2_ave', 'SW 2 Ave'),
        upstream('sw_1_st', 'SW 1 St')
      ],
      localTimeZone: ZONE
    });
    const target = document.querySelector('.target');
    expect(target).toBeTruthy();
    expect(target?.querySelectorAll('.upstream li').length).toBe(2);
    expect(document.querySelectorAll('.spans > .upstream').length).toBe(0);
  });

  it('carries the open/closed state onto both the target and each child', () => {
    // The colour is driven entirely by data-state, on the target block, the
    // bascule drawing, and every child row.
    render(BridgeSpans, {
      intervals: [
        interval({ state: 'up', startedAt: '2026-08-15T18:00:00Z' }),
        upstream('sw_2_ave', 'SW 2 Ave', { state: 'down' }),
        upstream('sw_1_st', 'SW 1 St', { state: 'up', startedAt: '2026-08-15T18:05:00Z' })
      ],
      localTimeZone: ZONE
    });
    expect(document.querySelector('.target')?.getAttribute('data-state')).toBe('up');
    const rows = Array.from(document.querySelectorAll('.upstream li')).map((node) =>
      node.getAttribute('data-state')
    );
    expect(rows).toEqual(['down', 'up']);
    // Both the target and the children draw a bascule, not just a label.
    expect(document.querySelectorAll('.bascule').length).toBe(1);
    expect(document.querySelectorAll('.mini-bascule').length).toBe(2);
  });
});
