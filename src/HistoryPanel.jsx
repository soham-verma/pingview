import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { LinkButton, Sheet, SheetSection } from './Sheet.jsx';
import { HistoryIcon } from './Icons.jsx';

function parseHistoryJsonl(text) {
  const byIcao = new Map();
  for (const line of text.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let row;
    try {
      row = JSON.parse(trimmed);
    } catch {
      continue;
    }
    if (row.lat == null || row.lon == null || !row.icao) continue;
    const t = Date.parse(row.ts);
    if (Number.isNaN(t)) continue;
    const pts = byIcao.get(row.icao) || [];
    pts.push({ lat: row.lat, lon: row.lon, altBaro: row.alt_baro ?? row.alt_geom, t });
    byIcao.set(row.icao, pts);
  }
  return Array.from(byIcao, ([icao, points]) => ({
    icao,
    points: points.sort((a, b) => a.t - b.t),
  }));
}

// "2026-10-13" → "Today" / "Yesterday" / "Mon 12 Oct".
function dayLabel(date) {
  const today = new Date().toISOString().slice(0, 10);
  const yesterday = new Date(Date.now() - 86_400_000).toISOString().slice(0, 10);
  if (date === today) return 'Today';
  if (date === yesterday) return 'Yesterday';
  return new Date(`${date}T00:00:00Z`).toLocaleDateString(undefined, {
    weekday: 'short',
    day: 'numeric',
    month: 'short',
    timeZone: 'UTC',
  });
}

// `loaded` is { label, count } for the history currently drawn on the map.
export default function HistoryPanel({ loaded, onLoadTracks, onClose }) {
  const [dates, setDates] = useState(null);
  const [error, setError] = useState('');
  const fileInput = useRef(null);

  useEffect(() => {
    invoke('history_list')
      .then(setDates)
      .catch((err) => {
        setDates([]);
        setError(`Couldn't list history: ${err}`);
      });
  }, []);

  async function show(date) {
    try {
      const tracks = parseHistoryJsonl(await invoke('history_read', { date }));
      onLoadTracks(tracks, dayLabel(date));
    } catch (err) {
      setError(`Couldn't load ${date}: ${err}`);
    }
  }

  async function saveDay(date) {
    const path = await save({
      defaultPath: `adsb-${date}.jsonl`,
      filters: [{ name: 'JSON Lines', extensions: ['jsonl'] }],
    });
    if (!path) return;
    invoke('history_save', { date, path }).catch((err) => setError(`Couldn't save: ${err}`));
  }

  function openFile(e) {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => onLoadTracks(parseHistoryJsonl(String(reader.result)), file.name);
    reader.readAsText(file);
    e.target.value = '';
  }

  return (
    <Sheet title="History" icon={<HistoryIcon />} onClose={onClose}>
      {loaded && (
        <div className="sheet-strip">
          <span>
            Showing <strong>{loaded.label}</strong> · {loaded.count} aircraft
          </span>
          <LinkButton onClick={() => onLoadTracks([], null)}>Clear</LinkButton>
        </div>
      )}

      <SheetSection title="Recorded days">
        {dates === null && <p className="sheet-empty">Loading…</p>}
        {dates?.length === 0 && (
          <p className="sheet-empty">Nothing recorded yet. Positions are saved while aircraft are in range.</p>
        )}
        {dates?.length > 0 && (
          <ul className="day-list">
            {dates.map((d) => (
              <li key={d}>
                <span>{dayLabel(d)}</span>
                <LinkButton onClick={() => show(d)}>Show</LinkButton>
                <LinkButton onClick={() => saveDay(d)}>Save…</LinkButton>
              </li>
            ))}
          </ul>
        )}
      </SheetSection>

      <div className="sheet-footer">
        <LinkButton onClick={() => fileInput.current?.click()}>Open file…</LinkButton>
        <LinkButton onClick={() => invoke('history_open_folder').catch((err) => setError(`${err}`))}>
          Open folder
        </LinkButton>
        <input ref={fileInput} type="file" accept=".jsonl,.json,.txt" onChange={openFile} hidden />
      </div>

      {error && <div className="sheet-error">{error}</div>}
    </Sheet>
  );
}
