import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

const REFRESH_MS = 10000;

// Network assist details for the selected aircraft, refreshed while it stays
// selected (the backend caches static parts; live network data is ~5 s fresh).
export function useAircraftDetails(hex, flight) {
  const [details, setDetails] = useState(null);

  useEffect(() => {
    setDetails(null);
    if (!hex) return undefined;
    let cancelled = false;
    let timer;
    async function load() {
      try {
        const d = await invoke('aircraft_details', { hex, flight: flight ?? null });
        if (!cancelled) setDetails(d);
      } catch {
        // Offline or assist turned off: the panel shows the Ping's data only.
      }
      if (!cancelled) timer = setTimeout(load, REFRESH_MS);
    }
    load();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [hex, flight]);

  return details;
}
