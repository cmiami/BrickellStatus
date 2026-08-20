<script lang="ts">
  import { onMount } from 'svelte';
  import { BellRing, MoonStar, ShieldCheck } from '@lucide/svelte';

  import SwitchField from '$lib/components/SwitchField.svelte';
  import ChannelTabs from '$lib/components/ChannelTabs.svelte';
  import QuietHoursPanel from '$lib/components/outputs/QuietHoursPanel.svelte';
  import { persistPreferences, preferences, saving } from '$lib/state';
  import type { AppPreferences, InterruptPreset, PolicyProfile } from '$lib/types';

  let draft = $state<AppPreferences | null>(null);
  let initialized = $state(false);

  $effect(() => {
    if ($preferences && !initialized) {
      draft = structuredClone($preferences);
      initialized = true;
    }
  });


  const interruptOptions: Array<{ id: InterruptPreset; label: string; detail: string }> = [
    { id: 'recommended', label: 'Recommended', detail: 'What usually matters for this channel.' },
    { id: 'confirmed_only', label: 'Only when it happens', detail: 'Never on a prediction. Only once a source confirms it.' },
    { id: 'meaningful', label: 'Every change', detail: 'Interrupt whenever this channel changes state.' },
    { id: 'off', label: 'Never', detail: 'Still shown on screen, but never interrupts.' },
    { id: 'custom', label: 'Custom (off)', detail: 'Stays silent until detailed rules are set up.' }
  ];

  const activeInterruptCount = $derived(
    draft?.profile.channels.filter((channel) => channel.enabled && channel.interruptPreset !== 'off').length ?? 0
  );
  const quietTimeZoneOptions = $derived.by(() => {
    const options = new Map<string, string>([
      ['America/New_York', 'Eastern time'],
      ['UTC', 'UTC'],
      ['America/Chicago', 'Central time'],
      ['America/Los_Angeles', 'Pacific time']
    ]);
    for (const area of draft?.areas ?? []) {
      if (!options.has(area.timeZone)) options.set(area.timeZone, `${area.label} · ${area.timeZone}`);
    }
    const selected = draft?.profile.quietHours.timeZone;
    if (selected && !options.has(selected)) options.set(selected, selected);
    return [...options].map(([id, label]) => ({ id, label }));
  });


  function setInterrupt(channelId: string, value: InterruptPreset) {
    if (!draft) return;
    const channel = draft.profile.channels.find((item) => item.id === channelId);
    if (!channel) return;
    channel.interruptPreset = value;
  }

  async function save() {
    if (!draft) return;
    await persistPreferences($state.snapshot(draft));
  }

  // Edits apply as they are made, matching every other desk. A Save button asks
  // the reader to remember unfinished business, then rewards pressing it with a
  // banner to dismiss.
  const SETTLE_MS = 700;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

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

  const unsaved = $derived(
    !!draft && !!$preferences && JSON.stringify($state.snapshot(draft)) !== JSON.stringify($preferences)
  );

  $effect(() => {
    if (unsaved) scheduleSave();
  });
</script>

<svelte:head>
  <title>Policy · BrickellStatus</title>
  <meta name="description" content="Configure interruption thresholds, quiet hours, and policy presets." />
</svelte:head>

<section class="page-sheet policy-page">
  <ChannelTabs />
  <header class="page-heading-row">
    <div>
      <p class="registration-label">Attention policy</p>
      <h1 class="sheet-heading">Policy</h1>
      <p class="sheet-intro">
        A channel can be collected, shown, and delivered without earning takeover rights. This page controls the
        scarce permission to interrupt; channel content and destinations stay independently configurable.
      </p>
    </div>
  </header>

  {#if draft}
    <div class="policy-register">
      <aside class="policy-summary" aria-label="Current policy consequence">
        <div class="summary-mark" aria-hidden="true"><ShieldCheck size={30} strokeWidth={1.45} /></div>
        <p>Current instruction</p>
        <h2>{draft.profile.name}</h2>
        <strong>{activeInterruptCount} enabled channels may interrupt.</strong>
        <span>
          Quiet hours {draft.profile.quietHours.enabled
            ? `run ${draft.profile.quietHours.start}–${draft.profile.quietHours.end}`
            : 'are disabled'}.
        </span>
        <a href="/channels">Edit collection and destinations →</a>
      </aside>

      <div class="policy-work">
        <QuietHoursPanel bind:draft />

        <section class="policy-section" aria-labelledby="interrupt-heading">
          <header>
            <div>
              <h2 id="interrupt-heading"><BellRing size={22} strokeWidth={1.5} aria-hidden="true" /> What may interrupt you</h2>
              <p>Each row says whether that channel can seize the e-paper home frame or create outbound notices.</p>
            </div>
          </header>

          <div class="interrupt-ledger">
            <div class="ledger-head" aria-hidden="true">
              <span>Enabled channel</span><span>Permission</span><span>Plain-language effect</span>
            </div>
            {#each draft.profile.channels as channel (channel.id)}
              <div class:disabled={!channel.enabled} class="interrupt-row">
                <div>
                  <strong>{channel.title}</strong>
                  <small>{channel.enabled ? channel.presence.replace('_', ' ') : 'collector disabled'}</small>
                </div>
                <label class="visually-hidden" for={`interrupt-${channel.id}`}>Interrupt policy for {channel.title}</label>
                <select
                  id={`interrupt-${channel.id}`}
                  value={channel.interruptPreset}
                  disabled={!channel.enabled}
                  onchange={(event) => setInterrupt(channel.id, event.currentTarget.value as InterruptPreset)}
                >
                  {#each interruptOptions as option}
                    <option value={option.id} disabled={option.id === 'custom'}>{option.label}</option>
                  {/each}
                </select>
                <p>{interruptOptions.find((option) => option.id === channel.interruptPreset)?.detail}</p>
              </div>
            {/each}
          </div>
        </section>
      </div>
    </div>
  {:else}
    <div class="empty-sheet" aria-busy="true">
      <h2>Loading attention policy</h2>
      <p>The saved profile has not arrived yet. No policy is being inferred from defaults.</p>
    </div>
  {/if}
</section>

<style>
  .policy-page {
    padding-inline: clamp(18px, 3vw, 48px);
  }  .policy-register {
    display: grid;
    grid-template-columns: minmax(230px, 0.82fr) minmax(620px, 2.7fr);
    align-items: start;
    border-block: 1px solid var(--rule-strong);
  }

  .policy-summary {
    position: sticky;
    top: 96px;
    display: grid;
    min-height: 390px;
    align-content: start;
    gap: 12px;
    padding: clamp(24px, 3vw, 42px);
    color: var(--white);
    background: var(--marine);
  }

  .summary-mark {
    display: grid;
    width: 54px;
    height: 54px;
    place-items: center;
    margin-bottom: 22px;
    border: 1px solid rgba(255, 255, 255, 0.58);
  }

  .policy-summary p,
  .policy-summary span {
    margin: 0;
    color: var(--nav-muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  .policy-summary > p {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .policy-summary h2 {
    margin: 0;
    font-size: var(--type-headline);
    line-height: 0.88;
    text-transform: uppercase;
    overflow-wrap: anywhere;
  }

  .policy-summary strong {
    margin-top: 16px;
    color: var(--white);
    font-size: var(--type-body-small);
    line-height: 1.4;
  }

  .policy-summary a {
    width: fit-content;
    margin-top: auto;
    padding-top: 52px;
    color: var(--white);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-decoration: underline;
    text-underline-offset: 4px;
    text-transform: uppercase;
  }

  .policy-work {
    min-width: 0;
    background: var(--frost);
    border-inline-start: 1px solid var(--rule-strong);
  }

  .policy-section {
    padding: clamp(26px, 3.5vw, 52px);
    border-bottom: 1px solid var(--rule-strong);
  }

  .policy-section:last-child {
    border-bottom: 0;
  }

  .policy-section > header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 28px;
    margin-bottom: 28px;
  }

  .policy-section header > div {
    max-width: 68ch;
  }

  .policy-section h2 {
    margin: 0;
    color: var(--marine);
    font-size: var(--type-section);
    line-height: 0.95;
    text-transform: uppercase;
  }

  .policy-section header p {
    margin: 8px 0 0;
    color: var(--muted);
    font-size: var(--type-body-small);
    line-height: 1.5;
  }  .interrupt-ledger {
    border-top: 1px solid var(--rule-strong);
  }

  .ledger-head,
  .interrupt-row {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) minmax(170px, 0.8fr) minmax(240px, 1.4fr);
    align-items: center;
    gap: 20px;
  }

  .ledger-head {
    padding: 10px 9px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .interrupt-row {
    min-height: 72px;
    border-top: 1px solid var(--rule);
    padding: 11px 9px;
  }

  .interrupt-row > div:first-child {
    display: grid;
    gap: 4px;
  }

  .interrupt-row select {
    width: 100%;
    min-height: 42px;
    color: var(--graphite);
    background: var(--paper);
    border: 1px solid var(--steel);
    border-radius: 2px;
    padding: 8px 10px;
  }

  .interrupt-row p {
    margin: 0;
  }

  .interrupt-row.disabled {
    opacity: 0.55;
  }

  @media (max-width: 1050px) {
    .policy-register {
      grid-template-columns: 1fr;
    }

    .policy-summary {
      position: static;
      min-height: 0;
      grid-template-columns: auto 1fr;
      align-items: center;
    }

    .summary-mark {
      grid-row: 1 / 5;
      margin: 0 16px 0 0;
    }

    .policy-summary a {
      grid-column: 2;
      margin-top: 4px;
      padding-top: 8px;
    }

    .policy-work {
      border-inline-start: 0;
      border-top: 1px solid var(--rule-strong);
    }
  }

  @media (max-width: 760px) {
    .policy-section > header {
      align-items: stretch;
      flex-direction: column;
    }    .ledger-head {
      display: none;
    }

    .interrupt-row select {
      grid-column: 2;
    }  }

  @media (max-width: 480px) {
    .policy-summary {
      display: block;
    }

    .summary-mark {
      margin-bottom: 20px;
    }

    .policy-summary > * + * {
      margin-top: 10px;
    }

    .policy-section {
      padding-inline: 16px;
    }  }</style>
