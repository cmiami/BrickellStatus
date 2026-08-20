import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { AppPreferences, AppSnapshot } from './types';

const api = vi.hoisted(() => ({
  getPreferences: vi.fn(),
  getSnapshot: vi.fn(),
  savePreferences: vi.fn()
}));

vi.mock('./api', () => api);

import { notice, persistPreferences, preferences, saving, snapshot } from './state';

const draft = { profile: { name: 'draft' } } as AppPreferences;
const saved = { profile: { name: 'saved by runtime' } } as AppPreferences;
const live = { generatedAt: '2026-08-15T13:00:00Z' } as AppSnapshot;

beforeEach(() => {
  vi.clearAllMocks();
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
    // Confirmation is transient: it appears and clears itself. The alternative
    // tried before this was a permanent indicator on each desk, which read as a
    // greyed-out Save button -- the one affordance this app must never appear
    // to have. Writes are debounced, so this is one notice per burst of edits.
    expect(get(notice)).toEqual({ ok: true, message: 'Saved.' });
  });

  it('still speaks up when a save fails', async () => {
    api.savePreferences.mockResolvedValue({ ok: false, message: 'nope' });

    await expect(persistPreferences(draft)).resolves.toEqual({ ok: false, message: 'nope' });
    expect(get(notice)).toEqual({ ok: false, message: 'nope' });
  });
});
