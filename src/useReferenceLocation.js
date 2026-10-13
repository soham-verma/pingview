import { invoke } from '@tauri-apps/api/core';
import { useCallback, useState } from 'react';

const STORAGE_KEY = 'pingview.reference-location';

function load() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

function save(location) {
  try {
    if (location) localStorage.setItem(STORAGE_KEY, JSON.stringify(location));
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    // localStorage unavailable (private mode etc.) — location just won't persist
  }
}

export function useReferenceLocation() {
  const [location, setLocation] = useState(load);
  const [error, setError] = useState(null);
  const [locating, setLocating] = useState(false);

  const setManual = useCallback((lat, lon) => {
    const loc = { lat, lon };
    setLocation(loc);
    save(loc);
    setError(null);
  }, []);

  const clear = useCallback(() => {
    setLocation(null);
    save(null);
  }, []);

  const requestGeolocation = useCallback(async () => {
    setError(null);
    setLocating(true);
    try {
      const fix = await invoke('locate_device');
      setManual(fix.lat, fix.lon);
    } catch (err) {
      setError(`${err} You can pick it on the map instead.`);
    } finally {
      setLocating(false);
    }
  }, [setManual]);

  const dismissError = useCallback(() => setError(null), []);

  return { location, error, locating, requestGeolocation, setManual, clear, dismissError };
}
