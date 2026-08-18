import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { FirmwareStatus } from '$lib/types';

const invoke = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invoke(command, args)
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {})
}));

const E213_V11 = { id: 'vision-master-e213-v11', label: 'Panel v1.1', panel: 'e213', panelRevision: 'v11', totalBytes: 529_104 } as const;
const E213_V1 = { id: 'vision-master-e213', label: 'Original panel', panel: 'e213', panelRevision: 'original', totalBytes: 529_104 } as const;
const E290 = { id: 'vision-master-e290', label: 'E290', panel: 'e290', totalBytes: 529_104 } as const;

function status(overrides: Partial<FirmwareStatus>): FirmwareStatus {
  return {
    port: '/dev/tty.usbmodem101',
    bundledBuild: 'abc1234',
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
  return await screen.findByRole('button', { name: /flash/i });
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

  /// The one question worth asking is the one a person can answer, and only on
  /// the board whose panel revision nothing can read back.
  it('asks whether the screen is readable only when another build could apply', async () => {
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

    expect(await screen.findByText(/can you read the display/i)).toBeInTheDocument();
    const other = await screen.findByRole('button', { name: /try the other panel/i });
    other.click();

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('flash_firmware', {
        variantId: 'vision-master-e213',
        port: '/dev/tty.usbmodem101'
      })
    );
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
});
