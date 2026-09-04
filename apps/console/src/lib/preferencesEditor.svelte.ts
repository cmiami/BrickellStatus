import { onMount, untrack } from 'svelte';
import { fromStore, get } from 'svelte/store';

import { persistPreferences, preferences } from './state';
import { rebasePreferences } from './preferences';
import type { AppPreferences, MutationResult } from './types';

/** Debounce form edits, flush on navigation, and adopt normalized saves. */
export function preferencesEditor(
  read: () => AppPreferences | null,
  write: (draft: AppPreferences) => void
): { saveNow: () => Promise<MutationResult | undefined> } {
  const saved = fromStore(preferences);
  let base: AppPreferences | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let inFlight: Promise<MutationResult> | null = null;
  let lastAttempt = '';

  function adopt(current: AppPreferences, previous: AppPreferences | null) {
    const draft = $state.snapshot(read());
    write(structuredClone(draft && previous ? rebasePreferences(draft, previous, current) : current));
    base = current;
  }

  $effect(() => {
    const current = saved.current;
    untrack(() => {
      if (current && current !== base && !inFlight) adopt(current, base);
    });
  });

  async function saveNow(): Promise<MutationResult | undefined> {
    clearTimeout(timer);
    timer = undefined;
    if (inFlight) {
      await inFlight;
      return saveNow();
    }
    const draft = read();
    if (!draft) return;
    const payload = $state.snapshot(draft);
    lastAttempt = JSON.stringify(payload);
    if (lastAttempt === JSON.stringify(base)) return { ok: true, message: 'Saved.' };
    inFlight = persistPreferences(payload, base);
    try {
      const result = await inFlight;
      const current = get(preferences);
      if (result.ok && current) adopt(current, payload);
      return result;
    } finally {
      inFlight = null;
      // An edit made during the request gets its own write. A rejected,
      // unchanged draft stays visible without an automatic retry loop.
      if (JSON.stringify(read()) !== lastAttempt && JSON.stringify(read()) !== JSON.stringify(base)) {
        void saveNow();
      }
    }
  }

  $effect(() => {
    const fingerprint = JSON.stringify(read());
    untrack(() => {
      clearTimeout(timer);
      timer = undefined;
      if (base && fingerprint !== JSON.stringify(base)) {
        timer = setTimeout(() => void saveNow(), 700);
      }
    });
  });

  onMount(() => () => {
    if (timer !== undefined) void saveNow();
  });

  return { saveNow };
}
