import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { KnownOpener, VesselDetail, VesselTrack } from '$lib/types';

import VesselDetailPanel from './VesselDetailPanel.svelte';

afterEach(cleanup);

const TRACK: VesselTrack = {
  mmsi: '367123456',
  vesselName: 'Island Time',
  vesselClass: 'pleasure craft',
  callSign: 'WDF7318',
  imoNumber: 9876543,
  destination: 'Miami River',
  lengthMeters: 24.6,
  beamMeters: 6.4,
  draughtMeters: 1.8,
  movement: 'approaching',
  posture: 'underway',
  knownOpener: true,
  likelyToOpenBrickell: true,
  routeIntersects: true,
  branch: 'river',
  sMeters: -420,
  speedKnots: 7.4,
  courseDegrees: 272.6,
  observedAt: '2026-08-23T15:40:00Z',
  etaMinMinutes: 3,
  etaMaxMinutes: 6,
  predictedOpeningAt: '2026-08-23T15:46:00Z',
  points: [
    { latitude: 25.7698, longitude: -80.188, observedAt: '2026-08-23T15:40:00Z' }
  ]
};

const DETAIL: VesselDetail = {
  mmsi: TRACK.mmsi,
  transitsOpened: 2,
  transitsFitsUnder: 1,
  transitsUnknown: 1,
  transitsPending: 0,
  firstSeenAt: '2026-08-18T12:00:00Z',
  lastSeenAt: '2026-08-23T15:40:00Z',
  lastCrossingAt: '2026-08-22T14:30:00Z',
  lastOpenedAt: '2026-08-22T14:30:00Z',
  openingPropensity: 6000,
  recentCrossings: [
    {
      mmsi: TRACK.mmsi,
      vesselName: TRACK.vesselName,
      vesselClass: TRACK.vesselClass,
      direction: 'upriver',
      crossedAt: '2026-08-22T14:30:00Z',
      speedKnots: 4.8,
      outcome: 'opened',
      resolvedAt: '2026-08-22T14:32:00Z'
    }
  ]
};

const KNOWN_OPENER: KnownOpener = {
  mmsi: TRACK.mmsi,
  vesselName: TRACK.vesselName,
  vesselClass: TRACK.vesselClass,
  callSign: TRACK.callSign,
  imoNumber: TRACK.imoNumber,
  transitsOpened: 2,
  transitsFitsUnder: 1,
  firstSeenAt: DETAIL.firstSeenAt,
  lastSeenAt: DETAIL.lastSeenAt,
  lastOpenedAt: DETAIL.lastOpenedAt,
  openingPropensity: DETAIL.openingPropensity
};

function mount(overrides: {
  track?: VesselTrack | null;
  knownOpener?: KnownOpener | null;
  detail?: VesselDetail | null;
  loading?: boolean;
  error?: string | null;
} = {}) {
  const onclose = vi.fn();
  const onretry = vi.fn();
  render(VesselDetailPanel, {
    track: overrides.track === undefined ? TRACK : overrides.track,
    knownOpener: overrides.knownOpener ?? null,
    detail: overrides.detail === undefined ? DETAIL : overrides.detail,
    loading: overrides.loading ?? false,
    error: overrides.error ?? null,
    localTimeZone: 'America/New_York',
    onclose,
    onretry
  });
  return { onclose, onretry };
}

describe('VesselDetailPanel', () => {
  it('shows the selected AIS identity, motion, and physical details', async () => {
    mount();

    expect(screen.getByRole('heading', { name: 'Island Time' })).toBeInTheDocument();
    expect(screen.getByText('MMSI 367123456 · pleasure craft')).toBeInTheDocument();
    expect(screen.getByText('7.4')).toBeInTheDocument();
    expect(screen.getByText('knots')).toBeInTheDocument();
    expect(screen.getByText('273°')).toBeInTheDocument();
    expect(screen.getByText('Toward Brickell')).toBeInTheDocument();
    expect(screen.getByText('Known opener')).toBeInTheDocument();
    expect(screen.getByText('Likely to open Brickell')).toBeInTheDocument();
    expect(screen.getByText('WDF7318')).toBeInTheDocument();
    expect(screen.getByText('9876543')).toBeInTheDocument();
    expect(screen.getByText('Waterway').parentElement).toHaveTextContent('Miami River');
    expect(screen.getByText(/24\.6 m long · 6\.4 m beam · 1\.8 m draught/i)).toBeInTheDocument();
    expect(screen.getByText(/likely to open Brickell on this passage · 3–6 min to Brickell/i)).toBeInTheDocument();

    await waitFor(() => expect(screen.getByRole('complementary')).toHaveFocus());
  });

  it('states what Brickell learned without turning unknown history into a claim', () => {
    mount();

    expect(screen.getByText(/went up for this vessel on 2 of 3 confirmed passages/i)).toBeInTheDocument();
    expect(screen.getByText(/60% estimated opening chance/i)).toBeInTheDocument();
    expect(screen.getByText('Bridge went up')).toBeInTheDocument();
    expect(screen.getByText('Upriver · 4.8 kn')).toBeInTheDocument();
    expect(screen.getByText('Bridge up').parentElement).toHaveTextContent('2');
    expect(screen.getByText('Bridge down').parentElement).toHaveTextContent('1');
    expect(screen.getByText('Not confirmed').parentElement).toHaveTextContent('1');
  });

  it('uses a plain generic name and honest empty history when little is known', () => {
    mount({
      track: {
        ...TRACK,
        mmsi: '368000001',
        vesselName: undefined,
        vesselClass: undefined,
        callSign: undefined,
        imoNumber: undefined,
        destination: undefined,
        lengthMeters: undefined,
        beamMeters: undefined,
        draughtMeters: undefined
      },
      detail: null
    });

    expect(screen.getByRole('heading', { name: 'Vessel 368000001' })).toBeInTheDocument();
    expect(screen.getByText('MMSI 368000001')).toBeInTheDocument();
    expect(screen.getByText(/no recorded Brickell passage for this vessel yet/i)).toBeInTheDocument();
    expect(screen.queryByText('Call sign')).toBeNull();
    expect(screen.queryByText('Dimensions')).toBeNull();
  });

  it('keeps the current reading usable while history loads or fails', async () => {
    const loading = mount({ detail: null, loading: true });
    expect(screen.getByRole('status')).toHaveTextContent(/loading this vessel’s Brickell history/i);
    expect(screen.getByText('7.4')).toBeInTheDocument();
    cleanup();

    const failed = mount({ detail: null, error: 'unavailable' });
    expect(screen.getByRole('alert')).toHaveTextContent(/latest AIS reading above is still available/i);
    await fireEvent.click(screen.getByRole('button', { name: /try again/i }));
    expect(failed.onretry).toHaveBeenCalledOnce();

    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(failed.onclose).toHaveBeenCalledOnce();
    expect(loading.onclose).not.toHaveBeenCalled();
  });

  it('opens saved Brickell history for a known opener outside the live AIS window', () => {
    mount({ track: null, knownOpener: KNOWN_OPENER });

    expect(screen.getByRole('heading', { name: 'Island Time' })).toBeInTheDocument();
    expect(screen.getByText('Known opener')).toBeInTheDocument();
    expect(screen.getByText(/no AIS position in the last hour/i)).toBeInTheDocument();
    expect(screen.getByText(/its saved Brickell history remains available below/i)).toBeInTheDocument();
    expect(screen.getByText(/went up for this vessel on 2 of 3 confirmed passages/i)).toBeInTheDocument();
  });
});
