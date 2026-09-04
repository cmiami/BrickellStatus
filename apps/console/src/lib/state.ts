import { get, writable } from 'svelte/store';

import { getDisplayStatus, getPreferences, getSnapshot, savePreferences } from './api';
import { rebasePreferences } from './preferences';
import type {
  AppPreferences,
  AppSnapshot,
  DisplayConnectionStatus,
  MutationResult
} from './types';

export const snapshot = writable<AppSnapshot | null>(null);
export const preferences = writable<AppPreferences | null>(null);
export const loading = writable(true);
export const loadError = writable<string | null>(null);
export const saving = writable(false);
export const notice = writable<MutationResult | null>(null);
export const displayStatus = writable<DisplayConnectionStatus | null>(null);

let refreshTimer: ReturnType<typeof setTimeout> | undefined;
let displayStatusTimer: ReturnType<typeof setTimeout> | undefined;
let refreshGeneration = 0;
let displayGeneration = 0;
let snapshotRequest = 0;
let appRequest = 0;
let saveQueue = Promise.resolve();
let pendingSaves = 0;

export async function loadApp(): Promise<void> {
  const request = ++appRequest;
  const nextSnapshotRequest = ++snapshotRequest;
  loading.set(true);
  loadError.set(null);
  try {
    const [nextSnapshot, nextPreferences] = await Promise.all([getSnapshot(), getPreferences()]);
    if (request !== appRequest) return;
    if (nextSnapshotRequest === snapshotRequest) snapshot.set(nextSnapshot);
    preferences.set(nextPreferences);
  } catch (error) {
    if (request === appRequest) {
      loadError.set(error instanceof Error ? error.message : 'The console could not load.');
    }
  } finally {
    if (request === appRequest) loading.set(false);
  }
}

export function startSnapshotRefresh(intervalMs = 10_000): void {
  stopSnapshotRefresh();
  const generation = refreshGeneration;
  async function refresh() {
    const request = ++snapshotRequest;
    try {
      const next = await getSnapshot();
      if (generation === refreshGeneration && request === snapshotRequest) {
        snapshot.set(next);
        loadError.set(null);
      }
    } catch (error) {
      if (generation === refreshGeneration && request === snapshotRequest) {
        loadError.set(error instanceof Error ? error.message : 'Live refresh failed.');
      }
    } finally {
      if (generation === refreshGeneration) refreshTimer = setTimeout(refresh, intervalMs);
    }
  }
  refreshTimer = setTimeout(refresh, intervalMs);
}

export function stopSnapshotRefresh(): void {
  refreshGeneration += 1;
  clearTimeout(refreshTimer);
  refreshTimer = undefined;
}

export async function loadDisplayStatus(): Promise<void> {
  const generation = displayGeneration;
  const previous = get(displayStatus);
  try {
    const next = await getDisplayStatus();
    if (generation === displayGeneration && get(displayStatus) === previous) displayStatus.set(next);
  } catch {
    if (generation === displayGeneration && get(displayStatus) === previous) displayStatus.set(null);
  }
}

export function startDisplayStatusRefresh(intervalMs = 5_000): void {
  stopDisplayStatusRefresh();
  const generation = displayGeneration;
  async function refresh() {
    await loadDisplayStatus();
    if (generation === displayGeneration) displayStatusTimer = setTimeout(refresh, intervalMs);
  }
  displayStatusTimer = setTimeout(refresh, intervalMs);
}

export function stopDisplayStatusRefresh(): void {
  displayGeneration += 1;
  clearTimeout(displayStatusTimer);
  displayStatusTimer = undefined;
}

export async function persistPreferences(
  next: AppPreferences,
  base: AppPreferences | null = get(preferences)
): Promise<MutationResult> {
  pendingSaves += 1;
  saving.set(true);
  const previousSave = saveQueue;
  let release!: () => void;
  saveQueue = new Promise<void>((resolve) => { release = resolve; });
  await previousSave;
  try {
    const current = get(preferences);
    const result = await savePreferences(base && current ? rebasePreferences(next, base, current) : next);
    if (result.ok) {
      const [savedPreferences, nextSnapshot] = await Promise.all([getPreferences(), getSnapshot()]);
      // Discard reads started before this mutation was published.
      snapshotRequest += 1;
      appRequest += 1;
      loading.set(false);
      preferences.set(savedPreferences);
      snapshot.set(nextSnapshot);
      loadError.set(null);
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
    pendingSaves -= 1;
    saving.set(pendingSaves > 0);
    release();
  }
}
