<script lang="ts">
  import { page } from '$app/stores';
  import { ChevronDown, ChevronUp, MapPinned, RadioTower, Save } from '@lucide/svelte';

  import ChannelScopeEditor from '$lib/components/ChannelScopeEditor.svelte';
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
    { id: 'home', label: 'Home', detail: 'The display returns here after routine frames.' },
    { id: 'rotation', label: 'Rotation', detail: 'Appears in normal user-ordered rotation.' },
    { id: 'active_only', label: 'When active', detail: 'Appears only while material is current.' },
    { id: 'messages_only', label: 'Messages only', detail: 'Never enters the e-paper rotation.' },
    { id: 'off', label: 'Off', detail: 'Collected only when another enabled module needs it.' }
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
  }

  function replaceSelected(channel: ChannelPreference) {
    if (!draft) return;
    const index = draft.profile.channels.findIndex((item) => item.id === channel.id);
    if (index < 0) return;
    draft.profile.channels[index] = channel;
    draft.profile.preset = 'custom';
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
  }

  async function save() {
    if (!draft) return;
    await persistPreferences($state.snapshot(draft));
  }
</script>

<svelte:head><title>Channels · Tender’s Log</title></svelte:head>

<section class="page-sheet channels-page">
  <header class="page-heading-row">
    <div>
      <p class="registration-label">Signal roster</p>
      <h1 class="sheet-heading">Choose what earns space</h1>
      <p class="sheet-intro">
        Appearance, interruption, and delivery are separate decisions. Disabling a screen does not silently
        disable a safety dependency, and enabling a feed does not grant it permission to message you.
      </p>
    </div>
    <div class="heading-actions">
      <a class="secondary-action" href="/map"><MapPinned size={17} aria-hidden="true" /> Open map</a>
      <button class="primary-action" onclick={save} disabled={!draft || $saving}>
        <Save size={17} aria-hidden="true" /> {$saving ? 'Saving policy' : 'Save channel policy'}
      </button>
    </div>
  </header>

  {#if draft && selected}
    <div class="channel-workbench">
      <aside class="channel-index" aria-label="Channel roster">
        <div class="index-heading">
          <span>Channel</span><span>Collection</span>
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
            <p class="registration-label">Collection</p>
            <h3>What runs</h3>
            <p>Collection stays explicit. A disabled channel may still expose an offline reason; it never carries a stale value forward as current.</p>
          </div>
          <div class:enabled={selected.enabled} class="collection-gate">
            <div class="gate-state">
              <span>Collector circuit</span>
              <strong>{selected.enabled ? 'Running' : 'Parked'}</strong>
              <small>
                {selected.enabled
                  ? 'Fresh data may enter policy evaluation.'
                  : 'No polling, evaluation, rotation, or dispatch for this channel.'}
              </small>
            </div>
            <SwitchField
              checked={selected.enabled}
              label={selected.enabled ? 'Channel enabled' : 'Channel disabled'}
              description={selected.enabled
                ? 'Its configured collectors are allowed to run.'
                : 'Settings remain editable without starting the source.'}
              onchange={(enabled) => {
                selected.enabled = enabled;
                draft!.profile.preset = 'custom';
              }}
            />
          </div>
          <div class="two-fields">
            <label class="field">
              <span>Maximum accepted age</span>
              <input type="number" min="1" max="1440" bind:value={selected.maxAgeMinutes} />
              <small class="field-note">Minutes before the channel becomes stale.</small>
            </label>
            <label class="field">
              <span>Maximum rotation items</span>
              <input type="number" min="1" max="10" bind:value={selected.maxItems} />
              <small class="field-note">A cap prevents news or markets from flooding the display.</small>
            </label>
          </div>
        </section>

        <section class="editor-section">
          <div class="section-copy">
            <p class="registration-label">Content scope</p>
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
          />
        </section>

        <section class="editor-section">
          <div class="section-copy">
            <p class="registration-label">Presence</p>
            <h3>Where it appears</h3>
            <p>Presence controls the console and e-paper rotation. It does not grant interrupt or message permission.</p>
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
            <span>Normal dwell</span>
            <input type="number" min="10" max="120" bind:value={selected.rotationSeconds} />
            <small class="field-note">Seconds. Interrupt holds are controlled by event policy.</small>
          </label>
        </section>

        <section class="editor-section">
          <div class="section-copy">
            <p class="registration-label">Interrupts</p>
            <h3>What may take over</h3>
            <p>Built-in presets map to explicit event rules. Custom rules stay parked until a detailed matrix is configured.</p>
          </div>
          <label class="field compact-field">
            <span>Interrupt policy</span>
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
            <h3>Where notices go</h3>
            <p>Destinations receive only events allowed by this channel’s policy and the destination’s quiet hours.</p>
          </div>
          <div class="destination-register">
            {#each destinations as destination}
              <SwitchField
                checked={selected.destinations.includes(destination.id)}
                label={destination.label}
                description={destination.id === 'whatsapp'
                  ? 'Template-based, material-change-only messages.'
                  : destination.id === 'epaper'
                    ? 'Rotation or takeover according to this channel’s presence.'
                  : 'Best-effort local notification; the OS does not return a displayed or read receipt.'}
                onchange={(enabled) => toggleDestination(destination.id, enabled)}
              />
            {/each}
          </div>
        </section>
      </div>

      {#if $snapshot}
        <aside class="live-preview">
          <p class="registration-label">Selected channel frame</p>
          <EpaperPreview
            decision={$snapshot.decision}
            channel={liveSelected}
            evidence={$snapshot.evidence.filter((strip) => strip.channelId === selected.id)}
          />
          <div class="policy-sentence">
            <RadioTower size={19} aria-hidden="true" />
            <p>
              <strong>{selected.title}</strong> is {selected.presence.replace('_', ' ')}. It
              {selected.interruptPreset === 'off' ? ' never interrupts' : ` uses ${selected.interruptPreset.replace('_', ' ')} interrupts`}.
              {selected.destinations.length
                ? ` Notices may route to ${selected.destinations.join(', ')}.`
                : ' No outbound notices are permitted.'}
            </p>
          </div>
        </aside>
      {/if}
    </div>
  {:else}
    <div class="empty-sheet"><h2>Loading channel policy</h2><p>The saved configuration has not arrived yet.</p></div>
  {/if}
</section>

<style>
  .channels-page {
    padding-inline: clamp(18px, 2.5vw, 38px);
  }

  .heading-actions,
  .heading-actions .primary-action,
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
  }
</style>
