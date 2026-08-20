<script lang="ts">
  import { onMount } from 'svelte';
  import {
    ArrowLeft,
    Crosshair,
    LocateFixed,
    MapPinned,
    Move,
    Network,
    Plus,
    Search,
    Trash2
  } from '@lucide/svelte';

  import LocationMap from '$lib/components/LocationMap.svelte';
  import PinModal from '$lib/components/PinModal.svelte';
  import SwitchField from '$lib/components/SwitchField.svelte';
  import { getDeviceLocation, getRadarLayer, searchLocations } from '$lib/api';
  import { notice, persistPreferences, preferences, saving, snapshot } from '$lib/state';
  import { formatSpeedKnots } from '$lib/units';
  import type {
    AlertArea,
    AppPreferences,
    LocationMapPoint,
    LocationSearchResult,
    RadarLayer,
    VesselTrack
  } from '$lib/types';

  let draft = $state<AppPreferences | null>(null);
  let initialized = $state(false);
  let locating = $state(false);
  let candidate = $state<AlertArea | null>(null);
  let candidateEditingId = $state<string | null>(null);
  let selectedAreaId = $state<string | null>(null);
  let selectedVesselId = $state<string | null>(null);
  let radar = $state<RadarLayer | null>(null);
  let pinSaving = $state(false);
  let pinError = $state<string | null>(null);

  // RainViewer publishes a new composite about every ten minutes; the backend
  // caches within that, so this interval costs nothing when nothing has changed.
  $effect(() => {
    let disposed = false;
    const refresh = () => {
      void getRadarLayer()
        .then((layer) => {
          if (!disposed) radar = layer;
        })
        .catch(() => {
          if (!disposed) radar = null;
        });
    };
    refresh();
    const timer = window.setInterval(refresh, 5 * 60 * 1000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  });

  $effect(() => {
    if ($preferences && !initialized) {
      draft = structuredClone($preferences);
      selectedAreaId = draft.areas.find((area) => area.enabled)?.id ?? draft.areas[0]?.id ?? null;
      initialized = true;
    }
  });

  const activeAreaCount = $derived(draft?.areas.filter((area) => area.enabled).length ?? 0);
  const bridgePoint = $derived.by<LocationMapPoint | null>(() => {
    const scope = draft?.profile.channels.find((channel) => channel.kind === 'bridge')?.scope;
    if (!scope) return null;
    let latitude = typeof scope.latitude === 'number' ? scope.latitude : Number.NaN;
    let longitude = typeof scope.longitude === 'number' ? scope.longitude : Number.NaN;
    if ((!Number.isFinite(latitude) || !Number.isFinite(longitude)) && typeof scope.point === 'string') {
      [latitude, longitude] = scope.point.split(',').map(Number);
    }
    if (!Number.isFinite(latitude) || !Number.isFinite(longitude)) return null;
    return {
      id: 'bridge.brickell.position',
      label: 'Brickell Avenue Bridge',
      latitude,
      longitude,
      detail: 'AISStream prediction target',
      kind: 'bridge',
      enabled: true
    };
  });
  const vesselTracks = $derived<VesselTrack[]>($snapshot?.vesselTracks ?? []);
  const corridor = $derived($snapshot?.riverCorridor ?? null);
  const mapPoints = $derived<LocationMapPoint[]>([
    ...(draft?.areas.map((area) => ({
      id: area.id,
      label: area.label,
      latitude: area.latitude,
      longitude: area.longitude,
      detail: area.adminArea ?? area.countryCode ?? area.timeZone,
      kind: 'saved' as const,
      enabled: area.enabled
    })) ?? []),
    ...(bridgePoint ? [bridgePoint] : []),
    ...vesselTracks.flatMap((track) => {
      const latest = track.points.at(-1);
      if (!latest) return [];
      return [{
        id: `vessel.${track.mmsi}`,
        label: track.vesselName ?? `Vessel ${track.mmsi}`,
        latitude: latest.latitude,
        longitude: latest.longitude,
        detail: `${track.movement} · ${formatSpeedKnots(track.speedKnots, draft?.unitSystem ?? 'imperial')} · ${track.points.length} course points`,
        kind: 'vessel' as const,
        enabled: true,
        courseDegrees: track.courseDegrees
      }];
    })
  ]);
  const candidatePoint = $derived<LocationMapPoint | null>(
    candidate
      ? {
          id: candidate.id,
          label: candidate.label || 'Pinned location',
          latitude: candidate.latitude,
          longitude: candidate.longitude,
          detail: candidate.timeZone,
          kind: 'candidate',
          enabled: true,
          draggable: true
        }
      : null
  );

  function areaId(): string {
    return `area.${crypto.randomUUID()}`;
  }

  function assignAreaToChannels(area: AlertArea) {
    if (!draft) return;
    const assignments: Array<[string, boolean]> = [
      ['weather.miami', area.enabled && area.weatherEnabled],
      ['official.miami', area.enabled && area.officialAlertsEnabled],
      ['hurricane.atlantic', area.enabled && area.tropicalContextEnabled]
    ];
    for (const [channelId, enabled] of assignments) {
      const channel = draft.profile.channels.find((item) => item.id === channelId);
      if (!channel) continue;
      const current = Array.isArray(channel.scope.areaIds)
        ? channel.scope.areaIds.filter((item): item is string => typeof item === 'string')
        : [];
      channel.scope.areaIds = enabled
        ? [...new Set([...current, area.id])]
        : current.filter((id) => id !== area.id);
    }
    draft.profile.preset = 'custom';
  }

  function resultToArea(result: LocationSearchResult, source: AlertArea['source']): AlertArea {
    return {
      id: `candidate.${crypto.randomUUID()}`,
      label: result.label,
      latitude: result.latitude,
      longitude: result.longitude,
      timeZone: result.timeZone,
      countryCode: result.countryCode,
      adminArea: result.adminArea,
      source,
      enabled: true,
      weatherEnabled: true,
      officialAlertsEnabled: result.countryCode === 'US',
      tropicalContextEnabled: false
    };
  }

  function stageResult(result: LocationSearchResult, source: AlertArea['source'] = 'search') {
    candidate = resultToArea(result, source);
    candidateEditingId = null;
    selectedAreaId = null;
  }

  function stagePinnedLocation(latitude: number, longitude: number) {
    if (candidate) {
      candidate.latitude = Number(latitude.toFixed(5));
      candidate.longitude = Number(longitude.toFixed(5));
      candidate.source = candidateEditingId ? candidate.source : 'manual';
      return;
    }
    candidate = resultToArea(
      {
        id: `pin:${latitude},${longitude}`,
        label: 'Pinned location',
        latitude: Number(latitude.toFixed(5)),
        longitude: Number(longitude.toFixed(5)),
        timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
      },
      'manual'
    );
    candidateEditingId = null;
    selectedAreaId = null;
  }

  function editOnMap(area: AlertArea) {
    candidate = { ...$state.snapshot(area), id: `candidate.${area.id}` };
    candidateEditingId = area.id;
    selectedAreaId = null;
  }

  function discardCandidate() {
    candidate = null;
    candidateEditingId = null;
    pinError = null;
  }

  // Saving the pin saves the place. Dropping a pin and then hunting for a
  // separate Save button elsewhere on the page is how a coordinate gets lost.
  async function commitCandidate(named: AlertArea) {
    if (!draft) return;
    pinError = null;

    const duplicate = draft.areas.find(
      (area) =>
        area.id !== candidateEditingId &&
        Math.abs(area.latitude - named.latitude) < 0.0001 &&
        Math.abs(area.longitude - named.longitude) < 0.0001
    );
    if (duplicate) {
      pinError = `${duplicate.label} already covers this point.`;
      return;
    }

    const committed: AlertArea = { ...named, id: candidateEditingId ?? areaId() };
    if (candidateEditingId) {
      draft.areas = draft.areas.map((area) => (area.id === candidateEditingId ? committed : area));
    } else {
      draft.areas = [...draft.areas, committed];
    }
    assignAreaToChannels(committed);
    selectedAreaId = committed.id;

    pinSaving = true;
    try {
      await persistPreferences($state.snapshot(draft));
    } finally {
      pinSaving = false;
    }
    candidate = null;
    candidateEditingId = null;
  }


  async function useDeviceLocation() {
    locating = true;
    try {
      const position = await getDeviceLocation();
      stageResult(
        {
          id: `device:${position.timestamp}`,
          label: 'This computer’s location',
          latitude: Number(position.coords.latitude.toFixed(5)),
          longitude: Number(position.coords.longitude.toFixed(5)),
          timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
        },
        'device'
      );
    } catch (error) {
      const message =
        error instanceof GeolocationPositionError
          ? error.code === error.PERMISSION_DENIED
            ? 'Location permission was denied. Search for a city or place a pin instead.'
            : error.message
          : error instanceof Error
            ? error.message
            : 'This computer could not provide a location.';
      notice.set({ ok: false, message });
    } finally {
      locating = false;
    }
  }

  function updateArea(area: AlertArea) {
    assignAreaToChannels(area);
  }

  function removeArea(area: AlertArea) {
    if (!draft) return;
    draft.areas = draft.areas.filter((item) => item.id !== area.id);
    for (const channel of draft.profile.channels) {
      if (!Array.isArray(channel.scope.areaIds)) continue;
      channel.scope.areaIds = channel.scope.areaIds.filter((id) => id !== area.id);
    }
    if (selectedAreaId === area.id) selectedAreaId = draft.areas[0]?.id ?? null;
    draft.profile.preset = 'custom';
    notice.set({ ok: true, message: `${area.label} removed from the draft. Save to apply; reload to undo.` });
  }

  async function save() {
    if (!draft) return;
    await persistPreferences($state.snapshot(draft));
  }

  // Edits apply as they are made, matching the outputs desk. A Save button asks
  // the reader to remember they have unfinished business, then rewards pressing
  // it with a banner to dismiss -- two chores in exchange for nothing they had
  // not already told the app.
  //
  // Long enough that a number being typed is one write rather than twenty.
  const SETTLE_MS = 700;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = undefined;
      void save();
    }, SETTLE_MS);
  }

  // Whatever is in flight goes with the reader; an edit must not die with the
  // page just because the debounce had not fired yet.
  onMount(() => () => {
    if (saveTimer) {
      clearTimeout(saveTimer);
      void save();
    }
  });

  const unsaved = $derived(
    !!draft && !!$preferences && JSON.stringify($state.snapshot(draft)) !== JSON.stringify($preferences)
  );

  $effect(() => {
    if (unsaved) scheduleSave();
  });
</script>

<svelte:head>
  <title>Map · BrickellStatus</title>
  <meta name="description" content="Inspect saved coverage, the bridge target, and live AIS vessel courses on one map." />
</svelte:head>

<section class="page-sheet map-page">
  <header class="page-heading-row">
    <div>
      <a class="back-link" href="/channels"><ArrowLeft size={16} aria-hidden="true" /> Back to channels</a>
      <h1 class="sheet-heading">Place what you watch</h1>
      <p class="sheet-intro">Saved coverage, the Brickell target, and the last hour of vessel courses.</p>
    </div>
    <!-- Says what is true right now, and asks for nothing. It is a status
         line, not a banner: nothing to dismiss and nothing to press. -->
  </header>

  {#if draft}
    <div class="map-workbench">
      {#if candidate}
        <PinModal
          area={candidate}
          editing={candidateEditingId !== null}
          saving={pinSaving}
          error={pinError}
          onsave={commitCandidate}
          oncancel={discardCandidate}
        />
      {/if}

      <LocationMap
        points={mapPoints}
        {vesselTracks}
        {corridor}
        {radar}
        candidate={candidatePoint}
        selectedId={candidate?.id ?? selectedVesselId ?? selectedAreaId}
        unitSystem={draft.unitSystem}
        ariaLabel="Map of the tracked AIS corridor, saved coverage areas, Brickell Avenue Bridge, and recent AISStream vessel courses."
        onselect={(point) => {
          if (point.id === candidate?.id) return;
          if (point.kind === 'vessel') {
            selectedVesselId = point.id;
            selectedAreaId = null;
          } else {
            selectedAreaId = point.id;
            selectedVesselId = null;
          }
          candidate = null;
          candidateEditingId = null;
        }}
        onpick={stagePinnedLocation}
      />
    </div>

    <section class="ais-map-register" aria-labelledby="ais-map-heading">
      <header>
        <div>
          <p class="registration-label">AISStream course history</p>
          <h2 id="ais-map-heading">Vessels received in the last hour</h2>
        </div>
        <strong>{vesselTracks.length.toString().padStart(2, '0')}</strong>
      </header>
      {#if vesselTracks.length}
        <div class="vessel-ledger">
          {#each vesselTracks as track (track.mmsi)}
            <button class:selected={selectedVesselId === `vessel.${track.mmsi}`} onclick={() => {
              selectedVesselId = `vessel.${track.mmsi}`;
              selectedAreaId = null;
            }}>
              <span><strong>{track.vesselName ?? `Vessel ${track.mmsi}`}</strong><small>MMSI {track.mmsi}</small></span>
              <span><strong>{track.movement}</strong><small>{track.routeIntersects ? 'Course intersects bridge approach' : 'No bridge-course intersection'}</small></span>
              <span><strong>{formatSpeedKnots(track.speedKnots, draft.unitSystem)}</strong><small>{track.courseDegrees.toFixed(0)}° course · {track.points.length} points</small></span>
              <time datetime={track.observedAt}>{new Date(track.observedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>
            </button>
          {/each}
        </div>
      {:else}
        {@const aisSource = $snapshot?.system.sources.find((source) => source.sourceId.startsWith('aisstream.'))}
        <p class="empty-vessels">
          {aisSource?.detail ?? 'AISStream has not reported a vessel position yet.'}
        </p>
      {/if}
    </section>

    <section class="area-roster" aria-labelledby="roster-heading">
      <header class="roster-heading">
        <div>
          <h2 id="roster-heading">Places you watch</h2>
          <span>Click anywhere on the map to add one. Each place decides what gets collected for it.</span>
        </div>
        <button class="location-action" onclick={useDeviceLocation} disabled={locating}>
          <LocateFixed size={18} strokeWidth={1.5} aria-hidden="true" />
          {locating ? 'Asking this computer' : 'Use where I am'}
        </button>
      </header>

      {#if draft.areas.length}
        <div class="area-ledger">
          {#each draft.areas as area, index (area.id)}
            <article class:disabled={!area.enabled} class:selected={selectedAreaId === area.id} class="area-row">
              <button class="area-selector" onclick={() => (selectedAreaId = area.id)} aria-label={`Show ${area.label} on map`}>
                <span>{(index + 1).toString().padStart(2, '0')}</span>
                <div>
                  <strong>{area.label}</strong>
                  <small>{area.adminArea ?? area.countryCode ?? 'Custom point'} · {area.timeZone}</small>
                </div>
              </button>

              <div class="area-switches">
                <SwitchField
                  checked={area.enabled}
                  label="Watch this place"
                  description="Off means nothing is collected for this place."
                  onchange={(enabled) => { area.enabled = enabled; updateArea(area); }}
                />
                <SwitchField
                  checked={area.weatherEnabled}
                  label="Weather & rain"
                  description="Forecast, rain probability, and wind thresholds."
                  onchange={(enabled) => { area.weatherEnabled = enabled; updateArea(area); }}
                />
                <SwitchField
                  checked={area.officialAlertsEnabled}
                  label="Official alerts"
                  description="Authoritative point alerts where supported."
                  onchange={(enabled) => { area.officialAlertsEnabled = enabled; updateArea(area); }}
                />
                <SwitchField
                  checked={area.tropicalContextEnabled}
                  label="Tropical context"
                  description="Relate storm changes to this location."
                  onchange={(enabled) => { area.tropicalContextEnabled = enabled; updateArea(area); }}
                />
              </div>

              <footer>
                <details>
                  <summary>Advanced coordinates</summary>
                  <div class="advanced-grid">
                    <label class="field">
                      <span>Latitude</span>
                      <input type="number" min="-90" max="90" step="0.00001" bind:value={area.latitude} />
                    </label>
                    <label class="field">
                      <span>Longitude</span>
                      <input type="number" min="-180" max="180" step="0.00001" bind:value={area.longitude} />
                    </label>
                    <label class="field">
                      <span>Time zone</span>
                      <input bind:value={area.timeZone} maxlength="80" />
                    </label>
                  </div>
                </details>
                <div class="row-actions">
                  <button onclick={() => editOnMap(area)}><Move size={16} aria-hidden="true" /> Tune pin</button>
                  <button class="remove-action" onclick={() => removeArea(area)}><Trash2 size={16} aria-hidden="true" /> Remove</button>
                </div>
              </footer>
            </article>
          {/each}
        </div>
      {:else}
        <div class="empty-roster">
          <MapPinned size={30} strokeWidth={1.35} aria-hidden="true" />
          <h3>The atlas is clean</h3>
          <p>Search, locate once, or click the map. Weather and point alerts remain explicitly unconfigured until an area is saved.</p>
        </div>
      {/if}
    </section>
  {:else}
    <div class="empty-sheet" aria-busy="true"><h2>Loading map settings</h2><p>Waiting for locally saved preferences.</p></div>
  {/if}
</section>

<style>
  .map-page {
    padding-inline: clamp(18px, 2.5vw, 36px);
  }

  /* The map is the instrument on this page, so the header yields to it: a
     section-scale title rather than the headline other sheets open with. */
  .map-page .sheet-heading {
    font-size: var(--type-section);
    letter-spacing: -0.01em;
  }

  .map-page .sheet-intro {
    max-width: 62ch;
    margin-top: 6px;
  }

  .map-page .back-link {
    margin-bottom: 10px;
  }

  .map-page .page-heading-row {
    margin-bottom: 18px;
  }  .back-link {
    gap: 6px;
    margin-bottom: 18px;
    color: var(--channel);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-decoration: none;
    text-transform: uppercase;
  }

  .back-link:hover {
    color: var(--graphite);
    text-decoration: underline;
    text-underline-offset: 4px;
  }  .map-workbench {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    align-items: stretch;
    border-block: 1px solid var(--marine);
  }  .location-action {
    display: grid;
    width: 100%;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 12px;
    color: var(--white);
    background: transparent;
    border-block: 1px solid rgba(255, 255, 255, 0.25);
    padding: 14px 4px;
    text-align: left;
    cursor: pointer;
  }  .area-roster {
    margin-top: clamp(28px, 3vw, 46px);
    border-top: 1px solid var(--rule-strong);
  }

  .ais-map-register {
    margin-top: 28px;
    border-block: 1px solid var(--rule-strong);
  }

  .ais-map-register > header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 20px;
    padding: 16px 0;
  }

  .ais-map-register h2 {
    margin: 5px 0 0;
    color: var(--marine);
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .ais-map-register > header > strong {
    color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-section);
  }

  .vessel-ledger {
    border-top: 1px solid var(--rule);
  }

  .vessel-ledger button {
    display: grid;
    width: 100%;
    min-height: 68px;
    grid-template-columns: minmax(180px, 1.1fr) minmax(210px, 1.2fr) minmax(180px, 0.9fr) auto;
    align-items: center;
    gap: 18px;
    color: var(--graphite);
    background: transparent;
    border-bottom: 1px solid var(--rule);
    padding: 12px 8px;
    text-align: left;
    cursor: pointer;
  }

  .vessel-ledger button:hover,
  .vessel-ledger button.selected {
    background: var(--amber-sheet);
  }

  .vessel-ledger span {
    display: grid;
    min-width: 0;
    gap: 3px;
  }

  .vessel-ledger strong {
    overflow-wrap: anywhere;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    text-transform: uppercase;
  }

  .vessel-ledger small,
  .vessel-ledger time,
  .empty-vessels {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.4;
  }

  .empty-vessels {
    margin: 0;
    border-top: 1px solid var(--rule);
    padding: 18px 8px;
  }

  .roster-heading {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 24px;
    padding: 18px 0;
    border-bottom: 1px solid var(--rule-strong);
  }

  .roster-heading h2 {
    margin: 5px 0 4px;
    color: var(--marine);
    font-size: var(--type-section);
    line-height: 1;
    text-transform: uppercase;
  }

  .roster-heading div > span {
    color: var(--muted);
    font-size: var(--type-body-small);
  }  .area-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    border-bottom: 1px solid var(--rule-strong);
  }

  .area-row.selected {
    box-shadow: inset 6px 0 0 var(--amber);
  }

  .area-row.disabled {
    background: var(--frost);
  }

  .area-selector {
    display: grid;
    grid-template-columns: auto 1fr;
    align-content: start;
    align-items: start;
    gap: 13px;
    color: var(--graphite);
    background: transparent;
    border-bottom: 1px solid var(--rule-strong);
    padding: 18px 24px;
    text-align: left;
    cursor: pointer;
  }

  .area-selector:hover {
    background: var(--amber-sheet);
  }

  .area-selector > span {
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-label);
  }

  .area-selector > div {
    display: grid;
    gap: 5px;
  }

  .area-selector strong {
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    line-height: 1;
    text-transform: uppercase;
  }

  .area-selector small {
    color: var(--muted);
    font-size: var(--type-caption);
    line-height: 1.35;
  }

  .area-switches {
    display: grid;
    grid-template-columns: repeat(4, minmax(145px, 1fr));
  }

  .area-switches > :global(*) {
    border-right: 1px solid var(--rule);
    padding: 24px 18px;
  }

  .area-switches > :global(*:last-child) {
    border-right: 0;
  }

  .area-row > footer {
    grid-column: 1 / -1;
    display: flex;
    min-height: 54px;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 8px 18px 8px 62px;
    background: var(--frost);
    border-top: 1px solid var(--rule);
  }

  .area-row details {
    min-width: min(560px, 65vw);
  }

  .area-row summary {
    width: fit-content;
    color: var(--muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .advanced-grid {
    display: grid;
    grid-template-columns: 1fr 1fr 1.4fr;
    gap: 10px;
    margin: 12px 0 6px;
  }

  .row-actions {
    display: flex;
    gap: 8px;
  }

  .row-actions button {
    gap: 6px;
    min-height: 36px;
    color: var(--channel);
    background: transparent;
    border: 1px solid var(--rule-strong);
    padding: 8px 11px;
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.055em;
    text-transform: uppercase;
    cursor: pointer;
  }

  .row-actions .remove-action {
    color: var(--danger);
  }

  .empty-roster {
    display: grid;
    min-height: 190px;
    place-items: center;
    align-content: center;
    gap: 8px;
    text-align: center;
    border-bottom: 1px solid var(--rule-strong);
  }

  .empty-roster h3 {
    margin: 5px 0 0;
    color: var(--marine);
    font-size: var(--type-title);
    text-transform: uppercase;
  }

  .empty-roster p {
    max-width: 58ch;
    color: var(--muted);
    font-size: var(--type-body-small);
    line-height: 1.5;
  }

  @media (max-width: 1180px) {  }

  @media (max-width: 1050px) {
    .map-workbench {
      grid-template-columns: 1fr;
    }  }

  @media (max-width: 860px) {

    .area-switches {
      grid-template-columns: 1fr 1fr;
    }

    .vessel-ledger button {
      grid-template-columns: 1fr 1fr;
    }

    .area-switches > :global(*) {
      border-bottom: 1px solid var(--rule);
    }
  }

  @media (max-width: 680px) {
    .area-switches,
    .advanced-grid {
      grid-template-columns: 1fr;
    }

    .area-switches > :global(*) {
      border-right: 0;
    }

    .area-row > footer {
      display: grid;
      padding: 14px;
    }

    .area-row details {
      min-width: 0;
    }

    .row-actions {
      justify-content: space-between;
    }

    .vessel-ledger button {
      grid-template-columns: 1fr;
      gap: 8px;
    }
  }</style>
