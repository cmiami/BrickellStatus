<script lang="ts">
  import { MoonStar } from '@lucide/svelte';

  import SwitchField from '$lib/components/SwitchField.svelte';
  import type { AppPreferences } from '$lib/types';

  // Quiet hours gate desktop and WhatsApp dispatches, so they live beside the
  // destinations they hold rather than on a page of their own.
  let { draft = $bindable() }: { draft: AppPreferences } = $props();
</script>

<section class="output-band quiet-section" aria-labelledby="quiet-heading">
  <header class="band-heading">
    <span class="route-mark"><MoonStar size={22} strokeWidth={1.5} aria-hidden="true" /></span>
    <div>
      <p class="eyebrow">Delivery schedule</p>
      <h2 id="quiet-heading">Quiet hours</h2>
      <p>Pause ordinary desktop and WhatsApp notices overnight. Live always stays current.</p>
    </div>
  </header>
  <div class="quiet-work">
    <SwitchField
      checked={draft.profile.quietHours.enabled}
      label="Use quiet hours"
      description="Emergency notices still come through automatically."
      onchange={(enabled) => {
        draft.profile.quietHours.enabled = enabled;
      }}
    />
    {#if draft.profile.quietHours.enabled}
      <div class="time-window">
        <label class="field">
          <span>Starts</span>
          <input type="time" required bind:value={draft.profile.quietHours.start} />
        </label>
        <label class="field">
          <span>Ends</span>
          <input type="time" required bind:value={draft.profile.quietHours.end} />
        </label>
      </div>
    {/if}
  </div>
</section>

<style>
  .quiet-work {
    display: grid;
    grid-template-columns: minmax(0, 1.1fr) minmax(260px, 0.9fr);
    gap: 32px;
    align-items: start;
    padding: clamp(22px, 2.7vw, 34px);
  }

  .time-window {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }

  @media (max-width: 720px) {
    .quiet-work {
      grid-template-columns: 1fr;
    }

    .time-window {
      grid-template-columns: 1fr 1fr;
    }
  }

  @media (max-width: 460px) {
    .time-window {
      grid-template-columns: 1fr;
    }
  }
</style>
