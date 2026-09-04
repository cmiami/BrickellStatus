<script lang="ts">
  import DesktopOutputPanel from '$lib/components/outputs/DesktopOutputPanel.svelte';
  import EpaperOutputPanel from '$lib/components/outputs/EpaperOutputPanel.svelte';
  import QuietHoursPanel from '$lib/components/outputs/QuietHoursPanel.svelte';
  import SystemTabs from '$lib/components/SystemTabs.svelte';
  import WhatsAppOutputPanel from '$lib/components/outputs/WhatsAppOutputPanel.svelte';
  import { preferencesEditor } from '$lib/preferencesEditor.svelte';
  import type { AppPreferences } from '$lib/types';
  import './outputs.css';

  let draft = $state<AppPreferences | null>(null);
  preferencesEditor(() => draft, (next) => { draft = next; });
</script>

<svelte:head>
  <title>Outputs · BrickellStatus</title>
  <meta name="description" content="Configure the e-paper display, desktop notices, and WhatsApp delivery." />
</svelte:head>

<section class="page-sheet outputs-page">
  <SystemTabs />
  <header class="page-heading-row">
    <div>
      <p class="registration-label">Connections and delivery</p>
      <h1 class="sheet-heading">Outputs</h1>
      <p class="sheet-intro">Connect each output once. Enabled channels share one automatic urgency policy.</p>
    </div>
  </header>

  {#if draft}
    <div class="output-stack">
      <EpaperOutputPanel bind:draft />
      <QuietHoursPanel bind:draft />
      <WhatsAppOutputPanel bind:draft />
      <DesktopOutputPanel />
    </div>
  {:else}
    <div class="empty-sheet" aria-busy="true"><h2>Loading outputs</h2><p>Waiting for saved settings.</p></div>
  {/if}
</section>
