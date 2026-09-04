import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { get } from 'svelte/store';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

import Harness from './components/__fixtures__/PreferencesEditorHarness.svelte';
import { notice, preferences } from './state';
import type { AppPreferences, MutationResult } from './types';

const api = vi.hoisted(() => ({
  getPreferences: vi.fn(), getSnapshot: vi.fn(), savePreferences: vi.fn()
}));
vi.mock('./api', () => api);

let stored: AppPreferences;
beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  stored = { profile: { name: 'Original', channels: [{ id: 'bridge', title: 'Bridge' }] }, unitSystem: 'imperial' } as AppPreferences;
  preferences.set(stored);
  notice.set(null);
  api.getPreferences.mockImplementation(() => stored);
  api.getSnapshot.mockResolvedValue(null);
  api.savePreferences.mockImplementation(async (next: AppPreferences) => {
    stored = { ...next, profile: { ...next.profile, name: next.profile.name.trim() } };
    return { ok: true, message: 'saved' };
  });
});

afterEach(async () => {
  cleanup();
  await vi.advanceTimersByTimeAsync(0);
  vi.useRealTimers();
});

it('debounces a burst and adopts backend normalization without resaving it', async () => {
  render(Harness);
  const input = screen.getByRole('textbox', { name: 'Name' });
  await fireEvent.input(input, { target: { value: 'N' } });
  await vi.advanceTimersByTimeAsync(500);
  await fireEvent.input(input, { target: { value: ' New name ' } });
  await vi.advanceTimersByTimeAsync(699);
  expect(api.savePreferences).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  expect(input).toHaveValue('New name');
  await vi.advanceTimersByTimeAsync(5000);
  expect(api.savePreferences).toHaveBeenCalledTimes(1);
});

it('keeps a local edit while adopting an external units change', async () => {
  render(Harness);
  await fireEvent.input(screen.getByRole('textbox', { name: 'Channel' }), { target: { value: 'Edited' } });
  preferences.set({ ...stored, unitSystem: 'metric' });
  await tick();
  expect(screen.getByText('metric')).toBeInTheDocument();
  await vi.advanceTimersByTimeAsync(700);
  expect(stored.profile.channels[0].title).toBe('Edited');
  expect(stored.unitSystem).toBe('metric');
});

it('saves an edit made while the previous write is pending', async () => {
  const pending = Promise.withResolvers<MutationResult>();
  api.savePreferences.mockImplementationOnce((next: AppPreferences) => {
    stored = next;
    return pending.promise;
  });
  render(Harness);
  const input = screen.getByRole('textbox', { name: 'Name' });
  await fireEvent.input(input, { target: { value: 'First' } });
  await vi.advanceTimersByTimeAsync(700);
  await fireEvent.input(input, { target: { value: 'Second' } });
  await vi.advanceTimersByTimeAsync(700);
  expect(api.savePreferences).toHaveBeenCalledTimes(1);
  pending.resolve({ ok: true, message: 'saved' });
  await vi.advanceTimersByTimeAsync(0);
  expect(stored.profile.name).toBe('Second');
  expect(api.savePreferences).toHaveBeenCalledTimes(2);
});

it('leaves a rejected draft visible without retrying indefinitely', async () => {
  api.savePreferences.mockResolvedValue({ ok: false, message: 'invalid name' });
  render(Harness);
  const input = screen.getByRole('textbox', { name: 'Name' });
  await fireEvent.input(input, { target: { value: 'Invalid' } });
  await vi.advanceTimersByTimeAsync(5000);
  expect(api.savePreferences).toHaveBeenCalledTimes(1);
  expect(input).toHaveValue('Invalid');
  expect(get(notice)?.ok).toBe(false);
});

it('flushes a pending edit on navigation', async () => {
  const { unmount } = render(Harness);
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Before leaving' } });
  unmount();
  await vi.advanceTimersByTimeAsync(0);
  expect(stored.profile.name).toBe('Before leaving');
  expect(api.savePreferences).toHaveBeenCalledTimes(1);
});
