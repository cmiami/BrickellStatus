<script lang="ts">
  import { Check, MapPin, X } from '@lucide/svelte';

  import type { AlertArea } from '$lib/types';

  let {
    area,
    editing = false,
    saving = false,
    error = null,
    onsave,
    oncancel
  }: {
    area: AlertArea;
    editing?: boolean;
    saving?: boolean;
    error?: string | null;
    onsave: (area: AlertArea) => void;
    oncancel: () => void;
  } = $props();

  // Seeded once: the name is the reader's to type, so it must not be
  // overwritten by a prop update while they are typing it. The coordinates do
  // follow the pin, which is what the effect below is for.
  // svelte-ignore state_referenced_locally
  let draft = $state(structuredClone($state.snapshot(area)));
  let nameInput = $state<HTMLInputElement | null>(null);

  // The pin can be dragged while the dialog is open, so the coordinates follow
  // the marker rather than freezing at whatever the first click produced.
  $effect(() => {
    draft.latitude = area.latitude;
    draft.longitude = area.longitude;
  });

  $effect(() => {
    nameInput?.focus();
    nameInput?.select();
  });

  const named = $derived(draft.label.trim().length > 0);

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!named || saving) return;
    onsave({ ...$state.snapshot(draft), label: draft.label.trim() });
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === 'Escape') oncancel();
  }}
/>

<div class="pin-modal-scrim" role="presentation" onclick={oncancel}></div>

<div class="pin-modal" role="dialog" aria-modal="true" aria-labelledby="pin-modal-title">
  <h2 id="pin-modal-title">
    <MapPin size={18} strokeWidth={1.7} aria-hidden="true" />
    {editing ? 'Rename this place' : 'Name this place'}
  </h2>

  <form onsubmit={submit}>
    <label class="pin-field">
      <span>Name</span>
      <input
        bind:this={nameInput}
        bind:value={draft.label}
        maxlength="100"
        placeholder="Home, office, marina…"
        autocomplete="off"
      />
    </label>

    <p class="pin-coordinate">
      {draft.latitude.toFixed(5)}, {draft.longitude.toFixed(5)}
      <small>{draft.timeZone}</small>
    </p>

    <!-- Weather is the reason most people drop a pin, so it is on. The other
         two reach different providers and are opt-in rather than assumed. -->
    <fieldset class="pin-uses">
      <legend>Use this place for</legend>
      <label>
        <input type="checkbox" bind:checked={draft.weatherEnabled} />
        <span>Rain and wind</span>
      </label>
      <label>
        <input type="checkbox" bind:checked={draft.officialAlertsEnabled} />
        <span>Official alerts</span>
      </label>
      <label>
        <input type="checkbox" bind:checked={draft.tropicalContextEnabled} />
        <span>Tropical storms</span>
      </label>
    </fieldset>

    {#if error}
      <p class="pin-error" role="alert">{error}</p>
    {/if}

    <div class="pin-actions">
      <button type="submit" class="pin-save" disabled={!named || saving}>
        <Check size={16} aria-hidden="true" />
        {saving ? 'Saving…' : 'Save'}
      </button>
      <button type="button" class="pin-cancel" onclick={oncancel}>
        <X size={16} aria-hidden="true" /> Cancel
      </button>
    </div>
  </form>
</div>

<style>
  .pin-modal-scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(15, 42, 68, 0.42);
  }

  .pin-modal,
  .pin-modal form {
    display: grid;
    gap: 16px;
  }

  .pin-modal {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 41;
    width: min(420px, calc(100vw - 48px));
    padding: 24px;
    background: var(--paper);
    border: 1px solid var(--marine);
    box-shadow: var(--strip-shadow);
    transform: translate(-50%, -50%);
  }

  .pin-modal h2 {
    display: flex;
    gap: 9px;
    align-items: center;
    margin: 0;
    font-size: var(--type-title);
  }

  .pin-field {
    display: grid;
    gap: 6px;
  }

  .pin-field span {
    font-size: var(--type-caption);
    font-weight: 600;
    color: var(--muted);
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .pin-field input {
    padding: 11px 13px;
    font: inherit;
    font-size: 1rem;
    color: var(--ink);
    background: var(--white);
    border: 1px solid var(--rule-strong);
  }

  .pin-coordinate {
    display: flex;
    gap: 10px;
    align-items: baseline;
    margin: 0;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    font-size: var(--type-caption);
  }

  .pin-uses {
    display: grid;
    gap: 9px;
    padding: 0;
    margin: 0;
    border: 0;
  }

  .pin-uses legend {
    padding: 0 0 7px;
    font-size: var(--type-caption);
    font-weight: 600;
    color: var(--muted);
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .pin-uses label {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .pin-error {
    margin: 0;
    color: var(--alert);
    font-size: var(--type-caption);
  }

  .pin-actions {
    display: flex;
    gap: 10px;
  }

  .pin-actions button {
    display: flex;
    flex: 1;
    gap: 8px;
    align-items: center;
    justify-content: center;
    padding: 12px;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
    border: 1px solid var(--marine);
  }

  .pin-save {
    color: var(--white);
    background: var(--marine);
  }

  .pin-save:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .pin-cancel {
    color: var(--ink);
    background: var(--white);
  }
</style>
