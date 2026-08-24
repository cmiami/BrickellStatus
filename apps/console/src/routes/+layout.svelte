<script lang="ts">
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';

  import '../app.css';
  import AppNav from '$lib/components/AppNav.svelte';
  import FirmwarePrompt from '$lib/components/FirmwarePrompt.svelte';
  import TopBar from '$lib/components/TopBar.svelte';
  import { openExternalUrl } from '$lib/api';
  import {
    displayStatus,
    loadApp,
    loadDisplayStatus,
    loadError,
    notice,
    startDisplayStatusRefresh,
    startSnapshotRefresh,
    stopDisplayStatusRefresh,
    stopSnapshotRefresh
  } from '$lib/state';

  let { children } = $props();

  // A result the reader did not ask to keep should not wait to be clicked away.
  // Failures dwell longer than confirmations because they are the ones worth
  // reading, but both leave on their own.
  const NOTICE_MS = { ok: 3_500, failed: 7_000 };
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const current = $notice;
    clearTimeout(noticeTimer);
    if (!current) return;
    noticeTimer = setTimeout(
      () => notice.set(null),
      current.ok ? NOTICE_MS.ok : NOTICE_MS.failed
    );
    return () => clearTimeout(noticeTimer);
  });

  function openExternalLink(event: MouseEvent) {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const anchor = event.composedPath().find((node): node is HTMLAnchorElement => node instanceof HTMLAnchorElement);
    if (!anchor) return;
    const target = new URL(anchor.href, window.location.href);
    if (target.origin === window.location.origin || target.protocol !== 'https:') return;
    event.preventDefault();
    void openExternalUrl(target.href).catch((error) => {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The system browser could not open this link.' });
    });
  }

  onMount(() => {
    void loadApp();
    void loadDisplayStatus();
    startSnapshotRefresh();
    startDisplayStatusRefresh();
    document.addEventListener('click', openExternalLink);
    const statusListener = import('@tauri-apps/api/event')
      .then(({ listen }) =>
        listen<import('$lib/types').DisplayConnectionStatus>(
          'display-connection-status',
          (event) => displayStatus.set(event.payload)
        )
      )
      .catch(() => () => {});
    return () => {
      stopSnapshotRefresh();
      stopDisplayStatusRefresh();
      document.removeEventListener('click', openExternalLink);
      void statusListener.then((unlisten) => unlisten());
    };
  });
</script>

<svelte:head>
  <title>BrickellStatus · BrickellStatus</title>
</svelte:head>

<a class="skip-link" href="#main-content">Skip to content</a>
<AppNav />
<TopBar />

<main id="main-content" class="app-main" tabindex="-1">
  {#if $loadError}
    <div class="connection-warning" role="alert">
      <strong>Live refresh paused.</strong>
      <span>{$loadError} The last complete snapshot remains visible.</span>
      <button onclick={() => loadApp()}>Try again</button>
    </div>
  {/if}

  {#if $notice}
    <div
      class="global-notice"
      data-ok={$notice.ok}
      role={$notice.ok ? 'status' : 'alert'}
      in:fade={{ duration: 140 }}
      out:fade={{ duration: 420 }}
    >
      <span>{$notice.message}</span>
    </div>
  {/if}

  {@render children()}
</main>

<!-- Sits outside the page flow so a connected board is offered firmware
     wherever the operator happens to be. -->
<FirmwarePrompt />

<style>
  .app-main {
    min-height: calc(100vh - 72px);
    margin-left: 104px;
  }

  .skip-link {
    position: fixed;
    z-index: 100;
    top: 10px;
    left: 120px;
    padding: 10px 14px;
    color: var(--white);
    background: var(--marine);
    transform: translateY(-160%);
  }

  .skip-link:focus {
    transform: translateY(0);
  }

  .connection-warning {
    position: relative;
    z-index: 18;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 22px;
    color: var(--graphite);
    background: var(--amber-sheet);
    border-bottom: 1px solid var(--amber-ink);
    font-size: var(--type-label);
  }

  /* A result that leaves on its own must not push the page down and pull it
     back up on the way out, so it lifts off the sheet instead of sitting in
     the flow. Temporary overlays are the one thing allowed to lift. */
  .global-notice {
    position: fixed;
    inset-inline: 0;
    bottom: 26px;
    z-index: 40;
    width: fit-content;
    max-width: min(78ch, calc(100vw - 32px));
    margin-inline: auto;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 20px;
    color: var(--graphite);
    background: var(--frost);
    border: 1px solid var(--rule-strong);
    border-radius: 2px;
    box-shadow: 0 10px 22px -14px rgb(17 20 24 / 55%);
    font-size: var(--type-label);
    pointer-events: none;
  }

  .connection-warning strong {
    white-space: nowrap;
  }

  .connection-warning button {
    color: var(--graphite);
    background: transparent;
    border-bottom: 1px solid currentColor;
    padding: 3px;
    font-size: var(--type-caption);
    font-weight: 600;
    cursor: pointer;
  }

  .global-notice[data-ok='true'] {
    background: var(--success-sheet);
    border-color: var(--success);
  }

  .global-notice[data-ok='false'] {
    color: var(--white);
    background: var(--danger);
    border-color: var(--graphite);
  }


  @media (prefers-reduced-motion: reduce) {
    .global-notice {
      transition: none;
    }
  }

  @media (max-width: 720px) {
    .app-main {
      /* The tab bar is fixed, so every page has to end above it. Each route
         used to reserve that space itself with a hard-coded number, none of
         which counted env(safe-area-inset-bottom) -- so on a phone with gesture
         navigation the last rows sat under the bar with no way to scroll to
         them. Reserved once here, from the same expression AppNav sizes itself
         with, so the two cannot drift. */
      min-height: calc(100dvh - 58px - env(safe-area-inset-top));
      padding-bottom: calc(76px + env(safe-area-inset-bottom));
      margin-left: 0;
    }

    .skip-link {
      left: 12px;
    }

    .connection-warning {
      align-items: flex-start;
      padding: 10px 14px;
    }

    .global-notice {
      bottom: 14px;
      padding: 11px 15px;
    }

    .connection-warning strong {
      display: none;
    }
  }
</style>
