import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import SignalBoard from './SignalBoard.svelte';
import type { ChannelPriority, ChannelSnapshot } from '$lib/types';

function channel(
  id: string,
  kind: ChannelSnapshot['kind'],
  headline: string,
  priority: Partial<ChannelPriority>
): ChannelSnapshot {
  return {
    id,
    kind,
    title: id,
    sourceLabel: 'Some Feed',
    availability: 'fresh',
    ageSeconds: 12,
    coverageComplete: true,
    summary: `${id} summary`,
    materialKey: `material:${id}`,
    signal: { headline, detail: `${headline} detail`, action: 'act' },
    priority: { score: 300, urgency: 'heads_up', confirmed: false, ...priority },
    enabled: true,
    active: true,
    presence: 'rotation',
    interruptPreset: 'meaningful',
    destinations: ['epaper']
  };
}

describe('SignalBoard', () => {
  it('orders by the engine score rather than by kind', () => {
    const { getAllByRole } = render(SignalBoard, {
      channels: [
        channel('markets.watch', 'markets', 'AMD +2.4%', { score: 100, urgency: 'routine' }),
        channel('weather.miami', 'weather', 'Rain in 8 minutes', { score: 470, imminenceMinutes: 8 }),
        channel('hurricane.atlantic', 'hurricane', 'TS Fixture', { score: 300 })
      ]
    });
    const headings = getAllByRole('article').map((node) => node.textContent ?? '');
    expect(headings[0]).toContain('Rain in 8 minutes');
    expect(headings[1]).toContain('TS Fixture');
    expect(headings[2]).toContain('AMD +2.4%');
  });

  it('sets the countdown in type, because it is why the card ranks where it does', () => {
    const { getByText } = render(SignalBoard, {
      channels: [channel('weather.miami', 'weather', 'Rain in 8 minutes', { imminenceMinutes: 8 })]
    });
    expect(getByText('T‑8 MIN')).toBeTruthy();
  });

  it('says NOW rather than T-0 for something already happening', () => {
    const { getByText } = render(SignalBoard, {
      channels: [channel('weather.miami', 'weather', 'Rain falling', { imminenceMinutes: 0 })]
    });
    expect(getByText('NOW')).toBeTruthy();
  });

  // The anchor owns the page above this. An empty board must take no space at
  // all rather than leaving a titled box explaining that nothing is happening.
  it('renders nothing when no secondary channel is active', () => {
    const quiet = { ...channel('news.local', 'news', 'Something', {}), active: false };
    const { container } = render(SignalBoard, { channels: [quiet] });
    expect(container.textContent?.trim()).toBe('');
  });

  it('never shows the feed that produced a signal', () => {
    const { container } = render(SignalBoard, {
      channels: [channel('weather.miami', 'weather', 'Rain in 8 minutes', {})]
    });
    expect(container.textContent).not.toContain('Some Feed');
  });

  // The bridge has the entire page above this one.
  it('leaves the bridge out', () => {
    const { container } = render(SignalBoard, {
      channels: [channel('bridge.brickell', 'bridge', 'BRIDGE OPEN', { score: 1005 })]
    });
    expect(container.textContent?.trim()).toBe('');
  });
});
