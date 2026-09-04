<script lang="ts">
  import {
    BellRing,
    Clock3,
    Database,
    MapPinned,
    RadioTower,
  } from '@lucide/svelte';
  import { page } from '$app/stores';

  // The rail is for places you go to decide something. The log and the output
  // desk are places you go to check on the machine, so they live under System
  // rather than taking a seat of their own.
  const items = [
    { href: '/', label: 'Live', icon: Clock3 },
    { href: '/channels', label: 'Channels', icon: RadioTower },
    { href: '/map', label: 'Map', icon: MapPinned },
    { href: '/system', label: 'System', icon: Database }
  ];

  function active(href: string): boolean {
    return href === '/' ? $page.url.pathname === '/' : $page.url.pathname.startsWith(href);
  }
</script>

<aside class="app-nav" aria-label="Primary">
  <a class="brand-mark" href="/" aria-label="BrickellStatus live console">
    <BellRing size={26} strokeWidth={1.6} aria-hidden="true" />
    <span class="brand-word">BRICKELL<br />STATUS</span>
  </a>

  <nav>
    {#each items as item}
      <a href={item.href} class:active={active(item.href)} aria-current={active(item.href) ? 'page' : undefined}>
        <svelte:component this={item.icon} size={24} strokeWidth={1.55} aria-hidden="true" />
        <span>{item.label}</span>
      </a>
    {/each}
  </nav>

</aside>

<style>
  .app-nav {
    position: fixed;
    z-index: 30;
    inset: 0 auto 0 0;
    display: flex;
    width: 104px;
    min-height: 100vh;
    flex-direction: column;
    color: var(--white);
    background: var(--marine);
    border-right: 1px solid rgba(255, 255, 255, 0.22);
  }

  .brand-mark {
    display: flex;
    min-height: 92px;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--white);
    border-bottom: 1px solid rgba(255, 255, 255, 0.22);
    text-decoration: none;
  }

  .brand-word {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 700;
    line-height: 0.88;
    letter-spacing: 0.055em;
  }

  nav {
    display: grid;
    flex: 1;
    align-content: start;
    padding-top: 14px;
  }

  nav a {
    position: relative;
    display: grid;
    min-height: 86px;
    place-items: center;
    align-content: center;
    gap: 9px;
    color: var(--nav-muted);
    text-decoration: none;
    transition:
      color 160ms ease-out,
      background-color 160ms ease-out;
  }

  nav a::before {
    position: absolute;
    inset: 14px auto 14px 0;
    width: 1px;
    background: transparent;
    content: '';
  }

  nav a:hover {
    color: var(--white);
    background: rgba(255, 255, 255, 0.06);
  }

  nav a:focus-visible,
  .brand-mark:focus-visible {
    outline-color: var(--white);
  }

  nav a.active {
    color: var(--white);
    background: rgba(23, 79, 120, 0.56);
  }

  nav a.active::before {
    background: var(--amber);
  }

  nav a span {
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 600;
    letter-spacing: 0.045em;
    text-transform: uppercase;
  }

  @media (max-width: 720px) {
    .app-nav {
      inset: auto 0 0;
      width: 100%;
      min-height: auto;
      height: calc(76px + env(safe-area-inset-bottom));
      border-top: 1px solid rgba(255, 255, 255, 0.28);
      border-right: 0;
      padding-bottom: env(safe-area-inset-bottom);
    }

    .brand-mark {
      display: none;
    }

    nav {
      display: grid;
      height: 76px;
      /* One column per tab. Seven was left over from a longer nav, so the four
         that remain crowded into the left four-sevenths and the rest of the bar
         was empty marine -- visible on anything wider than 430px, where the
         rule below stops covering for it. */
      grid-auto-flow: column;
      grid-auto-columns: minmax(0, 1fr);
      align-content: stretch;
      padding: 0;
    }

    nav a {
      min-height: 76px;
      gap: 5px;
    }

    nav a::before {
      inset: 0 10px auto;
      width: auto;
      height: 1px;
    }

    nav a span {
      font-size: var(--type-micro);
    }
  }

  @media (max-width: 430px) {
    nav {
      grid-template-columns: none;
      grid-auto-columns: minmax(64px, 1fr);
      grid-auto-flow: column;
      overflow-x: auto;
      overscroll-behavior-inline: contain;
      scroll-snap-type: inline proximity;
      scrollbar-width: thin;
    }

    nav a {
      scroll-snap-align: start;
    }
  }
</style>
