<script lang="ts">
  import { RotateCw } from '@lucide/svelte';
  import { onMount } from 'svelte';

  import { refreshSources } from '$lib/api';
  import { notice, persistPreferences, preferences, saving, snapshot } from '$lib/state';
  import type { UnitSystem } from '$lib/types';

  // `$state`, not a plain `let`. Without a rune this component compiled in
  // legacy mode, where the template's dependencies are found by reading the
  // expressions in it: `{localTime()}` mentions `localTime` and never `now`, so
  // reassigning `now` every second invalidated nothing and the clock showed the
  // moment the app started, forever. Runes track reads at runtime, so calling
  // through a function is no longer a way to hide a dependency.
  //
  // `refreshing` has to come along. One rune puts the whole component in runes
  // mode, and a plain `let` left behind would quietly stop being reactive --
  // here that would have frozen the refresh button's spinner instead.
  let now = $state(new Date());
  let refreshing = $state(false);

  onMount(() => {
    const timer = setInterval(() => (now = new Date()), 1000);
    return () => clearInterval(timer);
  });

  // Hoisted: constructing an Intl formatter is expensive and these were being
  // rebuilt twice a second.
  const TIME_FORMAT = new Intl.DateTimeFormat('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false
  });
  const DATE_FORMAT = new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric'
  });

  const localTime = $derived(TIME_FORMAT.format(now));
  const localDate = $derived(DATE_FORMAT.format(now));

  const unhealthyCount = () =>
    $snapshot?.system.sources.filter((source) => source.availability !== 'fresh').length ?? 0;

  async function refresh() {
    refreshing = true;
    try {
      notice.set(await refreshSources());
    } finally {
      refreshing = false;
    }
  }

  async function setUnitSystem(unitSystem: UnitSystem) {
    if (!$preferences || $preferences.unitSystem === unitSystem || $saving) return;
    await persistPreferences({ ...structuredClone($preferences), unitSystem });
  }
</script>

<header class="top-bar">
  <div class="title-lockup">
    <a href="/">BrickellStatus</a>
    <span>Personal signal console</span>
  </div>

  {#if $preferences}
    <fieldset class="unit-switch" disabled={$saving}>
      <legend>Display units</legend>
      {#each ['imperial', 'metric'] as unit}
        <button
          type="button"
          role="radio"
          aria-checked={$preferences.unitSystem === unit}
          onclick={() => setUnitSystem(unit as UnitSystem)}
        >
          {unit === 'imperial' ? 'Imperial' : 'Metric'}
        </button>
      {/each}
    </fieldset>
  {/if}

  <div class="clock-block" aria-label={`Local time ${localTime}, ${localDate}`}>
    <span class="registration-label">Local time</span>
    <strong>{localTime}</strong>
    <small>{localDate}</small>
  </div>

  <div class="system-block">
    {#if $snapshot}
      <a class="system-status-link" href="/system#source-health">
        <span class="registration-label">Console status</span>
        <span class="status-word" data-state={$snapshot.system.status}>
          {$snapshot.system.status === 'nominal'
            ? 'All sources current'
            : `${$snapshot.system.status} · ${unhealthyCount()} need attention`}
        </span>
      </a>
    {/if}
    <button class="refresh-button" onclick={refresh} disabled={refreshing} aria-label="Refresh all sources">
      <RotateCw size={18} class={refreshing ? 'spinning' : undefined} aria-hidden="true" />
      <span>{refreshing ? 'Refreshing' : 'Refresh'}</span>
    </button>
  </div>
</header>

<style>
  .top-bar {
    position: sticky;
    z-index: 20;
    top: 0;
    display: grid;
    /* Android draws the webview edge to edge, underneath the status bar, so the
       header has to reserve that strip itself or the clock and battery land on
       top of the title. env() resolves to 0 wherever there is no inset, which
       is every desktop build. */
    padding-top: env(safe-area-inset-top);
    min-height: calc(72px + env(safe-area-inset-top));
    grid-template-columns: minmax(230px, 1fr) auto auto auto;
    align-items: stretch;
    margin-left: 104px;
    color: var(--graphite);
    background: rgba(244, 247, 249, 0.97);
    border-bottom: 1px solid var(--rule-strong);
  }

  .title-lockup {
    display: flex;
    align-items: baseline;
    gap: 18px;
    padding: 14px 28px;
  }

  .title-lockup a {
    color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-section);
    font-weight: 700;
    line-height: 1;
    letter-spacing: -0.015em;
    text-decoration: none;
  }

  .title-lockup span {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.065em;
    text-transform: uppercase;
  }

  .clock-block {
    display: grid;
    min-width: 154px;
    align-content: center;
    padding: 10px 24px;
    border-left: 1px solid var(--rule);
  }

  .unit-switch {
    display: grid;
    min-width: 150px;
    grid-template-columns: 1fr 1fr;
    align-content: center;
    margin: 0;
    border: 0;
    border-left: 1px solid var(--rule);
    padding: 9px 16px;
  }

  .unit-switch legend {
    grid-column: 1 / -1;
    width: 100%;
    margin-bottom: 5px;
    padding: 0;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .unit-switch button {
    min-height: 28px;
    color: var(--muted);
    background: transparent;
    border: 1px solid var(--rule-strong);
    padding: 5px 8px;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.045em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .unit-switch button + button {
    border-left: 0;
  }

  .unit-switch button[aria-checked='true'] {
    color: var(--white);
    background: var(--marine);
  }

  .clock-block strong {
    font-family: var(--font-instrument);
    font-size: var(--type-section);
    font-weight: 600;
    line-height: 1;
    letter-spacing: 0.07em;
  }

  .clock-block small {
    color: var(--muted);
    font-size: var(--type-micro);
  }

  .system-block {
    display: flex;
    min-width: 310px;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 10px 20px;
    border-left: 1px solid var(--rule);
  }

  .system-status-link {
    display: grid;
    gap: 7px;
    color: inherit;
    text-decoration: none;
  }

  .system-status-link:hover .status-word {
    text-decoration: underline;
    text-underline-offset: 4px;
  }

  .refresh-button {
    display: inline-flex;
    min-height: 40px;
    align-items: center;
    gap: 8px;
    color: var(--marine);
    background: transparent;
    border: 1px solid var(--rule-strong);
    border-radius: 2px;
    padding: 9px 11px;
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 600;
    letter-spacing: 0.055em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .refresh-button:hover:not(:disabled) {
    background: var(--paper);
  }

  :global(.spinning) {
    animation: spin 700ms linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 1020px) {
    .top-bar {
      grid-template-columns: 1fr auto auto;
    }

    .clock-block {
      display: none;
    }

    .system-block {
      min-width: 280px;
    }
  }

  @media (max-width: 720px) {
    .top-bar {
      min-height: calc(58px + env(safe-area-inset-top));
      grid-template-columns: 1fr auto;
      margin-left: 0;
    }

    .title-lockup {
      gap: 10px;
      padding: 10px 16px;
    }

    .title-lockup a {
      font-size: var(--type-title);
    }

    .title-lockup span {
      display: none;
    }

    .system-block {
      min-width: auto;
      padding: 8px 12px;
      border-left: 0;
    }

    .system-status-link {
      display: none;
    }

    .unit-switch {
      min-width: 126px;
      padding-inline: 8px;
    }

    .unit-switch legend {
      display: none;
    }

    .refresh-button span {
      display: none;
    }
  }
</style>
