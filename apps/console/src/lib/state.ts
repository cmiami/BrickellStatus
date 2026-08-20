import { derived, get, writable } from 'svelte/store';

import { getPreferences, getSnapshot, savePreferences } from './api';
import type { AppPreferences, AppSnapshot, MutationResult } from './types';

export const snapshot = writable<AppSnapshot | null>(null);
export const preferences = writable<AppPreferences | null>(null);
export const loading = writable(true);
export const loadError = writable<string | null>(null);
export const saving = writable(false);
export const notice = writable<MutationResult | null>(null);

export const activeChannels = derived(snapshot, ($snapshot) =>
  $snapshot?.channels.filter((channel) => channel.enabled) ?? []
);

let refreshTimer: ReturnType<typeof setInterval> | undefined;

export async function loadApp(): Promise<void> {
  loading.set(true);
  loadError.set(null);
  try {
    const [nextSnapshot, nextPreferences] = await Promise.all([getSnapshot(), getPreferences()]);
    snapshot.set(nextSnapshot);
    preferences.set(nextPreferences);
  } catch (error) {
    loadError.set(error instanceof Error ? error.message : 'The console could not load.');
  } finally {
    loading.set(false);
  }
}

export function startSnapshotRefresh(intervalMs = 10_000): void {
  stopSnapshotRefresh();
  refreshTimer = setInterval(async () => {
    try {
      snapshot.set(await getSnapshot());
      loadError.set(null);
    } catch (error) {
      loadError.set(error instanceof Error ? error.message : 'Live refresh failed.');
    }
  }, intervalMs);
}

export function stopSnapshotRefresh(): void {
  if (refreshTimer) clearInterval(refreshTimer);
  refreshTimer = undefined;
}

export async function persistPreferences(next: AppPreferences): Promise<MutationResult> {
  saving.set(true);
  try {
    const result = await savePreferences(next);
    if (result.ok) {
      const [savedPreferences, nextSnapshot] = await Promise.all([getPreferences(), getSnapshot()]);
      preferences.set(savedPreferences);
      snapshot.set(nextSnapshot);
      // Confirmation is transient and self-clearing. The desks used to carry a
      // quiet permanent indicator instead, which read as a greyed-out Save
      // button -- the exact affordance this app does not have and should never
      // appear to. Writes are debounced, so this is one notice per burst of
      // edits rather than per keystroke, and it leaves on its own.
      notice.set({ ok: true, message: 'Saved.' });
      return result;
    }
    notice.set(result);
    return result;
  } catch (error) {
    const result = {
      ok: false,
      message: error instanceof Error ? error.message : 'Preferences were not saved.'
    };
    notice.set(result);
    return result;
  } finally {
    saving.set(false);
  }
}

export function currentPreferences(): AppPreferences | null {
  return get(preferences);
}
