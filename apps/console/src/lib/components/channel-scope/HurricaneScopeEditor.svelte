<script lang="ts">
  import type { ChannelPreference } from '$lib/types';
  import type { ChannelChange } from './scope';

  // Retained so the dispatcher's props stay uniform across every editor.
  let { channel }: { channel: ChannelPreference; onchannelchange: ChannelChange } = $props();
</script>

<div class="hurricane-scope">
  <section class="source-register" aria-labelledby={`${channel.id}-source-heading`}>
    <header>
      <div>
        <p class="registration-label">Official sources</p>
        <h4 id={`${channel.id}-source-heading`}>National Hurricane Center</h4>
      </div>
      <span>2 live feeds</span>
    </header>
    <div class="source-rows">
      <div>
        <strong>CurrentStorms</strong>
        <span>Active systems, position, intensity, movement, and official products.</span>
      </div>
      <div>
        <strong>Atlantic outlook</strong>
        <span>Official basin-wide development context.</span>
      </div>
    </div>
  </section>

  <!-- The switch that used to live here read "activate for every Atlantic
       cyclone", defaulted off, and was the only way this channel could ever
       activate — so it never did. Proximity to a saved place decides now, and
       there is nothing left to configure. -->
  <p class="activation-note">
    A cyclone activates this channel when it comes within range of a place you saved with tropical
    context switched on. Storms elsewhere in the basin stay off the display.
  </p>

  <p class="scope-note">NWS remains the authority for location-specific warnings.</p>
</div>

<style>
  .hurricane-scope {
    display: grid;
    gap: 14px;
  }

  .activation-note {
    margin: 0;
    padding: 16px;
    color: var(--ink);
    font-size: var(--type-caption);
    line-height: 1.55;
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .source-register {
    background: var(--paper);
    border: 1px solid var(--rule-strong);
  }

  .source-register > header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 20px;
    padding: 16px;
    color: var(--white);
    background: var(--marine);
  }

  .source-register h4,
  .source-register p,
  .source-register h4 {
    margin-top: 4px;
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .source-register header > span {
    color: var(--nav-muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .source-rows {
    display: grid;
    grid-template-columns: 1fr 1fr;
  }

  .source-rows > div {
    display: grid;
    gap: 4px;
    padding: 16px;
    border-inline-end: 1px solid var(--rule);
  }

  .source-rows > div:last-child {
    border-inline-end: 0;
  }

  .source-rows strong,
  .source-rows span,
  .scope-note {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.45;
  }

  .scope-note {
    margin: 0;
  }

  @media (max-width: 720px) {
    .source-rows,
    .source-rows > div {
      border-inline-end: 0;
      border-bottom: 1px solid var(--rule);
    }

    .source-rows > div:last-child {
      border-bottom: 0;
    }
  }
</style>
