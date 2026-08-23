<script lang="ts">
  import { page } from '$app/stores';
  import { Check, ChevronDown, ChevronUp, MapPinned, RadioTower, Save } from '@lucide/svelte';
  import { onMount } from 'svelte';

  import ChannelScopeEditor from '$lib/components/ChannelScopeEditor.svelte';
  import ChannelTabs from '$lib/components/ChannelTabs.svelte';
  import EpaperPreview from '$lib/components/EpaperPreview.svelte';
  import SwitchField from '$lib/components/SwitchField.svelte';
  import { persistPreferences, preferences, saving, snapshot } from '$lib/state';
  import type { AppPreferences, ChannelPreference, DestinationId, SurfacePresence } from '$lib/types';

  let draft = $state<AppPreferences | null>(null);
  let selectedId = $state('');
  let initializedFromStore = $state(false);

  $effect(() => {
    if ($preferences && !initializedFromStore) {
      draft = structuredClone($preferences);
      selectedId = $page.url.searchParams.get('channel') ?? draft.profile.homeChannelId;
      initializedFromStore = true;
    }
  });

  let selected = $derived(draft?.profile.channels.find((channel) => channel.id === selectedId));
  let liveSelected = $derived($snapshot?.channels.find((channel) => channel.id === selectedId));

  const presenceOptions: { id: SurfacePresence; label: string; detail: string }[] = [
    { id: 'home', label: 'Main screen', detail: 'The panel keeps coming back to this one.' },
    { id: 'rotation', label: 'Always in the loop', detail: 'Takes its turn on the panel whether or not anything is happening.' },
    { id: 'active_only', label: 'Only when something happens', detail: 'Stays off the panel until it has something to say.' },
    { id: 'messages_only', label: 'Messages only', detail: 'Never on the panel. Can still send you a message.' },
    { id: 'off', label: 'Hidden', detail: 'Never on the panel and never messages you.' }
  ];

  const destinations: { id: DestinationId; label: string }[] = [
    { id: 'epaper', label: 'E-paper' },
    { id: 'whatsapp', label: 'WhatsApp' },
    { id: 'desktop', label: 'Desktop notice' }
  ];

  function toggleDestination(destination: DestinationId, enabled: boolean) {
    if (!selected) return;
    selected.destinations = enabled
      ? [...new Set([...selected.destinations, destination])]
      : selected.destinations.filter((item) => item !== destination);
    void saveNow();
  }

  function replaceSelected(channel: ChannelPreference, commit = false) {
    if (!draft) return;
    const index = draft.profile.channels.findIndex((item) => item.id === channel.id);
    if (index < 0) return;
    draft.profile.channels[index] = channel;
    draft.profile.preset = 'custom';
    if (commit) void saveNow();
    else scheduleSave();
  }

  function moveSelected(direction: -1 | 1) {
    if (!draft || !selected) return;
    const index = draft.profile.channels.findIndex((channel) => channel.id === selected.id);
    const nextIndex = index + direction;
    if (nextIndex < 0 || nextIndex >= draft.profile.channels.length) return;
    [draft.profile.channels[index], draft.profile.channels[nextIndex]] = [
      draft.profile.channels[nextIndex],
      draft.profile.channels[index]
    ];
    draft.profile.preset = 'custom';
    scheduleSave();
  }

  async function save() {
    if (!draft) return;
    await persistPreferences($state.snapshot(draft));
  }

  // A switch is a decision, not a draft: it commits the moment it is thrown.
  async function saveNow() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = undefined;
    await save();
  }

  // Typed fields commit too, just not per keystroke. Long enough that a name
  // being typed is one write rather than twenty; short enough that nobody can
  // leave the page believing an edit was applied when it was not.
  const SETTLE_MS = 700;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = undefined;
      void save();
    }, SETTLE_MS);
  }

  // A pending edit must not die with the page. Leaving flushes it.
  onMount(() => () => {
    if (saveTimer) {
      clearTimeout(saveTimer);
      void save();
    }
  });

  const unsaved = $derived(
    !!draft && !!$preferences && JSON.stringify($state.snapshot(draft)) !== JSON.stringify($preferences)
  );
</script>

<svelte:head><title>Channels · BrickellStatus</title></svelte:head>

<section class="page-sheet channels-page">
  <ChannelTabs />
  <header class="page-heading-row">
    <div>
      <p class="registration-label">Channels</p>
      <h1 class="sheet-heading">Choose what to track</h1>
      <p class="sheet-intro">
        Turn data sources on or off, choose what appears on the e-paper display, and decide which changes can send
        an alert.
      </p>
    </div>
    <div class="heading-actions">
      <a class="secondary-action" href="/map"><MapPinned size={17} aria-hidden="true" /> Open map</a>
      <!-- No Save button: every change on this page applies itself. The status
           exists so "applied" is something the reader can see, not assume. -->
    </div>
  </header>

  {#if draft && selected}
    <div class="channel-workbench">
      <aside class="channel-index" aria-label="Channels">
        <div class="index-heading">
          <span>Channel</span><span>Status</span>
        </div>
        {#each draft.profile.channels as channel (channel.id)}
          <button
            class:selected={channel.id === selectedId}
            class:disabled={!channel.enabled}
            onclick={() => (selectedId = channel.id)}
          >
            <span>
              <strong>{channel.title}</strong>
              <small>{channel.id}</small>
            </span>
            <em data-active={channel.enabled}>{channel.enabled ? 'Running' : 'Off'}</em>
          </button>
        {/each}
      </aside>

      <div class="channel-editor">
        <header class="editor-header">
          <div>
            <span class="registration-label">Selected channel</span>
            <h2>{selected.title}</h2>
            <p>{liveSelected?.summary ?? 'No live snapshot is available for this channel.'}</p>
          </div>
          <div class="order-buttons" aria-label="Rotation order">
            <button class="secondary-action" onclick={() => moveSelected(-1)} aria-label="Move channel earlier">
              <ChevronUp size={16} aria-hidden="true" /> Earlier
            </button>
            <button class="secondary-action" onclick={() => moveSelected(1)} aria-label="Move channel later">
              <ChevronDown size={16} aria-hidden="true" /> Later
            </button>
          </div>
        </header>

        <section class="editor-section">
          <div class="section-copy">
            <p class="registration-label">Updates</p>
            <h3>Data collection</h3>
            <p>Turn this channel on to fetch new data. Turning it off keeps its settings but stops updates and alerts.</p>
          </div>
          <div class:enabled={selected.enabled} class="collection-gate">
            <div class="gate-state">
              <span>Channel status</span>
              <strong>{selected.enabled ? 'On' : 'Off'}</strong>
              <small>
                {selected.enabled
                  ? 'The app checks this source for new information.'
                  : 'The app does not check, display, or send alerts for this channel.'}
              </small>
            </div>
            <SwitchField
              checked={selected.enabled}
              label={selected.enabled ? 'Channel on' : 'Channel off'}
              description={selected.enabled
                ? 'New data can update this channel.'
                : 'You can still change its settings.'}
              onchange={(enabled) => {
                selected.enabled = enabled;
                draft!.profile.preset = 'custom';
                void saveNow();
              }}
            />
          </div>
          <div class="two-fields">
            <label class="field">
              <span>Mark data stale after</span>
              <input type="number" min="1" max="1440" bind:value={selected.maxAgeMinutes} />
              <small class="field-note">Minutes without a new update.</small>
            </label>
            <label class="field">
              <span>Items in rotation</span>
              <input type="number" min="1" max="10" bind:value={selected.maxItems} />
              <small class="field-note">Limits how many items from this channel appear on the display.</small>
            </label>
          </div>
        </section>

        <section class="editor-section">
          <div class="section-copy">
            <p class="registration-label">Settings</p>
            <h3>What it watches</h3>
            <p>Locations, source URLs, and thresholds are saved with this channel.</p>
          </div>
            <ChannelScopeEditor
              channel={selected}
              areas={draft.areas}
              ais={draft.ais}
              unitSystem={draft.unitSystem}
            onchannelchange={replaceSelected}
            onaischange={(ais) => {
              draft!.ais = ais;
              draft!.profile.preset = 'custom';
            }}
            onareaadd={(area) => {
              // Areas are shared between channels, so a pin dropped here joins
              // the same register the map page keeps rather than becoming a
              // private copy only this channel can see.
              draft!.areas = [...draft!.areas, area];
              draft!.profile.preset = 'custom';
              void saveNow();
            }}
          />
        </section>

        <section class="editor-section">
          <div class="section-copy">
            <h3>Where it appears</h3>
            <p>
              Choose whether this channel appears on the e-paper panel and how long it stays on screen. Alert settings
              are separate below.
            </p>
          </div>
          <div class="choice-register" role="radiogroup" aria-label="Display presence">
            {#each presenceOptions as option}
              <button
                type="button"
                role="radio"
                aria-checked={selected.presence === option.id}
                onclick={() => {
                  selected!.presence = option.id;
                  if (option.id === 'home') draft!.profile.homeChannelId = selected!.id;
                  draft!.profile.preset = 'custom';
                }}
              >
                <strong>{option.label}</strong><span>{option.detail}</span>
              </button>
            {/each}
          </div>
          <label class="field compact-field">
            <span>Time on screen</span>
            <input type="number" min="10" max="120" bind:value={selected.rotationSeconds} />
            <small class="field-note">Seconds before the display rotates to the next item.</small>
          </label>
        </section>

        <section class="editor-section">
          <div class="section-copy">
            <p class="registration-label">Interrupts</p>
            <h3>When it can alert you</h3>
            <p>Choose which changes can send a notification or take over the e-paper home screen.</p>
          </div>
          <label class="field compact-field">
            <span>Alert setting</span>
            <select bind:value={selected.interruptPreset}>
              <option value="recommended">Recommended</option>
              <option value="confirmed_only">Confirmed only</option>
              <option value="meaningful">All meaningful changes</option>
              <option value="off">Off</option>
              <option value="custom" disabled>Custom matrix · not configured</option>
            </select>
          </label>
        </section>

        <section class="editor-section">
          <div class="section-copy">
            <p class="registration-label">Delivery</p>
            <h3>Where alerts go</h3>
            <p>Alerts follow this channel’s setting and each destination’s quiet hours.</p>
          </div>
          <div class="destination-register">
            {#each destinations as destination}
              <SwitchField
                checked={selected.destinations.includes(destination.id)}
                label={destination.label}
                description={destination.id === 'whatsapp'
                  ? 'Template-based, material-change-only messages.'
                  : destination.id === 'epaper'
                    ? 'Shown on the panel according to the choice above.'
                  : 'Best-effort local notification; the OS does not return a displayed or read receipt.'}
                onchange={(enabled) => toggleDestination(destination.id, enabled)}
              />
            {/each}
          </div>
        </section>
      </div>

      {#if $snapshot}
        <aside class="live-preview">
          <p class="registration-label">Panel preview</p>
          <EpaperPreview
            decision={$snapshot.decision}
            channel={liveSelected}
            evidence={$snapshot.evidence.filter((strip) => strip.channelId === selected.id)}
          />
          <div class="policy-sentence">
            <RadioTower size={19} aria-hidden="true" />
            <p>
              <strong>{selected.title}</strong> —
              {presenceOptions.find((option) => option.id === selected!.presence)?.label.toLowerCase() ?? selected.presence}.
              {selected.interruptPreset === 'off' ? ' Alerts are off.' : ` Alert setting: ${selected.interruptPreset.replace('_', ' ')}.`}
              {selected.destinations.length
                ? ` Alerts can go to ${selected.destinations.join(', ')}.`
                : ' No alert destinations are selected.'}
            </p>
          </div>
        </aside>
      {/if}
    </div>
  {:else}
    <div class="empty-sheet"><h2>Loading channels</h2><p>Waiting for saved settings.</p></div>
  {/if}
</section>

<style>
  .channels-page {
    padding-inline: clamp(18px, 2.5vw, 38px);
  }

  .heading-actions,
  .heading-actions .secondary-action {
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }

  .heading-actions .secondary-action {
    text-decoration: none;
  }

  .channel-workbench {
    display: grid;
    grid-template-columns: minmax(210px, 0.9fr) minmax(470px, 2.35fr) minmax(300px, 1.2fr);
    align-items: start;
    border-top: 1px solid var(--rule-strong);
    border-bottom: 1px solid var(--rule-strong);
  }

  .channel-index {
    position: sticky;
    top: 92px;
    max-height: calc(100vh - 118px);
    overflow-y: auto;
    border-right: 1px solid var(--rule-strong);
  }

  .index-heading {
    display: flex;
    justify-content: space-between;
    padding: 12px 14px;
    color: var(--muted);
    background: var(--frost);
    border-bottom: 1px solid var(--rule);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .channel-index button {
    position: relative;
    display: flex;
    width: 100%;
    min-height: 68px;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    color: var(--graphite);
    background: var(--paper);
    border-bottom: 1px solid var(--rule);
    padding: 12px 14px;
    text-align: left;
    cursor: pointer;
  }

  .channel-index button::before {
    position: absolute;
    inset: 10px auto 10px 0;
    width: 1px;
    background: transparent;
    content: '';
  }

  .channel-index button:hover,
  .channel-index button.selected {
    background: var(--frost);
  }

  .channel-index button.selected::before {
    background: var(--amber);
  }

  .channel-index button.disabled {
    color: var(--muted);
  }

  .channel-index button > span {
    display: grid;
    min-width: 0;
    gap: 4px;
  }

  .channel-index strong {
    overflow: hidden;
    font-family: var(--font-instrument);
    font-size: var(--type-body);
    font-weight: 600;
    line-height: 1;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .channel-index small,
  .channel-index em {
    color: var(--muted);
    font-size: var(--type-micro);
    font-style: normal;
    line-height: 1.2;
  }

  .channel-index em {
    flex: 0 0 auto;
    font-family: var(--font-instrument);
    font-weight: 600;
    letter-spacing: 0.045em;
    text-transform: uppercase;
  }

  .channel-index em::before {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 6px;
    background: var(--steel);
    border-radius: 50%;
    content: '';
  }

  .channel-index em[data-active='true']::before {
    background: var(--success);
  }

  .channel-editor {
    min-width: 0;
    background: var(--frost);
    border-right: 1px solid var(--rule-strong);
  }

  .editor-header {
    display: flex;
    min-height: 132px;
    align-items: flex-end;
    justify-content: space-between;
    gap: 24px;
    padding: 24px clamp(22px, 3vw, 38px);
    border-bottom: 1px solid var(--rule-strong);
  }

  .editor-header > div:first-child {
    display: grid;
    gap: 8px;
  }

  .editor-header h2 {
    margin: 0;
    color: var(--marine);
    font-size: var(--type-headline);
    font-weight: 700;
    line-height: 0.82;
    letter-spacing: -0.02em;
    text-transform: uppercase;
  }

  .editor-header p {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
  }

  .order-buttons {
    display: flex;
    gap: 7px;
  }

  .order-buttons button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 9px 10px;
  }

  .editor-section {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 22px;
    padding: 32px clamp(22px, 3vw, 38px);
    border-bottom: 1px solid var(--rule);
  }

  .editor-section > :not(.section-copy) {
    grid-column: 1;
    min-width: 0;
  }

  .section-copy {
    display: grid;
    align-content: start;
    gap: 8px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--rule);
  }

  .section-copy h3 {
    margin: 0;
    font-size: var(--type-section);
    font-weight: 700;
    line-height: 0.95;
    text-transform: uppercase;
  }

  .section-copy p:last-child {
    max-width: 72ch;
    margin: 2px 0 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.5;
  }

  .two-fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }

  .collection-gate {
    display: grid;
    grid-template-columns: minmax(175px, 0.75fr) minmax(220px, 1.25fr);
    color: var(--graphite);
    background: var(--paper);
    border-top: 5px solid var(--steel);
  }

  .collection-gate.enabled {
    border-top-color: var(--success);
  }

  .collection-gate > :global(*) {
    padding: 17px;
  }

  .gate-state {
    display: grid;
    align-content: start;
    gap: 4px;
    border-right: 1px solid var(--rule);
  }

  .gate-state > span {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .gate-state strong {
    color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    line-height: 1;
    text-transform: uppercase;
  }

  .gate-state small {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .compact-field {
    max-width: 320px;
  }

  .choice-register {
    display: grid;
    border-top: 1px solid var(--rule);
  }

  .choice-register button {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 16px;
    color: var(--graphite);
    background: transparent;
    border-bottom: 1px solid var(--rule);
    padding: 12px 9px;
    text-align: left;
    cursor: pointer;
  }

  .choice-register button:hover,
  .choice-register button[aria-checked='true'] {
    background: var(--paper);
  }

  .choice-register button[aria-checked='true'] strong::after {
    margin-left: 8px;
    color: var(--channel);
    content: '●';
  }

  .choice-register strong {
    font-family: var(--font-instrument);
    text-transform: uppercase;
  }

  .choice-register span {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .destination-register {
    display: grid;
    grid-template-columns: 1fr 1fr;
    align-content: start;
    gap: 6px;
  }

  .live-preview {
    position: sticky;
    top: 92px;
    display: grid;
    gap: 14px;
    padding: 24px clamp(18px, 2vw, 28px);
  }

  .policy-sentence {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 10px;
    margin-top: 8px;
    padding-top: 16px;
    border-top: 1px solid var(--rule-strong);
  }

  .policy-sentence :global(svg) {
    color: var(--marine);
  }

  .policy-sentence p {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.5;
  }

  .policy-sentence strong {
    color: var(--graphite);
  }

  @media (max-width: 1240px) {
    .channel-workbench {
      grid-template-columns: minmax(190px, 0.8fr) minmax(450px, 2fr);
    }

    .live-preview {
      position: static;
      grid-column: 1 / -1;
      grid-template-columns: minmax(340px, 1fr) minmax(260px, 0.8fr);
      align-items: start;
      border-top: 1px solid var(--rule-strong);
    }

    .live-preview > .registration-label {
      grid-column: 1 / -1;
    }
  }

  @media (max-width: 820px) {
    .channel-workbench {
      grid-template-columns: 1fr;
    }

    .channel-index {
      position: static;
      display: flex;
      max-height: none;
      overflow-x: auto;
      border-right: 0;
      border-bottom: 1px solid var(--rule-strong);
    }

    .index-heading {
      display: none;
    }

    .channel-index button {
      min-width: 172px;
      border-right: 1px solid var(--rule);
      border-bottom: 0;
    }

    .channel-editor {
      border-right: 0;
    }

    .live-preview {
      grid-template-columns: 1fr;
    }
  }

  @media (max-width: 620px) {
    .heading-actions {
      width: 100%;
      flex-direction: column;
    }

    .heading-actions > * {
      justify-content: center;
      width: 100%;
    }

    .editor-header {
      align-items: flex-start;
      flex-direction: column;
    }

    .editor-section {
      grid-template-columns: 1fr;
      gap: 20px;
      padding: 26px 18px;
    }

    .collection-gate {
      grid-template-columns: 1fr;
    }

    .gate-state {
      border-right: 0;
      border-bottom: 1px solid var(--rule);
    }

    .editor-section > :not(.section-copy) {
      grid-column: 1;
    }

    .two-fields,
    .destination-register {
      grid-template-columns: 1fr;
    }

    .choice-register button {
      grid-template-columns: 90px 1fr;
    }

    .live-preview {
      padding: 24px 16px;
    }
  }</style>
