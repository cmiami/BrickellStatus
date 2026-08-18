import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ChannelPreference } from '$lib/types';
import { catalogEntries } from '$lib/catalog';
import FeedCatalogPicker from './FeedCatalogPicker.svelte';

afterEach(cleanup);

function channel(feeds: string[] = []): ChannelPreference {
  return {
    id: 'news.local',
    kind: 'news',
    title: 'Headlines',
    enabled: true,
    presence: 'rotation',
    interruptPreset: 'off',
    destinations: ['epaper'],
    maxAgeMinutes: 180,
    maxItems: 3,
    rotationSeconds: 24,
    scope: { feeds, topics: [], excludeTopics: [], breakingOnly: false }
  } as ChannelPreference;
}

const bbcWorld = catalogEntries().find((entry) => entry.id === 'bbc.world')!.url;

describe('FeedCatalogPicker', () => {
  it('ticks a catalog feed into scope without the reader typing a URL', async () => {
    const onchannelchange = vi.fn();
    render(FeedCatalogPicker, { channel: channel(), onchannelchange, kind: 'news' });

    await fireEvent.click(screen.getByLabelText('BBC News / World'));

    expect(onchannelchange).toHaveBeenCalledTimes(1);
    const [next, commit] = onchannelchange.mock.calls[0];
    expect(next.scope.feeds).toEqual([bbcWorld]);
    // A tick is a finished decision, so it saves immediately rather than
    // waiting out the typing debounce.
    expect(commit).toBe(true);
  });

  it('unticks a feed that is already subscribed', async () => {
    const onchannelchange = vi.fn();
    render(FeedCatalogPicker, { channel: channel([bbcWorld]), onchannelchange, kind: 'news' });

    const box = screen.getByLabelText('BBC News / World') as HTMLInputElement;
    expect(box.checked).toBe(true);
    await fireEvent.click(box);

    expect(onchannelchange.mock.calls[0][0].scope.feeds).toEqual([]);
  });

  it('shows the sports catalog when asked for it, not the news one', () => {
    render(FeedCatalogPicker, { channel: channel(), onchannelchange: vi.fn(), kind: 'sports' });
    expect(screen.getByRole('button', { name: /by sport/i })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /by country/i })).toBeNull();
  });

  describe('the custom entry', () => {
    async function openCustom() {
      await fireEvent.click(screen.getByRole('button', { name: /custom feed/i }));
    }

    it('accepts a public https feed', async () => {
      const onchannelchange = vi.fn();
      render(FeedCatalogPicker, { channel: channel(), onchannelchange, kind: 'news' });
      await openCustom();

      await fireEvent.input(screen.getByLabelText('Feed address'), {
        target: { value: 'https://example.com/feed' }
      });
      await fireEvent.click(screen.getByRole('button', { name: 'Add feed' }));

      expect(onchannelchange.mock.calls[0][0].scope.feeds).toEqual(['https://example.com/feed']);
    });

    // Each of these is a rule the Rust side enforces too. Failing here means
    // the reader is told which feed is wrong instead of losing an unrelated
    // save to a validation error further downstream.
    it.each([
      ['http://example.com/feed', /must use https/i],
      ['not a url', /not a web address/i],
      ['https://example.com/feed#top', /remove the #/i],
      ['https://example.com/feed?api_key=secret', /api_key/i]
    ])('refuses %s and says why', async (value, message) => {
      const onchannelchange = vi.fn();
      render(FeedCatalogPicker, { channel: channel(), onchannelchange, kind: 'news' });
      await openCustom();

      await fireEvent.input(screen.getByLabelText('Feed address'), { target: { value } });
      await fireEvent.click(screen.getByRole('button', { name: 'Add feed' }));

      expect(onchannelchange).not.toHaveBeenCalled();
      expect(screen.getByText(message)).toBeTruthy();
    });

    it('points a catalog URL back at its tick box rather than duplicating it', async () => {
      const onchannelchange = vi.fn();
      render(FeedCatalogPicker, { channel: channel(), onchannelchange, kind: 'news' });
      await openCustom();

      await fireEvent.input(screen.getByLabelText('Feed address'), { target: { value: bbcWorld } });
      await fireEvent.click(screen.getByRole('button', { name: 'Add feed' }));

      expect(onchannelchange).not.toHaveBeenCalled();
      expect(screen.getByText(/BBC News \/ World, which is in the list above/)).toBeTruthy();
    });

    it('lists a feed the reader added so it can be removed again', async () => {
      const onchannelchange = vi.fn();
      render(FeedCatalogPicker, {
        channel: channel(['https://example.com/feed']),
        onchannelchange,
        kind: 'news'
      });
      await openCustom();

      await fireEvent.click(screen.getByRole('button', { name: 'Remove example.com' }));
      expect(onchannelchange.mock.calls[0][0].scope.feeds).toEqual([]);
    });
  });
});
