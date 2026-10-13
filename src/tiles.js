import { convertFileSrc } from '@tauri-apps/api/core';

// The map that ships with the app (Natural Earth): no key, no internet.
export const BUILTIN_STYLE = 'builtin';
export const DEFAULT_STYLE = BUILTIN_STYLE;
// What saving a MapTiler key switches the offline map to.
export const DEFAULT_MAPTILER_STYLE = 'dataviz-dark';

export const isCustomStyle = (style) => style?.startsWith('custom-');
export const isMapTilerStyle = (style) => Boolean(style) && style !== BUILTIN_STYLE && !isCustomStyle(style);

export function newCustomId() {
  return Math.random().toString(36).slice(2, 10);
}

export const MAPTILER_ATTRIBUTION =
  '<a href="https://www.maptiler.com/copyright/">&copy; MapTiler</a> ' +
  '<a href="https://www.openstreetmap.org/copyright">&copy; OpenStreetMap contributors</a>';

// Tiles come from the Rust side (MapTiler + an on-disk cache) through the
// pingview:// scheme. convertFileSrc gives the right form per OS
// (pingview://localhost on macOS/Linux, http://pingview.localhost on Windows).
const BASE = convertFileSrc('tiles', 'pingview');

export function tileLayerUrl(style) {
  // Users' own maps come at one resolution; MapTiler has retina tiles.
  const scale = window.devicePixelRatio > 1 && isMapTilerStyle(style) ? '@2x' : '';
  return `${BASE}/${style}/{z}/{x}/{y}${scale}.png`;
}

// A real tile of each style (zoom 6 over Sydney) for the Map Options cards.
export function styleThumbnail(style) {
  return `${BASE}/${style}/6/58/38@2x.png`;
}
