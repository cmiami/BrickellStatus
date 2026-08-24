<script lang="ts">
  import { KeyRound, MessageCircle, Send } from '@lucide/svelte';

  import SwitchField from '$lib/components/SwitchField.svelte';
  import { clearWhatsAppToken, getPreferences, setWhatsAppToken, testWhatsApp } from '$lib/api';
  import { notice, preferences, snapshot } from '$lib/state';
  import type { AppPreferences } from '$lib/types';

  let { draft = $bindable() }: { draft: AppPreferences } = $props();

  let token = $state('');
  let tokenBusy = $state(false);
  let removalArmed = $state(false);
  let testBusy = $state(false);

  const output = $derived($snapshot?.outputs.find((item) => item.id === 'whatsapp'));
  const settingsDirty = $derived(
    Boolean($preferences && JSON.stringify(draft.whatsapp) !== JSON.stringify($preferences.whatsapp))
  );
  const consentCurrent = $derived(
    draft.whatsapp.consent === 'opted_in' &&
      draft.whatsapp.consentRecipient === draft.whatsapp.recipient.trim() &&
      typeof draft.whatsapp.consentRecordedAtMillis === 'number' &&
      Number.isFinite(draft.whatsapp.consentRecordedAtMillis) &&
      draft.whatsapp.consentRecordedAtMillis > 0
  );

  async function reloadSecretState(forceParked = false) {
    const saved = await getPreferences();
    preferences.update((current) => current ? { ...current, whatsapp: structuredClone(saved.whatsapp) } : saved);
    draft.whatsapp.tokenConfigured = saved.whatsapp.tokenConfigured;
    if (forceParked) draft.whatsapp.enabled = saved.whatsapp.enabled;
  }

  async function storeToken() {
    const value = token.trim();
    if (!value) {
      notice.set({ ok: false, message: 'Paste a Meta access token before storing it.' });
      return;
    }
    tokenBusy = true;
    try {
      const result = await setWhatsAppToken(value);
      notice.set(result);
      await reloadSecretState();
      if (result.ok) token = '';
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The Meta token could not be stored.' });
    } finally {
      tokenBusy = false;
      removalArmed = false;
    }
  }

  async function removeToken() {
    tokenBusy = true;
    try {
      const result = await clearWhatsAppToken();
      notice.set(result);
      await reloadSecretState(true);
      if (result.ok) token = '';
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The Meta token could not be removed.' });
    } finally {
      tokenBusy = false;
      removalArmed = false;
    }
  }

  async function sendTest() {
    testBusy = true;
    try {
      notice.set(await testWhatsApp());
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The WhatsApp test failed.' });
    } finally {
      testBusy = false;
    }
  }

  function changeRecipient(recipient: string) {
    if (recipient === draft.whatsapp.recipient) return;
    draft.whatsapp.recipient = recipient;
    draft.whatsapp.consent = 'not_recorded';
    draft.whatsapp.consentRecipient = null;
    draft.whatsapp.consentRecordedAtMillis = null;
  }

  function recordConsent(consent: AppPreferences['whatsapp']['consent']) {
    if (consent === 'not_recorded') {
      draft.whatsapp.consent = consent;
      draft.whatsapp.consentRecipient = null;
      draft.whatsapp.consentRecordedAtMillis = null;
      return;
    }
    const recipient = draft.whatsapp.recipient.trim();
    if (consent === 'opted_in' && !recipient) {
      notice.set({ ok: false, message: 'Enter the recipient before recording opt-in consent.' });
      return;
    }
    draft.whatsapp.recipient = recipient;
    draft.whatsapp.consent = consent;
    draft.whatsapp.consentRecipient = recipient || null;
    draft.whatsapp.consentRecordedAtMillis = recipient ? Date.now() : null;
  }

  function consentTime(value: number | null | undefined): string {
    if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) return 'Unknown time';
    return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value));
  }
</script>

<section class="output-band whatsapp-band" aria-labelledby="whatsapp-heading">
  <header class="band-heading">
    <div class="route-mark"><MessageCircle size={26} strokeWidth={1.45} aria-hidden="true" /></div>
    <div><h2 id="whatsapp-heading">WhatsApp Cloud API</h2><p>Optional Meta-hosted delivery to one explicitly consented recipient.</p></div>
    <span class="status-word" data-state={output?.state ?? 'unconfigured'}>{output?.state ?? 'unconfigured'}</span>
  </header>

  <div class="whatsapp-work">
    <div class="whatsapp-config">
      <SwitchField checked={draft.whatsapp.enabled} label="WhatsApp delivery enabled" description="Important changes from every enabled channel use this route." onchange={(enabled) => (draft.whatsapp.enabled = enabled)} />

      <div class="two-fields">
        <label class="field"><span>Phone number ID</span><input bind:value={draft.whatsapp.phoneNumberId} maxlength="64" inputmode="numeric" autocomplete="off" /></label>
        <label class="field"><span>Recipient</span><input value={draft.whatsapp.recipient} oninput={(event) => changeRecipient(event.currentTarget.value)} maxlength="24" inputmode="tel" autocomplete="tel" placeholder="+15551234567" /><small class="field-note">E.164 with leading +.</small></label>
      </div>

      <div class="three-fields">
        <label class="field"><span>Graph API version</span><input bind:value={draft.whatsapp.graphVersion} maxlength="16" pattern="v[0-9]+\.[0-9]+" /></label>
        <label class="field"><span>Approved template</span><input bind:value={draft.whatsapp.templateName} maxlength="512" /></label>
        <label class="field"><span>Language</span><input bind:value={draft.whatsapp.languageCode} maxlength="16" /></label>
      </div>

      <div class="secret-register">
        <KeyRound size={20} strokeWidth={1.5} aria-hidden="true" />
        <div><strong>Access token</strong><span>{draft.whatsapp.tokenConfigured ? 'Stored in the app’s private local credential file.' : 'No token is configured.'}</span></div>
        <label class="visually-hidden" for="whatsapp-token">Meta access token</label>
        <input id="whatsapp-token" type="password" bind:value={token} maxlength="4096" autocomplete="new-password" placeholder={draft.whatsapp.tokenConfigured ? 'Replace stored token' : 'Paste token'} />
        <div class="secret-actions">
          <button class="secondary-action" onclick={storeToken} disabled={tokenBusy || !token.trim()}>{tokenBusy ? 'Securing' : 'Store secret'}</button>
          {#if draft.whatsapp.tokenConfigured}
            {#if removalArmed}
              <button class="remove-secret-action is-armed" onclick={removeToken} disabled={tokenBusy}>Confirm removal</button><button class="cancel-secret-action" onclick={() => (removalArmed = false)} disabled={tokenBusy}>Keep it</button>
            {:else}<button class="remove-secret-action" onclick={() => (removalArmed = true)} disabled={tokenBusy}>Remove secret</button>{/if}
          {/if}
        </div>
      </div>

      <section class="consent-note" aria-labelledby="consent-heading">
        <h3 id="consent-heading">Recipient consent</h3>
        <p>Proactive delivery remains blocked until consent is recorded for the exact saved recipient.</p>
        <div class="consent-register" role="radiogroup" aria-label="WhatsApp recipient consent">
          <button type="button" role="radio" aria-checked={draft.whatsapp.consent === 'not_recorded'} onclick={() => recordConsent('not_recorded')}><span>Not recorded</span><small>Suppress proactive sends</small></button>
          <button type="button" role="radio" aria-checked={draft.whatsapp.consent === 'opted_in'} disabled={!draft.whatsapp.recipient.trim()} onclick={() => recordConsent('opted_in')}><span>Opted in</span><small>Bind consent now</small></button>
          <button type="button" role="radio" aria-checked={draft.whatsapp.consent === 'unsubscribed'} onclick={() => recordConsent('unsubscribed')}><span>Unsubscribed</span><small>Hard stop</small></button>
        </div>
        {#if consentCurrent}
          <p class="consent-proof">Opt-in recorded {consentTime(draft.whatsapp.consentRecordedAtMillis)} for <strong>{draft.whatsapp.consentRecipient}</strong>. Editing the recipient revokes this record.</p>
        {:else if !draft.whatsapp.recipient.trim()}
          <p class="consent-proof">Enter a recipient before recording opt-in.</p>
        {/if}
      </section>

      <div class="test-line">
        <div><strong>{output?.detail ?? 'WhatsApp is not configured'}</strong><span>{settingsDirty ? 'Applying these settings…' : 'This sends one real template to the saved recipient.'}</span></div>
        <button class="secondary-action action-with-icon" onclick={sendTest} disabled={testBusy || settingsDirty || !draft.whatsapp.enabled || !draft.whatsapp.tokenConfigured || !consentCurrent}><Send size={16} aria-hidden="true" /> {testBusy ? 'Submitting' : settingsDirty ? 'Applying changes…' : 'Send template test'}</button>
      </div>
    </div>

    <aside class="delivery-contract">
      <p>Message contract</p><h3>Material changes only</h3>
      <dl>
        <div><dt>Bridge</dt><dd>State, ETA, and confidence when predictive</dd></div>
        <div><dt>Other channels</dt><dd>New or escalating important notices</dd></div>
        <div><dt>Resolved</dt><dd>One all-clear after an active signal clears</dd></div>
      </dl>
      <small>Meta acceptance is stored locally. The app does not infer delivered or read status.</small>
    </aside>
  </div>
</section>
