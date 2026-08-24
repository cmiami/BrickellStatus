export interface AnnotationPoint {
  x: number;
  y: number;
}

export interface AnnotationRect extends AnnotationPoint {
  width: number;
  height: number;
}

export interface AnnotationRoute {
  points: AnnotationPoint[];
  halfWidth: number;
}

export interface AnnotationVessel {
  id: string;
  anchor: AnnotationPoint;
  angleDegrees: number;
  hullWidth: number;
  hullHeight: number;
  /** Absolute bounds of the hull plus fixed badges and direction marks. */
  avoidanceRect?: AnnotationRect;
  cardWidth: number;
  cardHeight: number;
  priority: number;
}

export interface AnnotationPlacement {
  id: string;
  card: AnnotationRect;
  leader: { from: AnnotationPoint; to: AnnotationPoint };
}

export interface AnnotationLayout {
  placements: AnnotationPlacement[];
  unplacedIds: string[];
}

const EDGE_GAP = 12;
const ROUTE_GAP = 12;
const HULL_GAP = 10;
const CARD_GAP = 10;
const LEADER_GAP = 10;

function inflate(rect: AnnotationRect, amount: number): AnnotationRect {
  return {
    x: rect.x - amount,
    y: rect.y - amount,
    width: rect.width + amount * 2,
    height: rect.height + amount * 2
  };
}

function rectsOverlap(left: AnnotationRect, right: AnnotationRect): boolean {
  return (
    left.x < right.x + right.width &&
    left.x + left.width > right.x &&
    left.y < right.y + right.height &&
    left.y + left.height > right.y
  );
}

function pointToRectDistanceSquared(point: AnnotationPoint, rect: AnnotationRect): number {
  const dx = Math.max(rect.x - point.x, 0, point.x - (rect.x + rect.width));
  const dy = Math.max(rect.y - point.y, 0, point.y - (rect.y + rect.height));
  return dx * dx + dy * dy;
}

function pointToSegmentDistanceSquared(
  point: AnnotationPoint,
  start: AnnotationPoint,
  end: AnnotationPoint
): number {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const lengthSquared = dx * dx + dy * dy;
  if (lengthSquared === 0) {
    return (point.x - start.x) ** 2 + (point.y - start.y) ** 2;
  }
  const fraction = Math.max(
    0,
    Math.min(1, ((point.x - start.x) * dx + (point.y - start.y) * dy) / lengthSquared)
  );
  const x = start.x + fraction * dx;
  const y = start.y + fraction * dy;
  return (point.x - x) ** 2 + (point.y - y) ** 2;
}

function segmentIntersectsRect(
  start: AnnotationPoint,
  end: AnnotationPoint,
  rect: AnnotationRect
): boolean {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  let near = 0;
  let far = 1;
  const tests: Array<[number, number]> = [
    [-dx, start.x - rect.x],
    [dx, rect.x + rect.width - start.x],
    [-dy, start.y - rect.y],
    [dy, rect.y + rect.height - start.y]
  ];
  for (const [p, q] of tests) {
    if (p === 0) {
      if (q < 0) return false;
      continue;
    }
    const ratio = q / p;
    if (p < 0) near = Math.max(near, ratio);
    else far = Math.min(far, ratio);
    if (near > far) return false;
  }
  return true;
}

function segmentToRectDistanceSquared(
  start: AnnotationPoint,
  end: AnnotationPoint,
  rect: AnnotationRect
): number {
  if (segmentIntersectsRect(start, end, rect)) return 0;
  const corners = [
    { x: rect.x, y: rect.y },
    { x: rect.x + rect.width, y: rect.y },
    { x: rect.x, y: rect.y + rect.height },
    { x: rect.x + rect.width, y: rect.y + rect.height }
  ];
  return Math.min(
    pointToRectDistanceSquared(start, rect),
    pointToRectDistanceSquared(end, rect),
    ...corners.map((corner) => pointToSegmentDistanceSquared(corner, start, end))
  );
}

function routeOverlapsCard(route: AnnotationRoute, card: AnnotationRect): boolean {
  const clearanceSquared = (route.halfWidth + ROUTE_GAP) ** 2;
  for (let index = 0; index + 1 < route.points.length; index += 1) {
    if (
      segmentToRectDistanceSquared(route.points[index], route.points[index + 1], card) <
      clearanceSquared
    ) {
      return true;
    }
  }
  return false;
}

function normalized(x: number, y: number): AnnotationPoint {
  const length = Math.hypot(x, y) || 1;
  return { x: x / length, y: y / length };
}

function nearestCardPoint(anchor: AnnotationPoint, card: AnnotationRect): AnnotationPoint {
  return {
    x: Math.max(card.x, Math.min(card.x + card.width, anchor.x)),
    y: Math.max(card.y, Math.min(card.y + card.height, anchor.y))
  };
}

function hullEdge(
  anchor: AnnotationPoint,
  toward: AnnotationPoint,
  width: number,
  height: number
): AnnotationPoint {
  const dx = toward.x - anchor.x;
  const dy = toward.y - anchor.y;
  const scale = Math.min(
    dx === 0 ? Number.POSITIVE_INFINITY : width / 2 / Math.abs(dx),
    dy === 0 ? Number.POSITIVE_INFINITY : height / 2 / Math.abs(dy)
  );
  const safeScale = Number.isFinite(scale) ? scale : 0;
  return { x: anchor.x + dx * safeScale, y: anchor.y + dy * safeScale };
}

function candidateDirections(vessel: AnnotationVessel, target: AnnotationRect): AnnotationPoint[] {
  const radians = (vessel.angleDegrees * Math.PI) / 180;
  const tangent = { x: Math.cos(radians), y: Math.sin(radians) };
  const normal = { x: -tangent.y, y: tangent.x };
  const targetCenter = { x: target.x + target.width / 2, y: target.y + target.height / 2 };
  const away =
    normal.x * (vessel.anchor.x - targetCenter.x) +
      normal.y * (vessel.anchor.y - targetCenter.y) >=
    0
      ? normal
      : { x: -normal.x, y: -normal.y };
  const other = { x: -away.x, y: -away.y };
  return [
    away,
    other,
    normalized(away.x + tangent.x * 0.55, away.y + tangent.y * 0.55),
    normalized(away.x - tangent.x * 0.55, away.y - tangent.y * 0.55),
    normalized(other.x + tangent.x * 0.55, other.y + tangent.y * 0.55),
    normalized(other.x - tangent.x * 0.55, other.y - tangent.y * 0.55),
    tangent,
    { x: -tangent.x, y: -tangent.y }
  ];
}

export function layoutVesselAnnotations(input: {
  bounds: AnnotationRect;
  routes: AnnotationRoute[];
  targetExclusion: AnnotationRect;
  obstacles?: AnnotationRect[];
  vessels: AnnotationVessel[];
}): AnnotationLayout {
  const obstacles = [input.targetExclusion, ...(input.obstacles ?? [])];
  const vessels = input.vessels.slice().sort((left, right) => {
    const leftDistance = Math.hypot(
      left.anchor.x - (input.targetExclusion.x + input.targetExclusion.width / 2),
      left.anchor.y - (input.targetExclusion.y + input.targetExclusion.height / 2)
    );
    const rightDistance = Math.hypot(
      right.anchor.x - (input.targetExclusion.x + input.targetExclusion.width / 2),
      right.anchor.y - (input.targetExclusion.y + input.targetExclusion.height / 2)
    );
    return (
      right.priority - left.priority ||
      leftDistance - rightDistance ||
      left.id.localeCompare(right.id)
    );
  });
  const hulls = vessels.map((vessel) => ({
    id: vessel.id,
    rect: vessel.avoidanceRect ?? {
        x: vessel.anchor.x - vessel.hullWidth / 2,
        y: vessel.anchor.y - vessel.hullHeight / 2,
        width: vessel.hullWidth,
        height: vessel.hullHeight
      }
  }));
  const placements: AnnotationPlacement[] = [];
  const unplacedIds: string[] = [];

  const legal = (card: AnnotationRect) => {
    const inside =
      card.x >= input.bounds.x + EDGE_GAP &&
      card.y >= input.bounds.y + EDGE_GAP &&
      card.x + card.width <= input.bounds.x + input.bounds.width - EDGE_GAP &&
      card.y + card.height <= input.bounds.y + input.bounds.height - EDGE_GAP;
    return (
      inside &&
      !input.routes.some((route) => routeOverlapsCard(route, card)) &&
      !obstacles.some((obstacle) => rectsOverlap(card, inflate(obstacle, CARD_GAP))) &&
      !hulls.some((hull) => rectsOverlap(card, inflate(hull.rect, HULL_GAP))) &&
      !placements.some((placed) => rectsOverlap(card, inflate(placed.card, CARD_GAP)))
    );
  };

  for (const vessel of vessels) {
    let chosen: AnnotationRect | null = null;
    const directions = candidateDirections(vessel, input.targetExclusion);
    for (const ring of [0, 34, 68, 108, 154, 208, 270]) {
      for (const direction of directions) {
        const hullSupport =
          Math.abs(direction.x) * vessel.hullWidth / 2 +
          Math.abs(direction.y) * vessel.hullHeight / 2;
        const cardSupport =
          Math.abs(direction.x) * vessel.cardWidth / 2 +
          Math.abs(direction.y) * vessel.cardHeight / 2;
        const distance = hullSupport + cardSupport + LEADER_GAP + ring;
        const card = {
          x: vessel.anchor.x + direction.x * distance - vessel.cardWidth / 2,
          y: vessel.anchor.y + direction.y * distance - vessel.cardHeight / 2,
          width: vessel.cardWidth,
          height: vessel.cardHeight
        };
        if (legal(card)) {
          chosen = card;
          break;
        }
      }
      if (chosen) break;
    }

    if (!chosen) {
      let nearest = Number.POSITIVE_INFINITY;
      for (
        let y = input.bounds.y + EDGE_GAP;
        y <= input.bounds.y + input.bounds.height - vessel.cardHeight - EDGE_GAP;
        y += 18
      ) {
        for (
          let x = input.bounds.x + EDGE_GAP;
          x <= input.bounds.x + input.bounds.width - vessel.cardWidth - EDGE_GAP;
          x += 18
        ) {
          const card = { x, y, width: vessel.cardWidth, height: vessel.cardHeight };
          if (!legal(card)) continue;
          const distance = Math.hypot(
            x + vessel.cardWidth / 2 - vessel.anchor.x,
            y + vessel.cardHeight / 2 - vessel.anchor.y
          );
          if (distance < nearest) {
            nearest = distance;
            chosen = card;
          }
        }
      }
    }

    if (!chosen) {
      unplacedIds.push(vessel.id);
      continue;
    }
    const to = nearestCardPoint(vessel.anchor, chosen);
    placements.push({
      id: vessel.id,
      card: chosen,
      leader: {
        from: hullEdge(vessel.anchor, to, vessel.hullWidth, vessel.hullHeight),
        to
      }
    });
  }

  return { placements, unplacedIds };
}
