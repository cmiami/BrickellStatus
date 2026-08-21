import { cleanup, render } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';

import TransitBascule from './TransitBascule.svelte';

afterEach(cleanup);

describe('TransitBascule', () => {
  it('renders the complete mechanical mark around the origin', () => {
    const { container, getByRole } = render(TransitBascule, { state: 'down' });
    const mark = getByRole('img', { name: /bridge down/i });

    expect(mark).toHaveAttribute('data-state', 'down');
    expect(mark).toHaveAttribute('data-scale', 'mini');
    expect(container.querySelectorAll('.leaf')).toHaveLength(2);
    expect(container.querySelectorAll('.pivot-outer')).toHaveLength(2);
    expect(container.querySelectorAll('.pier')).toHaveLength(2);
    expect(container.querySelectorAll('.barrier-arm')).toHaveLength(2);
    expect(container.querySelector('.center-lock')).toBeTruthy();
    expect(container.querySelector('.channel-flow')).toBeTruthy();
  });

  it('updates state, scale and accessible title through its public props', async () => {
    const { container, rerender } = render(TransitBascule, {
      state: 'unknown',
      title: 'Brickell Bridge has no controller reading'
    });

    const mark = () => container.querySelector<SVGGElement>('.transit-bascule');
    expect(mark()).toHaveAttribute('data-state', 'unknown');
    expect(mark()).toHaveAttribute('aria-label', 'Brickell Bridge has no controller reading');

    await rerender({ state: 'up', hero: true, title: 'Brickell Bridge up' });

    expect(mark()).toHaveAttribute('data-state', 'up');
    expect(mark()).toHaveAttribute('data-scale', 'hero');
    expect(mark()).toHaveAttribute('aria-label', 'Brickell Bridge up');
    expect(mark()?.querySelector('title')?.textContent).toBe('Brickell Bridge up');
    expect(mark()?.querySelector('.signal-mast')).toBeNull();

    const controlHouses = [
      ...(mark()?.querySelectorAll<SVGGElement>('.control-house') ?? [])
    ];
    expect(controlHouses).toHaveLength(1);
    expect(controlHouses[0]).toHaveClass('control-house-bay');

    const translateX = Number(
      controlHouses[0]?.getAttribute('transform')?.match(/translate\(([-\d.]+)/)?.[1]
    );
    expect(translateX).toBeGreaterThan(0);
  });
});
