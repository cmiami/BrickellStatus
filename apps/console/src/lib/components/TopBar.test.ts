import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import TopBar from './TopBar.svelte';

describe('TopBar clock', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-08-16T13:45:07'));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  // The clock read the moment the app started and never moved again: the
  // component compiled in legacy mode, where `{localTime()}` declares a
  // dependency on `localTime` and not on the value it reads.
  it('advances as time passes', async () => {
    const { getByLabelText } = render(TopBar);
    const clock = () => getByLabelText(/^Local time/).getAttribute('aria-label') ?? '';

    const first = clock();
    expect(first).toContain('13:45:07');

    await vi.advanceTimersByTimeAsync(3_000);
    const later = clock();
    expect(later).toContain('13:45:10');
    expect(later).not.toBe(first);
  });

  it('stops ticking once unmounted', async () => {
    const { unmount } = render(TopBar);
    unmount();
    // A surviving interval would keep writing to a destroyed component.
    expect(vi.getTimerCount()).toBe(0);
  });
});
