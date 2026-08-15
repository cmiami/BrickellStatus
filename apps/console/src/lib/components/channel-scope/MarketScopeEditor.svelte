<script lang="ts">
  import type { ChannelPreference } from '$lib/types';

  let {
    channel,
    onchannelchange
  }: {
    channel: ChannelPreference;
    onchannelchange: (channel: ChannelPreference) => void;
  } = $props();

  let symbolDraft = $state('');
  let symbolIssue = $state('');
  const symbols = $derived(
    Array.isArray(channel.scope.symbols)
      ? channel.scope.symbols.filter((value): value is string => typeof value === 'string')
      : []
  );

  function set(key: string, value: string | number | string[]) {
    onchannelchange({ ...channel, scope: { ...channel.scope, [key]: value } });
  }

  function number(key: string, fallback: number): number {
    const value = channel.scope[key];
    return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
  }

  function addSymbol() {
    const symbol = symbolDraft.trim().toUpperCase();
    if (!symbol) {
      symbolIssue = 'Enter a ticker or market symbol first.';
      return;
    }
    if (symbol.length > 32 || !/^[A-Z0-9.^=_\-/]+$/.test(symbol)) {
      symbolIssue = 'Use up to 32 letters, numbers, or . ^ = _ - / characters.';
      return;
    }
    if (symbols.some((existing) => existing.toUpperCase() === symbol)) {
      symbolIssue = `${symbol} is already included.`;
      return;
    }
    if (symbols.length >= 16) {
      symbolIssue = 'A market channel can hold up to 16 symbols.';
      return;
    }
    set('symbols', [...symbols, symbol]);
    symbolDraft = '';
    symbolIssue = '';
  }

  function removeSymbol(symbol: string) {
    set('symbols', symbols.filter((existing) => existing !== symbol));
    symbolIssue = '';
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' || event.key === ',') {
      event.preventDefault();
      addSymbol();
    }
  }
</script>

<section class="market-scope" aria-labelledby={`${channel.id}-market-heading`}>
  <header>
    <div>
      <h4 id={`${channel.id}-market-heading`}>Market symbols</h4>
      <p>Enabling this channel starts the Yahoo Finance chart source. Disabling it stops every market request.</p>
    </div>
    <span>{symbols.length} / 16</span>
  </header>

  <div class="market-controls">
    <label class="field">
      <span>Material move</span>
      <span class="threshold-input">
        <input
          type="number"
          min="0.1"
          max="100"
          step="0.1"
          value={number('movePercent', 5)}
          oninput={(event) => set('movePercent', event.currentTarget.valueAsNumber)}
        />
        <b>%</b>
      </span>
      <small class="field-note">Absolute change from the previous close.</small>
    </label>
    <label class="field">
      <span>Refresh cadence</span>
      <select
        value={String(number('pollSeconds', 300))}
        onchange={(event) => set('pollSeconds', Number(event.currentTarget.value))}
      >
        <option value="120">Every 2 minutes</option>
        <option value="300">Every 5 minutes</option>
        <option value="600">Every 10 minutes</option>
        <option value="1800">Every 30 minutes</option>
      </select>
      <small class="field-note">Manual refresh can still request a quote immediately.</small>
    </label>
  </div>

  <div class="symbol-entry">
    <label>
      <span>Add a ticker or market symbol</span>
      <input
        bind:value={symbolDraft}
        maxlength="32"
        placeholder="AMD, ^GSPC, BTC-USD…"
        aria-invalid={symbolIssue ? 'true' : undefined}
        aria-describedby={`${channel.id}-symbol-help`}
        onkeydown={handleKeydown}
      />
    </label>
    <button class="secondary-action" type="button" onclick={addSymbol}>Add symbol</button>
  </div>
  <p id={`${channel.id}-symbol-help`} class:error={Boolean(symbolIssue)} class="symbol-help" aria-live="polite">
    {symbolIssue || 'Yahoo symbols are normalized to uppercase.'}
  </p>

  {#if symbols.length}
    <div class="symbol-register" role="group" aria-label="Configured market symbols">
      {#each symbols as symbol (symbol)}
        <span>
          <b>{symbol}</b>
          <button type="button" aria-label={`Remove ${symbol}`} onclick={() => removeSymbol(symbol)}>×</button>
        </span>
      {/each}
    </div>
  {:else}
    <p class="empty-symbols">Add at least one symbol before enabling this channel.</p>
  {/if}
</section>

<style>
  .market-scope {
    display: grid;
    overflow: hidden;
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .market-scope > header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 20px;
    padding: 16px;
    color: var(--white);
    background: var(--marine);
  }

  .market-scope > header div {
    display: grid;
    gap: 5px;
  }

  .market-scope h4,
  .market-scope p {
    margin: 0;
  }

  .market-scope h4 {
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .market-scope header p {
    max-width: 70ch;
    color: var(--nav-muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  .market-scope header > span {
    flex: 0 0 auto;
    color: var(--nav-muted);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 700;
    letter-spacing: 0.06em;
  }

  .market-controls {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
    padding: 18px 16px 0;
  }

  .threshold-input {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 42px;
  }

  .threshold-input input {
    border-inline-end: 0;
  }

  .threshold-input b {
    display: grid;
    place-items: center;
    color: var(--white);
    background: var(--marine);
    border: 1px solid var(--graphite);
    font-family: var(--font-instrument);
    font-size: var(--type-title);
  }

  .symbol-entry {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 10px;
    padding: 16px 16px 0;
  }

  .symbol-entry label {
    display: grid;
    gap: 7px;
  }

  .symbol-entry label > span {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .symbol-entry input {
    width: 100%;
    min-height: 44px;
    color: var(--graphite);
    background: var(--frost);
    border: 1px solid var(--steel);
    border-radius: 2px;
    padding: 10px 12px;
    font: inherit;
    text-transform: uppercase;
  }

  .symbol-entry input[aria-invalid='true'] {
    border-color: var(--danger);
    outline-color: var(--danger);
  }

  .symbol-help {
    padding: 6px 16px 0;
    color: var(--muted);
    font-size: var(--type-caption);
  }

  .symbol-help.error {
    color: var(--danger);
  }

  .symbol-register {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
    padding: 14px 16px 18px;
  }

  .symbol-register > span {
    display: grid;
    grid-template-columns: auto 30px;
    align-items: center;
    min-height: 36px;
    background: var(--white);
    border: 1px solid var(--rule-strong);
    border-radius: 2px;
  }

  .symbol-register b {
    padding: 0 10px;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    letter-spacing: 0.04em;
  }

  .symbol-register button {
    align-self: stretch;
    color: var(--muted);
    background: var(--frost);
    border-inline-start: 1px solid var(--rule);
    font-size: var(--type-title);
    cursor: pointer;
  }

  .symbol-register button:hover {
    color: var(--white);
    background: var(--danger);
  }

  .empty-symbols {
    margin: 14px 16px 18px;
    padding: 14px;
    color: var(--muted);
    background: var(--frost);
    border: 1px dashed var(--steel);
    font-size: var(--type-caption);
  }

  @media (max-width: 680px) {
    .market-controls,
    .symbol-entry {
      grid-template-columns: 1fr;
    }

    .symbol-entry .secondary-action {
      width: 100%;
      justify-content: center;
    }
  }
</style>
