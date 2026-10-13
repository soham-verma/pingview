import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { LinkButton, Segmented } from './Sheet.jsx';
import { newCustomId } from './tiles.js';

// Adds a user's own map: an XYZ tile server, or an MBTiles file on disk.
export default function AddMapForm({ onAdd, onCancel }) {
  const [kind, setKind] = useState('url');
  const [name, setName] = useState('');
  const [template, setTemplate] = useState('');
  const [file, setFile] = useState(null); // { path, info }
  const [attribution, setAttribution] = useState('');
  const [maxZoom, setMaxZoom] = useState('');
  const [error, setError] = useState('');

  async function chooseFile() {
    setError('');
    const path = await open({
      multiple: false,
      filters: [{ name: 'MBTiles map', extensions: ['mbtiles'] }],
    });
    if (!path) return;
    try {
      const info = await invoke('mbtiles_info', { path });
      setFile({ path, info });
      if (!name) setName(info.name ?? path.split(/[\\/]/).pop().replace(/\.mbtiles$/i, ''));
      if (!attribution && info.attribution) setAttribution(info.attribution);
    } catch (err) {
      setFile(null);
      setError(String(err));
    }
  }

  async function add() {
    setError('');
    const map = {
      id: newCustomId(),
      name: name.trim(),
      attribution: attribution.trim() || null,
      max_zoom: kind === 'url' ? (Number(maxZoom) || null) : (file?.info.maxzoom ?? null),
      ...(kind === 'url' ? { kind: 'url', template: template.trim() } : { kind: 'mbtiles', path: file?.path }),
    };
    if (kind === 'mbtiles' && !file) {
      setError('Choose an .mbtiles file first.');
      return;
    }
    try {
      await onAdd(map);
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div className="add-map">
      <Segmented
        label="Map source"
        options={[
          ['url', 'Tile URL'],
          ['mbtiles', 'MBTiles file'],
        ]}
        value={kind}
        onChange={setKind}
      />

      {kind === 'url' ? (
        <label className="field">
          <span>Tile URL</span>
          <input
            type="text"
            value={template}
            onChange={(e) => setTemplate(e.target.value)}
            placeholder="https://tiles.example.com/{z}/{x}/{y}.png"
            spellCheck={false}
          />
          <small>
            Use {'{z}'}, {'{x}'} and {'{y}'}. A server on your local network keeps working in Local-only mode.
          </small>
        </label>
      ) : (
        <div className="field">
          <span>Map file</span>
          <div className="file-pick">
            <LinkButton onClick={chooseFile}>{file ? 'Choose another…' : 'Choose .mbtiles file…'}</LinkButton>
            {file && (
              <small title={file.path}>
                {file.path.split(/[\\/]/).pop()} · zoom {file.info.minzoom ?? '?'}–{file.info.maxzoom ?? '?'} ·{' '}
                {file.info.format ?? 'raster'}
              </small>
            )}
          </div>
          <small>Raster MBTiles (PNG, JPEG or WebP). Works fully offline.</small>
        </div>
      )}

      <label className="field">
        <span>Name</span>
        <input type="text" value={name} onChange={(e) => setName(e.target.value)} placeholder="My map" />
      </label>

      <label className="field">
        <span>Credit (optional)</span>
        <input
          type="text"
          value={attribution}
          onChange={(e) => setAttribution(e.target.value)}
          placeholder="© OpenStreetMap contributors"
        />
      </label>

      {kind === 'url' && (
        <label className="field">
          <span>Max zoom (optional)</span>
          <input
            type="number"
            min="1"
            max="22"
            value={maxZoom}
            onChange={(e) => setMaxZoom(e.target.value)}
            placeholder="19"
          />
        </label>
      )}

      {error && <div className="sheet-error add-map-error">{error}</div>}

      <div className="add-map-actions">
        <LinkButton onClick={onCancel}>Cancel</LinkButton>
        <button type="button" className="reset-btn" onClick={add}>
          Add map
        </button>
      </div>
    </div>
  );
}
