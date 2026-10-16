import { useState } from 'react';
import { fmtBattery } from './units.js';

// Shown while the receiver's battery is critical. The receiver can't switch
// itself off (the Ping runs straight off the 3.3 V rail), so it would drain
// the cell until the pack's own protection cuts in. A dismissal lasts until
// the battery recovers (charged or swapped) and this unmounts.
export default function BatteryNotice({ battery }) {
  const [dismissed, setDismissed] = useState(false);
  if (dismissed) return null;

  return (
    <div className="map-notice battery-notice" role="alert">
      <span>
        Receiver battery critically low ({fmtBattery(battery)}). Switch the receiver off and charge it: it keeps
        draining the battery until it's flat.
      </span>
      <button type="button" className="link-btn notice-close" onClick={() => setDismissed(true)} aria-label="Dismiss">
        ✕
      </button>
    </div>
  );
}
