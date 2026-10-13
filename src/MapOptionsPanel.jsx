import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { MapOptionsIcon, ResetIcon } from './Icons.jsx';
import { Collapsible, Segmented, Sheet, SheetRow, SheetSection, Switch } from './Sheet.jsx';
import AddMapForm from './AddMapForm.jsx';
import { BUILTIN_STYLE, styleThumbnail } from './tiles.js';
import { UNIT_OPTIONS, useUnits } from './units.js';

const RADIUS_UNIT = { km: [1.852, 'km'], mi: [1.150779, 'mi'], nm: [1, 'nm'] };
const SCALES = [
  [0.75, '75%'],
  [1, '100%'],
  [1.25, '125%'],
  [1.5, '150%'],
  [2, '200%'],
];

function ThumbImage({ src }) {
  return (
    <img
      src={src}
      alt=""
      onError={(e) => {
        e.currentTarget.style.visibility = 'hidden';
      }}
    />
  );
}

// A tiny stylised coastline for the built-in offline map's card.
function OfflineThumb() {
  return (
    <svg viewBox="0 0 160 100" preserveAspectRatio="xMidYMid slice" aria-hidden="true">
      <rect width="160" height="100" fill="#0e1520" />
      <path
        d="M30 40 L52 26 L70 30 L84 20 L98 28 L112 24 L126 40 L132 58 L120 76 L100 82 L86 76 L70 84 L52 78 L40 64 Z"
        fill="#1b2230"
        stroke="#56627a"
        strokeWidth="1.2"
      />
      <path d="M84 20 L86 50 L120 76" fill="none" stroke="#4a5569" strokeDasharray="3 3" />
      <circle cx="112" cy="62" r="2" fill="#9aa3b2" />
      <circle cx="58" cy="52" r="2" fill="#9aa3b2" />
    </svg>
  );
}

function StyleCard({ label, sub, selected, disabled, onClick, onRemove, thumb }) {
  return (
    <div className={`style-card${selected ? ' on' : ''}${disabled ? ' disabled' : ''}`}>
      <button type="button" className="style-pick" onClick={onClick} disabled={disabled}>
        <span className="style-thumb">{thumb}</span>
        <span className="style-name">
          {label}
          {sub && <small>{sub}</small>}
        </span>
      </button>
      {onRemove && (
        <button type="button" className="style-remove" onClick={onRemove} aria-label={`Remove ${label}`}>
          ✕
        </button>
      )}
    </div>
  );
}

export default function MapOptionsPanel({
  settings,
  onSaved,
  prefs,
  onPrefs,
  onReset,
  onClose,
}) {
  const { overlays, airports, aircraft, brightness, networkRadius } = prefs;
  const [styles, setStyles] = useState([]);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState('');
  const hasKey = Boolean(settings.maptiler_key);
  const customMaps = settings.custom_maps ?? [];
  const units = useUnits();
  const [factor, radiusLabel] = RADIUS_UNIT[units.distance] ?? RADIUS_UNIT.nm;

  useEffect(() => {
    invoke('map_styles').then(setStyles);
  }, []);

  async function save(next) {
    await invoke('set_settings', { settings: next });
    onSaved(next);
  }

  async function pickStyle(id) {
    setError('');
    try {
      await save({ ...settings, map_style: id });
    } catch (err) {
      setError(String(err));
    }
  }

  // Throws on validation errors so the form can show them.
  async function addMap(map) {
    await save({ ...settings, custom_maps: [...customMaps, map], map_style: `custom-${map.id}` });
    setAdding(false);
  }

  async function removeMap(id) {
    const style = settings.map_style === `custom-${id}` ? BUILTIN_STYLE : settings.map_style;
    try {
      await save({ ...settings, custom_maps: customMaps.filter((m) => m.id !== id), map_style: style });
    } catch (err) {
      setError(String(err));
    }
  }

  const setOverlays = (patch) => onPrefs({ overlays: { ...overlays, ...patch } });
  const setAirports = (patch) => onPrefs({ airports: { ...airports, ...patch } });
  const setAircraft = (patch) => onPrefs({ aircraft: { ...aircraft, ...patch } });
  const setUnits = (patch) => onPrefs({ units: { ...units, ...patch } });

  return (
    <Sheet title="Map Options" icon={<MapOptionsIcon />} onClose={onClose}>
      <SheetSection title="Map">
        <div className="style-grid">
          <StyleCard
            label="Offline"
            sub="Built-in, always works"
            selected={settings.map_style === BUILTIN_STYLE}
            onClick={() => pickStyle(BUILTIN_STYLE)}
            thumb={<OfflineThumb />}
          />
          {styles.map((s) => (
            <StyleCard
              key={s.id}
              label={s.label}
              sub={hasKey ? 'MapTiler' : 'Needs a MapTiler key'}
              selected={settings.map_style === s.id}
              disabled={!hasKey}
              onClick={() => pickStyle(s.id)}
              thumb={hasKey ? <ThumbImage src={styleThumbnail(s.id)} /> : null}
            />
          ))}
          {customMaps.map((m) => (
            <StyleCard
              key={m.id}
              label={m.name}
              sub={m.kind === 'url' ? 'Your tile server' : 'Your map file'}
              selected={settings.map_style === `custom-${m.id}`}
              onClick={() => pickStyle(`custom-${m.id}`)}
              onRemove={() => removeMap(m.id)}
              thumb={<span className="thumb-icon">{m.kind === 'url' ? 'URL' : 'FILE'}</span>}
            />
          ))}
          {!adding && (
            <button type="button" className="style-card style-add" onClick={() => setAdding(true)}>
              <span className="style-thumb thumb-plus">+</span>
              <span className="style-name">Add your own map</span>
            </button>
          )}
        </div>
        {!hasKey && (
          <p className="sheet-empty">Add a free MapTiler key in Settings to unlock the detailed MapTiler styles.</p>
        )}
        {adding && <AddMapForm onAdd={addMap} onCancel={() => setAdding(false)} />}
        {error && <div className="sheet-error">{error}</div>}
      </SheetSection>

      <SheetSection title="Map brightness">
        <div className="slider-row">
          <input
            type="range"
            min="30"
            max="150"
            step="5"
            value={brightness}
            onChange={(e) => onPrefs({ brightness: Number(e.target.value) })}
            aria-label="Map brightness"
          />
          <span>{brightness}%</span>
        </div>
      </SheetSection>

      <div className="collapsibles">
        <Collapsible title="Overlays">
          <SheetRow
            label="Network traffic"
            hint={
              settings.assist_enabled
                ? "Aircraft your receiver can't hear, from adsb.lol / adsb.fi (tagged NET in the list)"
                : 'Needs Online assist, in Settings'
            }
            htmlFor="ov-network"
          >
            <Switch id="ov-network" checked={overlays.network} onChange={(v) => setOverlays({ network: v })} />
          </SheetRow>
          {overlays.network && (
            <div className="slider-row">
              <input
                type="range"
                min="25"
                max="250"
                step="25"
                value={networkRadius}
                onChange={(e) => onPrefs({ networkRadius: Number(e.target.value) })}
                aria-label="Network traffic radius"
              />
              <span>
                {Math.round(networkRadius * factor)} {radiusLabel}
              </span>
            </div>
          )}
        </Collapsible>

        <Collapsible title="Airports">
          <SheetRow label="Show airports" hint="2,700+ Australian aerodromes with frequencies" htmlFor="ov-aerodromes">
            <Switch id="ov-aerodromes" checked={overlays.aerodromes} onChange={(v) => setOverlays({ aerodromes: v })} />
          </SheetRow>
          <SheetRow label="Show heliports" htmlFor="ap-heliports">
            <Switch id="ap-heliports" checked={airports.heliports} onChange={(v) => setAirports({ heliports: v })} />
          </SheetRow>
          <Segmented
            label="Labels"
            options={[
              ['always', 'Always'],
              ['hover', 'On hover'],
            ]}
            value={airports.labels}
            onChange={(v) => setAirports({ labels: v })}
          />
          <Segmented label="Icon size" options={SCALES} value={airports.scale} onChange={(v) => setAirports({ scale: v })} />
        </Collapsible>

        <Collapsible title="Aircraft">
          <SheetRow label="Trails" hint="Altitude-coloured flight paths" htmlFor="ov-trails">
            <Switch id="ov-trails" checked={overlays.trails} onChange={(v) => setOverlays({ trails: v })} />
          </SheetRow>
          <Segmented label="Icon size" options={SCALES} value={aircraft.scale} onChange={(v) => setAircraft({ scale: v })} />
        </Collapsible>

        <Collapsible title="Aircraft labels">
          <SheetRow
            label="Callsign on every aircraft"
            hint="Off keeps the map clean; the selected aircraft is always labelled"
            htmlFor="ov-labels"
          >
            <Switch id="ov-labels" checked={overlays.labels} onChange={(v) => setOverlays({ labels: v })} />
          </SheetRow>
        </Collapsible>

        <Collapsible title="Units & labels">
          {UNIT_OPTIONS.map((opt) => (
            <Segmented
              key={opt.key}
              label={opt.label}
              options={opt.options}
              value={units[opt.key]}
              onChange={(v) => setUnits({ [opt.key]: v })}
            />
          ))}
        </Collapsible>
      </div>

      <div className="reset-row">
        <button type="button" className="reset-btn" onClick={onReset}>
          <ResetIcon />
          Reset preferences
        </button>
      </div>
    </Sheet>
  );
}
