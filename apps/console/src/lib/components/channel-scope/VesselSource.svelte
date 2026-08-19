<script lang="ts">
  import { KeyRound, ShieldCheck } from '@lucide/svelte';

  import { clearAisstreamApiKey, getAisstreamStatus, getPreferences, setAisstreamApiKey } from '$lib/api';
  import { notice, preferences } from '$lib/state';
  import type { AisSettings, AisStreamStatus } from '$lib/types';

  // One question: is there a key. Everything else the app can work out for
  // itself, and every extra control was a way to end up with a stored key and
  // nothing running. Coverage follows the charted corridor, so a radius the
  // reader picked never described the water actually being watched.
  let { ais, onaischange }: { ais: AisSettings; onaischange: (ais: AisSettings) => void } = $props();

  let apiKey = $state('');
  let busy = $state(false);
  let removalArmed = $state(false);
  let status = $state<AisStreamStatus>({
    configured: false,
    enabled: false,
    state: 'disabled',
    detail: 'Checking…'
  });

  // Green, amber, red. Connecting is deliberately not red: a socket that opened
  // a minute ago has not failed, and a river with no vessel on it is the normal
  // case rather than a fault.
  const tone = $derived(
    status.state === 'live'
      ? 'working'
      : status.state === 'ready'
        ? 'starting'
        : status.state === 'missing_key' || status.state === 'disabled'
          ? 'idle'
          : 'broken'
  );
  const word = $derived(
    { working: 'Working', starting: 'Connecting', idle: 'No key', broken: 'Not working' }[tone]
  );

  $effect(() => {
    if (ais.apiKeyConfigured || status.configured) void refresh();
  });

  async function refresh() {
    try {
      status = await getAisstreamStatus();
    } catch {
      status = { configured: ais.apiKeyConfigured, enabled: ais.enabled, state: 'degraded', detail: 'Source health is unavailable.' };
    }
  }

  async function reconcile() {
    const saved = await getPreferences();
    preferences.update((current) => (current ? { ...current, ais: structuredClone(saved.ais) } : saved));
    onaischange(structuredClone(saved.ais));
    await refresh();
  }

  async function storeKey() {
    const value = apiKey.trim();
    if (!value) return;
    busy = true;
    try {
      const result = await setAisstreamApiKey(value);
      notice.set(result);
      if (result.ok) apiKey = '';
      await reconcile();
    } finally {
      busy = false;
      removalArmed = false;
    }
  }

  async function removeKey() {
    busy = true;
    try {
      notice.set(await clearAisstreamApiKey());
      apiKey = '';
      await reconcile();
    } finally {
      busy = false;
      removalArmed = false;
    }
  }
</script>

<section class="vessel-source" aria-labelledby="vessel-source-heading">
  <header>
    <div class="mark" data-configured={ais.apiKeyConfigured} aria-hidden="true">
      {#if ais.apiKeyConfigured}<ShieldCheck size={20} strokeWidth={1.5} />{:else}<KeyRound size={20} strokeWidth={1.5} />{/if}
    </div>
    <div>
      <h4 id="vessel-source-heading">Vessel source</h4>
      <p>Approaching vessels, used as early evidence that the span is about to lift.</p>
    </div>
    <!-- The word carries the state, not the dot: a colour alone is unreadable
         to a good share of people and prints as grey. -->
    <span class="health" data-tone={tone}><i aria-hidden="true"></i>{word}</span>
  </header>

  <div class="key-row">
    <label>
      <span class="visually-hidden">AISStream API key</span>
      <input
        type="password"
        bind:value={apiKey}
        maxlength="4096"
        autocomplete="new-password"
        placeholder={ais.apiKeyConfigured ? 'Replace saved key' : 'Paste AISStream API key'}
      />
    </label>
    <button class="secondary-action" type="button" onclick={storeKey} disabled={busy || !apiKey.trim()}>
      {busy ? 'Saving' : ais.apiKeyConfigured ? 'Replace' : 'Save key'}
    </button>
    {#if ais.apiKeyConfigured}
      {#if removalArmed}
        <button class="secondary-action danger" type="button" onclick={removeKey} disabled={busy}>Confirm remove</button>
      {:else}
        <button class="secondary-action" type="button" onclick={() => (removalArmed = true)} disabled={busy}>Remove</button>
      {/if}
    {/if}
  </div>

  <p class="detail">{ais.apiKeyConfigured ? status.detail : 'A key starts the source. There is nothing else to set.'}</p>
</section>

<style>
  .vessel-source {
    display: grid;
    gap: 14px;
    padding: 16px;
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .vessel-source > header {
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
  }

  .mark {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    color: var(--muted);
    background: var(--frost);
    border: 1px solid var(--rule);
  }

  .mark[data-configured='true'] {
    color: var(--white);
    background: var(--marine);
    border-color: var(--marine);
  }

  .vessel-source h4 {
    margin: 0;
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    text-transform: uppercase;
  }

  .vessel-source header p,
  .detail {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .health {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .health i {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: currentColor;
  }

  .health[data-tone='working'] { color: var(--success); }
  .health[data-tone='starting'] { color: var(--amber-ink); }
  .health[data-tone='idle'] { color: var(--muted); }
  .health[data-tone='broken'] { color: var(--danger); }

  .key-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .key-row label {
    flex: 1 1 240px;
  }

  .key-row input {
    width: 100%;
    min-height: 42px;
    padding: 9px 12px;
    color: var(--graphite);
    background: var(--frost);
    border: 1px solid var(--steel);
    border-radius: 2px;
    font: inherit;
  }

  .danger {
    color: var(--white);
    background: var(--danger);
    border-color: var(--danger);
  }
</style>
