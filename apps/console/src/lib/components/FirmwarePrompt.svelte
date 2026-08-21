<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';

  import { getPlatformCapabilities } from '$lib/api';
  import { onMount } from 'svelte';

  import { PANEL_GEOMETRY, type FirmwareStatus } from '$lib/types';

  let status = $state<FirmwareStatus | null>(null);
  let dismissed = $state(false);
  // A phone cannot write firmware: there is no serial bootloader to reach.
  // Offering the flash anyway would fail after the reader agreed to it.
  let flashingSupported = $state(true);
  let flashing = $state(false);
  let written = $state(0);
  let total = $state(0);
  let stage = $state('writing');
  let error = $state<string | null>(null);
  let done = $state(false);

  // E213 has two internal controller images, but they are recovery mechanics,
  // not two panels for a reader to identify. The native flash command requires
  // an objective READY response and tries the other E213 image once when the
  // first is silent. The glass is never used as proof: e-paper can retain a
  // readable image from dead firmware indefinitely.

  const notable = $derived(
    status?.requirement.state === 'required' ||
      status?.requirement.state === 'deviceNewer' ||
      status?.requirement.state === 'differentBuild'
  );
  const prompted = $derived(!dismissed && !!status && notable);

  const variant = $derived(
    status?.variants.find((item) => item.id === status?.recommendedVariantId) ??
      status?.variants[0] ??
      null
  );

  const panelName = $derived(status?.board ? PANEL_GEOMETRY[status.board].label : null);
  const percent = $derived(total > 0 ? Math.min(100, Math.round((written / total) * 100)) : 0);
  const canFlash = $derived(flashingSupported && !!status?.port && !!variant && status?.requirement.state === 'required');

  function titleText(): string {
    const requirement = status?.requirement;
    if (requirement?.state === 'deviceNewer') return 'Update BrickellStatus';
    if (requirement?.state === 'differentBuild') return 'Different firmware build';
    if (requirement?.state !== 'required') return 'Panel firmware';
    switch (requirement.reason.kind) {
      case 'firmwareOutdated':
        return 'Panel firmware update required';
      case 'legacyConnection':
        return 'Panel setup needs attention';
      case 'wrongBoard':
        return 'Correct panel firmware required';
      default:
        return 'Repair panel firmware?';
    }
  }

  function reasonText(): string {
    const requirement = status?.requirement;
    if (requirement?.state === 'deviceNewer') {
      return `This panel runs firmware version ${requirement.device}, newer than version ${requirement.bundled} bundled with this app. Update BrickellStatus; the panel will not be downgraded.`;
    }
    if (requirement?.state === 'differentBuild') {
      return `This panel runs a different firmware build. The app cannot determine which build is newer, so it will not replace it automatically.`;
    }
    if (requirement?.state !== 'required') return '';
    switch (requirement.reason.kind) {
      case 'notResponding':
        return 'A panel is connected but is not answering the BrickellStatus firmware identity check.';
      case 'wrongBoard':
        return `This board is an ${PANEL_GEOMETRY[requirement.reason.board].label}, and it is running the build for the other panel.`;
      case 'firmwareOutdated':
        return requirement.reason.device === 1
          ? `This panel runs legacy firmware. Version ${requirement.reason.bundled} is required for reliable identity and reconnect.`
          : `This panel runs firmware version ${requirement.reason.device}. Version ${requirement.reason.bundled} bundled with this app is newer and must be installed.`;
      case 'incompatibleIdentity':
        return 'The panel returned an incompatible firmware identity. Reinstall the firmware bundled with this app.';
      case 'legacyConnection':
        return 'The saved panel cannot be identified safely for automatic reconnect. Connect it by USB to install current firmware and restore reconnect.';
      default:
        return 'The panel runs a different firmware build. Reinstall only if you intend to replace it with the build bundled in this app.';
    }
  }

  function actionText(): string {
    const requirement = status?.requirement;
    if (requirement?.state !== 'required') return '';
    switch (requirement.reason.kind) {
      case 'firmwareOutdated':
        return 'Update firmware';
      case 'wrongBoard':
        return 'Install correct firmware';
      default:
        return 'Repair firmware';
    }
  }

  async function refresh() {
    try {
      status = await invoke<FirmwareStatus>('get_firmware_status');
    } catch {
      // A build without the Tauri bridge, or without bundled firmware, simply
      // shows nothing rather than an error the reader cannot act on.
      status = null;
    }
  }

  onMount(() => {
    void getPlatformCapabilities()
      .then((capabilities) => (flashingSupported = capabilities.firmwareFlashing))
      .catch(() => (flashingSupported = false));
    void refresh();
    const poll = setInterval(refresh, 15_000);
    const unlisten = listen<{ stage: string; written: number; total: number }>(
      'firmware://progress',
      (event) => {
        stage = event.payload.stage;
        written = event.payload.written;
        total = event.payload.total;
      }
    );
    return () => {
      clearInterval(poll);
      void unlisten.then((off) => off());
    };
  });

  async function flash(target = variant) {
    if (!status?.port || !target) return;
    flashing = true;
    error = null;
    done = false;
    written = 0;
    total = target.totalBytes;
    try {
      await invoke('flash_firmware', { variantId: target.id, port: status.port });
      done = true;
      await refresh();
      dismissed = true;
    } catch (cause) {
      error = String(cause);
    } finally {
      flashing = false;
    }
  }

</script>

{#if prompted}
  <div class="firmware" role="alertdialog" aria-modal="false" aria-labelledby="firmware-title">
    <div class="firmware-head">
      <p class="registration-label">Device firmware</p>
      <h2 id="firmware-title">{titleText()}</h2>
      <p class="reason">{reasonText()}</p>
      <p class="port">
        {status?.port ?? (flashingSupported ? 'USB connection required' : 'Desktop USB connection required')}{panelName ? ` · ${panelName}` : ''}
      </p>
    </div>

    {#if flashing}
      <div class="progress" aria-live="polite">
        <div class="bar"><span style={`--progress:${percent / 100}`}></span></div>
        <small>
          {stage === 'checking' ? 'Checking panel' : stage === 'verifying' ? 'Verifying write' : 'Writing'} · {percent}% ·
          do not unplug the board
        </small>
      </div>
    {:else if error}
      <p class="failed" role="alert">{error}</p>
    {:else if done}
      <p class="done" role="status">Firmware written and verified.</p>
    {/if}

    <div class="firmware-actions">
      {#if canFlash}
        <button class="primary-action" onclick={() => flash()} disabled={flashing}>
          {flashing ? 'Flashing…' : actionText()}
        </button>
      {/if}
      <button class="secondary-action" onclick={() => (dismissed = true)} disabled={flashing}>
        {canFlash ? 'Cancel' : 'Dismiss'}
      </button>
    </div>
  </div>
{/if}

<style>
  .firmware {
    position: fixed;
    right: clamp(14px, 2vw, 28px);
    bottom: clamp(14px, 2vw, 28px);
    z-index: 40;
    display: grid;
    gap: 13px;
    width: min(400px, calc(100vw - 32px));
    padding: 18px 20px 20px;
    background: var(--frost);
    border: 1px solid var(--marine);
    box-shadow: var(--strip-shadow);
  }

  .firmware-head {
    display: grid;
    gap: 6px;
    padding-bottom: 11px;
    border-bottom: 1px solid var(--rule-strong);
  }

  h2 {
    margin: 0;
    font-family: var(--font-instrument);
    font-size: var(--type-section);
    font-weight: 700;
    line-height: 1;
    text-transform: uppercase;
  }

  .reason,
  .port {
    margin: 0;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    letter-spacing: 0.06em;
  }

  .progress {
    display: grid;
    gap: 6px;
  }

  .bar {
    height: 10px;
    background: var(--paper);
    border: 1px solid var(--marine);
  }

  /* Scaled rather than resized: the flash is writing to a serial bootloader on
     the same machine, and a bar that relayouts on every progress event competes
     with the one job that must not stall. */
  .bar span {
    display: block;
    width: 100%;
    height: 100%;
    background: var(--amber);
    transform: scaleX(var(--progress, 0));
    transform-origin: left center;
    transition: transform 200ms linear;
  }

  .progress small,
  .done,
  .failed {
    margin: 0;
    font-size: var(--type-caption);
  }

  .done {
    color: var(--success);
    font-weight: 600;
  }

  .failed {
    color: var(--danger);
    font-weight: 600;
  }

  .firmware-actions {
    display: flex;
    gap: 10px;
  }

  @media (prefers-reduced-motion: reduce) {
    .bar span {
      transition: none;
    }
  }
</style>
