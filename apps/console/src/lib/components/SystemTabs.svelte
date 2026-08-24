<script lang="ts">
  import { page } from '$app/stores';

  // Three desks that answer "is the machine working", kept together under one
  // heading instead of each claiming a seat in the rail.
  const tabs = [
    { href: '/system', label: 'Health' },
    { href: '/system/outputs', label: 'Outputs' },
    { href: '/system/log', label: 'History' }
  ];

  const current = $derived($page.url.pathname.replace(/\/$/, '') || '/system');
</script>

<nav class="system-tabs" aria-label="System sections">
  {#each tabs as tab (tab.href)}
    <a href={tab.href} aria-current={current === tab.href ? 'page' : undefined}>{tab.label}</a>
  {/each}
</nav>

<style>
  /* Index tabs of a working log: a marine rule under the one you are on, and
     the label itself carries the state so it survives without colour. */
  .system-tabs {
    display: flex;
    gap: 2px;
    margin: 0 0 24px;
    border-bottom: 1px solid var(--rule);
  }

  .system-tabs a {
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

  .system-tabs a:hover {
    color: var(--graphite);
    background: var(--frost);
  }

  .system-tabs a[aria-current='page'] {
    color: var(--marine);
    border-bottom-color: var(--marine);
  }
</style>
