<script lang="ts">
  import { MonitorUp } from '@lucide/svelte';
  import { snapshot } from '$lib/state';

  const output = $derived($snapshot?.outputs.find((item) => item.id === 'desktop'));
</script>

<section class="output-band desktop-band" aria-labelledby="desktop-heading">
  <header class="band-heading compact-band-heading">
    <div class="route-mark"><MonitorUp size={26} strokeWidth={1.45} aria-hidden="true" /></div>
    <div>
      <h2 id="desktop-heading">Desktop notices</h2>
      <p>Urgent changes from every enabled channel are submitted as native notifications.</p>
      <small>Current notices always remain on Live. Native delivery is best effort because the OS does not return a reliable displayed receipt.</small>
    </div>
    <span class="status-word" data-state={output?.state ?? 'offline'}>{output?.state ?? 'offline'}</span>
  </header>
</section>
