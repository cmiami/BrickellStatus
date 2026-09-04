import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AisSettings, ChannelPreference } from '$lib/types';
import BridgeScopeEditor from './BridgeScopeEditor.svelte';

vi.mock('$lib/api', () => ({
  getAisstreamStatus: vi.fn(async () => ({
    configured: true,
    enabled: false,
    state: 'ready',
    detail: 'Armed.'
  })),
  getPreferences: vi.fn(async () => ({ ais: { enabled: false, provider: 'aisstream', apiKeyConfigured: true } })),
  setAisstreamApiKey: vi.fn(async () => ({ ok: true, message: 'Stored.' })),
  clearAisstreamApiKey: vi.fn(async () => ({ ok: true, message: 'Removed.' }))
}));

afterEach(cleanup);

function channel(): ChannelPreference {
  return {
    id: 'bridge.brickell',
    kind: 'bridge',
    title: 'Brickell bridge',
    enabled: true,
    presence: 'home',
    interruptPreset: 'recommended',
    destinations: ['epaper'],
    maxAgeMinutes: 2,
    maxItems: 1,
    rotationSeconds: 28,
    scope: {
      bridge: 'Brickell Avenue Bridge',
      latitude: 25.7699,
      longitude: -80.19005,
      radiusMeters: 250,
      timeZone: 'America/New_York'
    }
  } as ChannelPreference;
}

const ais: AisSettings = { enabled: false, provider: 'aisstream', apiKeyConfigured: true };

function mount(overrides: Partial<AisSettings> = {}) {
  const onaischange = vi.fn();
  render(BridgeScopeEditor, {
    channel: channel(),
    ais: { ...ais, ...overrides },
    unitSystem: 'metric',
    onchannelchange: vi.fn(),
    onaischange
  });
  return onaischange;
}

describe('the Brickell bridge channel', () => {
  // AIS was configurable in two places, and the radius picker lived on the
  // output desk — which is where frames are delivered, not where evidence for
  // the forecast is chosen.
  it('holds the vessel source that used to live under Outputs', () => {
    mount();
    expect(screen.getByRole('heading', { name: /vessel source/i })).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/replace saved key|paste aisstream api key/i)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /get or manage aisstream api keys/i })).toHaveAttribute(
      'href',
      'https://aisstream.io/account'
    );
  });

  // A dropped socket and a quiet river look identical from here, and only one
  // of them is a fault worth a red light.
  it('names the health in words rather than by colour alone', () => {
    mount();
    expect(screen.getByText(/working|connecting|no key|not working/i)).toBeInTheDocument();
  });

  it('picks the span by name instead of by map', () => {
    mount();
    expect(screen.getByRole('combobox', { name: /watching/i })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /set on map/i })).toBeNull();
  });

  it('reads the time zone off the machine rather than asking', () => {
    mount();
    expect(screen.getByText(/taken from this computer/i)).toBeInTheDocument();
  });

  // Coverage follows the charted corridor, so a radius the reader picked never
  // described the water actually being watched.
  it('asks for nothing but a key', () => {
    mount();
    expect(screen.queryByRole('slider')).toBeNull();
    expect(screen.queryByRole('switch')).toBeNull();
    expect(screen.queryByText(/coverage radius/i)).toBeNull();
  });

  // Neither expressed an intent a reader could hold; switching one off only
  // made the forecast quietly worse.
  it('no longer offers switches for evidence the forecast decides', () => {
    mount();
    expect(screen.queryByRole('switch', { name: /ground truth/i })).toBeNull();
    expect(screen.queryByRole('switch', { name: /upstream progression/i })).toBeNull();
  });

  it('never names the agency behind bridge status reporting', () => {
    mount();
    expect(document.body.textContent).not.toMatch(/FL511|Florida 511/i);
  });
});
