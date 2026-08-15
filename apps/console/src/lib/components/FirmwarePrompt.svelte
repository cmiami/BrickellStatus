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

  /// The panel revision cannot be detected: both builds run on the same
  /// ESP32-S3 with the same USB identifiers and only the physical display
  /// differs. Asked once, remembered, and changeable when the screen comes up
  /// wrong.
  let panel = $state<PanelRevision | null>(null);

  const prompted = $derived(
    !dismissed && !!status && status.requirement.state === 'required' && !!status.port
  );
  const variant = $derived(
    status?.variants.find((item) => item.panelRevision === panel) ?? status?.variants[0] ?? null
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

  async function flash() {
    if (!status?.port || !variant) return;
    flashing = true;
    error = null;
    done = false;
    written = 0;
    total = variant.totalBytes;
    try {
      await invoke('flash_firmware', { variantId: variant.id, port: status.port });
      done = true;
      await refresh();
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
      <h2 id="firmware-title">Flash the connected board?</h2>
      <p class="reason">{reasonText()}</p>
      <p class="port">{status?.port}</p>
    </div>

    {#if status && status.variants.length > 1}
      <fieldset class="panel-choice" disabled={flashing}>
        <legend>Which panel does this board have?</legend>
        <p class="hint">
          Both builds run on the same board and cannot be told apart over USB. If the
          screen comes up garbled, flash the other one.
        </p>
        {#each status.variants as option (option.id)}
          <label>
            <input
              type="radio"
              name="panel"
              value={option.panelRevision}
              checked={panel === option.panelRevision}
              onchange={() => choosePanel(option.panelRevision)}
            />
            {option.label}
          </label>
        {/each}
      </fieldset>
    {/if}

    {#if flashing}
      <div class="progress" aria-live="polite">
        <div class="bar"><span style={`width:${percent}%`}></span></div>
        <small>
          {stage === 'verifying' ? 'Verifying' : 'Writing'} · {percent}% ·
          do not unplug the board
        </small>
      </div>
    {:else if done}
      <p class="done" role="status">Firmware written and verified.</p>
    {:else if error}
      <p class="failed" role="alert">{error}</p>
    {/if}

    <div class="firmware-actions">
      <button
        class="primary-action"
        onclick={flash}
        disabled={flashing || !variant || (status!.variants.length > 1 && panel === null)}
      >
        {flashing ? 'Flashing…' : 'Flash'}
      </button>
      <button class="secondary-action" onclick={() => (dismissed = true)} disabled={flashing}>
        Cancel
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
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
  }

  .port {
    margin: 0;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    letter-spacing: 0.06em;
  }

  .panel-choice {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 11px 12px;
    background: var(--white);
    border: 1px solid var(--rule);
  }

  legend {
    padding: 0 4px;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--type-body-small);
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
