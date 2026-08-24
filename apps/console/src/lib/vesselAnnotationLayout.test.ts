import { describe, expect, it } from 'vitest';

import { layoutVesselAnnotations } from './vesselAnnotationLayout';

const input = {
  bounds: { x: 0, y: 0, width: 1320, height: 640 },
  routes: [
    { points: [{ x: 40, y: 320 }, { x: 1280, y: 320 }], halfWidth: 18.5 },
    { points: [{ x: 990, y: 320 }, { x: 1250, y: 100 }], halfWidth: 13.5 }
  ],
  targetExclusion: { x: 456, y: 46, width: 468, height: 548 },
  obstacles: [{ x: 220, y: 250, width: 120, height: 140 }],
  vessels: [
    {
      id: 'one',
      anchor: { x: 180, y: 320 },
      angleDegrees: 0,
      hullWidth: 62,
      hullHeight: 36,
      cardWidth: 190,
      cardHeight: 58,
      priority: 2
    },
    {
      id: 'two',
      anchor: { x: 184, y: 322 },
      angleDegrees: 180,
      hullWidth: 58,
      hullHeight: 34,
      cardWidth: 190,
      cardHeight: 58,
      priority: 1
    },
    {
      id: 'three',
      anchor: { x: 1080, y: 244 },
      angleDegrees: 315,
      hullWidth: 54,
      hullHeight: 32,
      cardWidth: 190,
      cardHeight: 58,
      priority: 0
    }
  ]
};

describe('vessel annotation layout', () => {
  it('places dense cards deterministically without forcing an overlap', () => {
    const first = layoutVesselAnnotations(input);
    const second = layoutVesselAnnotations({ ...input, vessels: input.vessels.slice().reverse() });

    expect(first.unplacedIds).toEqual([]);
    expect(first.placements).toHaveLength(input.vessels.length);
    expect(second.placements).toEqual(first.placements);
    for (const [index, placement] of first.placements.entries()) {
      expect(placement.card.x).toBeGreaterThanOrEqual(12);
      expect(placement.card.y).toBeGreaterThanOrEqual(12);
      expect(placement.card.y + placement.card.height).toBeLessThanOrEqual(628);
      for (const other of first.placements.slice(index + 1)) {
        const separate =
          placement.card.x + placement.card.width + 10 <= other.card.x ||
          other.card.x + other.card.width + 10 <= placement.card.x ||
          placement.card.y + placement.card.height + 10 <= other.card.y ||
          other.card.y + other.card.height + 10 <= placement.card.y;
        expect(separate).toBe(true);
      }
    }
  });

  it('reports impossible cards instead of drawing through the route', () => {
    const result = layoutVesselAnnotations({
      bounds: { x: 0, y: 0, width: 180, height: 100 },
      routes: [{ points: [{ x: 0, y: 50 }, { x: 180, y: 50 }], halfWidth: 45 }],
      targetExclusion: { x: 0, y: 0, width: 0, height: 0 },
      vessels: [
        {
          id: 'blocked',
          anchor: { x: 90, y: 50 },
          angleDegrees: 0,
          hullWidth: 50,
          hullHeight: 30,
          cardWidth: 120,
          cardHeight: 50,
          priority: 0
        }
      ]
    });
    expect(result.placements).toEqual([]);
    expect(result.unplacedIds).toEqual(['blocked']);
  });
});
