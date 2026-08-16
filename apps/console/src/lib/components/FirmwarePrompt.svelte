<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  import type { FirmwareStatus, PanelRevision } from '$lib/types';

  const PANEL_CHOICE_KEY = 'tenders-log.panel-revision';

  let status = $state<FirmwareStatus | null>(null);
  let dismissed = $state(false);
  let flashing = $state(false);
  let written = $state(0);
  let total = $state(0);
  let stage = $state('writing');
  let error = $state<string | null>(null);
  let done = $state(false);

  // The panel revision cannot be detected: both builds run on the same ESP32-S3
  // with the same USB identifiers, and only the physical display differs.
  //
  // Asking up front made that our problem into the reader's — nobody knows
  // which revision is in a sealed case, and getting it wrong looks identical to
  // a failed flash. So we no longer ask. We flash the build that worked last
  // time, then ask the one question anyone can actually answer: can you read
  // the screen? A "no" flashes the other one and remembers.
  let panel = $state<PanelRevision | null>(null);
  let checkingScreen = $state(false);

  const prompted = $derived(
    !dismissed &&
      !!status &&
      !!status.port &&
      (status.requirement.state === 'required' || checkingScreen)
  );
  const variant = $derived(
    status?.variants.find((item) => item.panelRevision === panel) ?? status?.variants[0] ?? null
  );
  const otherVariant = $derived(
    status?.variants.find((item) => item.id !== variant?.id) ?? null
  );
  const percent = $derived(total > 0 ? Math.min(100, Math.round((written / total) * 100)) : 0);

  function reasonText(): string {
    const requirement = status?.requirement;
    if (requirement?.state !== 'required') return '';
    return requirement.reason.kind === 'notResponding'
      ? 'A board is connected but is not running Tender’s Log firmware.'
      : `The board is running build ${requirement.reason.device}; this app ships ${requirement.reason.bundled}.`;
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
    panel = (localStorage.getItem(PANEL_CHOICE_KEY) as PanelRevision | null) ?? null;
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

  function choosePanel(next: PanelRevision) {
    panel = next;
    localStorage.setItem(PANEL_CHOICE_KEY, next);
  }

  async function flash(target = variant) {
    if (!status?.port || !target) return;
    flashing = true;
    error = null;
    done = false;
    written = 0;
    total = target.totalBytes;
    try {
      await invoke('flash_firmware', { variantId: target.id, port: status.port });
      panel = target.panelRevision;
      done = true;
      // Keep the panel open for exactly one question, then get out of the way.
      checkingScreen = true;
      await refresh();
    } catch (cause) {
      error = String(cause);
    } finally {
      flashing = false;
    }
  }

  // The screen is readable: remember which build produced that and close.
  function confirmScreen() {
    if (panel) localStorage.setItem(PANEL_CHOICE_KEY, panel);
    checkingScreen = false;
    dismissed = true;
  }

  // The screen is wrong: the other panel build is the only remaining
  // explanation worth offering, so flash it rather than asking them to diagnose.
  async function tryOtherPanel() {
    if (!otherVariant) return;
    await flash(otherVariant);
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
      <p class="port">{status?.port}</p>
    </div>

    {#if flashing}
      <div class="progress" aria-live="polite">
        <div class="bar"><span style={`width:${percent}%`}></span></div>
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
        {#if otherVariant}
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

  .bar span {
    display: block;
    height: 100%;
    background: var(--amber);
    transition: width 200ms linear;
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
