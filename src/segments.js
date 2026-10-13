// Splits a time-ordered stream of aircraft positions into drawable line
// segments, following tar1090's approach rather than one continuous
// polyline: (1) reject implausible jumps before plotting, (2) break into a
// new segment on an altitude-bucket change or a stale time gap — gaps get
// their own short dashed "bridge" segment instead of a solid connecting
// line, (3) color each segment once, from its altitude.
//
// Input points: [{ lat, lon, altBaro, t }] (t = epoch ms), time-ordered.
// Used for both the live per-aircraft trail and rendering an uploaded/
// loaded history file.

import { altColor } from './planeIcon.js';
import { haversineMeters } from './geo.js';

const ALT_BUCKET_FT = 1000;
const SOUND_MPS = 343;

function altBucket(altBaro) {
  if (altBaro == null || altBaro === 'ground') return 'ground';
  return Math.round(Number(altBaro) / ALT_BUCKET_FT);
}

function rejectImplausibleJumps(points, maxPlausibleMachSpeed) {
  const clean = [];
  let prev = null;
  for (const p of points) {
    if (p.lat == null || p.lon == null) continue;
    if (prev) {
      const dtS = (p.t - prev.t) / 1000;
      if (dtS > 0) {
        const distM = haversineMeters(prev.lat, prev.lon, p.lat, p.lon);
        const machSpeed = distM / dtS / SOUND_MPS;
        if (machSpeed > maxPlausibleMachSpeed) continue; // discard, keep prev as the anchor
      }
    }
    clean.push(p);
    prev = p;
  }
  return clean;
}

function finalizeSegment(seg) {
  return {
    points: seg.points.map((p) => [p.lat, p.lon]),
    altBaro: seg.altBaro,
    estimated: false,
    color: altColor(seg.altBaro),
  };
}

export function buildSegments(points, opts = {}) {
  const { staleAirborneMs = 15000, staleGroundMs = 30000, maxPlausibleMachSpeed = 3.5 } = opts;

  const clean = rejectImplausibleJumps(points, maxPlausibleMachSpeed);
  const segments = [];
  let seg = null;

  for (const p of clean) {
    const bucket = altBucket(p.altBaro);

    if (seg) {
      const gapMs = p.t - seg.lastPoint.t;
      const staleThreshold = seg.bucket === 'ground' ? staleGroundMs : staleAirborneMs;

      if (gapMs > staleThreshold) {
        segments.push({
          points: [
            [seg.lastPoint.lat, seg.lastPoint.lon],
            [p.lat, p.lon],
          ],
          altBaro: p.altBaro,
          estimated: true,
          color: altColor(p.altBaro),
        });
        seg = null;
      } else if (bucket !== seg.bucket) {
        segments.push(finalizeSegment(seg));
        seg = null;
      }
    }

    if (!seg) {
      seg = { points: [p], bucket, altBaro: p.altBaro, lastPoint: p };
    } else {
      seg.points.push(p);
      seg.lastPoint = p;
    }
  }
  if (seg) segments.push(finalizeSegment(seg));

  return segments;
}
