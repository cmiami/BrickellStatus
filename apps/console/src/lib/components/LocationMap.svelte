<script lang="ts">
  import 'maplibre-gl/dist/maplibre-gl.css';
  // MapLibre 6 derives its worker URL from `import.meta.url` and bails out to an
  // empty string unless that URL is http(s). The packaged app is served from
  // `tauri://localhost`, so the built-in lookup yields no worker: the style and
  // background layer still paint, but nothing parses vector tiles and the map
  // renders empty. Hand MapLibre a Vite-bundled worker URL instead.
  import maplibreWorkerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url';
  import { onMount } from 'svelte';
  import { CloudRain, Crosshair, Globe2, MapPin, MousePointer2 } from '@lucide/svelte';
  import type { GeoJSONSource, Map as MapLibreMap, Marker as MapLibreMarker, ScaleControl } from 'maplibre-gl';

  import { corridorFeatureCollection } from '$lib/river';
  import type {
    LocationMapPoint,
    RadarLayer,
    RiverCorridor,
    UnitSystem,
    VesselTrack
  } from '$lib/types';

  let {
    points = [],
    vesselTracks = [],
    corridor = null,
    radar = null,
    candidate = null,
    selectedId = null,
    unitSystem = 'imperial',
    variant = 'hero',
    ariaLabel = 'Interactive global location map',
    onselect = () => {},
    onpick = () => {}
  }: {
    points?: LocationMapPoint[];
    vesselTracks?: VesselTrack[];
    corridor?: RiverCorridor | null;
    radar?: RadarLayer | null;
    candidate?: LocationMapPoint | null;
    selectedId?: string | null;
    unitSystem?: UnitSystem;
    variant?: 'hero' | 'compact';
    ariaLabel?: string;
    onselect?: (point: LocationMapPoint) => void;
    onpick?: (latitude: number, longitude: number) => void;
  } = $props();

  let mapHost: HTMLDivElement;
  let map: MapLibreMap | null = null;
  let maplibreModule: typeof import('maplibre-gl') | null = null;
  let markers: MapLibreMarker[] = [];
  let scaleControl: ScaleControl | null = null;
  let loaded = $state(false);
  let radarVisible = $state(true);
  let failed = $state<string | null>(null);
  let prefersReducedMotion = false;
  let lastCameraTarget = '';
  let initialFramed = false;

  const radarAge = $derived.by(() => {
    if (!radar) return '';
    const minutes = Math.round(radar.ageSeconds / 60);
    return minutes < 1 ? 'just now' : `${minutes} min ago`;
  });

  const allPoints = $derived(candidate ? [...points.filter((point) => point.id !== candidate.id), candidate] : points);
  const activePoint = $derived(allPoints.find((point) => point.id === selectedId) ?? candidate ?? null);

  function markerElement(point: LocationMapPoint): HTMLButtonElement {
    const element = document.createElement('button');
    element.type = 'button';
    element.className = `location-map-pin location-map-pin--${point.kind ?? 'saved'}`;
    // Also inline, not only in the stylesheet. MapLibre measures this element to
    // work out how far to shift it for the requested anchor, and it measures at
    // the moment the marker is added. An element measured before its CSS applies
    // measures 0x0, so the anchor shift is zero and every mark sits a fixed
    // number of screen pixels from its coordinate -- which does not look like a
    // constant offset, it looks like the marks sliding around as you zoom.
    // MapLibre also requires every marker root to be absolute. Letting this
    // button fall back into normal flow adds its place in the horizontal row to
    // the projected coordinate, which lines otherwise unrelated vessels up
    // east of their real fixes.
    element.style.position = 'absolute';
    element.style.width = '34px';
    element.style.height = '34px';
    if (point.enabled === false) element.classList.add('is-disabled');
    if (point.id === selectedId || point.id === candidate?.id) element.classList.add('is-selected');
    element.setAttribute(
      'aria-label',
      `${point.label}. Select ${point.kind === 'vessel' ? 'vessel' : 'location'}.`
    );
    element.title = point.detail ? `${point.label} · ${point.detail}` : point.label;
    if (point.courseDegrees != null) element.style.setProperty('--course', `${point.courseDegrees}deg`);
    // MapLibre writes an inline transform on the root every camera frame, so
    // the pin's own shape and hover motion live one level in; anything
    // transformed or transitioned on the root fights the map for position.
    const body = document.createElement('span');
    body.className = 'location-map-pin__body';
    body.setAttribute('aria-hidden', 'true');
    const dot = document.createElement('span');
    dot.className = 'location-map-pin__dot';
    body.append(dot);
    element.append(body);
    element.addEventListener('click', (event) => {
      event.stopPropagation();
      onselect(point);
    });
    return element;
  }

  function clearMarkers() {
    for (const marker of markers) marker.remove();
    markers = [];
  }

  function syncMarkers() {
    if (!map || !loaded || !maplibreModule) return;
    clearMarkers();
    for (const point of allPoints) {
      const marker = new maplibreModule.Marker({
        element: markerElement(point),
        // Anchor the part of the mark that actually claims the coordinate. A
        // vessel is a disc centred on its fix; a pin is a point that touches
        // the ground at its tip. Anchoring both at 'bottom' drew every vessel
        // half its own height north of where it was, and because the error is
        // a fixed number of screen pixels it reads as the mark wandering as
        // you zoom.
        anchor: point.kind === 'vessel' ? 'center' : 'bottom',
        draggable: point.draggable === true
      }).setLngLat([point.longitude, point.latitude]);

      if (point.draggable) {
        marker.on('dragend', () => {
          const coordinate = marker.getLngLat();
          onpick(coordinate.lat, coordinate.lng);
        });
      }
      marker.addTo(map);
      markers.push(marker);
    }

    const cameraPoint = activePoint;
    if (cameraPoint) {
      const cameraTarget = `${cameraPoint.id}:${cameraPoint.latitude.toFixed(5)}:${cameraPoint.longitude.toFixed(5)}`;
      if (cameraTarget !== lastCameraTarget) {
        lastCameraTarget = cameraTarget;
        map.flyTo({
          center: [cameraPoint.longitude, cameraPoint.latitude],
          zoom: Math.max(map.getZoom(), candidate || cameraPoint.kind === 'bridge' || cameraPoint.kind === 'vessel' ? 11.4 : 8.5),
          duration: initialFramed && !prefersReducedMotion ? 900 : 0,
          essential: false
        });
      }
    } else if (!initialFramed && allPoints.length > 1) {
      const bounds = new maplibreModule.LngLatBounds();
      for (const point of allPoints) bounds.extend([point.longitude, point.latitude]);
      map.fitBounds(bounds, { padding: 70, maxZoom: 8, duration: 0 });
    }
    initialFramed = true;
  }

  // The tracked corridor: the water AIS is actually subscribed to, drawn from
  // the engine's own published geometry. It sits at the very bottom of the
  // stack — it is the ground the rest of the AIS evidence stands on, and it
  // must never obscure a vessel, a storm cell, or a marker.
  function syncCorridor() {
    if (!map || !loaded || !map.isStyleLoaded()) return;
    const data = corridor
      ? corridorFeatureCollection(corridor)
      : { type: 'FeatureCollection' as const, features: [] };
    const source = map.getSource('ais-corridor') as GeoJSONSource | undefined;
    if (source) {
      source.setData(data);
      return;
    }
    if (!data.features.length) return;
    map.addSource('ais-corridor', { type: 'geojson', data });
    map.addLayer({
      id: 'ais-corridor-area',
      type: 'fill',
      source: 'ais-corridor',
      paint: { 'fill-color': '#6e4fa3', 'fill-opacity': 0.14 }
    });
    map.addLayer({
      id: 'ais-corridor-edge',
      type: 'line',
      source: 'ais-corridor',
      paint: {
        'line-color': '#5b3e8c',
        'line-width': 1.25,
        'line-opacity': 0.62
      }
    });
  }

  function syncVesselTracks() {
    if (!map || !loaded || !map.isStyleLoaded()) return;
    const data = {
      type: 'FeatureCollection' as const,
      features: vesselTracks
        .filter((track) => track.points.length > 1)
        .map((track) => ({
          type: 'Feature' as const,
          properties: { mmsi: track.mmsi, movement: track.movement },
          geometry: {
            type: 'LineString' as const,
            coordinates: track.points.map((point) => [point.longitude, point.latitude])
          }
        }))
    };
    const source = map.getSource('ais-vessel-tracks') as GeoJSONSource | undefined;
    if (source) {
      source.setData(data);
      return;
    }
    map.addSource('ais-vessel-tracks', { type: 'geojson', data });
    map.addLayer({
      id: 'ais-vessel-courses',
      type: 'line',
      source: 'ais-vessel-tracks',
      paint: {
        'line-color': [
          'match',
          ['get', 'movement'],
          'approaching', '#765300',
          'diverging', '#5a6b7c',
          'stationary', '#46515b',
          '#174f78'
        ],
        'line-width': 3,
        'line-opacity': 0.82
      }
    });
  }

  // Radar sits under the vessel courses so a track is never lost in a storm
  // cell, and under every marker for the same reason. Rebuilt rather than
  // mutated when the frame changes: a raster source's tile URL is fixed at
  // construction, and each frame is a new URL.
  function syncRadar() {
    if (!map || !loaded || !map.isStyleLoaded()) return;
    if (map.getLayer('rainviewer-radar')) map.removeLayer('rainviewer-radar');
    if (map.getSource('rainviewer-radar')) map.removeSource('rainviewer-radar');
    if (!radar || !radarVisible) return;
    map.addSource('rainviewer-radar', {
      type: 'raster',
      tiles: [radar.tileUrlTemplate],
      tileSize: 512,
      // The deepest zoom RainViewer documents. Declaring it makes MapLibre
      // overzoom that tile rather than request deeper ones, so the overlay
      // survives the ceiling being enforced instead of disappearing.
      maxzoom: radar.maxZoom,
      attribution: radar.attribution
    });
    map.addLayer(
      {
        id: 'rainviewer-radar',
        type: 'raster',
        source: 'rainviewer-radar',
        paint: { 'raster-opacity': 0.62 }
      },
      map.getLayer('ais-vessel-courses') ? 'ais-vessel-courses' : undefined
    );
  }

  $effect(() => {
    const radarSignature = `${radar?.tileUrlTemplate ?? ''}:${radarVisible}`;
    void radarSignature;
    if (loaded) queueMicrotask(syncRadar);
  });

  $effect(() => {
    const markerSignature = allPoints
      .map((point) => `${point.id}:${point.latitude}:${point.longitude}:${point.enabled}:${point.draggable}`)
      .join('|');
    const trackSignature = vesselTracks
      .map((track) => `${track.mmsi}:${track.observedAt}:${track.points.length}:${track.movement}`)
      .join('|');
    const corridorSignature = corridor?.branches
      .map((branch) => `${branch.id}:${branch.centerline.length}:${branch.corridorOffsetMeters}`)
      .join('|');
    void markerSignature;
    void trackSignature;
    void corridorSignature;
    void selectedId;
    if (loaded) queueMicrotask(() => {
      syncCorridor();
      syncMarkers();
      syncVesselTracks();
      syncRadar();
    });
  });

  $effect(() => {
    scaleControl?.setUnit(unitSystem);
  });

  onMount(() => {
    let disposed = false;
    let resizeObserver: ResizeObserver | null = null;
    let loadTimer: number | undefined;
    const motionPreference = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    const updateMotionPreference = () => {
      prefersReducedMotion = motionPreference?.matches ?? false;
    };
    updateMotionPreference();
    motionPreference?.addEventListener?.('change', updateMotionPreference);

    void import('maplibre-gl')
      .then((maplibre) => {
        if (disposed) return;
        maplibreModule = maplibre;
        maplibre.setWorkerUrl(maplibreWorkerUrl);
        map = new maplibre.Map({
          container: mapHost,
          style: 'https://tiles.openfreemap.org/styles/liberty',
          center: activePoint ? [activePoint.longitude, activePoint.latitude] : [-28, 22],
          zoom: activePoint ? (variant === 'compact' ? 10.2 : 9) : 1.45,
          minZoom: 1,
          maxZoom: 18,
          pitch: 0,
          bearing: 0,
          canvasContextAttributes: { antialias: true },
          attributionControl: false,
          cooperativeGestures: true,
          maxTileCacheSize: 72
        });
        map.addControl(new maplibre.NavigationControl({ visualizePitch: true, showCompass: true }), 'top-right');
        scaleControl = new maplibre.ScaleControl({ unit: unitSystem, maxWidth: 90 });
        map.addControl(scaleControl, 'bottom-left');
        map.addControl(new maplibre.AttributionControl({ compact: true }), 'bottom-right');
        const finishLoading = () => {
          if (!map || disposed) return;
          failed = null;
          loaded = true;
          if (loadTimer) window.clearTimeout(loadTimer);
          syncCorridor();
          syncMarkers();
          syncVesselTracks();
        };
        map.once('load', finishLoading);
        map.on('click', (event) => onpick(event.lngLat.lat, event.lngLat.lng));
        loadTimer = window.setTimeout(() => {
          if (!loaded) failed = 'The map style did not load. Search and saved coordinates remain available.';
        }, 12_000);

        resizeObserver = new ResizeObserver(() => map?.resize());
        resizeObserver.observe(mapHost);
      })
      .catch(() => {
        failed = 'The live map could not be loaded. Search and saved coordinates remain available.';
      });

    return () => {
      disposed = true;
      motionPreference?.removeEventListener?.('change', updateMotionPreference);
      if (loadTimer) window.clearTimeout(loadTimer);
      resizeObserver?.disconnect();
      clearMarkers();
      map?.remove();
      map = null;
      maplibreModule = null;
      scaleControl = null;
    };
  });
</script>

<div class:compact={variant === 'compact'} class="location-map-shell">
  <div bind:this={mapHost} class="location-map" role="application" aria-label={ariaLabel}></div>

  {#if radar}
    <button
      type="button"
      class="map-radar-toggle"
      class:is-on={radarVisible}
      aria-pressed={radarVisible}
      onclick={() => (radarVisible = !radarVisible)}
    >
      <CloudRain size={15} strokeWidth={1.7} aria-hidden="true" />
      <span>Radar</span>
      <!-- Radar composites are minutes old by the time they publish. Saying so
           is the difference between an overlay and a claim. -->
      <small>{radarAge}</small>
    </button>
  {/if}

  <!-- A wash of colour over water is a claim about what is being watched. It
       gets a written key, so the shape is never left to be guessed at. -->
  {#if corridor}
    <div class="map-corridor-key">
      <span class="corridor-swatch" aria-hidden="true"></span>
      <span>
        <strong>Tracked AIS corridor</strong>
        <small>Vessel positions are collected inside this water only</small>
      </span>
    </div>
  {/if}

  <div class="map-instruction">
    {#if candidate?.draggable}
      <MapPin size={16} strokeWidth={1.6} aria-hidden="true" /> Drag the amber pin to tune
    {:else}
      <MousePointer2 size={16} strokeWidth={1.6} aria-hidden="true" /> Click the map to place a pin
    {/if}
  </div>

  {#if !loaded && !failed}
    <div class="map-loading" aria-live="polite">
      <Crosshair size={23} strokeWidth={1.4} aria-hidden="true" />
      <span><strong>Loading map</strong><small>Requesting the live street layer…</small></span>
    </div>
  {/if}

  {#if failed}
    <div class="map-fallback" role="status">
      <Globe2 size={26} strokeWidth={1.35} aria-hidden="true" />
      <strong>Map offline</strong>
      <span>{failed}</span>
    </div>
  {/if}
</div>

<style>
  .location-map-shell {
    position: relative;
    min-height: clamp(430px, 59vh, 680px);
    overflow: hidden;
    color: var(--white);
    background: var(--marine);
    border: 1px solid var(--marine);
    isolation: isolate;
  }

  .location-map-shell.compact {
    min-height: 360px;
  }

  .location-map {
    position: absolute;
    inset: 0;
  }

  .location-map::after {
    position: absolute;
    inset: 0;
    z-index: 1;
    border: 8px solid rgba(15, 42, 68, 0.08);
    pointer-events: none;
    content: '';
  }

  .map-instruction,
  .map-corridor-key,
  .map-radar-toggle,
  .map-loading,
  .map-fallback {
    position: absolute;
    z-index: 2;
  }

  /* Keyed under the registration block, in the same marine sheet, so the two
     read as one column of annotation rather than competing overlays. */
  .map-corridor-key {
    top: 132px;
    left: 22px;
    display: flex;
    align-items: flex-start;
    gap: 9px;
    width: min(330px, calc(100% - 96px));
    padding: 10px 13px 11px;
    color: var(--white);
    background: var(--marine);
    border: 1px solid var(--nav-subdued);
    box-shadow: var(--strip-shadow);
    pointer-events: none;
  }

  .corridor-swatch {
    flex: none;
    width: 15px;
    height: 15px;
    margin-top: 1px;
    background: var(--corridor);
    border: 1px solid var(--corridor-sheet);
  }

  .map-corridor-key strong {
    display: block;
    font-family: var(--font-instrument);
    font-size: var(--type-label);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .map-corridor-key small {
    display: block;
    color: var(--nav-muted);
    font-size: var(--type-caption);
    line-height: 1.35;
  }

  .map-radar-toggle {
    top: 22px;
    right: 22px;
    display: flex;
    gap: 8px;
    align-items: baseline;
    padding: 9px 13px;
    font: inherit;
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--white);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    cursor: pointer;
    background: var(--marine);
    border: 1px solid var(--nav-subdued);
    box-shadow: var(--strip-shadow);
  }

  .map-radar-toggle:not(.is-on) {
    opacity: 0.55;
  }

  .map-radar-toggle small {
    font-weight: 500;
    text-transform: none;
    opacity: 0.72;
  }





  .map-instruction {
    right: 15px;
    bottom: 38px;
    display: flex;
    align-items: center;
    gap: 7px;
    max-width: calc(100% - 30px);
    padding: 8px 10px;
    color: var(--graphite);
    background: var(--amber);
    border: 1px solid var(--amber-ink);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 700;
    letter-spacing: 0.055em;
    text-transform: uppercase;
    box-shadow: var(--strip-shadow);
    pointer-events: none;
  }

  .map-loading,
  .map-fallback {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 12px 14px;
    color: var(--white);
    background: rgba(15, 42, 68, 0.96);
  }

  .map-loading {
    right: 14px;
    bottom: 38px;
    border: 1px solid var(--nav-subdued);
  }

  .map-fallback {
    inset: 0;
    padding: 24px;
  }

  .map-loading span,
  .map-fallback {
    text-align: center;
  }

  .map-loading span {
    display: grid;
    gap: 3px;
    text-align: left;
  }

  .map-loading strong,
  .map-fallback strong {
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    text-transform: uppercase;
  }

  .map-loading small,
  .map-fallback span {
    color: var(--nav-muted);
    font-size: var(--type-caption);
  }

  /* The root carries only its footprint: MapLibre owns its transform and
     rewrites it on every camera frame, so a transform or a transition here
     makes the pin chase the map instead of holding its coordinate. */
  /* The pin is a 24px square with one sharp corner, turned 45 degrees so that
     corner points straight down. Rotating about the corner itself (rather than
     the square's middle) means the tip is the one part of the mark that never
     moves — not when the square is placed, and not when it grows on hover.

     The geometry is exact rather than eyeballed: a 24px square measures 24√2 ≈
     33.9px across its diagonal, so a 34px box holds it with the tip landing on
     the bottom edge, dead centre. `anchor: 'bottom'` then puts that tip on the
     coordinate. Change the square and this box has to change with it. */
  :global(.location-map-pin) {
    /* Preserve MapLibre's positioning contract. The root itself must never
       participate in layout; its transform is the projected longitude and
       latitude. */
    position: absolute;
    width: 34px;
    height: 34px;
    padding: 0;
    background: none;
    border: 0;
    cursor: pointer;
  }

  :global(.location-map-pin__body) {
    position: absolute;
    /* Places the square's sharp corner on the box's bottom centre: 17 - 24 to
       the left, 34 - 24 down. */
    inset: 10px auto auto -7px;
    display: grid;
    width: 24px;
    height: 24px;
    place-items: center;
    color: var(--white);
    background: var(--marine);
    border: 2px solid var(--white);
    border-radius: 50% 50% 4px 50%;
    box-shadow: 0 3px 0 rgba(17, 20, 24, 0.28), 0 8px 20px rgba(17, 20, 24, 0.24);
    transform: rotate(45deg);
    transform-origin: 100% 100%;
    transition: transform 140ms ease-out, background-color 140ms ease-out,
      border-color 140ms ease-out, color 140ms ease-out;
  }

  :global(.location-map-pin__dot) {
    width: 8px;
    height: 8px;
    background: currentColor;
    border-radius: 50%;
    /* Undoes the head's turn, so the mark inside stays upright. */
    transform: rotate(-45deg);
  }

  :global(.location-map-pin:hover) :global(.location-map-pin__body),
  :global(.location-map-pin:focus-visible) :global(.location-map-pin__body),
  :global(.location-map-pin.is-selected) :global(.location-map-pin__body) {
    color: var(--graphite);
    background: var(--amber);
    border-color: var(--graphite);
    transform: rotate(45deg) scale(1.14);
  }

  :global(.location-map-pin--candidate) :global(.location-map-pin__body),
  :global(.location-map-pin--bridge) :global(.location-map-pin__body) {
    color: var(--graphite);
    background: var(--amber);
    border-color: var(--graphite);
  }

  /* A vessel is a disc centred on its fix, not a pin standing on it, so it
     keeps its own square box and no 45-degree turn. It is anchored 'center' in
     syncMarkers to match. */
  :global(.location-map-pin--vessel) {
    width: 28px;
    height: 28px;
  }

  :global(.location-map-pin--vessel) :global(.location-map-pin__body) {
    inset: 0;
    width: 100%;
    height: 100%;
    color: var(--white);
    background: var(--channel);
    border-color: var(--white);
    border-radius: 50%;
    transform: none;
    transform-origin: 50% 50%;
  }

  :global(.location-map-pin--vessel:hover) :global(.location-map-pin__body),
  :global(.location-map-pin--vessel:focus-visible) :global(.location-map-pin__body),
  :global(.location-map-pin--vessel.is-selected) :global(.location-map-pin__body) {
    transform: scale(1.14);
  }

  :global(.location-map-pin--vessel) :global(.location-map-pin__dot) {
    width: 10px;
    height: 12px;
    background: currentColor;
    border: 0;
    border-radius: 0;
    clip-path: polygon(50% 0, 100% 100%, 50% 76%, 0 100%);
    transform: rotate(var(--course, 0deg));
  }

  :global(.location-map-pin.is-disabled) :global(.location-map-pin__body) {
    color: var(--marine);
    background: var(--paper);
    border-color: var(--marine);
    opacity: 0.68;
  }

  :global(.location-map-shell .maplibregl-ctrl-group) {
    overflow: hidden;
    background: var(--frost);
    border: 1px solid var(--marine);
    border-radius: 0;
    box-shadow: var(--strip-shadow);
  }

  :global(.location-map-shell .maplibregl-ctrl-group button) {
    width: 34px;
    height: 34px;
    border-bottom-color: var(--rule);
  }

  :global(.location-map-shell .maplibregl-ctrl-attrib) {
    color: var(--muted);
    background: rgba(244, 247, 249, 0.94);
    font-size: var(--type-micro);
  }

  :global(.location-map-shell .maplibregl-ctrl-scale) {
    color: var(--graphite);
    background: rgba(244, 247, 249, 0.88);
    border-color: var(--marine);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
  }

  @media (max-width: 680px) {
    .location-map-shell,
    .location-map-shell.compact {
      min-height: 440px;
    }


    .map-corridor-key {
      top: 118px;
      left: 14px;
      width: calc(100% - 76px);
    }

    .map-instruction {
      right: 9px;
      bottom: 32px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.location-map-pin__body) {
      transition: none;
    }
  }
</style>
