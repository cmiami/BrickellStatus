import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/components/LiveDeck.svelte', async () => {
  const { default: Stub } = await import('$lib/components/__fixtures__/LiveDeckStub.svelte');
  return { default: Stub };
});

import { displayStatus, loading, snapshot } from '$lib/state';
import type { AppSnapshot, ChannelSnapshot } from '$lib/types';
import LivePage from './+page.svelte';

const GENERATED_AT = '2026-08-23T16:00:00Z';

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(GENERATED_AT));
});

function notice(id: string, headline: string, expiresAt: string): ChannelSnapshot {
  return {
    id,
    kind: id.startsWith('weather') ? 'weather' : 'earthquake',
    title: id.startsWith('weather') ? 'Miami weather' : 'Significant earthquakes',
    sourceLabel: 'Fixture source',
    availability: 'fresh',
    ageSeconds: 10,
    coverageComplete: true,
    summary: headline,
    materialKey: id,
    signal: { headline, detail: `${headline} detail`, action: 'Review', expiresAt },
    priority: { score: 500, urgency: 'heads_up', confirmed: true },
    enabled: true,
    active: true,
    presence: 'active_only',
    interruptPreset: 'recommended',
    destinations: ['epaper']
  };
}

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  snapshot.set(null);
  displayStatus.set(null);
  loading.set(true);
});

describe('Live page current notices', () => {
  it('mounts the notice rail and excludes notices at the current expiry boundary', () => {
    snapshot.set({
      generatedAt: GENERATED_AT,
      localTimeZone: 'America/New_York',
      channels: [
        notice('weather.miami', 'Rain in 8 minutes', '2026-08-23T16:20:00Z'),
        notice('earthquake.significant', 'Expired earthquake', GENERATED_AT)
      ],
      vesselTracks: [],
      bridgeIntervals: [],
      bridgeCrossings: []
    } as unknown as AppSnapshot);
    loading.set(false);

    render(LivePage);

    expect(screen.getByTestId('live-deck-stub')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Alerts' })).toBeInTheDocument();
    expect(screen.getByText('Rain in 8 minutes')).toBeInTheDocument();
    expect(screen.queryByText('Expired earthquake')).toBeNull();
  });
});
