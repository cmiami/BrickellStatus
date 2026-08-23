<script lang="ts">
  import { MoonStar } from '@lucide/svelte';

  import SwitchField from '$lib/components/SwitchField.svelte';
  import type { AppPreferences } from '$lib/types';

  // Quiet hours gate desktop and WhatsApp dispatches, so they live beside the
  // destinations they hold rather than on a page of their own.
  let { draft = $bindable() }: { draft: AppPreferences } = $props();

  const quietTimeZoneOptions = $derived.by(() => {
    const selected = draft.profile.quietHours.timeZone;
    const zones = new Set(['America/New_York', 'UTC']);
    if (selected) zones.add(selected);
    return [...zones].map((id) => ({ id, label: id }));
  });
</script>

<section class="output-band quiet-section" aria-labelledby="quiet-heading">
  <header>
    <div>
      <h2 id="quiet-heading"><MoonStar size={22} strokeWidth={1.5} aria-hidden="true" /> Quiet hours</h2>
      <p>Pause ordinary alerts on a schedule. Live status and source warnings stay visible.</p>
    </div>
  </header>
  <div class="quiet-grid">
    <SwitchField
      checked={draft.profile.quietHours.enabled}
      label="Quiet hours enabled"
      description="Pauses non-emergency desktop and WhatsApp alerts."
      onchange={(enabled) => {
        draft.profile.quietHours.enabled = enabled;
      }}
    />
    <label class="field">
      <span>Starts</span>
      <input type="time" required bind:value={draft.profile.quietHours.start} />
    </label>
    <label class="field">
      <span>Ends</span>
      <input type="time" required bind:value={draft.profile.quietHours.end} />
    </label>
    <label class="field">
      <span>Time zone</span>
      <select bind:value={draft.profile.quietHours.timeZone}>
        {#each quietTimeZoneOptions as option}
  <option value={option.id}>{option.label}</option>
        {/each}
      </select>
    </label>
    <SwitchField
      checked={draft.profile.quietHours.bypassEmergency}
      label="Critical bypass"
      description="Extreme official alerts and a bridge confirmed up may still get through."
      onchange={(enabled) => {
        draft.profile.quietHours.bypassEmergency = enabled;
      }}
    />
  </div>
</section>
