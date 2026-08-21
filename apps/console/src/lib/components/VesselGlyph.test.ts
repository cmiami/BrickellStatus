import { cleanup, render } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';

import VesselGlyph from './VesselGlyph.svelte';

afterEach(cleanup);

function glyph(container: HTMLElement): SVGGElement {
  const node = container.querySelector<SVGGElement>('.vessel-glyph');
  expect(node).toBeTruthy();
  return node!;
}

describe('VesselGlyph', () => {
  it.each([undefined, 'vessel', 'unrecognised AIS class'])(
    'uses the solid generic Miami motor-yacht for %s',
    (kind) => {
      const { container } = render(VesselGlyph, { kind });
      const node = glyph(container);

      expect(node).toHaveAttribute('data-family', 'generic-motor-yacht');
      expect(node.querySelectorAll('.hull')).toHaveLength(1);
      expect(node.querySelectorAll('.house')).toHaveLength(2);
      expect(node.querySelector('text')).toBeNull();
      expect(node.querySelector('[stroke-dasharray]')).toBeNull();
      expect(node.textContent).not.toContain('?');
    }
  );

  it.each([
    ['tug', 'tug'],
    ['tug + tow', 'tug'],
    ['cargo', 'cargo'],
    ['tanker', 'tanker'],
    ['sailing', 'sailing'],
    ['pleasure craft', 'yacht'],
    ['passenger', 'passenger'],
    ['fishing', 'fishing'],
    ['pilot', 'pilot']
  ])('preserves the %s AIS silhouette as %s', (kind, family) => {
    const { container } = render(VesselGlyph, { kind });
    expect(glyph(container)).toHaveAttribute('data-family', family);
  });

  it('keeps opener emphasis on the generic silhouette', () => {
    const { container } = render(VesselGlyph, { opener: true });
    const node = glyph(container);

    expect(node).toHaveAttribute('data-family', 'generic-motor-yacht');
    expect(node).toHaveClass('is-opener');
  });
});
