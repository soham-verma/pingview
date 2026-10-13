import { useEffect } from 'react';
import { useMap, useMapEvents } from 'react-leaflet';

const CREDIT =
  'Traffic: <a href="https://adsb.lol">adsb.lol</a> (ODbL) · <a href="https://adsb.fi">adsb.fi</a>';

// Inside the map: reports the map centre as it moves, and credits the
// network services in the attribution while network traffic is shown.
export default function NetworkTraffic({ enabled, onCenter }) {
  const map = useMapEvents({
    moveend: () => onCenter(map.getCenter()),
  });

  useEffect(() => {
    onCenter(map.getCenter());
  }, [map, onCenter]);

  useEffect(() => {
    if (!enabled) return undefined;
    map.attributionControl.addAttribution(CREDIT);
    return () => map.attributionControl.removeAttribution(CREDIT);
  }, [map, enabled]);

  return null;
}
