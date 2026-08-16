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

  import type { LocationMapPoint, RadarLayer, UnitSystem, VesselTrack } from '$lib/types';

  let {
    points = [],
    vesselTracks = [],
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
    if (point.enabled === false) element.classList.add('is-disabled');
    if (point.id === selectedId || point.id === candidate?.id) element.classList.add('is-selected');
    element.setAttribute('aria-label', `${point.label}. Select location.`);
    element.title = point.detail ? `${point.label} · ${point.detail}` : point.label;
    if (point.courseDegrees != null) element.style.setProperty('--course', `${point.courseDegrees}deg`);
    const dot = document.createElement('span');
    dot.setAttribute('aria-hidden', 'true');
    element.append(dot);
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
        anchor: 'bottom',
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
    void markerSignature;
    void trackSignature;
    void selectedId;
    if (loaded) queueMicrotask(() => {
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

  <div class="map-registration" aria-hidden="true">
    <span><Globe2 size={15} strokeWidth={1.6} /> Global coverage</span>
    <strong>{activePoint?.label ?? 'Drag anywhere on Earth'}</strong>
    <small>
      {activePoint
        ? `${activePoint.latitude.toFixed(4)} · ${activePoint.longitude.toFixed(4)}`
        : 'Search, pan, zoom, then click to set a precise point'}
    </small>
  </div>

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

  .map-registration,
  .map-instruction,
  .map-radar-toggle,
  .map-loading,
  .map-fallback {
    position: absolute;
    z-index: 2;
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

  .map-registration {
    top: 22px;
    left: 22px;
    display: grid;
    width: min(330px, calc(100% - 96px));
    gap: 5px;
    padding: 15px 17px 16px;
    color: var(--white);
    background: var(--marine);
    border: 1px solid var(--nav-subdued);
    box-shadow: var(--strip-shadow);
  }

  .map-registration > span {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--nav-muted);
    font-family: var(--font-instrument);
    font-size: var(--type-micro);
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .map-registration strong {
    overflow-wrap: anywhere;
    font-family: var(--font-instrument);
    font-size: var(--type-title);
    line-height: 1;
    text-transform: uppercase;
  }

  .map-registration small {
    color: var(--nav-muted);
    font-size: var(--type-caption);
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

  :global(.location-map-pin) {
    position: relative;
    display: grid;
    width: 30px;
    height: 38px;
    place-items: center;
    padding: 0;
    color: var(--white);
    background: var(--marine);
    border: 2px solid var(--white);
    border-radius: 50% 50% 50% 4px;
    box-shadow: 0 3px 0 rgba(17, 20, 24, 0.28), 0 8px 20px rgba(17, 20, 24, 0.24);
    cursor: pointer;
    transform: rotate(-45deg);
    transition: transform 140ms ease-out, background-color 140ms ease-out;
  }

  :global(.location-map-pin > span) {
    width: 8px;
    height: 8px;
    background: currentColor;
    border-radius: 50%;
  }

  :global(.location-map-pin:hover),
  :global(.location-map-pin:focus-visible),
  :global(.location-map-pin.is-selected) {
    color: var(--graphite);
    background: var(--amber);
    border-color: var(--graphite);
    transform: rotate(-45deg) scale(1.14);
  }

  :global(.location-map-pin--candidate),
  :global(.location-map-pin--bridge) {
    color: var(--graphite);
    background: var(--amber);
    border-color: var(--graphite);
  }

  :global(.location-map-pin--vessel) {
    width: 28px;
    height: 28px;
    color: var(--white);
    background: var(--channel);
    border-color: var(--white);
    border-radius: 50%;
    transform: none;
  }

  :global(.location-map-pin--vessel > span) {
    width: 10px;
    height: 12px;
    background: currentColor;
    border: 0;
    border-radius: 0;
    clip-path: polygon(50% 0, 100% 100%, 50% 76%, 0 100%);
    transform: rotate(var(--course, 0deg));
  }

  :global(.location-map-pin--vessel:hover),
  :global(.location-map-pin--vessel:focus-visible),
  :global(.location-map-pin--vessel.is-selected) {
    transform: scale(1.14);
  }

  :global(.location-map-pin.is-disabled) {
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

    .map-registration {
      top: 14px;
      left: 14px;
      width: calc(100% - 76px);
    }

    .map-instruction {
      right: 9px;
      bottom: 32px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.location-map-pin) {
      transition: none;
    }
  }
</style>
