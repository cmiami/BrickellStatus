<script lang="ts">
  import { Save } from '@lucide/svelte';

  import AisOutputPanel from '$lib/components/outputs/AisOutputPanel.svelte';
  import DesktopOutputPanel from '$lib/components/outputs/DesktopOutputPanel.svelte';
  import EpaperOutputPanel from '$lib/components/outputs/EpaperOutputPanel.svelte';
  import WhatsAppOutputPanel from '$lib/components/outputs/WhatsAppOutputPanel.svelte';
  import { persistPreferences, preferences, saving } from '$lib/state';
  import type { AppPreferences } from '$lib/types';
  import './outputs.css';

  let draft = $state<AppPreferences | null>(null);
  let initialized = $state(false);

  $effect(() => {
    if ($preferences && !initialized) {
      draft = structuredClone($preferences);
      initialized = true;
    }
  });

  async function save() {
    if (!draft) return;
    await persistPreferences($state.snapshot(draft));
  }
</script>

<svelte:head>
  <title>Outputs · Tender’s Log</title>
  <meta name="description" content="Configure the e-paper display, AIS vessel evidence, desktop notices, and WhatsApp delivery." />
</svelte:head>

<section class="page-sheet outputs-page">
  <header class="page-heading-row">
    <div>
      <p class="registration-label">Connections and delivery</p>
      <h1 class="sheet-heading">Outputs</h1>
      <p class="sheet-intro">Connect the physical display and choose which real external services are allowed to run.</p>
    </div>
    <button class="primary-action save-action" onclick={save} disabled={!draft || $saving}>
      <Save size={17} aria-hidden="true" /> {$saving ? 'Saving outputs' : 'Save output settings'}
    </button>
  </header>

  {#if draft}
    <div class="output-stack">
      <EpaperOutputPanel bind:draft />
      <AisOutputPanel bind:draft />
      <WhatsAppOutputPanel bind:draft />
      <DesktopOutputPanel />
    </div>
  {:else}
    <div class="empty-sheet" aria-busy="true"><h2>Loading outputs</h2><p>Waiting for saved settings.</p></div>
  {/if}
</section>
