import type { RiverCorridor, VesselTrack } from './types';
import { makeChannelProjector, travelDirection } from './river';
import { detectLikelyTowGroups, type PublicVesselGroup, type TowGroupFix } from './tow-inference';
export type { PublicVesselGroup } from './tow-inference';

/** Use retained fixes, including their original times; never backfill old motion. */
export function nativeTowGroups(corridor: RiverCorridor, tracks: VesselTrack[], nowMs: number): PublicVesselGroup[] {
  const project = makeChannelProjector(corridor);
  const fixes: TowGroupFix[] = [];
  for (const track of tracks) {
    for (const point of track.points) {
      const projection = point.sMeters != null && point.branch && point.offsetMeters != null
        ? { sMeters: point.sMeters, branchId: point.branch, offsetMeters: point.offsetMeters }
        : project?.(point.latitude, point.longitude);
      if (!projection) continue;
      fixes.push({ rawId: track.mmsi, latitude: point.latitude, longitude: point.longitude,
        sMeters: projection.sMeters, branch: projection.branchId, offsetMeters: projection.offsetMeters,
        observedAtMs: Date.parse(point.observedAt) });
    }
  }
  return detectLikelyTowGroups(tracks.map((track) => {
    const direction = travelDirection(track);
    return { rawId: track.mmsi, publicId: track.mmsi,
      type: track.vesselClass === 'tug + tow' ? 'tow' : track.vesselClass ?? 'unknown',
      corridor: track.branch === 'river' ? 'miami_river' : track.branch ?? 'unknown',
      direction: direction === 'upriver' ? 'inbound' : direction === 'downriver' ? 'outbound' : 'holding',
      speedKnots: track.speedKnots, courseDegrees: track.courseDegrees, observedAtMs: Date.parse(track.observedAt) };
  }), fixes, nowMs);
}
