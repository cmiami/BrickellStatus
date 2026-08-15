import type { UnitSystem } from './types';

const MILES_PER_KILOMETER = 0.621_371;
const FEET_PER_METER = 3.280_84;
const MILES_PER_KNOT = 1.150_779;
const KILOMETERS_PER_KNOT = 1.852;
const KILOMETERS_PER_MILE = 1.609_344;

export function formatDistanceKilometers(kilometers: number, units: UnitSystem): string {
  return units === 'metric'
    ? `${kilometers.toFixed(1)} km`
    : `${(kilometers * MILES_PER_KILOMETER).toFixed(1)} mi`;
}

export function formatDistanceMeters(meters: number, units: UnitSystem): string {
  if (units === 'metric') {
    return meters < 1_000 ? `${meters.toFixed(0)} m` : `${(meters / 1_000).toFixed(1)} km`;
  }
  const miles = meters / 1_609.344;
  return miles < 0.5 ? `${(meters * FEET_PER_METER).toFixed(0)} ft` : `${miles.toFixed(1)} mi`;
}

export function formatSpeedKnots(knots: number, units: UnitSystem): string {
  return units === 'metric'
    ? `${(knots * KILOMETERS_PER_KNOT).toFixed(1)} km/h`
    : `${(knots * MILES_PER_KNOT).toFixed(1)} mph`;
}

export function windMphForDisplay(mph: number, units: UnitSystem): number {
  return units === 'metric' ? mph * KILOMETERS_PER_MILE : mph;
}

export function windDisplayToMph(value: number, units: UnitSystem): number {
  return units === 'metric' ? value / KILOMETERS_PER_MILE : value;
}
