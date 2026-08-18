<script lang="ts">
  import { Check, Clipboard, Database, RefreshCw, ShieldCheck, TriangleAlert } from '@lucide/svelte';

  import { refreshSources } from '$lib/api';
  import { notice, snapshot } from '$lib/state';

  let refreshing = $state(false);
  let copying = $state(false);

  const percentage = $derived(
    $snapshot?.system.collectorsTotal
      ? Math.round(($snapshot.system.collectorsOnline / $snapshot.system.collectorsTotal) * 100)
      : 0
  );

  const age = (seconds: number) => {
    if (seconds <= 0) return 'No current sample';
    if (seconds < 60) return `${Math.round(seconds)} seconds ago`;
    if (seconds < 3600) return `${Math.round(seconds / 60)} minutes ago`;
    return `${Math.round(seconds / 3600)} hours ago`;
  };

  const since = (value?: string) => {
    if (!value) return 'Never';
    return age(Math.max(0, (Date.now() - new Date(value).getTime()) / 1000));
  };

  const storageSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  async function refresh() {
    refreshing = true;
    try {
      notice.set(await refreshSources());
    } finally {
      refreshing = false;
    }
  }

  async function copyDiagnostics() {
    if (!$snapshot) return;
    copying = true;
    const diagnostic = JSON.stringify(
      {
        generatedAt: $snapshot.generatedAt,
        system: $snapshot.system,
        decision: {
          state: $snapshot.decision.state,
          availability: $snapshot.decision.availability,
          sourceAgeSeconds: $snapshot.decision.sourceAgeSeconds
        },
        channels: $snapshot.channels.map(({ id, availability, ageSeconds, enabled }) => ({
          id,
          availability,
          ageSeconds,
          enabled
        })),
        outputs: $snapshot.outputs.map(({ id, state, deliveryState }) => ({ id, state, deliveryState }))
      },
      null,
      2
    );
    try {
      await navigator.clipboard.writeText(diagnostic);
      notice.set({ ok: true, message: 'Sanitized diagnostics copied. Credentials and message contents were excluded.' });
    } catch {
      notice.set({ ok: false, message: 'Clipboard access was denied. No diagnostic data was copied.' });
    } finally {
      copying = false;
    }
  }
</script>

<svelte:head>
  <title>System · BrickellStatus</title>
  <meta name="description" content="Inspect collector freshness and runtime health." />
</svelte:head>

<section class="page-sheet system-page">
  <header class="page-heading-row">
    <div>
      <p class="registration-label">Engine room</p>
      <h1 class="sheet-heading">Trust the state because you can inspect it</h1>
      <p class="sheet-intro">
        Collector health, source age, storage version, and output readiness remain visible. An offline source becomes
        unknown or stale; it never quietly turns into a reassuring clear state.
      </p>
    </div>
    <div class="heading-actions">
      <button class="secondary-action" onclick={copyDiagnostics} disabled={!$snapshot || copying}>
        <Clipboard size={17} aria-hidden="true" /> {copying ? 'Copying' : 'Copy diagnostics'}
      </button>
      <button class="primary-action" onclick={refresh} disabled={refreshing}>
        <RefreshCw size={17} class={refreshing ? 'spinning' : ''} aria-hidden="true" />
        {refreshing ? 'Polling sources' : 'Poll sources now'}
      </button>
    </div>
  </header>

  {#if $snapshot}
    <div class="system-register">
      <section class="system-verdict" aria-labelledby="verdict-heading">
        <div class="verdict-heading">
          <div class="verdict-icon" data-state={$snapshot.system.status}>
            {#if $snapshot.system.status === 'nominal'}
              <Check size={34} strokeWidth={1.5} aria-hidden="true" />
            {:else}
              <TriangleAlert size={34} strokeWidth={1.5} aria-hidden="true" />
            {/if}
          </div>
          <div>
            <p>Current engine verdict</p>
            <h2 id="verdict-heading">
              {$snapshot.system.status === 'nominal' ? 'Operational' : $snapshot.system.status}
            </h2>
          </div>
        </div>
        <div class="verdict-coverage">
          <strong>{$snapshot.system.collectorsOnline} of {$snapshot.system.collectorsTotal} collectors reporting</strong>
          <div class="availability-rule" aria-label={`${percentage}% of collectors reporting`}>
            <span style={`--availability: ${percentage}%`}></span>
          </div>
        </div>
        <dl>
          <div><dt>Last complete cycle</dt><dd>{new Date($snapshot.system.lastCycleAt).toLocaleString()}</dd></div>
          <div><dt>Snapshot source</dt><dd>Live runtime</dd></div>
          <div><dt>Local time zone</dt><dd>{$snapshot.localTimeZone}</dd></div>
        </dl>
      </section>

      <div class="system-work">
        <section id="source-health" class="system-section" aria-labelledby="sources-heading">
          <header>
            <div>
              <h2 id="sources-heading">Source freshness</h2>
              <p>Every registered collector is listed with its current reason. Failures stay visible until that source reports successfully again.</p>
            </div>
          </header>

          <div class="source-ledger">
            <div class="source-head" aria-hidden="true">
              <span>Channel</span><span>Source</span><span>Last success</span><span>Current reason</span><span>State</span>
            </div>
            {#each $snapshot.system.sources as source (source.sourceId)}
              {@const channel = $snapshot.channels.find((item) => item.id === source.channelId)}
              <article class="source-row" data-state={source.availability}>
                <div>
                  <strong>{channel?.title ?? source.channelId}</strong>
                  <small>{source.channelId}</small>
                </div>
                <div>
                  <span>{source.sourceId}</span>
                  <small>{source.failureCount ? `${source.failureCount} consecutive failures` : 'No active failures'}</small>
                </div>
                <div>
                  <span>{since(source.lastSuccessAt)}</span>
                  <small>Attempted {since(source.lastAttemptAt)}</small>
                </div>
                <p>{source.detail}</p>
                <span class="status-word" data-state={source.availability}>
                  {source.availability}
                </span>
              </article>
            {/each}
          </div>
        </section>

        <section class="system-section" aria-labelledby="runtime-heading">
          <header>
            <div>
              <h2 id="runtime-heading">Runtime and storage</h2>
              <p>Live SQLite allocation and runtime versions.</p>
            </div>
          </header>

          <div class="runtime-register">
            <div>
              <Database size={22} strokeWidth={1.5} aria-hidden="true" />
              <span><strong>SQLite</strong><small>{$snapshot.system.sqliteVersion}</small></span>
              <em>{storageSize($snapshot.system.databaseSizeBytes)} used</em>
            </div>
            <div>
              <ShieldCheck size={22} strokeWidth={1.5} aria-hidden="true" />
              <span><strong>BrickellStatus engine</strong><small>Policy and collector runtime</small></span>
              <em>{$snapshot.system.engineVersion}</em>
            </div>
          </div>
        </section>

      </div>
    </div>
  {:else}
    <div class="empty-sheet" aria-busy="true"><h2>Waiting for engine health</h2><p>No complete system snapshot has arrived.</p></div>
  {/if}
</section>

<style>
  .system-page {
    padding-inline: clamp(18px, 2.5vw, 36px);
  }

  .heading-actions {
    display: flex;
    gap: 9px;
  }

  .heading-actions button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  :global(.spinning) {
    animation: spin 700ms linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .system-register {
    border-block: 1px solid var(--rule-strong);
  }

  .system-verdict {
    display: grid;
    grid-template-columns: minmax(260px, 0.9fr) minmax(240px, 0.8fr) minmax(440px, 1.6fr);
    align-items: center;
    gap: clamp(24px, 3vw, 48px);
    padding: clamp(24px, 2.7vw, 34px);
    color: var(--white);
    background: var(--marine);
  }

  .verdict-heading {
    display: flex;
    align-items: center;
    gap: 18px;
    min-width: 0;
  }

  .verdict-icon {
    flex: 0 0 auto;
    display: grid;
    width: 62px;
    height: 62px;
    place-items: center;
    border: 1px solid var(--nav-muted);
  }

  .verdict-icon[data-state='nominal'] {
    color: var(--signal-green);
  }

  .verdict-icon[data-state='degraded'] {
    color: var(--amber);
  }

  .verdict-icon[data-state='offline'] {
    color: var(--white);
  }

  .verdict-heading p {
    margin: 0;
    color: var(--nav-muted);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .system-verdict h2 {
    max-width: 100%;
    margin: 0;
    font-size: clamp(2.2rem, 3.2vw, 3.1rem);
    line-height: 0.88;
    overflow-wrap: anywhere;
    text-transform: uppercase;
  }

  .verdict-coverage {
    display: grid;
    gap: 14px;
  }

  .verdict-coverage > strong {
    font-size: var(--type-body-small);
    line-height: 1.4;
  }

  .availability-rule {
    height: 8px;
    margin: 0;
    border: 1px solid var(--nav-subdued);
  }

  .availability-rule span {
    display: block;
    width: var(--availability);
    height: 100%;
    background: var(--signal-green);
  }

  .system-verdict dl {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    margin: 0;
    border-inline-start: 1px solid rgba(255, 255, 255, 0.28);
  }

  .system-verdict dl > div {
    display: grid;
    gap: 4px;
    min-width: 0;
    padding: 4px 16px;
    border-inline-end: 1px solid rgba(255, 255, 255, 0.2);
  }

  .system-verdict dt {
    color: var(--nav-muted);
    font-size: var(--type-caption);
  }

  .system-verdict dd {
    margin: 0;
    color: var(--white);
    font-size: var(--type-body-small);
    overflow-wrap: anywhere;
  }

  .system-work {
    container-type: inline-size;
    min-width: 0;
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
  }

  .system-section {
    padding: clamp(24px, 2.8vw, 36px);
    border-bottom: 1px solid var(--rule-strong);
  }

  .system-section:last-child {
    border-bottom: 0;
  }

  .system-section > header {
    margin-bottom: 24px;
  }

  .system-section header > div {
    max-width: 68ch;
  }

  .system-section h2 {
    margin: 0;
    color: var(--marine);
    font-size: var(--type-section);
    line-height: 0.95;
    text-transform: uppercase;
  }

  .system-section header p {
    margin: 8px 0 0;
    color: var(--muted);
    font-size: var(--type-body-small);
    line-height: 1.5;
  }

  .source-ledger {
    border-top: 1px solid var(--rule-strong);
  }

  .source-head,
  .source-row {
    display: grid;
    grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.15fr) minmax(0, 0.72fr) minmax(0, 1.45fr) 76px;
    align-items: center;
    gap: 14px;
  }

  .source-head > *,
  .source-row > * {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .source-head {
    padding: 10px 8px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .source-row {
    min-height: 76px;
    border-top: 1px solid var(--rule);
    padding: 10px 8px;
  }

  .source-row > div {
    display: grid;
    gap: 3px;
  }

  .source-row strong,
  .runtime-register strong {
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    line-height: 1;
    text-transform: uppercase;
  }

  .source-row small,
  .source-row p,
  .source-row span,
  .runtime-register small,
  .runtime-register em {
    color: var(--muted);
    font-size: var(--type-caption);
    font-style: normal;
    line-height: 1.4;
  }

  .source-row p {
    margin: 0;
  }

  .source-row[data-state='delayed'],
  .source-row[data-state='stale'] {
    background: color-mix(in srgb, var(--amber-sheet) 34%, transparent);
  }

  .source-row[data-state='offline'] {
    background: color-mix(in srgb, var(--danger) 7%, transparent);
  }

  .runtime-register {
    display: grid;
    grid-template-columns: 1fr 1fr;
    border-block: 1px solid var(--rule-strong);
  }

  .runtime-register > div {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 14px;
    min-height: 86px;
    padding: 14px 16px;
    border-inline-end: 1px solid var(--rule);
  }

  .runtime-register > div:last-child {
    border-inline-end: 0;
  }

  .runtime-register > div > span {
    display: grid;
    gap: 4px;
  }

  .runtime-register em {
    color: var(--graphite);
    font-weight: 600;
  }

  @container (max-width: 920px) {
    .source-head {
      display: none;
    }

    .source-row {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr) auto;
      align-items: start;
      gap: 10px 18px;
      padding-block: 14px;
    }

    .source-row > div:nth-child(3) {
      grid-column: 1;
      grid-row: 2;
    }

    .source-row p {
      grid-column: 2 / 4;
      grid-row: 2;
    }

    .source-row .status-word {
      grid-column: 3;
      grid-row: 1;
    }
  }

  @media (max-width: 1050px) {
    .system-verdict {
      grid-template-columns: minmax(0, 1fr) minmax(220px, 0.8fr);
    }

    .system-verdict dl {
      grid-column: 1 / -1;
    }
  }

  @media (max-width: 760px) {
    .heading-actions {
      width: 100%;
      flex-direction: column;
    }

    .heading-actions button {
      justify-content: center;
      width: 100%;
    }

    .system-verdict {
      grid-template-columns: 1fr;
    }

    .system-verdict dl {
      grid-column: auto;
      grid-template-columns: 1fr;
      border-inline-start: 0;
      border-top: 1px solid rgba(255, 255, 255, 0.28);
    }

    .system-verdict dl > div {
      padding: 12px 0;
      border-inline-end: 0;
      border-bottom: 1px solid rgba(255, 255, 255, 0.2);
    }

    .source-head {
      display: none;
    }

    .source-row {
      grid-template-columns: 1fr auto;
      gap: 8px 18px;
      padding-block: 14px;
    }

    .source-row > div:nth-child(2),
    .source-row > div:nth-child(3),
    .source-row p {
      grid-column: 1;
    }

    .source-row .status-word {
      grid-column: 2;
      grid-row: 1;
    }

    .runtime-register {
      grid-template-columns: 1fr;
    }

    .runtime-register > div {
      border-inline-end: 0;
      border-bottom: 1px solid var(--rule);
    }

    .runtime-register > div:last-child {
      border-bottom: 0;
    }

  }
</style>
