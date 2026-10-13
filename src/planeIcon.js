import L from 'leaflet';

// Simple top-down plane silhouette; rotated in CSS to match track heading
// (0deg = pointing north, matching ADS-B track convention).
const PLANE_PATH =
  'M12 2 L15 9 L22 12 L15 13 L14 20 L17 22 L12 20 L7 22 L10 20 L9 13 L2 12 L9 9 Z';

// Altitude -> color ramp: orange at low altitude, through yellow/green/sky,
// to pink at high altitude. Exported so the legend can render the same
// gradient. Ground / unknown altitude renders slate gray.
export const ON_GROUND_COLOR = '#64748b';

export const ALTITUDE_STOPS = [
  [500, '#C2410C'],
  [5000, '#EA580C'],
  [10000, '#FB923C'],
  [15000, '#FACC15'],
  [20000, '#22C55E'],
  [25000, '#7DD3FC'],
  [30000, '#1E40AF'],
  [40000, '#EC4899'],
  [50000, '#F472B6'],
];

function hexToRgb(hex) {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function lerp(a, b, t) {
  return Math.round(a + (b - a) * t);
}

export function altColor(altBaro) {
  if (altBaro == null || altBaro === 'ground') return ON_GROUND_COLOR;
  const alt = Number(altBaro) || 0;

  if (alt <= ALTITUDE_STOPS[0][0]) return ALTITUDE_STOPS[0][1];
  const last = ALTITUDE_STOPS[ALTITUDE_STOPS.length - 1];
  if (alt >= last[0]) return last[1];

  for (let i = 0; i < ALTITUDE_STOPS.length - 1; i++) {
    const [lowAlt, lowColor] = ALTITUDE_STOPS[i];
    const [highAlt, highColor] = ALTITUDE_STOPS[i + 1];
    if (alt >= lowAlt && alt <= highAlt) {
      const t = (alt - lowAlt) / (highAlt - lowAlt);
      const [r1, g1, b1] = hexToRgb(lowColor);
      const [r2, g2, b2] = hexToRgb(highColor);
      return `rgb(${lerp(r1, r2, t)}, ${lerp(g1, g2, t)}, ${lerp(b1, b2, t)})`;
    }
  }
  return last[1];
}

export function createPlaneIcon({ track, color, selected, scale = 1 }) {
  const rotation = Number.isFinite(track) ? track : 0;
  const size = Math.round((selected ? 30 : 22) * scale);
  const html = `
    <svg width="${size}" height="${size}" viewBox="0 0 24 24"
         style="transform: rotate(${rotation}deg); filter: drop-shadow(0 0 2px rgba(0,0,0,0.6));">
      <path d="${PLANE_PATH}" fill="${color}" stroke="#111" stroke-width="0.75" />
    </svg>`;
  return L.divIcon({
    html,
    className: 'plane-icon',
    iconSize: [size, size],
    iconAnchor: [size / 2, size / 2],
  });
}
