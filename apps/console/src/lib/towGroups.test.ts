import { describe, expect, it } from 'vitest';
import { nativeTowGroups } from './towGroups';
import type { RiverCorridor, VesselTrack } from './types';
const now = Date.parse('2031-04-03T12:00:00Z');
const corridor: RiverCorridor = { bridgeLatitude: 25.77, bridgeLongitude: -80.2, aisLive: true, branches: [] };
function track(mmsi: string, vesselClass: string, offset: number): VesselTrack {
  return { mmsi, vesselClass, movement: 'approaching', routeIntersects: true,
    speedKnots: 3.2, courseDegrees: 90, observedAt: new Date(now).toISOString(), sMeters: 500 + offset, branch: 'river', posture: 'underway',
    points: [0, 30, 60].map((seconds) => ({ latitude: 25.77, longitude: -80.2 + (600 + offset - seconds * 1.6) / 100000,
      observedAt: new Date(now - 60000 + seconds * 1000).toISOString(), sMeters: 600 + offset - seconds * 1.6,
      branch: 'river', offsetMeters: 3 })) };
}
describe('native tow inference from retained AIS', () => {
  it('recognizes two working tugs and one cargo without grouping by name', () => {
    const tracks = [track('tug-a', 'tug + tow', -50), track('cargo', 'cargo', 0), track('tug-b', 'tug', 70)];
    const copy = structuredClone(tracks);
    const groups = nativeTowGroups(corridor, tracks, now);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.tugIds).toEqual(['tug-a', 'tug-b']);
    expect(groups[0]?.towIds).toEqual(['cargo']);
    expect(groups[0]?.memberOffsetsMeters).toEqual({ 'tug-a': -50, cargo: 0, 'tug-b': 70 });
    expect(tracks).toEqual(copy);
  });
  it('rejects moored, stale, future, and single-fix pairs', () => {
    for (const variant of ['moored', 'stale', 'future', 'single']) {
      const tracks = [track('a', 'tug', -50), track('b', 'cargo', 0)];
      if (variant === 'single') tracks.forEach((t) => { t.points = t.points.slice(-1); });
      if (variant === 'moored') tracks.forEach((t) => { t.points.forEach((p) => { p.sMeters = t.sMeters!; p.longitude = -80.2; }); });
      if (variant === 'stale') tracks[0]!.observedAt = new Date(now - 360001).toISOString();
      if (variant === 'future') tracks[0]!.observedAt = new Date(now + 31000).toISOString();
      expect(nativeTowGroups(corridor, tracks, now)).toEqual([]);
    }
  });
});
