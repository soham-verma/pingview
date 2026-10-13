import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import L from 'leaflet';
import { useMap } from 'react-leaflet';

// Renders React children as a real Leaflet control, so Leaflet stacks and
// aligns it with its own controls (zoom, attribution) in the same corner.
export default function LeafletControl({ position, children }) {
  const map = useMap();
  const [container, setContainer] = useState(null);

  useEffect(() => {
    const control = L.control({ position });
    control.onAdd = () => {
      const div = L.DomUtil.create('div', 'pv-control');
      L.DomEvent.disableClickPropagation(div);
      L.DomEvent.disableScrollPropagation(div);
      return div;
    };
    control.addTo(map);
    setContainer(control.getContainer());
    return () => control.remove();
  }, [map, position]);

  return container ? createPortal(children, container) : null;
}
