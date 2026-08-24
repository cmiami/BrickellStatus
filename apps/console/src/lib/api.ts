import { browser } from '$app/environment';
import type {
  AppPreferences,
  AppSnapshot,
  AisStreamStatus,
  DisplayConnectionStatus,
  DisplayDeviceCandidate,
  EinkPreview,
  LocationSearchResult,
  MutationResult,
  PlatformCapabilities,
  RadarLayer,
  VesselDetail
} from './types';

const LOCATION_SEARCH_TIMEOUT_MS = 12_000;
const LOCATION_SEARCH_MAX_BYTES = 256 * 1024;
const LOCATION_SEARCH_MAX_RESULTS = 8;
const LOCATION_SEARCH_MAX_QUERY_CHARS = 160;

function tauriAvailable(): boolean {
  return browser && Boolean(window.__TAURI_INTERNALS__);
}

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
  return tauriInvoke<T>(command, args);
}

function desktopInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!tauriAvailable()) {
    return Promise.reject(
      new Error('The live BrickellStatus backend is unavailable. Open the desktop application.')
    );
  }
  return invoke<T>(command, args);
}

// The current radar frame, or null when RainViewer has nothing recent.
//
// Resolves to null outside the desktop shell rather than throwing: radar is a
// decoration, and a browser preview losing it is not an error worth surfacing.
export async function getRadarLayer(): Promise<RadarLayer | null> {
  if (!tauriAvailable()) return null;
  return invoke<RadarLayer | null>('get_radar_layer');
}

// What this build can physically reach. Outside the app shell there is no
// hardware at all, so the browser preview claims the desktop capabilities and
// lets the individual calls fail on their own terms, exactly as before.
export async function getPlatformCapabilities(): Promise<PlatformCapabilities> {
  if (!tauriAvailable()) return { usbDisplay: true, firmwareFlashing: false };
  return invoke<PlatformCapabilities>('get_platform_capabilities');
}

export async function searchLocations(query: string): Promise<LocationSearchResult[]> {
  const name = query.trim();
  if (name.length < 2) return [];
  if ([...name].length > LOCATION_SEARCH_MAX_QUERY_CHARS) {
    throw new Error(`Location search is limited to ${LOCATION_SEARCH_MAX_QUERY_CHARS} characters.`);
  }
  if (tauriAvailable()) return invoke<LocationSearchResult[]>('search_locations', { query: name });

  const endpoint = new URL('https://geocoding-api.open-meteo.com/v1/search');
  endpoint.searchParams.set('name', name);
  endpoint.searchParams.set('count', '8');
  endpoint.searchParams.set('language', 'en');
  endpoint.searchParams.set('format', 'json');
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), LOCATION_SEARCH_TIMEOUT_MS);
  try {
    const response = await fetch(endpoint, {
      headers: { accept: 'application/json' },
      redirect: 'error',
      signal: controller.signal
    });
    if (!response.ok) throw new Error(`Location search returned HTTP ${response.status}.`);
    const body = (await readBoundedLocationSearch(response)) as {
      results?: Array<{
        id: number;
        name: string;
        latitude: number;
        longitude: number;
        timezone?: string;
        country_code?: string;
        admin1?: string;
        country?: string;
      }>;
    };
    return (body.results ?? []).slice(0, LOCATION_SEARCH_MAX_RESULTS).map((result) => {
      if (
        !Number.isFinite(result.latitude) ||
        !Number.isFinite(result.longitude) ||
        result.latitude < -90 ||
        result.latitude > 90 ||
        result.longitude < -180 ||
        result.longitude > 180
      ) {
        throw new Error('Location search returned coordinates outside valid ranges.');
      }
      const label = [result.name, result.admin1, result.country]
        .filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
        .map((value) => value.trim())
        .join(', ');
      if (!label) throw new Error('Location search returned a result without a name.');
      const countryCode = result.country_code?.trim().toUpperCase();
      if (countryCode && !/^[A-Z]{2}$/.test(countryCode)) {
        throw new Error('Location search returned an invalid country code.');
      }
      return {
        id: `open-meteo:${result.id}`,
        label,
        latitude: result.latitude,
        longitude: result.longitude,
        timeZone: result.timezone?.trim() || 'UTC',
        countryCode,
        adminArea: result.admin1?.trim() || undefined
      };
    });
  } catch (error) {
    if (controller.signal.aborted) throw new Error('Location search timed out.');
    throw error;
  } finally {
    window.clearTimeout(timeout);
  }
}

async function readBoundedLocationSearch(response: Response): Promise<unknown> {
  const declaredLength = Number(response.headers.get('content-length'));
  if (Number.isFinite(declaredLength) && declaredLength > LOCATION_SEARCH_MAX_BYTES) {
    throw new Error('Location search response was too large.');
  }
  if (!response.body) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.byteLength > LOCATION_SEARCH_MAX_BYTES) {
      throw new Error('Location search response was too large.');
    }
    return JSON.parse(new TextDecoder().decode(bytes));
  }

  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > LOCATION_SEARCH_MAX_BYTES) {
      await reader.cancel();
      throw new Error('Location search response was too large.');
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return JSON.parse(new TextDecoder().decode(bytes));
}

export function getDeviceLocation(): Promise<GeolocationPosition> {
  return new Promise((resolve, reject) => {
    if (!navigator.geolocation) {
      reject(new Error('This device does not expose location services.'));
      return;
    }
    navigator.geolocation.getCurrentPosition(resolve, reject, {
      enableHighAccuracy: false,
      timeout: 12_000,
      maximumAge: 5 * 60_000
    });
  });
}

export async function getSnapshot(): Promise<AppSnapshot> {
  return desktopInvoke<AppSnapshot>('get_app_snapshot');
}

export async function getPreferences(): Promise<AppPreferences> {
  return desktopInvoke<AppPreferences>('get_preferences');
}

export async function savePreferences(preferences: AppPreferences): Promise<MutationResult> {
  return desktopInvoke<MutationResult>('save_preferences', { preferences });
}

export async function refreshSources(): Promise<MutationResult> {
  return desktopInvoke<MutationResult>('refresh_sources');
}

export async function sendDisplayTestFrame(): Promise<MutationResult> {
  return desktopInvoke<MutationResult>('send_display_test_frame');
}

export async function getEinkPreview(channelId?: string): Promise<EinkPreview> {
  return desktopInvoke<EinkPreview>('get_eink_preview', { channelId });
}

export async function getDisplayStatus(): Promise<DisplayConnectionStatus> {
  return desktopInvoke<DisplayConnectionStatus>('get_display_status');
}

export async function scanDisplayDevices(): Promise<DisplayDeviceCandidate[]> {
  return desktopInvoke<DisplayDeviceCandidate[]>('scan_display_devices');
}

export async function connectDisplayDevice(
  deviceId: string,
  transport: DisplayDeviceCandidate['transport']
): Promise<DisplayConnectionStatus> {
  return desktopInvoke<DisplayConnectionStatus>('connect_display_device', { deviceId, transport });
}

export async function disconnectDisplayDevice(): Promise<DisplayConnectionStatus> {
  return desktopInvoke<DisplayConnectionStatus>('disconnect_display_device');
}

export async function testWhatsApp(): Promise<MutationResult> {
  return desktopInvoke<MutationResult>('test_whatsapp');
}

export async function setWhatsAppToken(token: string): Promise<MutationResult> {
  return desktopInvoke<MutationResult>('set_whatsapp_token', { token });
}

export async function clearWhatsAppToken(): Promise<MutationResult> {
  return desktopInvoke<MutationResult>('clear_whatsapp_token');
}

export async function getAisstreamStatus(): Promise<AisStreamStatus> {
  return desktopInvoke<AisStreamStatus>('get_aisstream_status');
}

/** The local opening record for this hull, or null before its first ledger entry. */
export async function getVesselDetail(mmsi: string): Promise<VesselDetail | null> {
  return desktopInvoke<VesselDetail | null>('get_vessel_detail', { mmsi });
}

export async function setAisstreamApiKey(apiKey: string): Promise<MutationResult> {
  return desktopInvoke<MutationResult>('set_aisstream_api_key', { apiKey });
}

export async function clearAisstreamApiKey(): Promise<MutationResult> {
  return desktopInvoke<MutationResult>('clear_aisstream_api_key');
}

export async function openExternalUrl(url: string): Promise<void> {
  if (tauriAvailable()) return invoke<void>('open_external_url', { url });
  const opened = window.open(url, '_blank', 'noopener,noreferrer');
  if (!opened) throw new Error('The browser blocked the external link.');
}
