import { useCallback, useEffect, useRef, useState, Fragment } from 'react';
import { MapContainer, TileLayer, Marker, Polyline, Tooltip, ZoomControl, useMap, useMapEvents } from 'react-leaflet';
import L from 'leaflet';
import { altColor, createPlaneIcon } from './planeIcon.js';
import AerodromeLayer from './AerodromeLayer.jsx';
import LeafletControl from './LeafletControl.jsx';
import NetworkTraffic from './NetworkTraffic.jsx';
import OfflineBasemap from './OfflineBasemap.jsx';
import WorldMap, { webglSupported } from './WorldMap.jsx';
import { buildSegments } from './segments.js';
import { BUILTIN_STYLE, DEFAULT_STYLE, MAPTILER_ATTRIBUTION, isCustomStyle, tileLayerUrl } from './tiles.js';

const MAX_TRAIL_POINTS = 200;

function FitBoundsOnce({ aircraft }) {
  const map = useMap();
  const doneRef = useRef(false);

  useEffect(() => {
    if (doneRef.current || aircraft.length === 0) return;
    const bounds = L.latLngBounds(aircraft.map((ac) => [ac.lat, ac.lon]));
    map.fitBounds(bounds, { padding: [40, 40], maxZoom: 9 });
    doneRef.current = true;
  }, [aircraft, map]);

  return null;
}

function MapClickHandler({ active, onSetLocation, onDeselect }) {
  useMapEvents({
    click(e) {
      if (active) onSetLocation(e.latlng.lat, e.latlng.lng);
      else onDeselect();
    },
  });
  return null;
}

// Pans to the selected aircraft when selection changes from outside the map
// itself (e.g. clicking a row in the aircraft list panel).
function FocusSelected({ selectedHex, markerRefs }) {
  const map = useMap();
  useEffect(() => {
    if (!selectedHex) return;
    const marker = markerRefs.current.get(selectedHex);
    if (!marker) return;
    map.panTo(marker.getLatLng(), { animate: true });
  }, [selectedHex, map, markerRefs]);
  return null;
}

const referenceIcon = L.divIcon({
  html: '<div style="width:14px;height:14px;border-radius:50%;background:var(--accent);border:2px solid white;"></div>',
  className: 'reference-marker',
  iconSize: [14, 14],
  iconAnchor: [7, 7],
});

export default function AircraftMap({
  aircraft,
  selectedHex,
  onSelect,
  referenceLocation,
  setLocationMode,
  onSetLocation,
  historyTracks,
  mapStyle = DEFAULT_STYLE,
  customMap = null,
  tileKey = '',
  showAerodromes = true,
  showLabels = true,
  showTrails = true,
  controls = null,
  showNetwork = false,
  airports = {},
  aircraftScale = 1,
  onCenterChange,
}) {
  const [worldMapOk, setWorldMapOk] = useState(webglSupported);
  const onWorldMapFail = useCallback(() => setWorldMapOk(false), []);
  const pointsRef = useRef(new Map()); // hex -> [{lat, lon, altBaro, t}]
  const markerRefs = useRef(new Map()); // hex -> Leaflet marker instance
  const [, forceRender] = useState(0);

  // Keep a short in-memory position history per aircraft (no persistence —
  // cleared on reload; the bridge separately records to disk for history).
  useEffect(() => {
    const points = pointsRef.current;
    const seen = new Set();
    const now = Date.now();
    for (const ac of aircraft) {
      if (ac.lat == null || ac.lon == null) continue;
      seen.add(ac.hex);
      const pts = points.get(ac.hex) || [];
      const last = pts[pts.length - 1];
      if (!last || last.lat !== ac.lat || last.lon !== ac.lon) {
        pts.push({ lat: ac.lat, lon: ac.lon, altBaro: ac.alt_baro, t: now });
        if (pts.length > MAX_TRAIL_POINTS) pts.shift();
        points.set(ac.hex, pts);
      }
    }
    for (const hex of points.keys()) {
      if (!seen.has(hex)) points.delete(hex);
    }
    forceRender((n) => n + 1);
  }, [aircraft]);

  return (
    <>
      <MapContainer
        center={[-25, 134]}
        zoom={4}
        minZoom={2}
        maxZoom={18}
        worldCopyJump
        zoomControl={false}
        style={{ height: '100%', width: '100%' }}
      >
        {/* Built-in maps: the OpenStreetMap world map when WebGL works and the
            file loads, otherwise the small Natural Earth map. */}
        {worldMapOk ? <WorldMap onFail={onWorldMapFail} /> : <OfflineBasemap />}
        <ZoomControl position="bottomright" />
        {/* Added after zoom, so Leaflet stacks it directly above. */}
        {controls && <LeafletControl position="bottomright">{controls}</LeafletControl>}
        {/* The built-in offline map needs no tile layer: it's drawn below. */}
        {mapStyle !== BUILTIN_STYLE && (
          <TileLayer
            key={`${mapStyle}:${tileKey}:${customMap?.max_zoom ?? ''}`}
            attribution={isCustomStyle(mapStyle) ? customMap?.attribution ?? '' : MAPTILER_ATTRIBUTION}
            url={tileLayerUrl(mapStyle)}
            maxNativeZoom={isCustomStyle(mapStyle) ? customMap?.max_zoom ?? 19 : 22}
            maxZoom={18}
          />
        )}
        <FitBoundsOnce aircraft={aircraft} />
        <MapClickHandler
          active={setLocationMode}
          onSetLocation={onSetLocation}
          onDeselect={() => onSelect(null)}
        />
        <FocusSelected selectedHex={selectedHex} markerRefs={markerRefs} />
        {showAerodromes && (
          <AerodromeLayer labels={airports.labels} scale={airports.scale} showHeliports={airports.heliports} />
        )}
        {onCenterChange && <NetworkTraffic enabled={showNetwork} onCenter={onCenterChange} />}

        {referenceLocation && (
          <Marker position={[referenceLocation.lat, referenceLocation.lon]} icon={referenceIcon} />
        )}

        {(historyTracks || []).map((track) => {
          const segments = buildSegments(track.points);
          return segments.map((seg, i) => (
            <Polyline
              key={`hist-${track.icao}-${i}`}
              positions={seg.points}
              pathOptions={{ color: seg.color, weight: 2, opacity: 0.3, dashArray: seg.estimated ? '4 6' : null }}
            />
          ));
        })}

        {aircraft.map((ac) => {
          const color = altColor(ac.alt_baro);
          const selected = ac.hex === selectedHex;
          const points = pointsRef.current.get(ac.hex) || [];
          const segments = buildSegments(points);

          return (
            <Fragment key={ac.hex}>
              {showTrails && segments.map((seg, i) => (
                <Polyline
                  key={i}
                  positions={seg.points}
                  pathOptions={{
                    color: seg.color,
                    weight: 1.5,
                    opacity: seg.estimated ? 0.35 : 0.6,
                    dashArray: seg.estimated ? '4 6' : null,
                  }}
                />
              ))}
              <Marker
                position={[ac.lat, ac.lon]}
                icon={createPlaneIcon({ track: ac.track, color, selected, scale: aircraftScale })}
                eventHandlers={{ click: () => onSelect(ac.hex) }}
                ref={(m) => {
                  if (m) markerRefs.current.set(ac.hex, m);
                  else markerRefs.current.delete(ac.hex);
                }}
              >
                {(showLabels || selected) && (
                  <Tooltip
                    permanent
                    direction="right"
                    offset={[Math.round((selected ? 15 : 11) * aircraftScale), 0]}
                    className={`ac-label${selected ? ' selected' : ''}`}
                  >
                    {ac.flight || ac.registration || ac.hex.toUpperCase()}
                    {ac.baro_rate > 150 && <span className="vs-up"> ↑</span>}
                    {ac.baro_rate < -150 && <span className="vs-down"> ↓</span>}
                  </Tooltip>
                )}
              </Marker>
            </Fragment>
          );
        })}
      </MapContainer>
    </>
  );
}
