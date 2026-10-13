import { useEffect, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { FullscreenIcon, LocateIcon } from './Icons.jsx';

// A button with a small menu that closes on outside click or after a pick.
function useMenu() {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);
  useEffect(() => {
    if (!open) return undefined;
    const handler = (e) => {
      if (ref.current && !ref.current.contains(e.target)) setOpen(false);
    };
    document.addEventListener('pointerdown', handler);
    return () => document.removeEventListener('pointerdown', handler);
  }, [open]);
  const pick = (fn) => () => {
    setOpen(false);
    fn();
  };
  return { open, toggle: () => setOpen((o) => !o), pick, ref };
}

async function toggleFullscreen() {
  const win = getCurrentWindow();
  await win.setFullscreen(!(await win.isFullscreen()));
}

// Fullscreen and your location, stacked above the zoom buttons (rendered as
// a Leaflet control, see AircraftMap).
export default function RightRail({ hasLocation, locating, onLocate, onPickLocation, onClearLocation }) {
  const menu = useMenu();
  return (
    <div className="right-rail" ref={menu.ref}>
      <button type="button" className="round-btn map-float" onClick={toggleFullscreen} title="Full screen">
        <FullscreenIcon />
      </button>
      <div className="rail-slot">
        {menu.open && (
          <div className="menu menu-left map-float">
            <button type="button" onClick={menu.pick(onLocate)} disabled={locating}>
              {locating ? 'Locating…' : 'Use my location'}
            </button>
            <button type="button" onClick={menu.pick(onPickLocation)}>
              Pick on map
            </button>
            {hasLocation && (
              <button type="button" onClick={menu.pick(onClearLocation)}>
                Clear location
              </button>
            )}
          </div>
        )}
        <button
          type="button"
          className={`round-btn map-float${hasLocation ? ' on' : ''}${locating ? ' busy' : ''}`}
          onClick={menu.toggle}
          title="My location"
        >
          <LocateIcon />
        </button>
      </div>
    </div>
  );
}
