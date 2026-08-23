<script lang="ts">
  import { page } from '$app/stores';

  // Two desks that answer "what reaches me, and when": the channels themselves,
  // and the rules deciding which of them may interrupt. Kept together under one
  // heading rather than each claiming a seat in the rail.
  const tabs = [
    { href: '/channels', label: 'Channels' },
    { href: '/channels/policy', label: 'Alerts' }
  ];

  const current = $derived($page.url.pathname.replace(/\/$/, '') || '/channels');
</script>

<nav class="channel-tabs" aria-label="Channel sections">
  {#each tabs as tab (tab.href)}
    <a href={tab.href} aria-current={current === tab.href ? 'page' : undefined}>{tab.label}</a>
  {/each}
</nav>

<style>
  /* Matches the System tabs exactly: a marine rule under the one you are on,
     and the label carries the state so it survives without colour. */
  .channel-tabs {
    display: flex;
    gap: 2px;
    margin: 0 0 24px;
    border-bottom: 1px solid var(--rule);
  }

  .channel-tabs a {
    padding: 10px 16px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 650;
    letter-spacing: 0.06em;
    text-decoration: none;
    text-transform: uppercase;
    border-bottom: 3px solid transparent;
    margin-bottom: -1px;
  }

  .channel-tabs a:hover {
    color: var(--graphite);
    background: var(--frost);
  }

  .channel-tabs a[aria-current='page'] {
    color: var(--marine);
    border-bottom-color: var(--marine);
  }
</style>
