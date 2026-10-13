import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import AircraftMap from './AircraftMap.jsx';
import AircraftCard from './AircraftCard.jsx';
import AircraftListPanel from './AircraftListPanel.jsx';
import BottomDock from './BottomDock.jsx';
import HistoryPanel from './HistoryPanel.jsx';
import MapNotice from './MapNotice.jsx';
import MapOptionsPanel from './MapOptionsPanel.jsx';
import ReceiverPanel from './ReceiverPanel.jsx';
import RightRail from './MapControls.jsx';
import SettingsPanel from './SettingsPanel.jsx';
import StatusPill from './StatusPill.jsx';
import { useAircraft } from './useAircraft.js';
import { DEFAULT_UNITS, UnitsContext } from './units.js';
import { DEFAULT_STYLE, isMapTilerStyle } from './tiles.js';
import { useReferenceLocation } from './useReferenceLocation.js';

const TOAST_MS = 6000;
// Map callsign labels are off by default: the side list names every aircraft.
const DEFAULT_OVERLAYS = { aerodromes: true, labels: false, trails: true, network: true };
const DEFAULT_AIRPORTS = { labels: 'always', scale: 1, heliports: true };
const DEFAULT_AIRCRAFT = { scale: 1 };
const DEFAULT_BRIGHTNESS = 100;
const DEFAULT_NETWORK_RADIUS = 100;

// UI preferences that live in localStorage rather than the Rust settings.
function usePref(key, initial) {
  const [value, setValue] = useState(() => {
    try {
      const raw = localStorage.getItem(key);
      if (raw == null) return initial;
      const saved = JSON.parse(raw);
      // Objects merge over the defaults so new options get their default.
      return typeof initial === 'object' ? { ...initial, ...saved } : saved;
    } catch {
      return initial;
    }
  });
  useEffect(() => {
    localStorage.setItem(key, JSON.stringify(value));
  }, [key, value]);
  return [value, setValue];
}

export default function App() {
  const { aircraft, messages, receiver, net } = useAircraft();
  const [selectedHex, setSelectedHex] = useState(null);
  const { location, error, locating, requestGeolocation, setManual, clear, dismissError } =
    useReferenceLocation();
  const [pickingLocation, setPickingLocation] = useState(false);
  const [panel, setPanel] = useState(null); // 'map' | 'history' | 'receiver' | 'settings' | null
  const [historyTracks, setHistoryTracks] = useState([]);
  const [historyLoaded, setHistoryLoaded] = useState(null); // { label, count }
  const [settings, setSettings] = useState(null);
  const [tileStatus, setTileStatus] = useState('ok');
  const [overlays, setOverlays] = usePref('pingview.overlays.v2', DEFAULT_OVERLAYS);
  const [brightness, setBrightness] = usePref('pingview.brightness', DEFAULT_BRIGHTNESS);
  const [networkRadius, setNetworkRadius] = usePref('pingview.networkRadius', DEFAULT_NETWORK_RADIUS);
  const [units, setUnits] = usePref('pingview.units', DEFAULT_UNITS);
  const [airports, setAirports] = usePref('pingview.airports', DEFAULT_AIRPORTS);
  const [aircraftPrefs, setAircraftPrefs] = usePref('pingview.aircraft', DEFAULT_AIRCRAFT);
  const [mapCenter, setMapCenter] = useState(null);
  const onCenterChange = useCallback((c) => setMapCenter({ lat: c.lat, lon: c.lng }), []);

  // Network traffic is fetched around your location, or the map centre.
  const networkCenter = location ?? mapCenter;
  useEffect(() => {
    const area =
      overlays.network && networkCenter
        ? { lat: networkCenter.lat, lon: networkCenter.lon, radius_nm: networkRadius }
        : null;
    invoke('set_network_area', { area }).catch(() => {});
  }, [overlays.network, networkCenter?.lat, networkCenter?.lon, networkRadius]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    invoke('get_settings').then(setSettings);
    const unlisten = listen('tiles', ({ payload }) => setTileStatus(payload));
    return () => {
      unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    if (!error) return undefined;
    const t = setTimeout(dismissError, TOAST_MS);
    return () => clearTimeout(t);
  }, [error, dismissError]);

  const prefs = { overlays, airports, aircraft: aircraftPrefs, brightness, networkRadius };
  const setPrefs = (patch) => {
    if (patch.overlays) setOverlays(patch.overlays);
    if (patch.airports) setAirports(patch.airports);
    if (patch.aircraft) setAircraftPrefs(patch.aircraft);
    if (patch.units) setUnits(patch.units);
    if (patch.brightness != null) setBrightness(patch.brightness);
    if (patch.networkRadius != null) setNetworkRadius(patch.networkRadius);
  };
  const resetPrefs = async () => {
    setPrefs({
      overlays: DEFAULT_OVERLAYS,
      airports: DEFAULT_AIRPORTS,
      aircraft: DEFAULT_AIRCRAFT,
      units: DEFAULT_UNITS,
      brightness: DEFAULT_BRIGHTNESS,
      networkRadius: DEFAULT_NETWORK_RADIUS,
    });
    if (settings && settings.map_style !== DEFAULT_STYLE) {
      const next = { ...settings, map_style: DEFAULT_STYLE };
      await invoke('set_settings', { settings: next });
      setSettings(next);
    }
  };

  // Key hints only matter when a MapTiler style is picked.
  const usingMapTiler = settings && isMapTilerStyle(settings.map_style);
  const mapNotice = !usingMapTiler ? null : !settings.maptiler_key ? 'no_key' : tileStatus;
  const customMap = settings?.custom_maps?.find((m) => `custom-${m.id}` === settings.map_style) ?? null;
  const selected = aircraft.find((ac) => ac.hex === selectedHex);
  const closePanel = () => setPanel(null);
  const togglePanel = (name) => setPanel((p) => (p === name ? null : name));
  const savedSettings = (next) => {
    setSettings(next);
    setTileStatus('ok');
  };

  return (
    <UnitsContext.Provider value={units}>
    <div className="app">
      <div
        className={`map-wrap${panel ? ' drawer-open' : ''}`}
        style={{ '--tile-brightness': brightness / 100 }}
      >
        <AircraftMap
          aircraft={aircraft}
          selectedHex={selectedHex}
          onSelect={setSelectedHex}
          referenceLocation={location}
          setLocationMode={pickingLocation}
          onSetLocation={(lat, lon) => {
            setManual(lat, lon);
            setPickingLocation(false);
          }}
          historyTracks={historyTracks}
          mapStyle={settings?.map_style}
          customMap={customMap}
          tileKey={settings?.maptiler_key ?? ''}
          showAerodromes={overlays.aerodromes}
          showLabels={overlays.labels}
          showTrails={overlays.trails}
          showNetwork={overlays.network}
          airports={airports}
          aircraftScale={aircraftPrefs.scale}
          onCenterChange={onCenterChange}
          controls={
            <RightRail
              hasLocation={Boolean(location)}
              locating={locating}
              onLocate={requestGeolocation}
              onPickLocation={() => setPickingLocation(true)}
              onClearLocation={clear}
            />
          }
        />

        {selected ? (
          <AircraftCard ac={selected} referenceLocation={location} onClose={() => setSelectedHex(null)} />
        ) : (
          <StatusPill
            receiver={receiver}
            count={aircraft.length}
            net={net}
            onClick={() => togglePanel('receiver')}
          />
        )}

        {!panel && (
          <AircraftListPanel
            aircraft={aircraft}
            selectedHex={selectedHex}
            onSelect={setSelectedHex}
            referenceLocation={location}
          />
        )}

        <BottomDock panel={panel} onToggle={togglePanel} />

        {pickingLocation && (
          <div className="map-banner map-float">
            Click the map to set your location
            <button type="button" onClick={() => setPickingLocation(false)}>
              Cancel
            </button>
          </div>
        )}
        {error && (
          <div className="map-toast map-float" role="alert">
            {error}
            <button type="button" onClick={dismissError} aria-label="Dismiss">
              ✕
            </button>
          </div>
        )}
        {settings && !settings.local_only && panel !== 'settings' && panel !== 'map' && (
          <MapNotice status={mapNotice} onOpenSettings={() => setPanel('settings')} />
        )}

        {panel === 'map' && settings && (
          <MapOptionsPanel
            settings={settings}
            onSaved={savedSettings}
            prefs={prefs}
            onPrefs={setPrefs}
            onReset={resetPrefs}
            onClose={closePanel}
          />
        )}
        {panel === 'history' && (
          <HistoryPanel
            loaded={historyLoaded}
            onLoadTracks={(tracks, label) => {
              setHistoryTracks(tracks);
              setHistoryLoaded(tracks.length ? { label, count: tracks.length } : null);
            }}
            onClose={closePanel}
          />
        )}
        {panel === 'receiver' && (
          <ReceiverPanel receiver={receiver} messages={messages} aircraftCount={aircraft.length} onClose={closePanel} />
        )}
        {panel === 'settings' && settings && (
          <SettingsPanel settings={settings} onSaved={savedSettings} onClose={closePanel} />
        )}
      </div>
    </div>
    </UnitsContext.Provider>
  );
}
