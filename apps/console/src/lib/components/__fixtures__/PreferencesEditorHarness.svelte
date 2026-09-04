<script lang="ts">
  import { preferencesEditor } from '$lib/preferencesEditor.svelte';
  import type { AppPreferences } from '$lib/types';

  let draft = $state<AppPreferences | null>(null);
  preferencesEditor(() => draft, (next) => { draft = next; });
</script>

{#if draft}
  <label>Name <input bind:value={draft.profile.name} /></label>
  <label>Channel <input bind:value={draft.profile.channels[0].title} /></label>
  <span>{draft.unitSystem}</span>
{/if}
