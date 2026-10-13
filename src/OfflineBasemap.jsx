import { useEffect, useState } from 'react';
import L from 'leaflet';
import { CircleMarker, GeoJSON, Tooltip, useMapEvents } from 'react-leaflet';
import basemap from './basemap/basemap.json';

// Bundled Natural Earth map drawn *under* the online tiles. Online, the
// tiles cover it; offline (or local-only, or a tile that failed) it shows
// through, so the map is never blank.

const CREDIT = '<a href="https://www.naturalearthdata.com">Natural Earth</a>';

const LAND = { color: '#3a4558', weight: 0.8, fillColor: '#1b2230', fillOpacity: 1 };
const LAKE = { color: '#2a3446', weight: 0.6, fillColor: '#0e1520', fillOpacity: 1 };
const BORDER = { color: '#56627a', weight: 1, opacity: 0.9 };
const STATE = { color: '#4a5569', weight: 0.8, dashArray: '4 4', opacity: 0.9 };
const road = (f) => ({
  color: '#5b6476',
  weight: f.properties.major ? 1.2 : 0.7,
  opacity: f.properties.major ? 0.9 : 0.6,
});

// Town labels thin out as you zoom out (lower rank = more important).
function placeVisible(rank, zoom) {
  if (zoom >= 9) return true;
  if (zoom >= 7) return rank <= 7;
  if (zoom >= 5) return rank <= 4;
  return rank <= 2;
}

export default function OfflineBasemap() {
  const map = useMapEvents({ zoomend: () => setZoom(map.getZoom()) });
  const [zoom, setZoom] = useState(map.getZoom());
  const [renderer, setRenderer] = useState(null);

  // Panes are created once and never removed, and the canvas is tied to this
  // mount: React can mount twice (StrictMode), and a canvas left attached to
  // a removed pane draws where nobody can see it.
  useEffect(() => {
    for (const [name, z] of [
      ['basemap', 150],
      ['basemap-labels', 160],
    ]) {
      if (!map.getPane(name)) map.createPane(name).style.zIndex = String(z);
    }
    // One canvas for all the shapes: thousands of SVG paths would be slow.
    const canvas = L.canvas({ pane: 'basemap', padding: 0.5 });
    setRenderer(canvas);
    map.attributionControl.addAttribution(CREDIT);
    return () => {
      map.removeLayer(canvas);
      map.attributionControl.removeAttribution(CREDIT);
    };
  }, [map]);

  if (!renderer) return null;
  const places = basemap.au_places.features.filter((f) => placeVisible(f.properties.rank, zoom));

  return (
    <>
      <GeoJSON data={basemap.world_land} style={LAND} renderer={renderer} pane="basemap" interactive={false} />
      <GeoJSON data={basemap.au_land} style={LAND} renderer={renderer} pane="basemap" interactive={false} />
      <GeoJSON data={basemap.world_lakes} style={LAKE} renderer={renderer} pane="basemap" interactive={false} />
      <GeoJSON data={basemap.au_lakes} style={LAKE} renderer={renderer} pane="basemap" interactive={false} />
      <GeoJSON data={basemap.world_borders} style={BORDER} renderer={renderer} pane="basemap" interactive={false} />
      <GeoJSON data={basemap.au_states} style={STATE} renderer={renderer} pane="basemap" interactive={false} />
      {zoom >= 5 && (
        <GeoJSON data={basemap.au_roads} style={road} renderer={renderer} pane="basemap" interactive={false} />
      )}
      {places.map((f) => {
        const [lon, lat] = f.geometry.coordinates;
        return (
          <CircleMarker
            key={`${f.properties.name}-${lat}`}
            center={[lat, lon]}
            radius={2}
            pane="basemap-labels"
            interactive={false}
            pathOptions={{ color: '#9aa3b2', weight: 1, fillColor: '#9aa3b2', fillOpacity: 1 }}
          >
            <Tooltip permanent direction="right" offset={[4, 0]} pane="basemap-labels" className="place-label">
              {f.properties.name}
            </Tooltip>
          </CircleMarker>
        );
      })}
    </>
  );
}
