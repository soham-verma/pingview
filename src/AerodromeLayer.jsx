import { useMemo, useState } from 'react';
import { CircleMarker, Pane, Popup, Tooltip, useMapEvents } from 'react-leaflet';
import { AERODROMES, aerodromeCode } from './aviation.js';
import AerodromeCard from './AerodromeCard.jsx';

const MAX_VISIBLE = 400;

function isMajor(a) {
  return a.controlled || a.airport_type === 'large_airport' || a.airport_type === 'medium_airport';
}

// What shows at each zoom: large/controlled airports always, other
// airports from zoom 5, small strips from 7, heliports and seaplane bases
// from 11 (if heliports are on).
function visibleAt(a, zoom, showHeliports) {
  if (a.controlled || a.airport_type === 'large_airport') return true;
  if (a.airport_type === 'medium_airport') return zoom >= 5;
  if (a.airport_type === 'small_airport') return zoom >= 7;
  return showHeliports && zoom >= 11;
}

// Labels appear later than markers so the map never turns into a wall of
// codes: major airports from zoom 8, small strips from 10, the rest from 13.
function labelledAt(a, zoom) {
  if (isMajor(a)) return zoom >= 8;
  if (a.airport_type === 'small_airport') return zoom >= 10;
  return zoom >= 13;
}

function importance(a) {
  if (a.controlled) return 0;
  return { large_airport: 1, medium_airport: 2, small_airport: 3 }[a.airport_type] ?? 4;
}

function style(a) {
  if (a.controlled) return { radius: 6, color: '#7DD3FC' };
  if (a.airport_type === 'large_airport' || a.airport_type === 'medium_airport') {
    return { radius: 5, color: '#E5E7EB' };
  }
  if (a.airport_type === 'small_airport') return { radius: 3.5, color: '#9CA3AF' };
  return { radius: 3, color: '#C084FC' };
}

export default function AerodromeLayer({ labels = 'always', scale = 1, showHeliports = true }) {
  const map = useMapEvents({
    moveend: () => setView({ bounds: map.getBounds(), zoom: map.getZoom() }),
    zoomend: () => setView({ bounds: map.getBounds(), zoom: map.getZoom() }),
  });
  const [view, setView] = useState(() => ({ bounds: map.getBounds(), zoom: map.getZoom() }));

  const visible = useMemo(() => {
    const padded = view.bounds.pad(0.2);
    return AERODROMES.filter(
      (a) => visibleAt(a, view.zoom, showHeliports) && padded.contains([a.latitude, a.longitude]),
    )
      .sort((x, y) => importance(x) - importance(y))
      .slice(0, MAX_VISIBLE);
  }, [view, showHeliports]);

  // Own panes below Leaflet's marker pane (600), so aircraft and their
  // trails always draw on top of aerodromes and their labels.
  return (
    <Pane name="aerodromes" style={{ zIndex: 350 }}>
      <Pane name="aerodrome-labels" style={{ zIndex: 360 }} />
      {visible.map((a) => {
        const { radius, color } = style(a);
        return (
          <CircleMarker
            key={a.ident}
            center={[a.latitude, a.longitude]}
            radius={radius * scale}
            pane="aerodromes"
            pathOptions={{ color, weight: 1.5, fillColor: color, fillOpacity: 0.25 }}
          >
            {(labels === 'hover' || labelledAt(a, view.zoom)) && (
              <Tooltip
                key={labels}
                permanent={labels !== 'hover'}
                direction="right"
                offset={[radius * scale + 2, 0]}
                pane="aerodrome-labels"
                className="aerodrome-label"
              >
                {aerodromeCode(a)}
              </Tooltip>
            )}
            <Popup pane="popupPane" className="ad-popup" closeButton={false} maxWidth={360} minWidth={340}>
              <AerodromeCard a={a} />
            </Popup>
          </CircleMarker>
        );
      })}
    </Pane>
  );
}
