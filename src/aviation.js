import airlines from '../aviation-data/airlines.json';
import aerodromeData from '../aviation-data/australia_aerodromes.json';
import { bearingDegrees, haversineMeters } from './geo.js';

// Open aerodromes only; closed strips and the few nav-aid points are dropped.
export const AERODROMES = aerodromeData.records.filter(
  (r) => r.feature_kind === 'aerodrome' && !r.is_closed && r.airport_type !== 'closed',
);

const RUNWAY_TYPES = new Set(['large_airport', 'medium_airport', 'small_airport']);

// "QLK227D" → { name: 'QantasLink', telephony: 'QLINK', spoken: 'QLINK 227D' }.
// Only airline-style callsigns (three-letter ICAO prefix + flight number).
export function airlineFor(callsign) {
  const m = /^([A-Z]{3})(\d[A-Z0-9]*)$/.exec((callsign ?? '').trim());
  const airline = m && airlines[m[1]];
  if (!airline) return null;
  return { ...airline, spoken: `${airline.telephony} ${m[2]}` };
}

// Nearest open airport (heliports excluded) to a position.
export function nearestAerodrome(lat, lon) {
  let best = null;
  for (const a of AERODROMES) {
    if (!RUNWAY_TYPES.has(a.airport_type)) continue;
    const meters = haversineMeters(lat, lon, a.latitude, a.longitude);
    if (!best || meters < best.meters) best = { aerodrome: a, meters };
  }
  if (!best) return null;
  return {
    ...best,
    bearing: bearingDegrees(lat, lon, best.aerodrome.latitude, best.aerodrome.longitude),
  };
}

export function aerodromeCode(a) {
  return a.icao_code || a.ident;
}
