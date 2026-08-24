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
    expect(mark).toHaveAttribute('data-active-motion', 'none');
    expect(container.querySelectorAll('.leaf')).toHaveLength(2);
    expect(container.querySelectorAll('.pivot-outer')).toHaveLength(2);
    expect(container.querySelectorAll('.pier')).toHaveLength(2);
    expect(container.querySelectorAll('.barrier-arm')).toHaveLength(2);
    expect(container.querySelector('.center-lock')).toBeTruthy();
    expect(container.querySelectorAll('.road-flow')).toHaveLength(0);
    expect(container.querySelectorAll('.channel-flow')).toHaveLength(0);
  });

  it('updates state, scale and accessible title through its public props', async () => {
    const { container, rerender } = render(TransitBascule, {
      state: 'unknown',
      title: 'Brickell Bridge has no controller reading'
    });

    const mark = () => container.querySelector<SVGGElement>('.transit-bascule');
    expect(mark()).toHaveAttribute('data-state', 'unknown');
    expect(mark()).toHaveAttribute('data-active-motion', 'none');
    expect(mark()).toHaveAttribute('aria-label', 'Brickell Bridge has no controller reading');

    await rerender({ state: 'up', hero: true, title: 'Brickell Bridge up' });

    expect(mark()).toHaveAttribute('data-state', 'up');
    expect(mark()).toHaveAttribute('data-scale', 'hero');
    expect(mark()).toHaveAttribute('data-active-motion', 'channel-vertical');
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

  it('draws opposing horizontal road lanes and opposing vertical channel lanes', () => {
    const { container } = render(TransitBascule, { state: 'down', hero: true });
    const roadFlows = [...container.querySelectorAll<SVGLineElement>('.road-flow')];
    const channelFlows = [...container.querySelectorAll<SVGLineElement>('.channel-flow')];

    expect(roadFlows.map((flow) => flow.dataset.direction)).toEqual([
      'right-to-left',
      'left-to-right'
    ]);
    expect(
      roadFlows.every(
        (flow) => flow.getAttribute('y1') === flow.getAttribute('y2')
          && flow.getAttribute('x1') !== flow.getAttribute('x2')
      )
    ).toBe(true);
    expect(roadFlows[0].getAttribute('x1')).toBe(roadFlows[1].getAttribute('x1'));
    expect(roadFlows[0].getAttribute('x2')).toBe(roadFlows[1].getAttribute('x2'));

    expect(channelFlows.map((flow) => flow.dataset.direction)).toEqual([
      'bottom-to-top',
      'top-to-bottom'
    ]);
    expect(
      channelFlows.every(
        (flow) => flow.getAttribute('x1') === flow.getAttribute('x2')
          && flow.getAttribute('y1') !== flow.getAttribute('y2')
      )
    ).toBe(true);
    expect(channelFlows[0].getAttribute('y1')).toBe(channelFlows[1].getAttribute('y1'));
    expect(channelFlows[0].getAttribute('y2')).toBe(channelFlows[1].getAttribute('y2'));
  });
});
