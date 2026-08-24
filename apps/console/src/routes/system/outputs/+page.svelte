<script lang="ts">
  import { onMount } from 'svelte';

  import DesktopOutputPanel from '$lib/components/outputs/DesktopOutputPanel.svelte';
  import EpaperOutputPanel from '$lib/components/outputs/EpaperOutputPanel.svelte';
  import QuietHoursPanel from '$lib/components/outputs/QuietHoursPanel.svelte';
  import SystemTabs from '$lib/components/SystemTabs.svelte';
  import WhatsAppOutputPanel from '$lib/components/outputs/WhatsAppOutputPanel.svelte';
  import { persistPreferences, preferences } from '$lib/state';
  import type { AppPreferences } from '$lib/types';
  import './outputs.css';

  let draft = $state<AppPreferences | null>(null);
  let initialized = $state(false);
  let saveInFlight = false;
  let saveQueued = false;

  $effect(() => {
    if ($preferences && !initialized) {
      draft = structuredClone($preferences);
      initialized = true;
    }
  });

  async function save() {
    if (!draft) return;
    if (saveInFlight) {
      saveQueued = true;
      return;
    }

    saveInFlight = true;
    const payload = $state.snapshot(draft);
    const submittedFingerprint = JSON.stringify(payload);
    try {
      await persistPreferences(payload);
    } finally {
      saveInFlight = false;
      const currentFingerprint = draft ? JSON.stringify($state.snapshot(draft)) : '';
      const anotherSaveIsNeeded = saveQueued || currentFingerprint !== submittedFingerprint;
      saveQueued = false;
      if (anotherSaveIsNeeded) void save();
    }
  }

  // Edits apply as they are made, the way the channels desk already works.
  // A Save button asks the reader to remember they have unfinished business,
  // then rewards pressing it with a banner they have to dismiss — two chores
  // in exchange for nothing they did not already tell the app.
  //
  // Long enough that a number being typed is one write rather than twenty.
  const SETTLE_MS = 700;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  // Whatever is in flight goes with the reader; an edit must not die with the
  // page just because the debounce had not fired yet.
  function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = undefined;
      void save();
    }, SETTLE_MS);
  }

  onMount(() => () => {
    if (saveTimer) {
      clearTimeout(saveTimer);
      void save();
    }
  });

  const draftFingerprint = $derived(draft ? JSON.stringify($state.snapshot(draft)) : '');
  const savedFingerprint = $derived($preferences ? JSON.stringify($preferences) : '');

  $effect(() => {
    if (!initialized || !draftFingerprint || !savedFingerprint) return;
    if (draftFingerprint === savedFingerprint) {
      if (saveTimer) clearTimeout(saveTimer);
      saveTimer = undefined;
      return;
    }
    scheduleSave();
  });
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
    <!-- Says what is true right now, and asks for nothing. It is a status
         line, not a banner: nothing to dismiss and nothing to press. -->
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
