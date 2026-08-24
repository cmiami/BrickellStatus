<script lang="ts">
  import { Check, X } from '@lucide/svelte';

  import LocationMap from '$lib/components/LocationMap.svelte';
  import type { LocationMapPoint, UnitSystem } from '$lib/types';

  // Everywhere the app asks for a place, it asks the same way: one map, one
  // pin, and a decision. An inline map that is always open reads as scenery
  // and gives the reader nowhere to say "yes, that one" — every drag was
  // already the answer, so there was no way to look without committing.
  let {
    title,
    description,
    latitude,
    longitude,
    label = 'Selected place',
    unitSystem = 'imperial',
    confirmLabel = 'Use this place',
    nameSuggestion = '',
    onconfirm,
    oncancel
  }: {
    title: string;
    description: string;
    latitude: number;
    longitude: number;
    label?: string;
    unitSystem?: UnitSystem;
    confirmLabel?: string;
    nameSuggestion?: string;
    onconfirm: (latitude: number, longitude: number, name: string) => void;
    oncancel: () => void;
  } = $props();

  // Staged, not applied. Nothing reaches preferences until the reader says so,
  // which is the whole reason this is a dialog and not an inline map.
  // svelte-ignore state_referenced_locally
  let staged = $state({ latitude, longitude });
  let dialog = $state<HTMLDivElement | null>(null);

  // Offered, never demanded. Naming a place before looking at it is the thing
  // this dialog exists to avoid, but once the pin is down the reader knows
  // exactly what they picked, and the alternative was finding out later that it
  // had been called "Coverage area 3" and going to another screen to fix it.
  // Left blank, the caller keeps whatever it would have named this anyway.
  let name = $state('');

  const moved = $derived(staged.latitude !== latitude || staged.longitude !== longitude);

  // The only pin on the map. Saved places are deliberately hidden: the reader
  // is answering one question, and a field of other markers is just something
  // to click by mistake.
  const pin = $derived<LocationMapPoint>({
    id: 'picker.candidate',
    label: name.trim() || label,
    latitude: staged.latitude,
    longitude: staged.longitude,
    kind: 'candidate',
    draggable: true
  });

  $effect(() => {
    dialog?.focus();
  });

  function key(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      oncancel();
    }
  }
</script>

<svelte:window onkeydown={key} />

<!-- Clicking the scrim dismisses, which is a shortcut rather than the way
     out: Escape is bound on the window and Cancel is a real button, so a
     keyboard reader is never relying on this handler. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="picker-scrim" role="presentation" onclick={oncancel}>
  <div
    bind:this={dialog}
    class="picker"
    role="dialog"
    aria-modal="true"
    aria-labelledby="picker-title"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
  >
    <header>
      <div>
        <h2 id="picker-title">{title}</h2>
        <p>{description}</p>
      </div>
      <button class="picker-dismiss" type="button" aria-label="Close without saving" onclick={oncancel}>
        <X size={18} aria-hidden="true" />
      </button>
    </header>

    <div class="picker-map">
      <LocationMap
        variant="hero"
        points={[]}
        candidate={pin}
        selectedId={pin.id}
        {unitSystem}
        ariaLabel={`${title}. Drag the pin or click the map to choose a place.`}
        onpick={(nextLatitude, nextLongitude) =>
          (staged = { latitude: nextLatitude, longitude: nextLongitude })}
      />
    </div>

    <footer>
      <!-- The numbers stay visible while choosing. A pin is easy to place and
           hard to read back, and this is the value that gets stored. -->
      <p class="picker-readout" aria-live="polite">
        <span>{staged.latitude.toFixed(5)}, {staged.longitude.toFixed(5)}</span>
        <small>{moved ? 'Moved · not saved yet' : 'Unchanged'}</small>
      </p>
      <label class="picker-name">
        <span>Name (optional)</span>
        <input
          type="text"
          bind:value={name}
          maxlength="60"
          placeholder={nameSuggestion}
          autocomplete="off"
          enterkeyhint="done"
        />
      </label>
      <div class="picker-actions">
        <button class="secondary-action" type="button" onclick={oncancel}>Cancel</button>
        <button
          class="primary-action"
          type="button"
          onclick={() => onconfirm(staged.latitude, staged.longitude, name.trim())}
        >
          <Check size={16} aria-hidden="true" /> {confirmLabel}
        </button>
      </div>
    </footer>
  </div>
</div>

<style>
  .picker-scrim {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    /* The scrim itself covers the system bars, which is what a scrim should do;
       the card inside must not. Only matters where an inset exists -- a phone
       in landscape, where the card is tall enough to reach the cutout. */
    padding: calc(24px + env(safe-area-inset-top)) calc(24px + env(safe-area-inset-right))
      calc(24px + env(safe-area-inset-bottom)) calc(24px + env(safe-area-inset-left));
    background: rgba(15, 42, 68, 0.58);
  }

  .picker {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    width: min(980px, 100%);
    height: min(720px, 100%);
    overflow: hidden;
    background: var(--paper);
    border: 1px solid var(--rule-strong);
    border-radius: 2px;
    /* Offset and blur, so the sheet reads as lifted off the page rather than
       outlined against it. */
    box-shadow: 0 18px 48px rgba(17, 20, 24, 0.34);
  }

  .picker > header {
    display: flex;
    align-items: start;
    justify-content: space-between;
    gap: 20px;
    padding: 18px 20px;
    color: var(--white);
    background: var(--marine);
  }

  .picker h2 {
    margin: 0;
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .picker header p {
    max-width: 70ch;
    margin: 5px 0 0;
    color: var(--nav-muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  .picker-dismiss {
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    width: 34px;
    height: 34px;
    color: var(--white);
    background: transparent;
    border: 1px solid var(--nav-subdued);
    border-radius: 2px;
    cursor: pointer;
  }

  .picker-dismiss:hover {
    color: var(--marine);
    background: var(--white);
  }

  .picker-map {
    position: relative;
    min-height: 0;
  }

  .picker-map :global(.location-map) {
    height: 100%;
  }

  .picker > footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 14px 20px;
    background: var(--frost);
    border-top: 1px solid var(--rule);
  }

  .picker-readout {
    display: grid;
    gap: 2px;
    margin: 0;
  }

  .picker-readout span {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    letter-spacing: 0.04em;
  }

  .picker-readout small {
    color: var(--muted);
    font-size: var(--type-micro);
    font-family: var(--font-instrument);
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .picker-name {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .picker-name span {
    color: var(--muted);
    font-size: var(--type-caption);
  }

  .picker-name input {
    min-width: 0;
    /* Past the 44px touch minimum: this sits beside the two buttons a thumb is
       already reaching for. */
    min-height: 44px;
    padding: 0 12px;
    color: var(--ink);
    background: var(--white);
    border: 1px solid var(--rule-strong);
  }

  .picker-actions {
    display: flex;
    gap: 10px;
  }

  @media (max-width: 680px) {
    .picker-scrim {
      padding: 0;
    }

    .picker {
      width: 100%;
      height: 100%;
      border: 0;
    }

    .picker > footer {
      flex-direction: column;
      align-items: stretch;
      /* The scrim's padding is what held this card off the system bars, and the
         rule above drops it so the map can run full bleed. The footer then has
         to hold itself off: without this the confirm and cancel buttons sit
         under the navigation bar, which is exactly where a thumb cannot reach
         them. */
      padding-bottom: calc(14px + env(safe-area-inset-bottom));
    }

    .picker-actions button {
      flex: 1;
      min-height: 48px;
      justify-content: center;
    }
  }
</style>
