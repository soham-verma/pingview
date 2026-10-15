import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { LinkButton, Sheet, SheetRow, SheetSection, Switch } from './Sheet.jsx';
import { BUILTIN_STYLE, DEFAULT_MAPTILER_STYLE, isMapTilerStyle } from './tiles.js';
import { GearIcon } from './Icons.jsx';

const AUTO = '';

// Every change is saved immediately; there's no Save button.
export default function SettingsPanel({ settings, onSaved, onClose }) {
  const [ports, setPorts] = useState([]);
  const [editingKey, setEditingKey] = useState(!settings.maptiler_key);
  const [keyDraft, setKeyDraft] = useState('');
  const [error, setError] = useState('');

  useEffect(() => {
    invoke('list_ports').then(setPorts).catch(() => setPorts([]));
  }, []);

  async function save(patch) {
    const next = { ...settings, ...patch };
    try {
      await invoke('set_settings', { settings: next });
      onSaved(next);
      setError('');
    } catch (err) {
      setError(`Couldn't save: ${err}`);
    }
  }

  async function saveKey() {
    const key = keyDraft.trim();
    if (!key) return;
    // Adding a key upgrades the built-in offline map to MapTiler's dark style.
    await save({
      maptiler_key: key,
      ...(settings.map_style === BUILTIN_STYLE ? { map_style: DEFAULT_MAPTILER_STYLE } : {}),
    });
    setKeyDraft('');
    setEditingKey(false);
  }

  return (
    <Sheet title="Settings" icon={<GearIcon />} onClose={onClose}>
      <SheetSection title="Connection">
        <SheetRow
          label="Local only"
          hint="Never use the internet. Aircraft, the offline map, aircraft database, airlines and airports all keep working."
          htmlFor="settings-local"
        >
          <Switch
            id="settings-local"
            checked={settings.local_only}
            onChange={(v) => save({ local_only: v })}
          />
        </SheetRow>
      </SheetSection>

      <SheetSection title="Receiver">
        <SheetRow label="Port" htmlFor="settings-port">
          <select
            id="settings-port"
            value={settings.port ?? AUTO}
            onChange={(e) => save({ port: e.target.value || null })}
          >
            <option value={AUTO}>Auto-detect</option>
            {settings.port && !ports.some((p) => p.name === settings.port) && (
              <option value={settings.port}>{settings.port} (not connected)</option>
            )}
            {ports.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name.startsWith('tcp://')
                  ? `${p.description} (${p.name.slice('tcp://'.length)})`
                  : p.name.replace('/dev/', '') + (p.likely_receiver ? ' (receiver)' : '')}
              </option>
            ))}
          </select>
        </SheetRow>
      </SheetSection>

      <SheetSection title="Map">
        {editingKey ? (
          <div className="sheet-row sheet-row-stack">
            <label htmlFor="settings-key">
              <span>MapTiler key</span>
              <small>Free from cloud.maptiler.com</small>
            </label>
            <div className="key-entry">
              <input
                id="settings-key"
                type="password"
                value={keyDraft}
                onChange={(e) => setKeyDraft(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && saveKey()}
                placeholder="Paste key"
                spellCheck={false}
                autoComplete="off"
                autoFocus={Boolean(settings.maptiler_key)}
              />
              <LinkButton onClick={saveKey} disabled={!keyDraft.trim()}>
                Save
              </LinkButton>
              {settings.maptiler_key && (
                <LinkButton onClick={() => setEditingKey(false)}>Cancel</LinkButton>
              )}
            </div>
          </div>
        ) : (
          <SheetRow label="MapTiler key">
            <span className="sheet-value">Saved</span>
            <LinkButton onClick={() => setEditingKey(true)}>Change</LinkButton>
            <LinkButton
              danger
              onClick={() => {
                // Without a key MapTiler styles can't load: fall back to offline.
                save({
                  maptiler_key: null,
                  ...(isMapTilerStyle(settings.map_style) ? { map_style: BUILTIN_STYLE } : {}),
                });
                setEditingKey(true);
              }}
            >
              Remove
            </LinkButton>
          </SheetRow>
        )}
      </SheetSection>

      <SheetSection title="Data">
        <SheetRow label="Record history" hint="Positions saved per day on this computer" htmlFor="settings-history">
          <Switch
            id="settings-history"
            checked={settings.history_enabled}
            onChange={(v) => save({ history_enabled: v })}
          />
        </SheetRow>
        <SheetRow
          label="Online assist"
          hint={
            settings.local_only
              ? 'Off while Local only is on'
              : 'Callsigns, photos, routes and network traffic from adsb.lol, adsb.fi, hexdb.io, planespotters'
          }
          htmlFor="settings-assist"
        >
          <Switch
            id="settings-assist"
            checked={settings.assist_enabled && !settings.local_only}
            disabled={settings.local_only}
            onChange={(v) => save({ assist_enabled: v })}
          />
        </SheetRow>
      </SheetSection>

      {error && <div className="sheet-error">{error}</div>}
    </Sheet>
  );
}
