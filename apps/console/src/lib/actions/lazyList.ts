/**
 * Grows a windowed list as its sentinel scrolls into view.
 *
 * The log can hold thousands of rows and every row carries formatted times and
 * nested markup, so rendering the whole history costs a visible pause on open.
 * The page renders a window instead and extends it only when the reader reaches
 * the end of what is already there.
 */
export interface LazyLoadOptions {
  /** Scroll container to observe within. Falls back to the viewport. */
  root?: HTMLElement | null;
  /** Called when the sentinel becomes visible and more rows remain. */
  onLoadMore: () => void;
  /** True once every row is rendered, which stops further callbacks. */
  exhausted?: boolean;
}

export function lazyLoad(node: HTMLElement, options: LazyLoadOptions) {
  let current = options;

  // Without IntersectionObserver the list still works; it just renders the
  // first window. Degrading to a smaller list beats throwing on an old engine.
  if (typeof IntersectionObserver === 'undefined') {
    return {
      update(next: LazyLoadOptions) {
        current = next;
      }
    };
  }

  const observer = new IntersectionObserver(
    (entries) => {
      if (current.exhausted) return;
      if (entries.some((entry) => entry.isIntersecting)) {
        current.onLoadMore();
      }
    },
    {
      root: current.root ?? null,
      // Start the next window slightly before the sentinel is on screen so the
      // rows are in place by the time the reader gets to them.
      rootMargin: '240px 0px',
      threshold: 0
    }
  );
  observer.observe(node);

  return {
    update(next: LazyLoadOptions) {
      current = next;
      // A newly exhausted list should stop firing, but keep observing so a
      // filter change that adds rows resumes without remounting the node.
    },
    destroy() {
      observer.disconnect();
    }
  };
}
