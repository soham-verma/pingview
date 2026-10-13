import { useEffect, useRef, useState } from 'react';
import AltitudeLegend from './AltitudeLegend.jsx';
import { GearIcon, HistoryIcon, MapOptionsIcon, MoreIcon } from './Icons.jsx';

const MORE = [
  { id: 'history', label: 'History', Icon: HistoryIcon },
  { id: 'settings', label: 'Settings', Icon: GearIcon },
];

// Bottom-left: Map Options, plus everything else behind "More". (Receiver
// details open from the status pill, top-left.)
export default function BottomDock({ panel, onToggle }) {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);

  useEffect(() => {
    if (!open) return undefined;
    const close = (e) => {
      if (ref.current && !ref.current.contains(e.target)) setOpen(false);
    };
    document.addEventListener('pointerdown', close);
    return () => document.removeEventListener('pointerdown', close);
  }, [open]);

  const moreActive = MORE.some((m) => m.id === panel);

  return (
    <div className="bottom-bar">
      <nav className="dock map-float" ref={ref}>
        <button
          type="button"
          className={`dock-btn${panel === 'map' ? ' on' : ''}`}
          onClick={() => onToggle('map')}
        >
          <span className="dock-icon">
            <MapOptionsIcon />
          </span>
          Map Options
        </button>
        <button
          type="button"
          className={`dock-btn${moreActive || open ? ' on' : ''}`}
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
        >
          <span className="dock-icon">
            <MoreIcon />
          </span>
          More
        </button>

        {open && (
          <div className="menu dock-menu map-float">
            {MORE.map(({ id, label, Icon }) => (
              <button
                key={id}
                type="button"
                onClick={() => {
                  setOpen(false);
                  onToggle(id);
                }}
              >
                <Icon />
                {label}
              </button>
            ))}
          </div>
        )}
      </nav>
      <AltitudeLegend />
    </div>
  );
}
