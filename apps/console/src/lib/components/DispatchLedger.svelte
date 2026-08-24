<script lang="ts">
  import type { ChannelSnapshot, DispatchRecord } from '$lib/types';

  let {
    dispatches,
    channels
  }: {
    dispatches: DispatchRecord[];
    channels: ChannelSnapshot[];
  } = $props();

  const channelName = (id: string) => channels.find((channel) => channel.id === id)?.title ?? id;
  const time = (value: string) =>
    new Intl.DateTimeFormat('en-US', { hour: 'numeric', minute: '2-digit' }).format(new Date(value));
</script>

<section class="dispatch-ledger" aria-labelledby="dispatch-heading">
  <header class="ruled-header">
    <div>
      <p class="registration-label">Message history</p>
      <h2 id="dispatch-heading">Recent messages</h2>
    </div>
    <a href="/system/log">View history</a>
  </header>

  {#if dispatches.length}
    <div class="table-wrap">
      <table>
        <thead>
          <tr><th>Updated</th><th>Channel</th><th>Change</th><th>Priority</th><th>Destination</th><th>Status</th></tr>
        </thead>
        <tbody>
          {#each dispatches.slice(0, 5) as dispatch (dispatch.id)}
            <tr>
              <td><time datetime={dispatch.at}>{time(dispatch.at)}</time></td>
              <td>{channelName(dispatch.channelId)}</td>
              <td><strong>{dispatch.title}</strong><small>Revision {dispatch.materialRevision} · {dispatch.state.replace('_', ' ')}</small></td>
              <td>{dispatch.urgency.replace('_', ' ')}</td>
              <td>{dispatch.destinations.join(' · ')}</td>
              <td><span class="status-word" data-state={dispatch.deliveryState}>{dispatch.deliveryState}</span></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="ledger-empty">
      <strong>No messages yet</strong>
      <span>WhatsApp alerts will appear here after they are sent.</span>
    </div>
  {/if}
</section>

<style>
  .dispatch-ledger {
    min-width: 0;
    padding: 24px clamp(20px, 3vw, 42px) 30px;
    background: var(--frost);
    border-top: 1px solid var(--rule-strong);
  }

  .ruled-header > div {
    display: grid;
    gap: 6px;
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

  .table-wrap {
    overflow-x: auto;
  }

  table {
    width: 100%;
    min-width: 760px;
    border-collapse: collapse;
    font-size: var(--type-label);
  }

  th,
  td {
    padding: 10px 12px;
    border-bottom: 1px solid var(--rule);
    text-align: left;
    vertical-align: middle;
  }

  th {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  td:first-child,
  td:nth-child(2),
  td:nth-child(4),
  td:nth-child(5) {
    font-family: var(--font-instrument);
    font-weight: 600;
    letter-spacing: 0.025em;
    text-transform: uppercase;
  }

  td strong {
    display: block;
    font-weight: 600;
  }

  td small {
    display: block;
    margin-top: 3px;
    color: var(--muted);
    font-size: var(--type-micro);
  }

  .ledger-empty {
    display: grid;
    gap: 5px;
    padding: 22px 0 6px;
    color: var(--muted);
    border-top: 1px solid var(--rule);
  }

  .ledger-empty strong {
    color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    text-transform: uppercase;
  }

  @media (max-width: 720px) {
    .dispatch-ledger {
      padding-inline: 16px;
    }
  }
</style>
