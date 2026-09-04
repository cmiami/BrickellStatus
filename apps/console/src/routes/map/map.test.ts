import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';

import { preferences, snapshot } from '$lib/state';
import type { AppPreferences } from '$lib/types';
import MapPage from './+page.svelte';

const api = vi.hoisted(() => ({
  savePreferences: vi.fn(), getPreferences: vi.fn(), getSnapshot: vi.fn(),
  getRadarLayer: vi.fn().mockResolvedValue(null), getVesselDetail: vi.fn()
}));
vi.mock('$lib/api', () => api);
vi.mock('$lib/components/LocationMap.svelte', async () => ({
  default: (await import('$lib/components/__fixtures__/LocationMapStub.svelte')).default
}));

afterEach(() => {
  cleanup();
  preferences.set(null);
  snapshot.set(null);
});

it('keeps a failed pin save open and allows retrying without a duplicate area', async () => {
  api.savePreferences.mockResolvedValue({ ok: false, message: 'Storage unavailable' });
  preferences.set({
    unitSystem: 'imperial', areas: [], profile: { name: 'Fixture', channels: [] }
  } as unknown as AppPreferences);
  render(MapPage);
  await fireEvent.click(screen.getByTestId('map-pick'));
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Test place' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Storage unavailable');
  expect(screen.getByRole('dialog')).toBeInTheDocument();
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(api.savePreferences).toHaveBeenCalledTimes(2);
  for (const [payload] of api.savePreferences.mock.calls) expect(payload.areas).toHaveLength(1);
});
