import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';

import { snapshot } from '$lib/state';
import type { AppSnapshot } from '$lib/types';
import Log from './+page.svelte';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  snapshot.set(null);
});

it('allows access to the full history without IntersectionObserver', async () => {
  vi.stubGlobal('IntersectionObserver', undefined);
  snapshot.set({
    channels: [],
    dispatches: [],
    bridgeIntervals: Array.from({ length: 45 }, (_, index) => ({
      sourceId: 'fl511.bridge.brickell', bridgeKey: 'brickell', bridgeName: `Bridge ${index}`,
      relation: 'target', state: 'down', startedAt: new Date(index * 60_000).toISOString(), endedAt: null
    }))
  } as unknown as AppSnapshot);
  render(Log);
  expect(screen.queryByText('Bridge 44')).not.toBeInTheDocument();
  await fireEvent.click(screen.getByRole('button', { name: 'Show more intervals' }));
  expect(screen.getByText('Bridge 44')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Show more intervals' })).not.toBeInTheDocument();
});
