<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';

  import { getPlatformCapabilities } from '$lib/api';
  import { onMount } from 'svelte';

  import { PANEL_GEOMETRY, type FirmwareStatus, type FirmwareVariantSummary } from '$lib/types';

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

  // Which board is attached is never a question: the firmware probes its own
  // pins at boot and says so, and the app writes the build for whatever it
  // named. A board that has never been flashed cannot answer yet, so it is
  // written the most likely build — and if the probe then reports a different
  // board, the app writes the right one without asking anything.
  //
  // The one thing hardware cannot settle is the E213's panel revision: two
  // controllers, identical wiring, no read-back. Nobody knows which revision is
  // in a sealed case, and getting it wrong looks exactly like a failed flash.
  // So that one is not asked either. The app flashes a build and asks the only
  // question anyone can actually answer — can you read the screen? — and a "no"
  // flashes the other one and remembers.
  let checkingScreen = $state(false);
  let lastWritten = $state<FirmwareVariantSummary | null>(null);

  const prompted = $derived(
    flashingSupported &&
    !dismissed &&
      !!status &&
      !!status.port &&
      (status.requirement.state === 'required' || checkingScreen)
  );

  const variant = $derived(
    status?.variants.find((item) => item.id === status?.recommendedVariantId) ??
      status?.variants[0] ??
      null
  );

  // Only builds that could drive the board that is actually there. On a board
  // with one build there is nothing to try next, so nothing is offered.
  const alternatives = $derived(
    status && lastWritten
      ? status.variants.filter(
          (item) => item.panel === lastWritten?.panel && item.id !== lastWritten?.id
        )
      : []
  );

  const panelName = $derived(status?.board ? PANEL_GEOMETRY[status.board].label : null);
  const percent = $derived(total > 0 ? Math.min(100, Math.round((written / total) * 100)) : 0);

  function reasonText(): string {
    const requirement = status?.requirement;
    if (requirement?.state !== 'required') return '';
    switch (requirement.reason.kind) {
      case 'notResponding':
        return 'A board is connected but is not running BrickellStatus firmware.';
      case 'wrongBoard':
        return `This board is an ${PANEL_GEOMETRY[requirement.reason.board].label}, and it is running the build for the other panel.`;
      default:
        return `The board is running build ${requirement.reason.device}; this app ships ${requirement.reason.bundled}.`;
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
      lastWritten = target;
      done = true;
      await refresh();
      // A board whose panel this app cannot tell apart is the only reason to
      // keep this open. Everything else has already been settled by the device.
      checkingScreen =
        status?.variants.filter((item) => item.panel === target.panel).length > 1;
      if (!checkingScreen) dismissed = true;
    } catch (cause) {
      error = String(cause);
    } finally {
      flashing = false;
    }
  }

  // The screen is readable: this build is the remembered answer for this board.
  function confirmScreen() {
    checkingScreen = false;
    dismissed = true;
  }

  // The screen is wrong: the other build for this same board is the only
  // remaining explanation worth offering, so flash it rather than asking them
  // to diagnose a panel they cannot see the part number of.
  async function tryOtherPanel() {
    const other = alternatives[0];
    if (other) await flash(other);
  }
</script>

{#if prompted}
  <div class="firmware" role="alertdialog" aria-modal="false" aria-labelledby="firmware-title">
    <div class="firmware-head">
      <p class="registration-label">Device firmware</p>
      <h2 id="firmware-title">
        {checkingScreen ? 'Can you read the display?' : 'Flash the connected board?'}
      </h2>
      <p class="reason">
        {checkingScreen
          ? 'If the screen is blank, garbled, or mirrored, this board has the other panel and needs the other build.'
          : reasonText()}
      </p>
      <p class="port">{status?.port}{panelName ? ` · ${panelName}` : ''}</p>
    </div>

    {#if flashing}
      <div class="progress" aria-live="polite">
        <div class="bar"><span style={`--progress:${percent / 100}`}></span></div>
        <small>
          {stage === 'verifying' ? 'Verifying' : 'Writing'} · {percent}% ·
          do not unplug the board
        </small>
      </div>
    {:else if error}
      <p class="failed" role="alert">{error}</p>
    {:else if done}
      <p class="done" role="status">Firmware written and verified.</p>
    {/if}

    <div class="firmware-actions">
      {#if checkingScreen && !flashing}
        <button class="primary-action" onclick={confirmScreen}>Yes, it looks right</button>
        {#if alternatives.length}
          <button class="secondary-action" onclick={tryOtherPanel}>No — try the other panel</button>
        {/if}
      {:else}
        <button class="primary-action" onclick={() => flash()} disabled={flashing || !variant}>
          {flashing ? 'Flashing…' : 'Flash'}
        </button>
        <button class="secondary-action" onclick={() => (dismissed = true)} disabled={flashing}>
          Cancel
        </button>
      {/if}
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
