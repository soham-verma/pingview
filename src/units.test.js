import { describe, expect, it } from 'vitest';
import {
  DEFAULT_UNITS,
  airportCodes,
  fmtAltimeter,
  fmtAltitude,
  fmtCoords,
  fmtDistance,
  fmtSpeed,
  fmtTemperature,
  fmtTime,
  fmtVertical,
} from './units.js';

const u = (patch) => ({ ...DEFAULT_UNITS, ...patch });

describe('unit formatting', () => {
  it('altitude', () => {
    expect(fmtAltitude(11650, u())).toBe('11,650 ft');
    expect(fmtAltitude(10000, u({ altitude: 'm' }))).toBe('3,048 m');
    expect(fmtAltitude(null, u())).toBe('n/a');
  });

  it('vertical rate follows the altitude unit', () => {
    expect(fmtVertical(-1982, u())).toBe('1,982');
    expect(fmtVertical(1000, u(), { signed: true })).toBe('+1,000');
    expect(fmtVertical(1000, u({ altitude: 'm' }))).toBe('5.1'); // 1000 ft/min ≈ 5.08 m/s
  });

  it('speed', () => {
    expect(fmtSpeed(300, u())).toBe('300 kt');
    expect(fmtSpeed(100, u({ speed: 'kmh' }))).toBe('185 km/h');
    expect(fmtSpeed(100, u({ speed: 'mph' }))).toBe('115 mph');
  });

  it('distance', () => {
    expect(fmtDistance(1852 * 15.9, u())).toBe('15.9 nm');
    expect(fmtDistance(29400, u({ distance: 'km' }))).toBe('29.4 km');
    expect(fmtDistance(1609.344 * 2, u({ distance: 'mi' }))).toBe('2.0 mi');
  });

  it('coordinates in decimals and degrees', () => {
    expect(fmtCoords(-34.1673, 150.6823, u())).toBe('-34.1673°, 150.6823°');
    expect(fmtCoords(-33.9461, 151.177, u({ coords: 'dms' }))).toBe('33°56′46″S 151°10′37″E');
    // Rounding up to a whole minute must not print 60″.
    expect(fmtCoords(10.9999999, 0, u({ coords: 'dms' }))).toBe('11°00′00″N 0°00′00″E');
  });

  it('temperature and altimeter setting', () => {
    expect(fmtTemperature(-40, u({ temperature: 'f' }))).toBe('-40 °F');
    expect(fmtTemperature(15, u())).toBe('15 °C');
    expect(fmtAltimeter(1013.25, u({ altimeter: 'inhg' }))).toBe('29.92 inHg');
    expect(fmtAltimeter(1026, u())).toBe('1,026.0 hPa');
  });

  it('time in UTC', () => {
    expect(fmtTime(new Date('2026-10-13T09:05:00Z'), u({ timezone: 'utc' }))).toBe('09:05 UTC');
  });

  it('airport codes', () => {
    const syd = { iata: 'SYD', icao: 'YSSY' };
    expect(airportCodes(syd, u())).toEqual({ main: 'SYD', other: 'YSSY' });
    expect(airportCodes(syd, u({ designator: 'icao' }))).toEqual({ main: 'YSSY', other: 'SYD' });
    expect(airportCodes({ icao: 'YSCN', iata: null }, u())).toEqual({ main: 'YSCN', other: null });
  });
});
