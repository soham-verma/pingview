import { useEffect } from 'react';
import { useMap } from 'react-leaflet';
import { addProtocol, setWorkerUrl } from 'maplibre-gl';
// MapLibre's worker is an ES module that imports a shared chunk; let Vite
// bundle it as a worker so it loads from the app's own files.
import maplibreWorkerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url';
import 'maplibre-gl/dist/maplibre-gl.css';
import { maplibreGL } from '@maplibre/maplibre-gl-leaflet';
import { Protocol } from 'pmtiles';
import { layers, namedFlavor } from '@protomaps/basemaps';
import { convertFileSrc } from '@tauri-apps/api/core';

// The built-in world map: an OpenStreetMap vector extract (Protomaps, zoom
// 0-7, bundled with the app) drawn with MapLibre GL inside the Leaflet map.
// It sits under the online tiles, so it shows whenever they're off or missing.

const CREDIT =
  '<a href="https://www.openstreetmap.org/copyright">© OpenStreetMap</a> · <a href="https://protomaps.com">Protomaps</a>';
const WORLD_URL = convertFileSrc('basemap/world.pmtiles', 'pingview');

let protocolAdded = false;

export function webglSupported() {
  try {
    const canvas = document.createElement('canvas');
    return Boolean(canvas.getContext('webgl2') || canvas.getContext('webgl'));
  } catch {
    return false;
  }
}

function worldStyle() {
  const assets = `${window.location.origin}/basemap-assets`;
  return {
    version: 8,
    glyphs: `${assets}/fonts/{fontstack}/{range}.pbf`,
    sprite: `${assets}/sprites/v4/dark`,
    sources: {
      protomaps: { type: 'vector', url: `pmtiles://${WORLD_URL}`, attribution: '' },
    },
    layers: layers('protomaps', namedFlavor('dark'), { lang: 'en' }),
  };
}

// `onFail` is called if the map can't load (e.g. the file is missing), so
// the caller can fall back to the Natural Earth map.
export default function WorldMap({ onFail }) {
  const map = useMap();

  useEffect(() => {
    if (!protocolAdded) {
      setWorkerUrl(maplibreWorkerUrl);
      addProtocol('pmtiles', new Protocol().tile);
      protocolAdded = true;
    }
    if (!map.getPane('worldmap')) map.createPane('worldmap').style.zIndex = '170';

    const layer = maplibreGL({ style: worldStyle(), pane: 'worldmap', interactive: false });
    layer.addTo(map);
    map.attributionControl.addAttribution(CREDIT);

    const gl = layer.getMaplibreMap();
    const onError = (e) => {
      // Missing archive or unreadable header: give up on this map.
      if (e?.sourceId === 'protomaps' || /pmtiles/i.test(String(e?.error?.message ?? ''))) onFail?.();
    };
    gl?.on('error', onError);

    return () => {
      gl?.off('error', onError);
      map.removeLayer(layer);
      map.attributionControl.removeAttribution(CREDIT);
    };
  }, [map, onFail]);

  return null;
}
