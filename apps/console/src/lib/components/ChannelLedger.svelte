<script lang="ts">
  import {
    BellRing,
    ChartNoAxesCombined,
    CloudSun,
    Landmark,
    MonitorUp,
    Newspaper,
    RadioTower,
    Send,
    Trophy,
    Waves
  } from '@lucide/svelte';
  import type { ChannelSnapshot, OutputSnapshot } from '$lib/types';

  let {
    channels,
    outputs,
    compact = false
  }: {
    channels: ChannelSnapshot[];
    outputs: OutputSnapshot[];
    /**
     * Reference density: on the live page this is plumbing sitting below the
     * decision and the river, so it lays out in rows across the sheet rather
     * than as a full-height column competing with them.
     */
    compact?: boolean;
  } = $props();

  const age = (seconds: number) => {
    if (seconds === 0) return '—';
    if (seconds < 60) return `${Math.round(seconds)}s`;
    if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
    return `${Math.round(seconds / 3600)}h`;
  };

  const iconForChannel = (kind: ChannelSnapshot['kind']) => {
    if (kind === 'bridge') return RadioTower;
    if (kind === 'weather') return CloudSun;
    if (kind === 'official') return Landmark;
    if (kind === 'hurricane') return Waves;
    if (kind === 'news') return Newspaper;
    if (kind === 'sports') return Trophy;
    if (kind === 'markets') return ChartNoAxesCombined;
    return BellRing;
  };

  const iconForOutput = (id: OutputSnapshot['id']) => (id === 'epaper' ? MonitorUp : Send);
</script>

<aside class="ledger" class:compact aria-label="Enabled channels and destinations">
  <section>
    <header class="ruled-header">
      <div>
        <p class="registration-label">Roster</p>
        <h2>Enabled channels</h2>
      </div>
      <a href="/channels">Edit</a>
    </header>

    <div class="ledger-column-labels" aria-hidden="true">
      <span>Channel</span><span>Status</span><span>Age</span>
    </div>

    <div class="ledger-list">
      {#each channels.filter((channel) => channel.enabled) as channel (channel.id)}
        {@const ChannelIcon = iconForChannel(channel.kind)}
        <a class="ledger-row" href={`/channels?channel=${channel.id}`}>
          <ChannelIcon size={21} strokeWidth={1.55} aria-hidden="true" />
          <span class="ledger-copy">
            <strong>{channel.title}</strong>
            <small>{channel.summary}</small>
          </span>
          <span class="status-word" data-state={channel.availability}>{channel.availability}</span>
          <time>{age(channel.ageSeconds)}</time>
        </a>
      {/each}
    </div>
  </section>

  <section class="outputs">
    <header class="ruled-header">
      <div>
        <p class="registration-label">Routes</p>
        <h2>Destinations</h2>
      </div>
      <a href="/outputs">Edit</a>
    </header>

    <div class="ledger-list">
      {#each outputs as output (output.id)}
        {@const OutputIcon = iconForOutput(output.id)}
        <a class="ledger-row output-row" href="/outputs">
          <OutputIcon size={21} strokeWidth={1.55} aria-hidden="true" />
          <span class="ledger-copy">
            <strong>{output.title}</strong>
            <small>{output.detail}</small>
          </span>
          <span class="status-word" data-state={output.state}>{output.state}</span>
        </a>
      {/each}
    </div>
  </section>

  <section class="ledger-key" aria-label="Availability key">
    <span class="status-word" data-state="fresh">Fresh</span>
    <span class="status-word" data-state="delayed">Delayed</span>
    <span class="status-word" data-state="offline">Offline</span>
  </section>
</aside>

<style>
  .ledger {
    min-width: 0;
    color: var(--graphite);
    background: var(--frost);
    padding: clamp(24px, 2.4vw, 36px) clamp(18px, 2vw, 30px);
  }

  .ledger section + section {
    margin-top: 42px;
  }

  .ruled-header > div {
    display: grid;
    gap: 7px;
  }

  h2 {
    margin: 0;
    color: var(--marine);
    font-size: var(--type-section);
    font-weight: 700;
    line-height: 0.95;
    text-transform: uppercase;
  }

  header a {
    color: var(--channel);
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-decoration: none;
    text-transform: uppercase;
  }

  header a:hover {
    color: var(--graphite);
    text-decoration: underline;
    text-underline-offset: 4px;
  }

  .ledger-column-labels {
    display: grid;
    grid-template-columns: 1fr auto 40px;
    gap: 12px;
    padding: 11px 2px 7px 38px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.065em;
    text-transform: uppercase;
  }

  .ledger-column-labels span:nth-child(2),
  .ledger-column-labels span:nth-child(3) {
    text-align: right;
  }

  .ledger-list {
    border-top: 1px solid var(--rule);
  }

  .ledger-row {
    display: grid;
    min-width: 0;
    grid-template-columns: 25px minmax(0, 1fr) auto 36px;
    align-items: center;
    gap: 10px;
    color: var(--graphite);
    border-bottom: 1px solid var(--rule);
    padding: 12px 2px;
    text-decoration: none;
    transition: background-color 140ms ease-out;
  }

  .ledger-row:hover {
    background: var(--paper);
  }

  .ledger-row > :global(svg) {
    color: var(--marine);
  }

  .ledger-copy {
    display: grid;
    min-width: 0;
    gap: 3px;
  }

  .ledger-copy strong {
    overflow: hidden;
    font-family: var(--font-instrument);
    font-size: var(--type-body);
    font-weight: 600;
    line-height: 1;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .ledger-copy small {
    overflow: hidden;
    color: var(--muted);
    font-size: var(--type-micro);
    line-height: 1.25;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ledger-row .status-word {
    font-size: var(--type-micro);
  }

  .ledger-row time {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    text-align: right;
  }

  .output-row {
    grid-template-columns: 25px minmax(0, 1fr) auto;
  }

  .ledger-key {
    display: flex;
    flex-wrap: wrap;
    gap: 14px 18px;
    padding-top: 16px;
    border-top: 1px solid var(--rule-strong);
  }

  @media (max-width: 1180px) {
    .ledger {
      display: grid;
      grid-template-columns: 1.25fr 1fr;
      gap: 28px;
    }

    .ledger section + section {
      margin-top: 0;
    }

    .ledger-key {
      grid-column: 1 / -1;
    }
  }

  @media (max-width: 720px) {
    .ledger {
      grid-template-columns: 1fr;
      padding: 26px 16px 32px;
    }

    .ledger section + section {
      margin-top: 32px;
    }

    .ledger-key {
      grid-column: auto;
    }
  }

  /* Compact: a low horizontal band, not a tall column. */
  .ledger.compact {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 0 clamp(20px, 3vw, 42px);
    padding: clamp(12px, 1.6vw, 18px) clamp(20px, 3vw, 42px);
    min-height: 0;
  }

  .ledger.compact :global(h2) {
    font-size: var(--type-title);
  }

  .ledger.compact .ledger-list {
    max-height: none;
  }

  .ledger.compact .ledger-row {
    padding-top: 5px;
    padding-bottom: 5px;
  }
</style>