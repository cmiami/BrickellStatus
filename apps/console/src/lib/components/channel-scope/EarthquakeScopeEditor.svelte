<script lang="ts">
  import type { ChannelPreference } from '$lib/types';
  import { scopeNumber, setScope, type ChannelChange } from './scope';

  let { channel, onchannelchange }: { channel: ChannelPreference; onchannelchange: ChannelChange } = $props();

  // Mirrors DEFAULT_EARTHQUAKE_MAGNITUDE in crates/runtime/src/preferences.rs.
  const DEFAULT_MAGNITUDE = 7.0;
  const MINIMUM = 0;
  const MAXIMUM = 10;
  const STEP = 0.1;

  const magnitude = $derived(scopeNumber(channel, 'minimumMagnitude', DEFAULT_MAGNITUDE));

  // A tenth is the whole resolution of this value, so float dust from repeated
  // addition would otherwise show up as 6.999999999999999 in the readout.
  function commit(value: number): void {
    const clamped = Math.min(MAXIMUM, Math.max(MINIMUM, Math.round(value * 10) / 10));
    // Committed rather than deferred: a slider or a step is a finished gesture,
    // the way a switch is. There is no half-typed state to wait out.
    setScope(channel, onchannelchange, 'minimumMagnitude', clamped, true);
  }
</script>

<div class="earthquake-fields">
  <div class="field">
    <span class="field-label" id="magnitude-label">Minimum magnitude</span>
    <!-- Not a text box. The value is one digit and one decimal between 0 and
         10, and a typed field has to survive a half-deleted value: clearing it
         yields NaN, which falls back to the default and snaps the number back
         under the reader's cursor. Stepping and dragging cannot express an
         empty value, so there is no invalid state to recover from, and neither
         needs a keyboard on a phone. -->
    <div class="magnitude-control">
      <button
        type="button"
        class="step"
        aria-label="Lower the minimum magnitude"
        disabled={magnitude <= MINIMUM}
        onclick={() => commit(magnitude - STEP)}>−</button>
      <output class="readout" for="magnitude-slider" aria-live="polite">M {magnitude.toFixed(1)}</output>
      <button
        type="button"
        class="step"
        aria-label="Raise the minimum magnitude"
        disabled={magnitude >= MAXIMUM}
        onclick={() => commit(magnitude + STEP)}>+</button>
    </div>
    <input
      id="magnitude-slider"
      class="slider"
      type="range"
      min={MINIMUM}
      max={MAXIMUM}
      step={STEP}
      value={magnitude}
      aria-labelledby="magnitude-label"
      aria-valuetext="Magnitude {magnitude.toFixed(1)}"
      oninput={(event) => commit(event.currentTarget.valueAsNumber)} />
    <div class="scale" aria-hidden="true"><span>0</span><span>5</span><span>10</span></div>
    <small class="field-note">Events below this are ignored. The worldwide feed carries roughly twenty magnitude 4.5 events a day and a handful above 7 in a year.</small>
  </div>
  <p>New events move ahead automatically. They leave the current notices after 24 hours.</p>
</div>

<style>
  .earthquake-fields {
    display: grid;
    gap: 14px;
    max-width: 620px;
  }

  .field-label {
    display: block;
    margin-bottom: 10px;
  }

  .magnitude-control {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  /* Comfortably past the 44px touch target minimum: this is the control that
     was unusable on a phone. */
  .step {
    flex: none;
    width: 52px;
    height: 52px;
    font-size: 24px;
    line-height: 1;
    color: var(--ink);
    background: var(--paper);
    border: 1px solid var(--rule);
    cursor: pointer;
  }

  .step:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .readout {
    flex: 1;
    text-align: center;
    font-size: 28px;
    font-variant-numeric: tabular-nums;
  }

  .slider {
    width: 100%;
    height: 44px;
    margin-top: 8px;
    accent-color: var(--ink);
  }

  .scale {
    display: flex;
    justify-content: space-between;
    color: var(--muted);
    font-size: var(--type-caption);
    font-variant-numeric: tabular-nums;
  }

  .earthquake-fields > p {
    margin: 0;
    padding: 12px 14px;
    color: var(--muted);
    background: var(--paper);
    border: 1px solid var(--rule);
    font-size: var(--type-caption);
    line-height: 1.45;
  }
</style>
