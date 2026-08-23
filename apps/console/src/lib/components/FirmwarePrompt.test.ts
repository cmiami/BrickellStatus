import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { getPlatformCapabilities } from '$lib/api';
import type { FirmwareStatus } from '$lib/types';

const invoke = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invoke(command, args)
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {})
}));
// A desktop build: the prompt only offers a flash where one is possible, so
// these cases have to say which platform they are standing on.
vi.mock('$lib/api', () => ({
  getPlatformCapabilities: vi.fn(async () => ({ usbDisplay: true, firmwareFlashing: true }))
}));

const E213_V11 = { id: 'vision-master-e213-v11', label: 'Panel v1.1', panel: 'e213', panelRevision: 'v11', totalBytes: 529_104 } as const;
const E213_V1 = { id: 'vision-master-e213', label: 'Original panel', panel: 'e213', panelRevision: 'original', totalBytes: 529_104 } as const;
const E290 = { id: 'vision-master-e290', label: 'E290', panel: 'e290', totalBytes: 529_104 } as const;

function status(overrides: Partial<FirmwareStatus>): FirmwareStatus {
  return {
    port: '/dev/tty.usbmodem101',
    bundledBuild: 'abc1234',
    bundledVersion: 2,
    variants: [E213_V11, E213_V1, E290] as unknown as FirmwareStatus['variants'],
    requirement: { state: 'required', reason: { kind: 'notResponding' } },
    ...overrides
  };
}

/** Serves a status now, and whatever it becomes after a flash. */
function serve(first: FirmwareStatus, afterFlash: FirmwareStatus = first) {
  let flashed = false;
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_firmware_status') return flashed ? afterFlash : first;
    if (command === 'flash_firmware') {
      flashed = true;
      return null;
    }
    throw new Error(`unexpected command ${command}`);
  });
}

async function flashButton() {
  return await screen.findByRole('button', { name: /flash|update|install/i });
}

describe('FirmwarePrompt', () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it('writes the build the device asked for rather than offering a choice', async () => {
    serve(
      status({ board: 'e290', recommendedVariantId: 'vision-master-e290' })
    );
    const { default: FirmwarePrompt } = await import('./FirmwarePrompt.svelte');
    render(FirmwarePrompt);

    (await flashButton()).click();

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('flash_firmware', {
        variantId: 'vision-master-e290',
        port: '/dev/tty.usbmodem101'
      })
    );
    // No panel picker exists to click: the board named itself.
    expect(screen.queryByRole('button', { name: /e213/i })).toBeNull();
  });

  it('presents E213 as one panel and leaves controller recovery to READY verification', async () => {
    serve(
      status({ board: 'e213', recommendedVariantId: 'vision-master-e213-v11' }),
      status({
        board: 'e213',
        recommendedVariantId: 'vision-master-e213-v11',
        requirement: { state: 'upToDate', build: 'abc1234' }
      })
    );
    const { default: FirmwarePrompt } = await import('./FirmwarePrompt.svelte');
    render(FirmwarePrompt);

    (await flashButton()).click();

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('flash_firmware', {
        variantId: 'vision-master-e213-v11',
        port: '/dev/tty.usbmodem101'
      })
    );
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull());
    expect(screen.queryByText(/can you read the display/i)).toBeNull();
    expect(screen.queryByRole('button', { name: /other panel/i })).toBeNull();
  });

  it('closes without a question on a board that has only one build', async () => {
    serve(
      status({ board: 'e290', recommendedVariantId: 'vision-master-e290' }),
      status({
        board: 'e290',
        recommendedVariantId: 'vision-master-e290',
        requirement: { state: 'upToDate', build: 'abc1234' }
      })
    );
    const { default: FirmwarePrompt } = await import('./FirmwarePrompt.svelte');
    render(FirmwarePrompt);

    (await flashButton()).click();

    await waitFor(() => expect(screen.queryByText(/can you read the display/i)).toBeNull());
    expect(screen.queryByRole('button', { name: /try the other panel/i })).toBeNull();
  });

  /// A build that landed on the wrong board explains itself in the board's own
  /// terms, and the fix is the flash already being offered.
  it('names the board when the build on it is for the other panel', async () => {
    serve(
      status({
        board: 'e290',
        recommendedVariantId: 'vision-master-e290',
        requirement: { state: 'required', reason: { kind: 'wrongBoard', board: 'e290' } }
      })
    );
    const { default: FirmwarePrompt } = await import('./FirmwarePrompt.svelte');
    render(FirmwarePrompt);

    expect(await screen.findByText(/this board is an E290/i)).toBeInTheDocument();
  });

  it('notifies a BLE-connected legacy panel and requires USB before updating', async () => {
    serve(
      status({
        port: undefined,
        board: 'e290',
        recommendedVariantId: 'vision-master-e290',
        requirement: {
          state: 'required',
          reason: { kind: 'firmwareOutdated', device: 1, bundled: 2 }
        }
      })
    );
    const { default: FirmwarePrompt } = await import('./FirmwarePrompt.svelte');
    render(FirmwarePrompt);

    expect(await screen.findByText(/firmware update required/i)).toBeInTheDocument();
    expect(screen.getByText(/USB connection required/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /update firmware/i })).toBeNull();
  });

  it('never offers to downgrade firmware newer than the desktop app', async () => {
    serve(
      status({
        requirement: { state: 'deviceNewer', device: 3, bundled: 2 }
      })
    );
    const { default: FirmwarePrompt } = await import('./FirmwarePrompt.svelte');
    render(FirmwarePrompt);

    expect(await screen.findByRole('heading', { name: /update BrickellStatus/i })).toBeInTheDocument();
    expect(screen.getByText(/will not be downgraded/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /update firmware|reflash firmware/i })).toBeNull();
  });

  it('stays quiet when the current firmware release has a different source build', async () => {
    serve(
      status({
        requirement: {
          state: 'differentBuild',
          version: 2,
          device: 'local-dirty-build',
          bundled: 'release-build'
        }
      })
    );
    const { default: FirmwarePrompt } = await import('./FirmwarePrompt.svelte');
    render(FirmwarePrompt);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('get_firmware_status', undefined));
    expect(screen.queryByRole('alertdialog')).toBeNull();
  });

  it('offers nothing on a platform that cannot write firmware', async () => {
    // A phone reaches no serial bootloader, so a flash it agreed to would fail
    // after the fact. The prompt stays away rather than asking for consent it
    // cannot honour.
    vi.mocked(getPlatformCapabilities).mockResolvedValueOnce({
      usbDisplay: false,
      firmwareFlashing: false
    });
    serve(status({ board: 'e213', recommendedVariantId: 'vision-master-e213-v11' }));

    const { default: FirmwarePrompt } = await import('./FirmwarePrompt.svelte');
    render(FirmwarePrompt);

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByRole('button', { name: /flash|write/i })).toBeNull();
  });
});
