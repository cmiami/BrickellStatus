import { get } from 'svelte/store';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AppPreferences, AppSnapshot, MutationResult } from './types';

const api = vi.hoisted(() => ({
  getDisplayStatus: vi.fn(),
  getPreferences: vi.fn(),
  getSnapshot: vi.fn(),
  savePreferences: vi.fn()
}));

vi.mock('./api', () => api);

import {
  displayStatus, loadDisplayStatus, loadError, notice, persistPreferences, preferences, saving, snapshot,
  startDisplayStatusRefresh, startSnapshotRefresh, stopDisplayStatusRefresh, stopSnapshotRefresh
} from './state';

const draft = { profile: { name: 'draft' } } as AppPreferences;
const saved = { profile: { name: 'saved by runtime' } } as AppPreferences;
const live = { generatedAt: '2026-08-15T13:00:00Z' } as AppSnapshot;

beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  displayStatus.set(null);
  loadError.set(null);
  notice.set(null);
  preferences.set(null);
  snapshot.set(null);
  saving.set(false);
});

describe('persistPreferences', () => {
  it('turns a rejected backend invocation into a visible failed result', async () => {
    api.savePreferences.mockRejectedValue(new Error('minimum magnitude must be numeric'));

    await expect(persistPreferences(draft)).resolves.toEqual({
      ok: false,
      message: 'minimum magnitude must be numeric'
    });
    expect(get(notice)).toEqual({
      ok: false,
      message: 'minimum magnitude must be numeric'
    });
    expect(get(saving)).toBe(false);
    expect(get(preferences)).toBeNull();
  });

  it('adopts runtime-normalized preferences instead of retaining the draft', async () => {
    api.savePreferences.mockResolvedValue({ ok: true, message: 'saved' });
    api.getPreferences.mockResolvedValue(saved);
    api.getSnapshot.mockResolvedValue(live);

    await expect(persistPreferences(draft)).resolves.toEqual({ ok: true, message: 'saved' });
    expect(get(preferences)).toBe(saved);
    expect(get(snapshot)).toBe(live);
    expect(get(notice)).toEqual({ ok: true, message: 'Saved.' });
  });

  it('still speaks up when a save fails', async () => {
    api.savePreferences.mockResolvedValue({ ok: false, message: 'nope' });

    await expect(persistPreferences(draft)).resolves.toEqual({ ok: false, message: 'nope' });
    expect(get(notice)).toEqual({ ok: false, message: 'nope' });
  });
});


afterEach(() => {
  stopSnapshotRefresh();
  stopDisplayStatusRefresh();
  vi.useRealTimers();
});

it('serializes saves and stays busy until the last one finishes', async () => {
  const first = Promise.withResolvers<{ ok: boolean; message: string }>();
  api.savePreferences.mockReturnValueOnce(first.promise).mockResolvedValueOnce({ ok: true, message: 'second' });
  api.getPreferences.mockResolvedValue(saved);
  api.getSnapshot.mockResolvedValue(live);
  const a = persistPreferences(draft);
  const b = persistPreferences(saved);
  await vi.advanceTimersByTimeAsync(0);
  expect(api.savePreferences).toHaveBeenCalledTimes(1);
  expect(get(saving)).toBe(true);
  first.resolve({ ok: false, message: 'first failed' });
  await Promise.all([a, b]);
  expect(api.savePreferences.mock.calls.map(([value]) => value)).toEqual([draft, saved]);
  expect(get(preferences)).toBe(saved);
  expect(get(saving)).toBe(false);
});

it('waits for a slow snapshot before scheduling another poll and discards it after stop', async () => {
  const pending = Promise.withResolvers<AppSnapshot>();
  api.getSnapshot.mockReturnValue(pending.promise);
  startSnapshotRefresh(100);
  await vi.advanceTimersByTimeAsync(1000);
  expect(api.getSnapshot).toHaveBeenCalledTimes(1);
  stopSnapshotRefresh();
  pending.resolve(live);
  await vi.advanceTimersByTimeAsync(1000);
  expect(get(snapshot)).toBeNull();
  expect(api.getSnapshot).toHaveBeenCalledTimes(1);
});

it('does not let an old poll overwrite a completed preference save', async () => {
  const old = Promise.withResolvers<AppSnapshot>();
  api.getSnapshot.mockReturnValueOnce(old.promise).mockResolvedValue(live);
  api.savePreferences.mockResolvedValue({ ok: true, message: 'saved' });
  api.getPreferences.mockResolvedValue(saved);
  startSnapshotRefresh(100);
  await vi.advanceTimersByTimeAsync(100);
  await persistPreferences(draft);
  old.resolve({ ...live, generatedAt: '2026-08-14T13:00:00Z' });
  await vi.advanceTimersByTimeAsync(0);
  expect(get(snapshot)).toBe(live);
});

it('recovers after a poll fails', async () => {
  api.getSnapshot.mockRejectedValueOnce(new Error('offline')).mockResolvedValue(live);
  startSnapshotRefresh(100);
  await vi.advanceTimersByTimeAsync(100);
  expect(get(loadError)).toBe('offline');
  await vi.advanceTimersByTimeAsync(100);
  expect(get(loadError)).toBeNull();
  expect(get(snapshot)).toBe(live);
});

it('does not overwrite a pushed display status with an older read', async () => {
  const pending = Promise.withResolvers<import('./types').DisplayConnectionStatus>();
  api.getDisplayStatus.mockReturnValue(pending.promise);
  const loadingStatus = loadDisplayStatus();
  const pushed = { state: 'connected' } as unknown as import('./types').DisplayConnectionStatus;
  displayStatus.set(pushed);
  pending.reject(new Error('old read failed'));
  await loadingStatus;
  expect(get(displayStatus)).toBe(pushed);
});

it('does not overlap display polls or publish after stopping', async () => {
  const pending = Promise.withResolvers<import('./types').DisplayConnectionStatus>();
  api.getDisplayStatus.mockReturnValue(pending.promise);
  startDisplayStatusRefresh(100);
  await vi.advanceTimersByTimeAsync(1000);
  expect(api.getDisplayStatus).toHaveBeenCalledTimes(1);
  stopDisplayStatusRefresh();
  pending.resolve({} as import('./types').DisplayConnectionStatus);
  await vi.advanceTimersByTimeAsync(1000);
  expect(get(displayStatus)).toBeNull();
  expect(api.getDisplayStatus).toHaveBeenCalledTimes(1);
});

it('merges a queued edit with changes saved by the page just left', async () => {
  const base = { profile: { name: 'Original' }, unitSystem: 'imperial' } as AppPreferences;
  const first = { ...base, profile: { ...base.profile, name: 'First page' } };
  const second = { ...base, unitSystem: 'metric' as const };
  const merged = { ...first, unitSystem: 'metric' as const };
  preferences.set(base);
  const pending = Promise.withResolvers<MutationResult>();
  api.savePreferences.mockReturnValueOnce(pending.promise).mockResolvedValue({ ok: true, message: 'saved' });
  api.getPreferences.mockResolvedValueOnce(first).mockResolvedValueOnce(merged);
  api.getSnapshot.mockResolvedValue(live);
  const a = persistPreferences(first, base);
  const b = persistPreferences(second, base);
  await vi.advanceTimersByTimeAsync(0);
  expect(api.savePreferences).toHaveBeenCalledTimes(1);
  pending.resolve({ ok: true, message: 'saved' });
  await Promise.all([a, b]);
  expect(api.savePreferences).toHaveBeenNthCalledWith(2, merged);
  expect(get(preferences)).toEqual(merged);
});
