import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import SignalBoard from './SignalBoard.svelte';
import type { ChannelPriority, ChannelSignal, ChannelSnapshot } from '$lib/types';

const GENERATED_AT = '2026-08-23T16:00:00Z';

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(GENERATED_AT));
});

afterEach(() => {
  vi.useRealTimers();
});

function channel(
  id: string,
  kind: ChannelSnapshot['kind'],
  headline: string,
  priority: Partial<ChannelPriority>,
  signal: Partial<ChannelSignal> = {}
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
    signal: { headline, detail: `${headline} detail`, action: 'act', ...signal },
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
      generatedAt: GENERATED_AT,
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
      generatedAt: GENERATED_AT,
      channels: [channel('weather.miami', 'weather', 'Rain in 8 minutes', { imminenceMinutes: 8 })]
    });
    expect(getByText('T‑8 MIN')).toBeTruthy();
  });

  it('says NOW rather than T-0 for something already happening', () => {
    const { getByText } = render(SignalBoard, {
      generatedAt: GENERATED_AT,
      channels: [channel('weather.miami', 'weather', 'Rain falling', { imminenceMinutes: 0 })]
    });
    expect(getByText('NOW')).toBeTruthy();
  });

  // The anchor owns the page above this. An empty board must take no space at
  // all rather than leaving a titled box explaining that nothing is happening.
  it('renders nothing when no secondary channel is active', () => {
    const quiet = { ...channel('news.local', 'news', 'Something', {}), active: false };
    const { container } = render(SignalBoard, { channels: [quiet], generatedAt: GENERATED_AT });
    expect(container.textContent?.trim()).toBe('');
  });

  it('never shows the feed that produced a signal', () => {
    const { container } = render(SignalBoard, {
      generatedAt: GENERATED_AT,
      channels: [channel('weather.miami', 'weather', 'Rain in 8 minutes', {})]
    });
    expect(container.textContent).not.toContain('Some Feed');
  });

  // The bridge has the entire page above this one.
  it('leaves the bridge out', () => {
    const { container } = render(SignalBoard, {
      generatedAt: GENERATED_AT,
      channels: [channel('bridge.brickell', 'bridge', 'BRIDGE OPEN', { score: 1005 })]
    });
    expect(container.textContent?.trim()).toBe('');
  });

  it('excludes signals that expire at or before the snapshot time', () => {
    const { queryByText, getByText } = render(SignalBoard, {
      generatedAt: GENERATED_AT,
      channels: [
        channel('news.old', 'news', 'Already expired', {}, { expiresAt: '2026-08-23T15:59:59Z' }),
        channel('news.boundary', 'news', 'Expires now', {}, { expiresAt: GENERATED_AT }),
        channel('news.current', 'news', 'Still current', {}, { expiresAt: '2026-08-23T16:00:01Z' })
      ]
    });

    expect(queryByText('Already expired')).toBeNull();
    expect(queryByText('Expires now')).toBeNull();
    expect(getByText('Still current')).toBeInTheDocument();
  });

  it('removes a notice when wall-clock time reaches its expiry without a new snapshot', async () => {
    const { getByText, queryByText } = render(SignalBoard, {
      generatedAt: GENERATED_AT,
      channels: [
        channel('weather.current', 'weather', 'Brief shower', {}, { expiresAt: '2026-08-23T16:00:05Z' })
      ]
    });

    expect(getByText('Brief shower')).toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(queryByText('Brief shower')).toBeNull();
  });

  it('names the horizontal region and provides keyboard-focusable scroll controls', () => {
    const { getByRole } = render(SignalBoard, {
      generatedAt: GENERATED_AT,
      channels: [
        channel('weather.miami', 'weather', 'Rain soon', { score: 500 }),
        channel('earthquake.significant', 'earthquake', 'Magnitude 5.1 earthquake', { score: 400 })
      ]
    });

    const rail = getByRole('region', { name: 'Current notices, horizontally scrollable' });
    expect(rail).toHaveAttribute('id', 'current-notice-rail');
    expect(getByRole('button', { name: 'Previous notices' })).toHaveAttribute(
      'aria-controls',
      'current-notice-rail'
    );
    expect(getByRole('button', { name: 'Next notices' })).toHaveAttribute(
      'aria-controls',
      'current-notice-rail'
    );
  });

  it('shows every enabled active non-bridge signal', () => {
    const disabled = { ...channel('news.disabled', 'news', 'Disabled', {}), enabled: false };
    const inactive = { ...channel('sports.inactive', 'sports', 'Inactive', {}), active: false };
    const { getAllByRole, queryByText } = render(SignalBoard, {
      generatedAt: GENERATED_AT,
      channels: [
        channel('weather.miami', 'weather', 'Rain soon', { score: 500 }),
        channel('earthquake.significant', 'earthquake', 'Magnitude 6.1 earthquake', { score: 450 }),
        disabled,
        inactive,
        channel('bridge.brickell', 'bridge', 'Bridge likely', { score: 900 })
      ]
    });

    expect(getAllByRole('article')).toHaveLength(2);
    expect(queryByText('Disabled')).toBeNull();
    expect(queryByText('Inactive')).toBeNull();
    expect(queryByText('Bridge likely')).toBeNull();
  });

  it('renders multiple notices from one channel and interleaves them globally by priority', () => {
    const weather = channel('weather.miami', 'weather', 'Legacy weather signal', { score: 999 });
    weather.notices = [
      {
        key: 'rain-soon',
        signal: {
          headline: 'Rain in 8 minutes',
          detail: 'A short shower is approaching.',
          action: 'Expect wet roads.',
          expiresAt: '2026-08-23T16:30:00Z',
          imminenceMinutes: 8
        },
        priority: { score: 700, urgency: 'action', imminenceMinutes: 8, confirmed: false }
      },
      {
        key: 'wind-later',
        signal: {
          headline: 'Wind gusts this afternoon',
          detail: 'Gusts may reach 42 mph.',
          action: 'Secure loose items.',
          expiresAt: '2026-08-23T18:00:00Z'
        },
        priority: { score: 300, urgency: 'heads_up', confirmed: false }
      },
      {
        key: 'expired-rain',
        signal: {
          headline: 'Earlier rain passed',
          detail: 'The shower has cleared.',
          action: 'None.',
          expiresAt: GENERATED_AT
        },
        priority: { score: 900, urgency: 'routine', confirmed: true }
      }
    ];

    const { getAllByRole, queryByText } = render(SignalBoard, {
      generatedAt: GENERATED_AT,
      channels: [
        weather,
        channel('earthquake.significant', 'earthquake', 'Magnitude 6.1 earthquake', { score: 500 })
      ]
    });

    const notices = getAllByRole('article').map((article) => article.textContent ?? '');
    expect(notices).toHaveLength(3);
    expect(notices[0]).toContain('Rain in 8 minutes');
    expect(notices[1]).toContain('Magnitude 6.1 earthquake');
    expect(notices[2]).toContain('Wind gusts this afternoon');
    expect(queryByText('Legacy weather signal')).toBeNull();
    expect(queryByText('Earlier rain passed')).toBeNull();
  });
});
