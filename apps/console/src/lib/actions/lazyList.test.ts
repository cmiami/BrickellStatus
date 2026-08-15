import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { lazyLoad } from './lazyList';

type ObserverCallback = (entries: { isIntersecting: boolean }[]) => void;

let callbacks: ObserverCallback[] = [];
let disconnects = 0;
const original = globalThis.IntersectionObserver;

beforeEach(() => {
  callbacks = [];
  disconnects = 0;
  // The action calls `new IntersectionObserver(...)`, so the stub has to be
  // constructible; an arrow function is not.
  class StubObserver {
    constructor(callback: ObserverCallback) {
      callbacks.push(callback);
    }
    observe() {}
    unobserve() {}
    takeRecords() {
      return [];
    }
    disconnect() {
      disconnects += 1;
    }
  }
  globalThis.IntersectionObserver = StubObserver as unknown as typeof IntersectionObserver;
});

afterEach(() => {
  globalThis.IntersectionObserver = original;
});

function trigger(isIntersecting = true) {
  for (const callback of callbacks) callback([{ isIntersecting }]);
}

describe('lazyLoad', () => {
  it('asks for more rows when the sentinel comes into view', () => {
    const onLoadMore = vi.fn();
    lazyLoad(document.createElement('div'), { onLoadMore });
    trigger();
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it('stays quiet while the sentinel is off screen', () => {
    const onLoadMore = vi.fn();
    lazyLoad(document.createElement('div'), { onLoadMore });
    trigger(false);
    expect(onLoadMore).not.toHaveBeenCalled();
  });

  it('stops asking once every row is rendered', () => {
    const onLoadMore = vi.fn();
    const handle = lazyLoad(document.createElement('div'), { onLoadMore, exhausted: false });
    handle.update?.({ onLoadMore, exhausted: true });
    trigger();
    expect(onLoadMore).not.toHaveBeenCalled();
  });

  it('resumes when a filter change makes more rows available again', () => {
    const onLoadMore = vi.fn();
    const handle = lazyLoad(document.createElement('div'), { onLoadMore, exhausted: true });
    trigger();
    expect(onLoadMore).not.toHaveBeenCalled();
    handle.update?.({ onLoadMore, exhausted: false });
    trigger();
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it('disconnects on destroy', () => {
    const handle = lazyLoad(document.createElement('div'), { onLoadMore: () => {} });
    handle.destroy?.();
    expect(disconnects).toBe(1);
  });

  it('degrades to a static window when IntersectionObserver is unavailable', () => {
    // An engine without the API should render the first page rather than throw.
    globalThis.IntersectionObserver = undefined as unknown as typeof IntersectionObserver;
    const onLoadMore = vi.fn();
    expect(() => lazyLoad(document.createElement('div'), { onLoadMore })).not.toThrow();
    expect(onLoadMore).not.toHaveBeenCalled();
  });
});
