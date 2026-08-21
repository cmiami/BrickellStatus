import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AppPreferences } from '$lib/types';

import EpaperOutputPanel from './EpaperOutputPanel.svelte';

const mocks = vi.hoisted(() => ({
  connect: vi.fn(),
  disconnect: vi.fn(),
  getStatus: vi.fn(),
  getCapabilities: vi.fn(),
  invoke: vi.fn(),
  persist: vi.fn(),
  scan: vi.fn(),
  sendFrame: vi.fn()
}));

vi.mock('$lib/api', () => ({
  connectDisplayDevice: mocks.connect,
  disconnectDisplayDevice: mocks.disconnect,
  getDisplayStatus: mocks.getStatus,
  getPlatformCapabilities: mocks.getCapabilities,
  scanDisplayDevices: mocks.scan,
  sendDisplayTestFrame: mocks.sendFrame
}));

vi.mock('$lib/state', async () => {
  const { writable } = await import('svelte/store');
  return {
    notice: writable(null),
    preferences: writable(null),
    snapshot: writable(null),
    persistPreferences: mocks.persist
  };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mocks.invoke
}));

function preferencesFixture(): AppPreferences {
  return {
    display: {
      transport: 'ble',
      serialPort: 'auto',
      bleName: '',
      dwellSeconds: 28,
      fullRefreshEvery: 12,
      orientation: 'upright'
    }
  } as AppPreferences;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('remembered e-paper route', () => {
  it('saves the selected panel name before opening its BLE connection', async () => {
    const draft = preferencesFixture();
    mocks.getCapabilities.mockResolvedValue({ usbDisplay: true, firmwareFlashing: false });
    mocks.getStatus.mockResolvedValue({
      state: 'disconnected',
      transport: null,
      detail: 'No display connected.'
    });
    mocks.scan.mockResolvedValue([
      {
        id: 'ble:platform-peripheral-id',
        name: 'BrickellStatus 26B4',
        transport: 'ble',
        detail: 'Matching INK1 service'
      }
    ]);
    mocks.persist.mockResolvedValue({ ok: true, message: 'Saved.' });
    mocks.connect.mockResolvedValue({
      state: 'connected',
      transport: 'ble',
      deviceName: 'BrickellStatus 26B4',
      detail: 'INK1 protocol compatibility confirmed.'
    });

    render(EpaperOutputPanel, { draft });
    await fireEvent.click(screen.getByRole('button', { name: /Find my panel/i }));
    const connect = await screen.findByRole('button', { name: 'Connect' });
    await fireEvent.click(connect);

    await waitFor(() => expect(mocks.persist).toHaveBeenCalledOnce());
    const saved = mocks.persist.mock.calls[0][0] as AppPreferences;
    expect(saved.display.transport).toBe('ble');
    expect(saved.display.bleName).toBe('BrickellStatus 26B4');
    expect(mocks.connect).toHaveBeenCalledWith('ble:platform-peripheral-id', 'ble');
    const status = await screen.findByRole('heading', { name: 'Bluetooth LE connected' });
    expect(within(status.closest('header')!).getByText('BrickellStatus 26B4')).toBeInTheDocument();
  });

  it.each(['INK1 panel', 'InkDock E213', 'BrickellStatus 26b4', 'BrickellStatus 0000'])(
    'keeps the non-unique name %s session-only',
    async (name) => {
      const draft = preferencesFixture();
      mocks.getCapabilities.mockResolvedValue({ usbDisplay: true, firmwareFlashing: false });
      mocks.getStatus.mockResolvedValue({
        state: 'disconnected',
        transport: null,
        detail: 'No display connected.'
      });
      mocks.scan.mockResolvedValue([
        {
          id: 'ble:legacy-platform-id',
          name,
          transport: 'ble',
          detail: 'Matching INK1 service'
        }
      ]);
      mocks.persist.mockResolvedValue({ ok: true, message: 'Saved.' });
      mocks.connect.mockResolvedValue({
        state: 'connected',
        transport: 'ble',
        deviceName: name,
        detail: 'INK1 protocol compatibility confirmed.'
      });

      render(EpaperOutputPanel, { draft });
      await fireEvent.click(screen.getByRole('button', { name: /Find my panel/i }));
      await fireEvent.click(await screen.findByRole('button', { name: 'Connect' }));

      await waitFor(() => expect(mocks.persist).toHaveBeenCalledOnce());
      const saved = mocks.persist.mock.calls[0][0] as AppPreferences;
      expect(saved.display.transport).toBe('ble');
      expect(saved.display.bleName).toBe('');
      expect(mocks.connect).toHaveBeenCalledWith('ble:legacy-platform-id', 'ble');
      const status = await screen.findByRole('heading', { name: 'Bluetooth LE connected' });
      const statusHeader = within(status.closest('header')!);
      expect(statusHeader.getByText('BrickellStatus panel')).toBeInTheDocument();
      expect(statusHeader.queryByText(name)).not.toBeInTheDocument();
    }
  );

  it('offers one objective repair action for both internal E213 images', async () => {
    const draft = preferencesFixture();
    mocks.getCapabilities.mockResolvedValue({ usbDisplay: true, firmwareFlashing: true });
    mocks.getStatus.mockResolvedValue({
      state: 'disconnected',
      transport: null,
      panel: 'e213',
      detail: 'Panel is not answering.'
    });
    const firmware = {
      port: '/dev/cu.usbmodem14B4201',
      bundledBuild: 'abc1234',
      board: 'e213',
      recommendedVariantId: 'vision-master-e213-v11',
      variants: [
        { id: 'vision-master-e213-v11', label: 'Vision Master E213', panel: 'e213', panelRevision: 'v11', totalBytes: 529_104 },
        { id: 'vision-master-e213', label: 'Vision Master E213', panel: 'e213', panelRevision: 'original', totalBytes: 529_104 }
      ],
      requirement: { state: 'required', reason: { kind: 'notResponding' } }
    };
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'get_firmware_status') return firmware;
      if (command === 'flash_firmware') return null;
      throw new Error(`unexpected command ${command}`);
    });

    render(EpaperOutputPanel, { draft });

    const repair = await screen.findByRole('button', { name: 'Repair panel firmware' });
    expect(screen.queryByRole('button', { name: /other|original|v1\.1/i })).toBeNull();
    await fireEvent.click(repair);
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith('flash_firmware', {
        variantId: 'vision-master-e213-v11',
        port: '/dev/cu.usbmodem14B4201'
      })
    );
  });
});
