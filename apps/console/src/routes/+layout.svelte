<script lang="ts">
  import { onMount } from 'svelte';

  import '../app.css';
  import AppNav from '$lib/components/AppNav.svelte';
  import FirmwarePrompt from '$lib/components/FirmwarePrompt.svelte';
  import TopBar from '$lib/components/TopBar.svelte';
  import { openExternalUrl } from '$lib/api';
  import { loadApp, startSnapshotRefresh, stopSnapshotRefresh, loadError, notice } from '$lib/state';

  let { children } = $props();

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
    startSnapshotRefresh();
    document.addEventListener('click', openExternalLink);
    return () => {
      stopSnapshotRefresh();
      document.removeEventListener('click', openExternalLink);
    };
  });
</script>

<svelte:head>
  <title>Tender’s Log · PuenteGonorrea</title>
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
    <div class="global-notice" data-ok={$notice.ok} role="status">
      <span>{$notice.message}</span>
      <button aria-label="Dismiss notice" onclick={() => notice.set(null)}>Dismiss</button>
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

  .connection-warning,
  .global-notice {
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

  .connection-warning strong {
    white-space: nowrap;
  }

  .connection-warning button,
  .global-notice button {
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
    border-bottom-color: var(--success);
  }

  .global-notice[data-ok='false'] {
    color: var(--white);
    background: var(--danger);
    border-bottom-color: var(--graphite);
  }

  .global-notice[data-ok='false'] button {
    color: var(--white);
  }

  @media (max-width: 720px) {
    .app-main {
      min-height: calc(100vh - 134px);
      margin-left: 0;
    }

    .skip-link {
      left: 12px;
    }

    .connection-warning,
    .global-notice {
      align-items: flex-start;
      padding: 10px 14px;
    }

    .connection-warning strong {
      display: none;
    }
  }
</style>
