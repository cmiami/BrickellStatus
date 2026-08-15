<script lang="ts">
  import { BellRing, MoonStar, Save, ShieldCheck } from '@lucide/svelte';

  import SwitchField from '$lib/components/SwitchField.svelte';
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

  const presets: Array<{
    id: PolicyProfile['preset'];
    label: string;
    promise: string;
    detail: string;
  }> = [
    {
      id: 'bridge_first',
      label: 'Bridge First',
      promise: 'The road decision always comes home.',
      detail: 'Bridge, official alerts, and weather may interrupt; supporting channels stay quiet.'
    },
    {
      id: 'miami_watch',
      label: 'Miami Watch',
      promise: 'Local hazards join the bridge watch.',
      detail: 'Miami weather, official alerts, hurricanes, and significant earthquakes get priority.'
    },
    {
      id: 'full_signal_desk',
      label: 'Full Signal Desk',
      promise: 'Every enabled channel enters rotation.',
      detail: 'Broadest awareness, while outbound notices still require an explicit interrupt rule.'
    },
    {
      id: 'quiet_watch',
      label: 'Quiet Watch',
      promise: 'Only confirmed, consequential changes interrupt.',
      detail: 'Routine rotation remains visible without producing messages or desktop notices.'
    }
  ];

  const interruptOptions: Array<{ id: InterruptPreset; label: string; detail: string }> = [
    { id: 'recommended', label: 'Recommended', detail: 'Channel-specific material changes.' },
    { id: 'confirmed_only', label: 'Confirmed only', detail: 'Authoritative confirmed events only.' },
    { id: 'meaningful', label: 'Meaningful changes', detail: 'Every material state transition.' },
    { id: 'off', label: 'Never', detail: 'Visible only; no takeover or message.' },
    { id: 'custom', label: 'Custom · parked', detail: 'Fails closed until a detailed rule matrix is configured.' }
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

  function applyPreset(id: PolicyProfile['preset']) {
    if (!draft || id === 'custom') return;
    const profile = draft.profile;
    profile.preset = id;

    for (const channel of profile.channels) {
      if (id === 'bridge_first') {
        channel.enabled = !['earthquake.significant', 'markets.watchlist'].includes(channel.id);
        channel.presence = channel.id === 'bridge.brickell'
          ? 'home'
          : ['official.miami', 'hurricane.atlantic'].includes(channel.id)
            ? 'active_only'
            : 'rotation';
        channel.interruptPreset = channel.id === 'bridge.brickell'
          ? 'recommended'
          : channel.id === 'official.miami'
            ? 'confirmed_only'
            : channel.id === 'weather.miami'
              ? 'recommended'
              : 'off';
      }

      if (id === 'miami_watch') {
        channel.enabled = channel.id !== 'markets.watchlist';
        channel.presence = channel.id === 'bridge.brickell'
          ? 'home'
          : ['official.miami', 'hurricane.atlantic', 'earthquake.significant'].includes(channel.id)
            ? 'active_only'
            : 'rotation';
        channel.interruptPreset = ['bridge.brickell', 'weather.miami'].includes(channel.id)
          ? 'recommended'
          : ['official.miami', 'hurricane.atlantic', 'earthquake.significant'].includes(channel.id)
            ? 'confirmed_only'
            : 'off';
      }

      if (id === 'full_signal_desk') {
        channel.enabled = true;
        channel.presence = channel.id === 'bridge.brickell' ? 'home' : 'rotation';
        channel.interruptPreset = ['news.local', 'markets.watchlist'].includes(channel.id)
          ? 'off'
          : 'meaningful';
      }

      if (id === 'quiet_watch') {
        channel.enabled = true;
        channel.presence = channel.id === 'bridge.brickell'
          ? 'home'
          : ['official.miami', 'hurricane.atlantic'].includes(channel.id)
            ? 'active_only'
            : 'rotation';
        channel.interruptPreset = ['bridge.brickell', 'official.miami', 'hurricane.atlantic'].includes(channel.id)
          ? 'confirmed_only'
          : 'off';
      }
    }
  }

  function setInterrupt(channelId: string, value: InterruptPreset) {
    if (!draft) return;
    const channel = draft.profile.channels.find((item) => item.id === channelId);
    if (!channel) return;
    channel.interruptPreset = value;
    draft.profile.preset = 'custom';
  }

  async function save() {
    if (!draft) return;
    await persistPreferences($state.snapshot(draft));
  }
</script>

<svelte:head>
  <title>Policy · Tender’s Log</title>
  <meta name="description" content="Configure interruption thresholds, quiet hours, and policy presets." />
</svelte:head>

<section class="page-sheet policy-page">
  <header class="page-heading-row">
    <div>
      <p class="registration-label">Attention policy</p>
      <h1 class="sheet-heading">Decide what may interrupt you</h1>
      <p class="sheet-intro">
        A channel can be collected, shown, and delivered without earning takeover rights. This page controls the
        scarce permission to interrupt; channel content and destinations stay independently configurable.
      </p>
    </div>
    <button class="primary-action save-action" onclick={save} disabled={!draft || $saving}>
      <Save size={17} aria-hidden="true" /> {$saving ? 'Saving policy' : 'Save attention policy'}
    </button>
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
        <section class="policy-section" aria-labelledby="profile-heading">
          <header>
            <div>
              <h2 id="profile-heading">Starting profile</h2>
              <p>Profiles are legible defaults, not locked modes. Any manual change becomes a custom profile.</p>
            </div>
            <label class="field profile-name">
              <span>Profile name</span>
              <input bind:value={draft.profile.name} maxlength="72" autocomplete="off" />
            </label>
          </header>

          <div class="preset-ledger" role="radiogroup" aria-label="Policy profile">
            {#each presets as preset}
              <button
                type="button"
                role="radio"
                aria-checked={draft.profile.preset === preset.id}
                onclick={() => applyPreset(preset.id)}
              >
                <span class="register-box" aria-hidden="true"></span>
                <span>
                  <strong>{preset.label}</strong>
                  <em>{preset.promise}</em>
                </span>
                <small>{preset.detail}</small>
              </button>
            {/each}
          </div>
        </section>

        <section class="policy-section quiet-section" aria-labelledby="quiet-heading">
          <header>
            <div>
              <h2 id="quiet-heading"><MoonStar size={22} strokeWidth={1.5} aria-hidden="true" /> Quiet hours</h2>
              <p>Suppress ordinary notices during sleep without hiding the live console or stale-source warnings.</p>
            </div>
          </header>
          <div class="quiet-grid">
            <SwitchField
              checked={draft.profile.quietHours.enabled}
              label="Quiet hours enabled"
              description="Holds non-emergency desktop and WhatsApp dispatches."
              onchange={(enabled) => {
                draft!.profile.quietHours.enabled = enabled;
                draft!.profile.preset = 'custom';
              }}
            />
            <label class="field">
              <span>Starts</span>
              <input type="time" required bind:value={draft.profile.quietHours.start} />
            </label>
            <label class="field">
              <span>Ends</span>
              <input type="time" required bind:value={draft.profile.quietHours.end} />
            </label>
            <label class="field">
              <span>Time zone</span>
              <select bind:value={draft.profile.quietHours.timeZone}>
                {#each quietTimeZoneOptions as option}
                  <option value={option.id}>{option.label}</option>
                {/each}
              </select>
            </label>
            <SwitchField
              checked={draft.profile.quietHours.bypassEmergency}
              label="Critical bypass"
              description="Extreme official alerts and a bridge confirmed open may still interrupt."
              onchange={(enabled) => {
                draft!.profile.quietHours.bypassEmergency = enabled;
                draft!.profile.preset = 'custom';
              }}
            />
          </div>
        </section>

        <section class="policy-section" aria-labelledby="interrupt-heading">
          <header>
            <div>
              <h2 id="interrupt-heading"><BellRing size={22} strokeWidth={1.5} aria-hidden="true" /> Interrupt register</h2>
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
  }

  .save-action,
  .quiet-section h2,
  #interrupt-heading {
    display: inline-flex;
    align-items: center;
    gap: 9px;
  }

  .policy-register {
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
  }

  .profile-name {
    width: min(280px, 38vw);
  }

  .preset-ledger {
    border-top: 1px solid var(--rule);
  }

  .preset-ledger button {
    display: grid;
    width: 100%;
    min-height: 78px;
    grid-template-columns: 24px minmax(150px, 0.8fr) minmax(260px, 1.35fr);
    align-items: center;
    gap: 18px;
    color: var(--graphite);
    background: transparent;
    border-bottom: 1px solid var(--rule);
    padding: 14px 10px;
    text-align: start;
    cursor: pointer;
  }

  .preset-ledger button:hover,
  .preset-ledger button[aria-checked='true'] {
    background: var(--paper);
  }

  .register-box {
    width: 15px;
    height: 15px;
    border: 1px solid var(--graphite);
  }

  .preset-ledger button[aria-checked='true'] .register-box {
    background: var(--marine);
    box-shadow: inset 0 0 0 3px var(--paper);
  }

  .preset-ledger button > span:nth-child(2) {
    display: grid;
    gap: 3px;
  }

  .preset-ledger strong,
  .interrupt-row strong {
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    line-height: 1;
    text-transform: uppercase;
  }

  .preset-ledger em,
  .preset-ledger small,
  .interrupt-row small,
  .interrupt-row p {
    color: var(--muted);
    font-size: var(--type-caption);
    font-style: normal;
    line-height: 1.45;
  }

  .quiet-grid {
    display: grid;
    grid-template-columns: minmax(220px, 1.3fr) repeat(3, minmax(130px, 0.7fr));
    align-items: end;
    gap: 14px;
  }

  .quiet-grid > :global(:last-child) {
    grid-column: 1 / -1;
  }

  .interrupt-ledger {
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
    }

    .profile-name {
      width: 100%;
    }

    .preset-ledger button,
    .ledger-head,
    .interrupt-row {
      grid-template-columns: 22px 1fr;
    }

    .preset-ledger small,
    .interrupt-row p {
      grid-column: 2;
    }

    .ledger-head {
      display: none;
    }

    .interrupt-row select {
      grid-column: 2;
    }

    .quiet-grid {
      grid-template-columns: 1fr 1fr;
    }

    .quiet-grid > :global(:first-child),
    .quiet-grid > :global(:last-child),
    .quiet-grid .field:nth-child(4) {
      grid-column: 1 / -1;
    }
  }

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
    }

    .quiet-grid {
      grid-template-columns: 1fr;
    }

    .quiet-grid > * {
      grid-column: 1 !important;
    }
  }
</style>
