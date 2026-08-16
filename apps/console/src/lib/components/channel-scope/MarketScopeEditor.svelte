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
    .symbol-entry {
      grid-template-columns: 1fr;
    }

    .symbol-entry .secondary-action {
      width: 100%;
      justify-content: center;
    }
  }
</style>
