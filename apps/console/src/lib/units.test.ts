import { describe, expect, it } from 'vitest';

import {
  formatDistanceKilometers,
  formatDistanceMeters,
  formatSpeedKnots,
  windDisplayToMph,
  windMphForDisplay
} from './units';

describe('unit presentation', () => {
  it('formats stored metric distances for the selected system', () => {
    expect(formatDistanceKilometers(12, 'imperial')).toBe('7.5 mi');
    expect(formatDistanceKilometers(12, 'metric')).toBe('12.0 km');
    expect(formatDistanceMeters(250, 'imperial')).toBe('820 ft');
    expect(formatDistanceMeters(250, 'metric')).toBe('250 m');
  });

  it('converts vessel and wind speeds without changing stored thresholds', () => {
    expect(formatSpeedKnots(10, 'imperial')).toBe('11.5 mph');
    expect(formatSpeedKnots(10, 'metric')).toBe('18.5 km/h');
    expect(windDisplayToMph(windMphForDisplay(40, 'metric'), 'metric')).toBeCloseTo(40);
  });
});
