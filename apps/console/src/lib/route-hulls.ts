// Portable display layout; mirrored in BrickellStatus. Coordinates returned by
// `pointAt` are display units, while route measures and bounds remain metres.
export interface RouteHull {
  id: string;
  sMeters: number;
  minimum: number;
  maximum: number;
  radius: number;
  /** Members of a formation retain this order through asynchronous updates. */
  formation?: string;
  order?: number;
  pointAt: (sMeters: number) => { x: number; y: number };
}

/** Pack actual drawn footprints on their routes, keeping observations untouched. */
export function separateRouteHulls(hulls: readonly RouteHull[], stepMeters: number): Map<string, number> {
  const first = pack(hulls, stepMeters);
  if (first.collisions === 0) return first.positions;
  // A fresh fix next to a hard boundary must leave room for the member ahead.
  // Retry from the other end before accepting an obstructed preferred anchor.
  const reversed = pack([...hulls].reverse(), stepMeters);
  return reversed.collisions < first.collisions ? reversed.positions : first.positions;
}

function pack(hulls: readonly RouteHull[], stepMeters: number): { positions: Map<string, number>; collisions: number } {
  const placed: Array<{ hull: RouteHull; s: number; x: number; y: number }> = [];
  const result = new Map<string, number>();
  let collisions = 0;
  for (const hull of hulls) {
    let minimum = hull.minimum, maximum = hull.maximum;
    for (const other of placed) {
      if (!hull.formation || hull.formation !== other.hull.formation) continue;
      if ((hull.order ?? 0) > (other.hull.order ?? 0)) minimum = Math.max(minimum, other.s);
      else maximum = Math.min(maximum, other.s);
    }
    const preferred = Math.max(minimum, Math.min(maximum, hull.sMeters));
    let chosen = preferred;
    let bestClearance = -Infinity;
    const steps = Math.ceil(Math.max(preferred - minimum, maximum - preferred) / stepMeters);
    search: for (let index = 0; index <= steps; index += 1) {
      for (const sign of index === 0 ? [1] : [-1, 1]) {
        const s = preferred + sign * index * stepMeters;
        if (s < minimum || s > maximum) continue;
        const point = hull.pointAt(s);
        let clearance = Infinity;
        for (const other of placed) {
          clearance = Math.min(clearance,
            Math.hypot(point.x - other.x, point.y - other.y) - hull.radius - other.hull.radius);
        }
        if (clearance > bestClearance) { chosen = s; bestClearance = clearance; }
        if (clearance >= 0) break search;
      }
    }
    if (bestClearance < -1e-6) collisions += 1;
    const point = hull.pointAt(chosen);
    placed.push({ hull, s: chosen, ...point });
    result.set(hull.id, chosen);
  }
  return { positions: result, collisions };
}
