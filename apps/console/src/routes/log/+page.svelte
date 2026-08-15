<script lang="ts">
  import { Download, Search, SlidersHorizontal } from '@lucide/svelte';

  import { snapshot } from '$lib/state';
  import type { BridgeStateInterval, DeliveryState } from '$lib/types';

  let query = $state('');
  let channel = $state('all');
  let delivery = $state<'all' | DeliveryState>('all');

  const filtered = $derived.by(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return ($snapshot?.dispatches ?? []).filter((record) => {
      const matchesQuery = !normalized || `${record.title} ${record.channelId} ${record.state}`.toLocaleLowerCase().includes(normalized);
      const matchesChannel = channel === 'all' || record.channelId === channel;
      const matchesDelivery = delivery === 'all' || record.deliveryState === delivery;
      return matchesQuery && matchesChannel && matchesDelivery;
    });
  });

  const filteredBridgeIntervals = $derived.by(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return ($snapshot?.bridgeIntervals ?? []).filter((interval) => {
      const channelId = interval.sourceId.replace(/^fl511\./, '');
      const matchesQuery =
        !normalized ||
        `${interval.bridgeName} ${interval.bridgeKey} ${interval.relation} ${interval.state}`
          .toLocaleLowerCase()
          .includes(normalized);
      const matchesChannel = channel === 'all' || channel === channelId;
      return matchesQuery && matchesChannel;
    });
  });

  const formatTime = (value: string) =>
    new Intl.DateTimeFormat(undefined, {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      second: '2-digit'
    }).format(new Date(value));

  const channelTitle = (id: string) => $snapshot?.channels.find((item) => item.id === id)?.title ?? id;

  const intervalLabel = (interval: BridgeStateInterval) =>
    interval.state === 'up' ? 'Opened' : interval.state === 'down' ? 'Closed' : 'State unknown';

  function intervalDuration(interval: BridgeStateInterval): string {
    const elapsed = Math.max(0, new Date(interval.endedAt ?? Date.now()).getTime() - new Date(interval.startedAt).getTime());
    const minutes = Math.floor(elapsed / 60_000);
    const seconds = Math.floor((elapsed % 60_000) / 1_000);
    if (!interval.endedAt) return interval.state === 'up' ? `Open for ${minutes}m ${seconds}s` : 'Current state';
    return minutes ? `${minutes}m ${seconds}s` : `${seconds}s`;
  }

  function exportVisible() {
    const payload = JSON.stringify(
      {
        exportedAt: new Date().toISOString(),
        generatedAt: $snapshot?.generatedAt,
        bridgeIntervals: filteredBridgeIntervals,
        dispatches: filtered
      },
      null,
      2
    );
    const url = URL.createObjectURL(new Blob([payload], { type: 'application/json' }));
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `tenders-log-${new Date().toISOString().slice(0, 10)}.json`;
    anchor.click();
    URL.revokeObjectURL(url);
  }
</script>

<svelte:head>
  <title>Log · Tender’s Log</title>
  <meta name="description" content="Inspect recorded bridge openings and durable outbound delivery outcomes." />
</svelte:head>

<section class="page-sheet log-page">
  <header class="page-heading-row">
    <div>
      <p class="registration-label">Durable history</p>
      <h1 class="sheet-heading">See what changed, and when</h1>
      <p class="sheet-intro">
        This ledger records FL511 bridge state intervals and durable WhatsApp outcomes. Routine polling stays out;
        bridge changes and material delivery revisions stay in.
      </p>
    </div>
    <button class="secondary-action export-action" onclick={exportVisible} disabled={!filtered.length && !filteredBridgeIntervals.length}>
      <Download size={17} aria-hidden="true" /> Export visible JSON
    </button>
  </header>

  {#if $snapshot}
    <div class="log-register">
      <form class="log-filters" onsubmit={(event) => event.preventDefault()} aria-label="Filter dispatch log">
        <label class="search-field">
          <Search size={18} strokeWidth={1.5} aria-hidden="true" />
          <span class="visually-hidden">Search title, channel, or state</span>
          <input bind:value={query} placeholder="Search title, channel, or state" autocomplete="off" />
        </label>
        <label class="field compact-filter">
          <span>Channel</span>
          <select bind:value={channel}>
            <option value="all">All channels</option>
            {#each $snapshot.channels as item (item.id)}
              <option value={item.id}>{item.title}</option>
            {/each}
          </select>
        </label>
        <label class="field compact-filter">
          <span>Outcome</span>
          <select bind:value={delivery}>
            <option value="all">Every outcome</option>
            <option value="pending">Pending</option>
            <option value="accepted">Accepted</option>
            <option value="delivered">Delivered · relay required</option>
            <option value="failed">Failed</option>
            <option value="suppressed">Suppressed</option>
          </select>
        </label>
        <div class="filter-result" aria-live="polite">
          <SlidersHorizontal size={17} strokeWidth={1.5} aria-hidden="true" />
          <span>{filteredBridgeIntervals.length} bridge intervals · {filtered.length} delivery revisions</span>
        </div>
      </form>

      <section class="history-section" aria-labelledby="bridge-history-heading">
        <header class="register-heading">
          <div>
            <p class="registration-label">FL511 observations</p>
            <h2 id="bridge-history-heading">Bridge openings</h2>
          </div>
          <p>Target and upstream open/closed intervals retained for prediction history.</p>
        </header>
        {#if filteredBridgeIntervals.length}
          <div class="bridge-table" aria-label="Bridge state history">
            <div class="bridge-head" aria-hidden="true">
              <span>Started</span><span>Bridge</span><span>Observed state</span><span>Duration</span>
            </div>
            {#each filteredBridgeIntervals as interval (`${interval.sourceId}:${interval.bridgeKey}:${interval.startedAt}`)}
              <article class="bridge-row">
                <time datetime={interval.startedAt}>{formatTime(interval.startedAt)}</time>
                <div class="bridge-name">
                  <strong>{interval.bridgeName}</strong>
                  <span>{interval.relation === 'target' ? 'Brickell target' : 'Upstream bridge'}</span>
                </div>
                <span class="bridge-state" data-state={interval.state}>{intervalLabel(interval)}</span>
                <div class="interval-duration">
                  <strong>{intervalDuration(interval)}</strong>
                  {#if interval.endedAt}<small>until {formatTime(interval.endedAt)}</small>{/if}
                </div>
              </article>
            {/each}
          </div>
        {:else}
          <div class="empty-log compact-empty">
            <h3>No bridge intervals match</h3>
            <p>FL511 state changes will appear here after the next matching observation.</p>
          </div>
        {/if}
      </section>

      <section class="history-section" aria-labelledby="delivery-history-heading">
        <header class="register-heading">
          <div>
            <p class="registration-label">Outbound history</p>
            <h2 id="delivery-history-heading">Delivery revisions</h2>
          </div>
          <p>Material WhatsApp notices and provider acceptance outcomes.</p>
        </header>

      {#if filtered.length}
        <div class="dispatch-table" aria-label="Dispatch records">
          <div class="dispatch-head" aria-hidden="true">
            <span>Updated</span>
            <span>Channel and revision</span>
            <span>Attention</span>
            <span>Destinations</span>
            <span>Outcome</span>
          </div>
          {#each filtered as record (record.id)}
            <article class="dispatch-row">
              <time datetime={record.at}>{formatTime(record.at)}</time>
              <div class="dispatch-event">
                <strong>{record.title}</strong>
                <span>{channelTitle(record.channelId)} · revision {record.materialRevision} · {record.state.replace('_', ' ')}</span>
                <small>Incident {record.incidentId}</small>
              </div>
              <div class="attention-cell">
                <span class="urgency-mark" data-urgency={record.urgency}>{record.urgency.replace('_', ' ')}</span>
              </div>
              <div class="destinations-cell">
                {#if record.destinations.length}
                  {#each record.destinations as destination}
                    <span>{destination}</span>
                  {/each}
                {:else}
                  <em>No route</em>
                {/if}
              </div>
              <div>
                <span class="status-word" data-state={record.deliveryState}>{record.deliveryState}</span>
              </div>
            </article>
          {/each}
        </div>
      {:else}
        <div class="empty-log">
          <h2>{$snapshot.dispatches.length ? 'No dispatches match' : 'No durable messages yet'}</h2>
          <p>{$snapshot.dispatches.length
            ? 'Clear the search or broaden the channel and outcome filters. The stored log has not been changed.'
            : 'A row appears after an interrupt-eligible material change enters the WhatsApp outbox. Routine refreshes and native notices stay out of this ledger.'}</p>
          <button
            class="secondary-action"
            onclick={() => {
              query = '';
              channel = 'all';
              delivery = 'all';
            }}
          >Clear filters</button>
        </div>
      {/if}
      </section>
    </div>
  {:else}
    <div class="empty-sheet" aria-busy="true"><h2>Loading dispatch log</h2><p>Waiting for a complete engine snapshot.</p></div>
  {/if}
</section>

<style>
  .log-page {
    padding-inline: clamp(18px, 3vw, 48px);
  }

  .export-action {
    display: inline-flex;
    align-items: center;
    gap: 9px;
  }

  .log-register {
    background: var(--frost);
    border-block: 1px solid var(--rule-strong);
  }

  .log-filters {
    display: grid;
    grid-template-columns: minmax(240px, 1.5fr) minmax(170px, 0.72fr) minmax(170px, 0.72fr) auto;
    align-items: end;
    gap: 14px;
    padding: 18px clamp(16px, 2.5vw, 32px);
    border-bottom: 1px solid var(--rule-strong);
  }

  .search-field {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 9px;
    min-height: 44px;
    color: var(--muted);
    background: var(--paper);
    border: 1px solid var(--steel);
    border-radius: 2px;
    padding: 0 12px;
  }

  .search-field:focus-within {
    border-color: var(--channel);
  }

  .search-field input {
    min-width: 0;
    min-height: 42px;
    color: var(--graphite);
    background: transparent;
    border: 0;
    outline: 0;
  }

  .compact-filter {
    gap: 5px;
  }

  .filter-result {
    display: inline-flex;
    min-height: 44px;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    color: var(--muted);
    font-size: var(--type-caption);
    text-align: end;
  }

  .history-section + .history-section {
    border-top: 1px solid var(--rule-strong);
  }

  .register-heading {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 24px;
    padding: 20px clamp(16px, 2.5vw, 32px) 16px;
    background: var(--paper);
    border-bottom: 1px solid var(--rule);
  }

  .register-heading h2 {
    margin: 3px 0 0;
    color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .register-heading > p {
    max-width: 48ch;
    margin: 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
    text-align: right;
  }

  .bridge-table {
    overflow-x: auto;
  }

  .bridge-head,
  .bridge-row {
    display: grid;
    min-width: 760px;
    grid-template-columns: 170px minmax(280px, 1fr) 150px 180px;
    align-items: center;
    gap: 18px;
    padding-inline: clamp(16px, 2.5vw, 32px);
  }

  .bridge-head {
    min-height: 38px;
    color: var(--muted);
    background: var(--frost);
    border-bottom: 1px solid var(--rule);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.065em;
    text-transform: uppercase;
  }

  .bridge-row {
    min-height: 76px;
    border-bottom: 1px solid var(--rule);
  }

  .bridge-row:last-child {
    border-bottom: 0;
  }

  .bridge-row > time,
  .bridge-name span,
  .interval-duration small {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.35;
  }

  .bridge-name,
  .interval-duration {
    display: grid;
    gap: 3px;
  }

  .bridge-name strong,
  .interval-duration strong {
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    line-height: 1.2;
  }

  .bridge-state {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .bridge-state::before {
    width: 10px;
    height: 10px;
    border: 1px solid currentColor;
    content: '';
  }

  .bridge-state[data-state='up'] {
    color: var(--danger);
  }

  .bridge-state[data-state='up']::before {
    background: currentColor;
  }

  .bridge-state[data-state='down'] {
    color: var(--success);
  }

  .bridge-state[data-state='unknown'] {
    color: var(--muted);
  }

  .compact-empty {
    padding-block: 24px;
  }

  .compact-empty h3 {
    margin: 0;
    color: var(--marine);
    font-size: var(--type-title);
    text-transform: uppercase;
  }

  .dispatch-table {
    overflow-x: auto;
  }

  .dispatch-head,
  .dispatch-row {
    display: grid;
    min-width: 890px;
    grid-template-columns: 150px minmax(290px, 1.7fr) 120px minmax(170px, 0.85fr) 120px;
    align-items: center;
    gap: 18px;
  }

  .dispatch-head {
    padding: 11px clamp(16px, 2.5vw, 32px);
    color: var(--muted);
    background: var(--paper);
    border-bottom: 1px solid var(--rule);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.065em;
    text-transform: uppercase;
  }

  .dispatch-row {
    min-height: 92px;
    padding: 14px clamp(16px, 2.5vw, 32px);
    border-bottom: 1px solid var(--rule);
  }

  .dispatch-row:last-child {
    border-bottom: 0;
  }

  .dispatch-row > time {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .dispatch-event {
    display: grid;
    min-width: 0;
    gap: 4px;
  }

  .dispatch-event strong {
    overflow-wrap: anywhere;
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    line-height: 1.05;
    text-transform: uppercase;
  }

  .dispatch-event span,
  .dispatch-event small {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.35;
  }

  .dispatch-event small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .urgency-mark {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .urgency-mark::before {
    width: 9px;
    height: 9px;
    border: 1px solid currentColor;
    content: '';
  }

  .urgency-mark[data-urgency='routine'] {
    color: var(--muted);
  }

  .urgency-mark[data-urgency='heads_up'],
  .urgency-mark[data-urgency='action'] {
    color: var(--amber-ink);
  }

  .urgency-mark[data-urgency='heads_up']::before {
    background: var(--amber);
  }

  .urgency-mark[data-urgency='action']::before,
  .urgency-mark[data-urgency='emergency']::before {
    background: currentColor;
  }

  .urgency-mark[data-urgency='emergency'] {
    color: var(--danger);
  }

  .destinations-cell {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }

  .destinations-cell span {
    border: 1px solid var(--rule-strong);
    border-radius: 2px;
    padding: 4px 6px;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.045em;
    text-transform: uppercase;
  }

  .destinations-cell em {
    color: var(--muted);
    font-size: var(--type-caption);
    font-style: normal;
  }

  .empty-log {
    max-width: 650px;
    padding: clamp(28px, 4vw, 48px) clamp(20px, 3vw, 38px);
  }

  .empty-log h2 {
    margin: 0;
    color: var(--marine);
    font-size: var(--type-section);
    text-transform: uppercase;
  }

  .empty-log p {
    max-width: 62ch;
    margin: 9px 0 20px;
    color: var(--muted);
    font-size: var(--type-body-small);
    line-height: 1.5;
  }

  @media (max-width: 980px) {
    .log-filters {
      grid-template-columns: 1fr 1fr;
    }

    .register-heading {
      align-items: start;
      flex-direction: column;
      gap: 8px;
    }

    .register-heading > p {
      text-align: left;
    }

    .search-field,
    .filter-result {
      grid-column: 1 / -1;
    }

    .filter-result {
      justify-content: flex-start;
      min-height: 28px;
      text-align: start;
    }
  }

  @media (max-width: 560px) {
    .log-filters {
      grid-template-columns: 1fr;
    }

    .search-field,
    .filter-result {
      grid-column: 1;
    }
  }
</style>
