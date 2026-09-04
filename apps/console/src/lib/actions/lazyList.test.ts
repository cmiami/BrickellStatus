import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { lazyLoad } from './lazyList';

type ObserverCallback = (entries: { isIntersecting: boolean }[]) => void;

let callbacks: ObserverCallback[] = [];
let disconnects = 0;
let roots: (Element | Document | null | undefined)[] = [];
const original = globalThis.IntersectionObserver;

beforeEach(() => {
  callbacks = [];
  disconnects = 0;
  roots = [];
  // The action calls `new IntersectionObserver(...)`, so the stub has to be
  // constructible; an arrow function is not.
  class StubObserver {
    constructor(callback: ObserverCallback, options: IntersectionObserverInit) {
      callbacks.push(callback);
      roots.push(options.root);
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

  it('observes the scroll container once its element is bound', () => {
    const onLoadMore = vi.fn();
    const root = document.createElement('div');
    const handle = lazyLoad(document.createElement('div'), { root: null, onLoadMore });
    handle.update({ root, onLoadMore });
    expect(roots).toEqual([null, root]);
    expect(disconnects).toBe(1);
    handle.destroy();
  });
});
