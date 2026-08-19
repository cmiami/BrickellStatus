import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
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
  getPreferences: vi.fn(async () => ({ ais: { enabled: false, apiKeyConfigured: true, radiusKilometers: 12 } })),
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

const ais: AisSettings = { enabled: false, apiKeyConfigured: true, radiusKilometers: 12 };

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
  it('holds the vessel watch that used to live under Outputs', () => {
    mount();
    expect(screen.getByRole('heading', { name: /AISStream vessel watch/i })).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/replace saved key|paste api key/i)).toBeInTheDocument();
    expect(screen.getByRole('slider')).toBeInTheDocument();
  });

  it('hands a radius change straight back rather than staging it', async () => {
    const onaischange = mount();
    await fireEvent.input(screen.getByRole('slider'), { target: { value: '20' } });
    expect(onaischange).toHaveBeenCalledWith(expect.objectContaining({ radiusKilometers: 20 }));
    expect(screen.queryByText(/unsaved/i)).toBeNull();
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
