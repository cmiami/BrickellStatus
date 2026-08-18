<script lang="ts">
  import { onDestroy } from 'svelte';

  import { getEinkPreview } from '$lib/api';
  import {
    PANEL_GEOMETRY,
    type ChannelSnapshot,
    type DecisionSnapshot,
    type EvidenceStrip,
    type PanelModel
  } from '$lib/types';

  let {
    decision,
    channel,
    evidence = [],
    connected = false,
    transport = 'auto',
    panel = 'e213'
  }: {
    decision: DecisionSnapshot;
    channel?: ChannelSnapshot;
    evidence?: EvidenceStrip[];
    connected?: boolean;
    transport?: string;
    /** The board that answered. The bezel is that board's shape, not a
     * nominal one, so the preview is the size the frame really is. */
    panel?: PanelModel;
  } = $props();

  const geometry = $derived(PANEL_GEOMETRY[panel]);

  let previewUrl = $state<string | null>(null);
  let previewError = $state<string | null>(null);
  let loading = $state(true);
  let requestSequence = 0;

  const frameSubject = () => channel?.title ?? decision.subject;
  const frameState = () => channel?.signal?.headline ?? channel?.summary ?? decision.stateLabel;

  $effect(() => {
    const signature = `${channel?.id ?? decision.channelId}:${channel?.materialKey ?? decision.state}:${decision.confidenceBps ?? ''}:${evidence.length}`;
    void signature;
    const request = ++requestSequence;
    loading = true;
    previewError = null;
    void getEinkPreview(channel?.id ?? decision.channelId)
      .then((preview) => {
        if (request !== requestSequence) return;
        const nextUrl = URL.createObjectURL(new Blob([new Uint8Array(preview.png)], { type: 'image/png' }));
        if (previewUrl) URL.revokeObjectURL(previewUrl);
        previewUrl = nextUrl;
      })
      .catch((error) => {
        if (request !== requestSequence) return;
        previewError = error instanceof Error ? error.message : 'The exact e-paper frame could not be rendered.';
      })
      .finally(() => {
        if (request === requestSequence) loading = false;
      });
  });

  onDestroy(() => {
    requestSequence += 1;
    if (previewUrl) URL.revokeObjectURL(previewUrl);
  });
</script>

<figure class="preview-figure">
  <figcaption>
    <span>{geometry.width} × {geometry.height} device pixels</span>
    <span class="status-word" data-state={connected ? 'ready' : 'unconfigured'}>
      {connected ? transport : 'Preview'}
    </span>
  </figcaption>
  <div class="epaper-bezel">
    <div
      class="epaper-screen"
      style={`aspect-ratio: ${geometry.width} / ${geometry.height}`}
      role="img" aria-label={`Exact e-paper frame for ${frameSubject()}: ${frameState()}`}>
      {#if previewUrl}
        <img src={previewUrl} alt="" />
      {:else if loading}
        <span>Rendering current frame…</span>
      {:else}
        <span>{previewError ?? 'No current frame is available.'}</span>
      {/if}
    </div>
  </div>
  <p>This is the exact monochrome output sent to the panel, enlarged without smoothing.</p>
</figure>

<style>
  .preview-figure {
    margin: 0;
  }

  figcaption {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 10px;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-caption);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .epaper-bezel {
    width: 100%;
    padding: clamp(9px, 1.4vw, 14px);
    background: var(--device-frame);
    border-radius: 4px;
    box-shadow: 0 2px 1px rgba(17, 20, 24, 0.2), 0 12px 28px rgba(17, 20, 24, 0.13);
  }

  /* The panel's own proportions arrive as an inline custom property, because
     the two boards are not the same shape and a frame drawn for one must not be
     stretched into the other's box. */
  .epaper-screen {
    display: grid;
    width: 100%;
    place-items: center;
    overflow: hidden;
    color: var(--graphite);
    background: var(--eink-paper);
    border: 1px solid var(--steel);
  }

  .epaper-screen img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: fill;
    image-rendering: pixelated;
  }

  .epaper-screen span {
    max-width: 34ch;
    padding: 18px;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.45;
    text-align: center;
  }

  .preview-figure > p {
    margin: 9px 0 0;
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }
</style>
