export interface LazyLoadOptions {
  root?: HTMLElement | null;
  onLoadMore: () => void;
  exhausted?: boolean;
}

/** Extend a list near its end; the page also supplies a keyboard-accessible button. */
export function lazyLoad(node: HTMLElement, options: LazyLoadOptions) {
  let current = options;
  let observer: IntersectionObserver | undefined;

  function observe() {
    observer?.disconnect();
    if (current.exhausted || typeof IntersectionObserver === 'undefined') return;
    observer = new IntersectionObserver(
      (entries) => {
        if (!current.exhausted && entries.some((entry) => entry.isIntersecting)) {
          current.onLoadMore();
        }
      },
      { root: current.root ?? null, rootMargin: '240px 0px', threshold: 0 }
    );
    observer.observe(node);
  }
  observe();

  return {
    update(next: LazyLoadOptions) {
      const changed = next.root !== current.root || next.exhausted !== current.exhausted;
      current = next;
      if (changed) observe();
    },
    destroy() {
      observer?.disconnect();
    }
  };
}
