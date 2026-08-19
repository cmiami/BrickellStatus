<script lang="ts">
  import { Crosshair, ExternalLink, KeyRound, Radar, RefreshCw, Ship, ShieldCheck } from '@lucide/svelte';

  import SwitchField from '$lib/components/SwitchField.svelte';
  import { clearAisstreamApiKey, getAisstreamStatus, getPreferences, setAisstreamApiKey } from '$lib/api';
  import { notice, preferences } from '$lib/state';
  import { formatDistanceKilometers } from '$lib/units';
  import type { AisSettings, AisStreamStatus, UnitSystem } from '$lib/types';

  // Lives on the Brickell Bridge channel now rather than the output desk.
  // AIS is evidence the bridge forecast is built from, not a place frames
  // are delivered to, and it was previously configurable in two places.
  let {
    ais,
    unitSystem,
    onaischange
  }: { ais: AisSettings; unitSystem: UnitSystem; onaischange: (ais: AisSettings) => void } = $props();

  /// Hands every change straight back up. The channels desk saves as it goes,
  /// so there is no local draft to keep in step and nothing to press.
  function update(patch: Partial<AisSettings>) {
    onaischange({ ...ais, ...patch });
  }

  let apiKey = $state('');
  let keyBusy = $state(false);
  let statusBusy = $state(false);
  let removalArmed = $state(false);
  let savedSignature = $state('');
  let status = $state<AisStreamStatus>({
    configured: false,
    enabled: false,
    state: 'disabled',
    detail: 'Checking the local AIS source circuit…'
  });

  const radiusDisplayValue = $derived(
    unitSystem === 'metric'
      ? ais.radiusKilometers.toFixed(0)
      : (ais.radiusKilometers * 0.621_371).toFixed(1)
  );

  $effect(() => {
    const signature = $preferences
      ? `${$preferences.ais.enabled}:${$preferences.ais.apiKeyConfigured}:${$preferences.ais.radiusKilometers}`
      : '';
    if (signature && signature !== savedSignature) {
      savedSignature = signature;
      void refreshHealth(false);
    }
  });

  function stateLabel(state: AisStreamStatus['state']): string {
    if (state === 'missing_key') return 'Needs key';
    if (state === 'ready') return 'Armed';
    if (state === 'live') return 'Live';
    if (state === 'degraded') return 'Needs attention';
    return 'Parked';
  }

  function stateTone(state: AisStreamStatus['state']): string {
    if (state === 'live') return 'ready';
    if (state === 'degraded') return 'degraded';
    return 'unconfigured';
  }

  function positionTime(value?: string): string {
    if (!value) return 'No fresh position received';
    const parsed = new Date(value);
    if (Number.isNaN(parsed.getTime())) return 'Invalid source timestamp';
    return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(parsed);
  }

  function reconcileCredential(configured: boolean) {
    update({ apiKeyConfigured: configured });
    preferences.update((current) =>
      !current || current.ais.apiKeyConfigured === configured
        ? current
        : { ...current, ais: { ...current.ais, apiKeyConfigured: configured } }
    );
  }

  async function refreshHealth(announce = true) {
    statusBusy = true;
    try {
      status = await getAisstreamStatus();
      reconcileCredential(status.configured);
      if (announce) notice.set({ ok: true, message: status.detail });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'AISStream source health is unavailable.';
      status = {
        configured: ais.apiKeyConfigured,
        enabled: Boolean($preferences?.ais.enabled),
        state: 'degraded',
        detail: message
      };
      if (announce) notice.set({ ok: false, message });
    } finally {
      statusBusy = false;
    }
  }

  async function reloadSecretState(forceParked = false) {
    const saved = await getPreferences();
    preferences.update((current) => current ? { ...current, ais: structuredClone(saved.ais) } : saved);
    update(
      forceParked
        ? { apiKeyConfigured: saved.ais.apiKeyConfigured, enabled: saved.ais.enabled }
        : { apiKeyConfigured: saved.ais.apiKeyConfigured }
    );
  }

  async function storeKey() {
    const value = apiKey.trim();
    if (!value) {
      notice.set({ ok: false, message: 'Paste an AISStream API key before storing it.' });
      return;
    }
    keyBusy = true;
    try {
      const result = await setAisstreamApiKey(value);
      notice.set(result);
      await reloadSecretState();
      if (result.ok) apiKey = '';
      await refreshHealth(false);
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The AISStream key could not be stored.' });
    } finally {
      keyBusy = false;
      removalArmed = false;
    }
  }

  async function removeKey() {
    keyBusy = true;
    try {
      const result = await clearAisstreamApiKey();
      notice.set(result);
      await reloadSecretState(true);
      if (result.ok) apiKey = '';
      await refreshHealth(false);
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The AISStream key could not be removed.' });
    } finally {
      keyBusy = false;
      removalArmed = false;
    }
  }
</script>

<section id="aisstream" class="output-band ais-band" aria-labelledby="aisstream-heading">
  <header class="band-heading ais-band-heading">
    <div class="route-mark"><Ship size={26} strokeWidth={1.45} aria-hidden="true" /></div>
    <div>
      <h2 id="aisstream-heading">AISStream vessel watch</h2>
      <p>Real WebSocket positions normalized into time-decaying bridge approach evidence.</p>
    </div>
    <span class="status-word" data-state={stateTone(status.state)}>{stateLabel(status.state)}</span>
  </header>

  <div class="ais-work">
    <div class="ais-config">
      <section class:enabled={ais.enabled} class="ais-circuit-gate" aria-labelledby="ais-circuit-title">
        <div class="circuit-glyph" aria-hidden="true"><Radar size={25} strokeWidth={1.45} /></div>
        <div>
          <span>Optional predictive source</span>
          <h3 id="ais-circuit-title">Vessel evidence circuit</h3>
          <p>The backend worker follows this switch. Bridge status reporting continues independently.</p>
        </div>
        <SwitchField
          checked={ais.enabled}
          label={ais.enabled ? 'AISStream enabled' : 'AISStream disabled'}
          description={ais.enabled ? 'The bridge-centered worker is running.' : 'All AIS network work is parked.'}
          onchange={(enabled) => update({ enabled })}
        />
      </section>

      <section class="coverage-desk" aria-labelledby="coverage-heading">
        <figure class="coverage-instrument">
          <div class="coverage-dial" aria-hidden="true">
            <span class="coverage-ring outer"></span><span class="coverage-ring middle"></span><span class="coverage-ring inner"></span>
            <span class="coverage-axis horizontal"></span><span class="coverage-axis vertical"></span>
            <span class="bridge-fix"><Crosshair size={22} strokeWidth={1.7} /></span>
            <span class="coverage-readout"><strong>{radiusDisplayValue}</strong><small>{unitSystem === 'metric' ? 'KM' : 'MI'}</small></span>
          </div>
          <figcaption><span>Center fix</span><strong>{'the saved bridge pin'}</strong><small>Coverage follows the saved bridge pin.</small></figcaption>
        </figure>

        <div class="coverage-control">
          <div><span>Approach envelope</span><h3 id="coverage-heading">Listening radius</h3><p>A wider radius sees vessels earlier and admits more unrelated traffic.</p></div>
          <label class="radius-control" for="ais-radius">
            <span><b>Coverage radius</b><output for="ais-radius">{formatDistanceKilometers(ais.radiusKilometers, unitSystem)}</output></span>
            <input id="ais-radius" type="range" min="2" max="30" step="1" value={ais.radiusKilometers} oninput={(event) => update({ radiusKilometers: Number((event.currentTarget as HTMLInputElement).value) })} aria-valuetext={`${formatDistanceKilometers(ais.radiusKilometers, unitSystem)} around ${'the saved bridge pin'}`} />
            <small><span>{formatDistanceKilometers(2, unitSystem)} · harbor</span><span>{formatDistanceKilometers(12, unitSystem)} · balanced</span><span>{formatDistanceKilometers(30, unitSystem)} · wide</span></small>
          </label>
          <a href="/channels?channel=bridge.brickell">Move or rename the bridge target →</a>
        </div>
      </section>

      <section class="ais-secret-register" aria-labelledby="ais-key-heading">
        <div class="secret-proof-mark" data-configured={ais.apiKeyConfigured}>
          {#if ais.apiKeyConfigured}<ShieldCheck size={23} strokeWidth={1.45} aria-hidden="true" />{:else}<KeyRound size={23} strokeWidth={1.45} aria-hidden="true" />{/if}
        </div>
        <div class="ais-secret-copy"><span>Local secret</span><h3 id="ais-key-heading">AISStream API key</h3><p>{ais.apiKeyConfigured ? 'A key is stored in the app’s private local credential file.' : 'No key is stored.'}</p></div>
        <label class="ais-key-field" for="aisstream-key"><span class="visually-hidden">AISStream API key</span><input id="aisstream-key" type="password" bind:value={apiKey} maxlength="4096" autocomplete="new-password" placeholder={ais.apiKeyConfigured ? 'Replace saved key' : 'Paste API key'} /></label>
        <div class="secret-actions">
          <button class="secondary-action" onclick={storeKey} disabled={keyBusy || !apiKey.trim()}>{keyBusy ? 'Securing' : ais.apiKeyConfigured ? 'Replace key' : 'Store key'}</button>
          {#if ais.apiKeyConfigured}
            {#if removalArmed}
              <button class="remove-secret-action is-armed" onclick={removeKey} disabled={keyBusy}>Confirm removal</button><button class="cancel-secret-action" onclick={() => (removalArmed = false)} disabled={keyBusy}>Keep it</button>
            {:else}<button class="remove-secret-action" onclick={() => (removalArmed = true)} disabled={keyBusy}>Remove key</button>{/if}
          {/if}
        </div>
      </section>

      <aside class="ais-network-contract"><span>Network disclosure</span><p>When enabled, the backend sends the saved bridge-centered bounding box and API key to AISStream over WSS.</p></aside>
    </div>

    <aside class="ais-health" aria-labelledby="ais-health-heading">
      <div class="health-verdict" data-state={status.state} role="status" aria-live="polite">
        <div class="health-radar"><Radar size={28} strokeWidth={1.35} aria-hidden="true" /></div>
        <span>Saved worker health</span><h3 id="ais-health-heading">{stateLabel(status.state)}</h3><p>{status.detail}</p>
      </div>
      <dl class="ais-facts">
        <div><dt>Source</dt><dd>AISStream</dd></div>
        <div><dt>Last position</dt><dd>{positionTime(status.lastPositionAt)}</dd></div>
        <div><dt>Vessels in range</dt><dd>{status.vesselsInRange == null ? 'Awaiting count' : status.vesselsInRange}</dd></div>
        <div><dt>Predictor role</dt><dd>Directional approach evidence</dd></div>
      </dl>
      <div class="ais-health-actions">
        <button class="secondary-action action-with-icon" onclick={() => refreshHealth()} disabled={statusBusy}><RefreshCw size={16} class={statusBusy ? 'spinning' : undefined} aria-hidden="true" /> {statusBusy ? 'Checking' : 'Check source'}</button>
        <a href="https://aisstream.io/customer.html" target="_blank" rel="noreferrer">Manage key <ExternalLink size={14} aria-hidden="true" /></a>
      </div>
      <div class="ais-boundary-note"><strong>Evidence, never proof.</strong><p>AIS can be delayed or absent. A matching approach raises confidence; it does not prove an opening is required.</p></div>
    </aside>
  </div>
</section>
