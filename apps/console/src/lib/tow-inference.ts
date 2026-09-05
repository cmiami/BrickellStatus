// Portable tow inference; mirrored in BrickellStatus apps/console/src/lib/tow-inference.ts.
// Keep this module independent of either app's transport and rendering types.
type VesselCorridor = string;
type VesselDirection = "inbound" | "outbound" | "holding" | "unknown";
type VesselKind = string;
export interface PublicVesselGroup {
  id: string;
  kind: "likely_tow";
  tugIds: string[];
  towIds: string[];
  observedAt: string;
  /** Time-paired route offsets relative to the companion; estimates, never fixes. */
  memberOffsetsMeters?: Record<string, number> | undefined;
}

const HISTORY_WINDOW_MS = 15 * 60_000;
// Match the materialized-vessel TTL so a sparse but still-live AIS sender does
// not make a relationship blink out before its track does.
const FRESH_AFTER_MS = 6 * 60_000;
const MAX_FUTURE_SKEW_MS = 30_000;
const MIN_TRACK_SPAN_MS = 3 * 60_000;
// Real Class B traffic in the Miami River regularly has 4-10 minute gaps.
// The retained history is deliberately 15 minutes, so interpolate within it.
const MAX_INTERPOLATION_GAP_MS = 12 * 60_000;
const MIN_SAMPLE_SPACING_MS = 30_000;
const MIN_MOVING_SPEED_KNOTS = 0.75;
const MAX_CURRENT_SPEED_DELTA_KNOTS = 1.5;
const MIN_TRACK_TRAVEL_METERS = 40;
const MIN_SEPARATION_METERS = 12;
const MAX_SEPARATION_METERS = 250;
const MAX_SEPARATION_DEVIATION_METERS = 50;
const MIN_STABLE_SAMPLE_RATIO = 0.75;
const MIN_ALONG_GAP_METERS = 8;
const MAX_CROSS_TRACK_METERS = 65;
const MIN_COMMON_DIRECTION_COSINE = 0.25;
// Channel direction can flip briefly around Brickell's projection seam. When
// both AIS courses still agree, let the retained common-track evidence decide.
const MAX_DIRECTION_LABEL_COURSE_DELTA_DEGREES = 35;

export interface TowGroupVessel {
  /** Private source identifier; never copied into a public relationship. */
  rawId: string;
  publicId: string;
  type: VesselKind;
  corridor: VesselCorridor;
  direction: VesselDirection;
  speedKnots: number | null;
  courseDegrees: number | null;
  observedAtMs: number;
}

export interface TowGroupFix {
  rawId: string;
  latitude: number;
  longitude: number;
  sMeters: number;
  branch: string | null;
  offsetMeters: number | null;
  observedAtMs: number;
}

interface TrackPoint {
  latitude: number;
  longitude: number;
  sMeters: number;
  branch: string;
  offsetMeters: number;
  channelReliable: boolean;
}

interface PairedSample {
  time: number;
  anchorPoint: TrackPoint;
  companionPoint: TrackPoint;
  separation: number;
}

interface PairEvidence {
  anchor: TowGroupVessel;
  companion: TowGroupVessel;
  observedAtMs: number;
  score: number;
  routeOffsetMeters: number | null;
}

/**
 * Infer conservative one-to-one tug/tow relationships from the retained AIS
 * history. These are probabilities, not declarations of a physical coupling.
 */
export function detectLikelyTowGroups(
  vessels: readonly TowGroupVessel[],
  fixes: readonly TowGroupFix[],
  nowMs: number,
): PublicVesselGroup[] {
  const active = vessels.filter((vessel) => usableVessel(vessel, nowMs));
  const anchors = active.filter((vessel) => vessel.type === "tug" || vessel.type === "tow");
  const companions = active.filter((vessel) => vessel.type !== "tug" && vessel.type !== "tow");
  if (anchors.length === 0 || companions.length === 0) return [];

  const histories = historiesByRawId(fixes, nowMs);
  const candidates: PairEvidence[] = [];
  for (const anchor of anchors) {
    const anchorHistory = histories.get(anchor.rawId);
    if (!anchorHistory) continue;
    for (const companion of companions) {
      if (
        !corridorsCanShareTrack(anchor.corridor, companion.corridor) ||
        !directionsCanShareTrack(anchor, companion) ||
        !currentSpeedsCompatible(anchor, companion)
      ) {
        continue;
      }
      const companionHistory = histories.get(companion.rawId);
      if (!companionHistory) continue;
      const evidence = pairEvidence(anchor, companion, anchorHistory, companionHistory);
      if (evidence) candidates.push(evidence);
    }
  }

  candidates.sort((left, right) =>
    right.score - left.score ||
    left.anchor.publicId.localeCompare(right.anchor.publicId) ||
    left.companion.publicId.localeCompare(right.companion.publicId),
  );
  // A tug chooses one companion, but a companion can have several working tugs.
  const usedAnchors = new Set<string>();
  const byCompanion = new Map<string, PairEvidence[]>();
  for (const candidate of candidates) {
    if (usedAnchors.has(candidate.anchor.publicId)) continue;
    usedAnchors.add(candidate.anchor.publicId);
    const members = byCompanion.get(candidate.companion.publicId) ?? [];
    members.push(candidate);
    byCompanion.set(candidate.companion.publicId, members);
  }
  const groups: PublicVesselGroup[] = [];
  for (const [companionId, evidence] of byCompanion) {
    const tugIds = evidence.map((pair) => pair.anchor.publicId).sort();
    const members = [...tugIds, companionId].sort();
    const offsets: Record<string, number> = { [companionId]: 0 };
    for (const pair of evidence) {
      if (pair.routeOffsetMeters !== null) offsets[pair.anchor.publicId] = pair.routeOffsetMeters;
    }
    groups.push({
      id: `likely_tow.${members.join(".")}`,
      kind: "likely_tow",
      tugIds,
      towIds: [companionId],
      observedAt: new Date(Math.min(...evidence.map((pair) => pair.observedAtMs))).toISOString(),
      ...(Object.keys(offsets).length === members.length ? { memberOffsetsMeters: offsets } : {}),
    });
  }
  return groups.sort((left, right) => left.id.localeCompare(right.id));
}

function usableVessel(vessel: TowGroupVessel, nowMs: number): boolean {
  const underway =
    (vessel.direction === "inbound" || vessel.direction === "outbound") &&
    vessel.speedKnots !== null &&
    Number.isFinite(vessel.speedKnots) &&
    vessel.speedKnots >= MIN_MOVING_SPEED_KNOTS;
  const freshlyHolding =
    vessel.direction === "holding" &&
    vessel.speedKnots !== null &&
    Number.isFinite(vessel.speedKnots) &&
    vessel.speedKnots >= 0;
  return (
    (underway || freshlyHolding) &&
    Number.isFinite(vessel.observedAtMs) &&
    vessel.observedAtMs >= nowMs - FRESH_AFTER_MS &&
    vessel.observedAtMs <= nowMs + MAX_FUTURE_SKEW_MS
  );
}

function historiesByRawId(
  fixes: readonly TowGroupFix[],
  nowMs: number,
): Map<string, TowGroupFix[]> {
  const histories = new Map<string, TowGroupFix[]>();
  for (const fix of fixes) {
    if (
      !fix.rawId ||
      !Number.isFinite(fix.latitude) ||
      !Number.isFinite(fix.longitude) ||
      !Number.isFinite(fix.sMeters) ||
      typeof fix.branch !== "string" ||
      fix.offsetMeters === null ||
      !Number.isFinite(fix.offsetMeters) ||
      !Number.isFinite(fix.observedAtMs) ||
      fix.observedAtMs < nowMs - HISTORY_WINDOW_MS ||
      fix.observedAtMs > nowMs + MAX_FUTURE_SKEW_MS
    ) {
      continue;
    }
    const history = histories.get(fix.rawId) ?? [];
    history.push(fix);
    histories.set(fix.rawId, history);
  }
  for (const [rawId, history] of histories) {
    history.sort((left, right) => left.observedAtMs - right.observedAtMs);
    histories.set(rawId, deduplicateFixes(history));
  }
  return histories;
}

function pairEvidence(
  anchor: TowGroupVessel,
  companion: TowGroupVessel,
  anchorHistory: readonly TowGroupFix[],
  companionHistory: readonly TowGroupFix[],
): PairEvidence | null {
  // The short path still needs independently observed motion from both radios.
  // Tight speed/path/gap checks allow a close formation after a minute, while
  // the broad sparse-history path keeps its three-minute evidence requirement.
  const quick = closeFormationEvidence(anchor, companion, anchorHistory, companionHistory);
  if (quick) return quick;
  if (anchorHistory.length < 3 || companionHistory.length < 3) return null;
  const overlapStart = Math.max(
    anchorHistory[0]!.observedAtMs,
    companionHistory[0]!.observedAtMs,
  );
  const overlapEnd = Math.min(
    anchorHistory.at(-1)!.observedAtMs,
    companionHistory.at(-1)!.observedAtMs,
  );
  if (overlapEnd - overlapStart < MIN_TRACK_SPAN_MS) return null;

  const times = sampleTimes(anchorHistory, companionHistory, overlapStart, overlapEnd);
  const samples: PairedSample[] = [];
  for (const time of times) {
    const anchorPoint = interpolatePoint(anchorHistory, time);
    const companionPoint = interpolatePoint(companionHistory, time);
    if (!anchorPoint || !companionPoint) continue;
    samples.push({
      time,
      anchorPoint,
      companionPoint,
      separation: physicalSeparation(anchorPoint, companionPoint),
    });
  }
  if (samples.length < 3 || sampleSpan(samples) < MIN_TRACK_SPAN_MS) return null;

  const bounded = samples.filter((sample) =>
    sample.separation >= MIN_SEPARATION_METERS &&
    sample.separation <= MAX_SEPARATION_METERS,
  );
  if (!enoughSamples(bounded, samples.length)) return null;
  const medianSeparation = median(bounded.map((sample) => sample.separation));
  const stable = bounded.filter((sample) =>
    Math.abs(sample.separation - medianSeparation) <= MAX_SEPARATION_DEVIATION_METERS,
  );
  if (!enoughSamples(stable, bounded.length) || sampleSpan(stable) < MIN_TRACK_SPAN_MS) return null;

  const ordered = stable.flatMap((sample, index): Array<PairedSample & { alongGap: number }> => {
    const geometry = formationGeometry(stable, index);
    if (
      !geometry ||
      Math.abs(geometry.alongGap) < MIN_ALONG_GAP_METERS ||
      geometry.crossTrack > MAX_CROSS_TRACK_METERS
    ) return [];
    return [{ ...sample, alongGap: geometry.alongGap }];
  });
  if (!enoughSamples(ordered, stable.length)) return null;
  const orderingSign = majoritySign(ordered.map((sample) => sample.alongGap));
  if (orderingSign === null) return null;
  const orderedMajority = ordered.filter((sample) => Math.sign(sample.alongGap) === orderingSign);
  if (!enoughSamples(orderedMajority, ordered.length) || sampleSpan(orderedMajority) < MIN_TRACK_SPAN_MS) {
    return null;
  }
  if (!commonTrackMovement(orderedMajority)) return null;

  const firstEvidenceAtMs = orderedMajority[0]!.time;
  const observedAtMs = orderedMajority.at(-1)!.time;

  const coursePenalty = courseDifference(anchor.courseDegrees, companion.courseDegrees) / 180;
  const score =
    orderedMajority.length * 10 +
    (observedAtMs - firstEvidenceAtMs) / 60_000 -
    medianAbsoluteDeviation(stable.map((sample) => sample.separation)) / 10 -
    currentSpeedDelta(anchor, companion) * 2 -
    coursePenalty;
  const routeOffsets = orderedMajority.filter((sample) =>
    sample.anchorPoint.channelReliable && sample.companionPoint.channelReliable &&
    sample.anchorPoint.branch === sample.companionPoint.branch,
  ).map((sample) => sample.anchorPoint.sMeters - sample.companionPoint.sMeters);
  return { anchor, companion, observedAtMs, score,
    routeOffsetMeters: routeOffsets.length >= 2 ? median(routeOffsets) : null };

}

function closeFormationEvidence(
  anchor: TowGroupVessel, companion: TowGroupVessel,
  anchorHistory: readonly TowGroupFix[], companionHistory: readonly TowGroupFix[],
): PairEvidence | null {
  const end = Math.min(anchorHistory.at(-1)?.observedAtMs ?? 0, companionHistory.at(-1)?.observedAtMs ?? 0);
  const recent = (history: readonly TowGroupFix[]) => history.filter((fix) => fix.observedAtMs >= end - 180_000);
  const left = recent(anchorHistory), right = recent(companionHistory);
  if (left.length < 2 || right.length < 2 || currentSpeedDelta(anchor, companion) > 0.75) return null;
  const start = Math.max(left[0]!.observedAtMs, right[0]!.observedAtMs);
  if (end - start < 60_000) return null;
  const samples: PairedSample[] = [];
  for (const time of sampleTimes(left, right, start, end)) {
    const anchorPoint = interpolatePoint(left, time), companionPoint = interpolatePoint(right, time);
    if (!anchorPoint || !companionPoint || !anchorPoint.channelReliable ||
        !companionPoint.channelReliable || anchorPoint.branch !== companionPoint.branch) return null;
    samples.push({ time, anchorPoint, companionPoint, separation: physicalSeparation(anchorPoint, companionPoint) });
  }
  if (samples.length < 3 || sampleSpan(samples) < 60_000) return null;
  const gaps = samples.map((sample) => sample.anchorPoint.sMeters - sample.companionPoint.sMeters);
  const gap = median(gaps);
  if (samples.some((sample, index) => sample.separation < 8 || sample.separation > 180 ||
      Math.abs(sample.anchorPoint.offsetMeters - sample.companionPoint.offsetMeters) > 35 ||
      Math.abs(gaps[index]! - gap) > 20)) return null;
  const first = samples[0]!, last = samples.at(-1)!;
  const aTravel = last.anchorPoint.sMeters - first.anchorPoint.sMeters;
  const bTravel = last.companionPoint.sMeters - first.companionPoint.sMeters;
  if (aTravel * bTravel <= 0 || Math.min(Math.abs(aTravel), Math.abs(bTravel)) < 40 ||
      Math.abs(aTravel - bTravel) / Math.max(Math.abs(aTravel), Math.abs(bTravel)) > 0.2 ||
      !commonTrackMovement(samples)) return null;
  return { anchor, companion, observedAtMs: end, routeOffsetMeters: gap,
    score: samples.length * 10 + (end - start) / 60_000 -
      medianAbsoluteDeviation(gaps) / 10 - currentSpeedDelta(anchor, companion) * 2 -
      courseDifference(anchor.courseDegrees, companion.courseDegrees) / 180 };
}

function sampleTimes(
  left: readonly TowGroupFix[],
  right: readonly TowGroupFix[],
  start: number,
  end: number,
): number[] {
  const candidates = [...left, ...right]
    .map((fix) => fix.observedAtMs)
    .filter((time) => time >= start && time <= end)
    .sort((a, b) => a - b);
  const times: number[] = [];
  for (const time of candidates) {
    const previous = times.at(-1);
    if (previous === undefined || time - previous >= MIN_SAMPLE_SPACING_MS) times.push(time);
  }
  if (times.at(-1) !== end && end - (times.at(-1) ?? start) >= MIN_SAMPLE_SPACING_MS) {
    times.push(end);
  }
  return times;
}

function interpolatePoint(history: readonly TowGroupFix[], time: number): TrackPoint | null {
  const rightIndex = history.findIndex((fix) => fix.observedAtMs >= time);
  if (rightIndex < 0) return null;
  const right = history[rightIndex]!;
  if (right.observedAtMs === time) return pointFromFix(right);
  if (rightIndex === 0) return null;
  const left = history[rightIndex - 1]!;
  const elapsed = right.observedAtMs - left.observedAtMs;
  if (elapsed <= 0 || elapsed > MAX_INTERPOLATION_GAP_MS) return null;
  if (left.branch === null || left.offsetMeters === null || right.offsetMeters === null) return null;
  const fraction = (time - left.observedAtMs) / elapsed;
  const channelReliable = left.branch === right.branch;
  return {
    latitude: interpolate(left.latitude, right.latitude, fraction),
    longitude: interpolate(left.longitude, right.longitude, fraction),
    sMeters: interpolate(left.sMeters, right.sMeters, fraction),
    branch: channelReliable ? left.branch : "junction",
    offsetMeters: interpolate(left.offsetMeters, right.offsetMeters, fraction),
    channelReliable,
  };
}

function pointFromFix(fix: TowGroupFix): TrackPoint | null {
  if (fix.branch === null || fix.offsetMeters === null) return null;
  return {
    latitude: fix.latitude,
    longitude: fix.longitude,
    sMeters: fix.sMeters,
    branch: fix.branch,
    offsetMeters: fix.offsetMeters,
    channelReliable: true,
  };
}

function physicalSeparation(left: TrackPoint, right: TrackPoint): number {
  if (left.channelReliable && right.channelReliable && left.branch === right.branch) {
    const along = Math.abs(left.sMeters - right.sMeters);
    // Sparse geographic interpolation cuts across river bends. Use the paired
    // route measure for astern/pushing formations. At the same route measure,
    // unsigned bank offsets cannot distinguish opposite sides; use coordinates.
    if (along >= MIN_ALONG_GAP_METERS) {
      return Math.hypot(along, left.offsetMeters - right.offsetMeters);
    }
  }
  return haversineMeters(left, right);
}

function formationGeometry(
  samples: readonly PairedSample[],
  index: number,
): { alongGap: number; crossTrack: number } | null {
  const sample = samples[index];
  if (!sample) return null;
  if (
    sample.anchorPoint.channelReliable &&
    sample.companionPoint.channelReliable &&
    sample.anchorPoint.branch === sample.companionPoint.branch
  ) {
    return {
      alongGap: sample.anchorPoint.sMeters - sample.companionPoint.sMeters,
      crossTrack: Math.abs(sample.anchorPoint.offsetMeters - sample.companionPoint.offsetMeters),
    };
  }
  const previous = samples[Math.max(0, index - 1)];
  const next = samples[Math.min(samples.length - 1, index + 1)];
  if (!previous || !next || previous.time === next.time) return null;
  const movement = localMeters(groupCenter(previous), groupCenter(next));
  const movementLength = Math.hypot(movement.x, movement.y);
  if (movementLength < 1) return null;
  const relative = localMeters(sample.companionPoint, sample.anchorPoint);
  return {
    alongGap: (relative.x * movement.x + relative.y * movement.y) / movementLength,
    crossTrack: Math.abs(relative.x * movement.y - relative.y * movement.x) / movementLength,
  };
}

function commonTrackMovement(samples: readonly PairedSample[]): boolean {
  let anchorTravel = 0;
  let companionTravel = 0;
  let comparableLegs = 0;
  let commonDirectionLegs = 0;
  for (let index = 1; index < samples.length; index += 1) {
    const previous = samples[index - 1]!;
    const current = samples[index]!;
    const anchorMovement = localMeters(previous.anchorPoint, current.anchorPoint);
    const companionMovement = localMeters(previous.companionPoint, current.companionPoint);
    const anchorDistance = Math.hypot(anchorMovement.x, anchorMovement.y);
    const companionDistance = Math.hypot(companionMovement.x, companionMovement.y);
    anchorTravel += anchorDistance;
    companionTravel += companionDistance;
    if (anchorDistance < 5 || companionDistance < 5) continue;
    comparableLegs += 1;
    const cosine =
      (anchorMovement.x * companionMovement.x + anchorMovement.y * companionMovement.y) /
      (anchorDistance * companionDistance);
    if (cosine >= MIN_COMMON_DIRECTION_COSINE) commonDirectionLegs += 1;
  }
  return (
    anchorTravel >= MIN_TRACK_TRAVEL_METERS &&
    companionTravel >= MIN_TRACK_TRAVEL_METERS &&
    comparableLegs >= 2 &&
    commonDirectionLegs / comparableLegs >= MIN_STABLE_SAMPLE_RATIO
  );
}

function enoughSamples(samples: readonly unknown[], total: number): boolean {
  return samples.length >= 3 && total > 0 && samples.length / total >= MIN_STABLE_SAMPLE_RATIO;
}

function sampleSpan(samples: readonly PairedSample[]): number {
  return samples.length < 2 ? 0 : samples.at(-1)!.time - samples[0]!.time;
}

function majoritySign(values: readonly number[]): -1 | 1 | null {
  const positives = values.filter((value) => value > 0).length;
  const negatives = values.filter((value) => value < 0).length;
  const majority = Math.max(positives, negatives);
  if (values.length === 0 || majority / values.length < MIN_STABLE_SAMPLE_RATIO) return null;
  return positives > negatives ? 1 : -1;
}

function groupCenter(sample: PairedSample): Pick<TrackPoint, "latitude" | "longitude"> {
  return {
    latitude: (sample.anchorPoint.latitude + sample.companionPoint.latitude) / 2,
    longitude: (sample.anchorPoint.longitude + sample.companionPoint.longitude) / 2,
  };
}

function localMeters(
  from: Pick<TrackPoint, "latitude" | "longitude">,
  to: Pick<TrackPoint, "latitude" | "longitude">,
): { x: number; y: number } {
  const latitudeRadians = ((from.latitude + to.latitude) / 2) * Math.PI / 180;
  return {
    x: (to.longitude - from.longitude) * 111_320 * Math.cos(latitudeRadians),
    y: (to.latitude - from.latitude) * 110_540,
  };
}

function deduplicateFixes(history: readonly TowGroupFix[]): TowGroupFix[] {
  return history.filter((fix, index) => index === 0 || fix.observedAtMs !== history[index - 1]!.observedAtMs);
}

function median(values: readonly number[]): number {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  const right = sorted[middle] ?? 0;
  return sorted.length % 2 === 0 ? ((sorted[middle - 1] ?? right) + right) / 2 : right;
}

function medianAbsoluteDeviation(values: readonly number[]): number {
  const midpoint = median(values);
  return median(values.map((value) => Math.abs(value - midpoint)));
}

function interpolate(a: number, b: number, fraction: number): number {
  return a + (b - a) * fraction;
}

function courseDifference(left: number | null, right: number | null): number {
  if (left === null || right === null || !Number.isFinite(left) || !Number.isFinite(right)) return 0;
  return Math.abs(((left - right + 540) % 360) - 180);
}

function corridorsCanShareTrack(left: VesselCorridor, right: VesselCorridor): boolean {
  return left === right || left === "miami_river" || right === "miami_river";
}

function directionsCanShareTrack(left: TowGroupVessel, right: TowGroupVessel): boolean {
  if (left.direction === "unknown" || right.direction === "unknown") return false;
  if (
    left.direction === right.direction ||
    left.direction === "holding" ||
    right.direction === "holding"
  ) {
    return true;
  }
  return left.courseDegrees !== null && right.courseDegrees !== null &&
    Number.isFinite(left.courseDegrees) && Number.isFinite(right.courseDegrees) &&
    courseDifference(left.courseDegrees, right.courseDegrees) <=
    MAX_DIRECTION_LABEL_COURSE_DELTA_DEGREES;
}

function currentSpeedsCompatible(left: TowGroupVessel, right: TowGroupVessel): boolean {
  return currentSpeedDelta(left, right) <= MAX_CURRENT_SPEED_DELTA_KNOTS;
}

function currentSpeedDelta(left: TowGroupVessel, right: TowGroupVessel): number {
  if (left.direction === "holding" || right.direction === "holding") return 0;
  return Math.abs((left.speedKnots ?? 0) - (right.speedKnots ?? 0));
}

function haversineMeters(
  left: Pick<TrackPoint, "latitude" | "longitude">,
  right: Pick<TrackPoint, "latitude" | "longitude">,
): number {
  const radians = Math.PI / 180;
  const deltaLatitude = (right.latitude - left.latitude) * radians;
  const deltaLongitude = (right.longitude - left.longitude) * radians;
  const value =
    Math.sin(deltaLatitude / 2) ** 2 +
    Math.cos(left.latitude * radians) * Math.cos(right.latitude * radians) *
      Math.sin(deltaLongitude / 2) ** 2;
  return 6_371_000 * 2 * Math.atan2(Math.sqrt(value), Math.sqrt(1 - value));
}
