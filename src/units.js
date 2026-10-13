import { createContext, useContext } from 'react';

// Display units. Values are stored in the source units (feet, knots, metres,
// °C, hPa) and converted only when shown.

export const DEFAULT_UNITS = {
  timezone: 'local',
  designator: 'iata',
  altitude: 'ft',
  speed: 'kt',
  coords: 'decimal',
  distance: 'nm',
  temperature: 'c',
  altimeter: 'hpa',
};

export const UNIT_OPTIONS = [
  { key: 'timezone', label: 'Timezone', options: [['local', 'Local'], ['utc', 'UTC']] },
  { key: 'designator', label: 'Airport codes', options: [['iata', 'IATA'], ['icao', 'ICAO']] },
  { key: 'altitude', label: 'Altitude', options: [['ft', 'Feet'], ['m', 'Meters']] },
  { key: 'speed', label: 'Speed', options: [['kmh', 'km/h'], ['mph', 'mph'], ['kt', 'knots']] },
  { key: 'coords', label: 'Coordinates', options: [['decimal', 'Decimals'], ['dms', 'Degrees']] },
  { key: 'distance', label: 'Distance', options: [['km', 'km'], ['mi', 'mi'], ['nm', 'NM']] },
  { key: 'temperature', label: 'Temperature', options: [['f', '°F'], ['c', '°C']] },
  { key: 'altimeter', label: 'Altimeter setting', options: [['inhg', 'inHg'], ['hpa', 'hPa']] },
];

export const UnitsContext = createContext(DEFAULT_UNITS);
export const useUnits = () => useContext(UnitsContext);

const NA = 'n/a';
const M_PER_FT = 0.3048;
const KMH_PER_KT = 1.852;
const MPH_PER_KT = 1.150779;
const INHG_PER_HPA = 0.02953;
const METERS = { km: 1000, mi: 1609.344, nm: 1852 };

const n = (v, digits = 0) =>
  Number(v).toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits });

export function fmtAltitude(ft, u) {
  if (ft == null) return NA;
  return u.altitude === 'm' ? `${n(ft * M_PER_FT)} m` : `${n(ft)} ft`;
}

// Vertical rate follows the altitude unit: ft/min, or m/s for metres.
export function verticalUnit(u) {
  return u.altitude === 'm' ? 'm/s' : 'fpm';
}

export function fmtVertical(fpm, u, { signed = false } = {}) {
  if (fpm == null) return NA;
  const v = u.altitude === 'm' ? (fpm * M_PER_FT) / 60 : fpm;
  const text = n(Math.abs(v), u.altitude === 'm' ? 1 : 0);
  const sign = signed ? (v > 0 ? '+' : v < 0 ? '−' : '') : '';
  return `${sign}${text}`;
}

export function fmtSpeed(kt, u) {
  if (kt == null) return NA;
  if (u.speed === 'kmh') return `${n(kt * KMH_PER_KT)} km/h`;
  if (u.speed === 'mph') return `${n(kt * MPH_PER_KT)} mph`;
  return `${n(kt)} kt`;
}

export function fmtDistance(meters, u) {
  if (meters == null) return NA;
  const unit = METERS[u.distance] ? u.distance : 'nm';
  const label = unit === 'nm' ? 'nm' : unit;
  return `${n(meters / METERS[unit], 1)} ${label}`;
}

function dms(value, pos, neg) {
  const abs = Math.abs(value);
  let d = Math.floor(abs);
  let m = Math.floor((abs - d) * 60);
  let s = Math.round(((abs - d) * 60 - m) * 60);
  if (s === 60) {
    s = 0;
    m += 1;
  }
  if (m === 60) {
    m = 0;
    d += 1;
  }
  const pad = (x) => String(x).padStart(2, '0');
  return `${d}°${pad(m)}′${pad(s)}″${value < 0 ? neg : pos}`;
}

export function fmtCoords(lat, lon, u) {
  if (lat == null || lon == null) return NA;
  if (u.coords === 'dms') return `${dms(lat, 'N', 'S')} ${dms(lon, 'E', 'W')}`;
  return `${lat.toFixed(4)}°, ${lon.toFixed(4)}°`;
}

export function fmtTemperature(c, u) {
  if (c == null) return NA;
  return u.temperature === 'f' ? `${n((c * 9) / 5 + 32)} °F` : `${n(c)} °C`;
}

export function fmtAltimeter(hpa, u) {
  if (hpa == null) return NA;
  return u.altimeter === 'inhg' ? `${n(hpa * INHG_PER_HPA, 2)} inHg` : `${n(hpa, 1)} hPa`;
}

export function fmtTime(date, u) {
  const opts = { hour: '2-digit', minute: '2-digit', hour12: false };
  if (u.timezone === 'utc') {
    return `${date.toLocaleTimeString([], { ...opts, timeZone: 'UTC' })} UTC`;
  }
  return date.toLocaleTimeString([], { ...opts, timeZoneName: 'short' });
}

// The airport code shown large, and the other one shown small.
export function airportCodes(airport, u) {
  if (u.designator === 'icao' || !airport.iata) {
    return { main: airport.icao, other: airport.iata ?? null };
  }
  return { main: airport.iata, other: airport.icao };
}

// Altitude legend tick labels (the colour stops are defined in feet).
export function legendLabels(u) {
  if (u.altitude === 'm') return ['0', '4.6k', '9.1k', '15.2k m'];
  return ['0', '15k', '30k', '50k ft'];
}
