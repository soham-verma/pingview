import { useMemo, useState } from 'react';
import { haversineMeters } from './geo.js';
import { fmtAltitude, fmtDistance, useUnits } from './units.js';

const PAGE_SIZE = 8;

function identity(ac) {
  return (ac.flight || ac.registration || ac.hex || '').trim();
}

function metricLabel(ac, referenceLocation, units) {
  if (referenceLocation && ac.lat != null && ac.lon != null) {
    return fmtDistance(
      haversineMeters(referenceLocation.lat, referenceLocation.lon, ac.lat, ac.lon),
      units,
    );
  }
  return ac.alt_baro === 'ground' ? 'ground' : ac.alt_baro != null ? fmtAltitude(ac.alt_baro, units) : '—';
}

function sortAircraft(list, sortBy, referenceLocation) {
  const withDistance = list.map((ac) => ({
    ac,
    distance:
      referenceLocation && ac.lat != null && ac.lon != null
        ? haversineMeters(referenceLocation.lat, referenceLocation.lon, ac.lat, ac.lon)
        : Infinity,
  }));

  withDistance.sort((a, b) => {
    switch (sortBy) {
      case 'callsign':
        return identity(a.ac).localeCompare(identity(b.ac));
      case 'altitude': {
        const av = a.ac.alt_baro === 'ground' ? -1 : (a.ac.alt_baro ?? -2);
        const bv = b.ac.alt_baro === 'ground' ? -1 : (b.ac.alt_baro ?? -2);
        return bv - av;
      }
      case 'seen':
        return (a.ac.seen ?? 0) - (b.ac.seen ?? 0);
      case 'distance':
      default:
        return a.distance - b.distance;
    }
  });

  return withDistance.map((w) => w.ac);
}

export default function AircraftListPanel({ aircraft, selectedHex, onSelect, referenceLocation }) {
  const units = useUnits();
  const [sortBy, setSortBy] = useState('distance');
  const [expanded, setExpanded] = useState(false);

  const sorted = useMemo(
    () => sortAircraft(aircraft, sortBy, referenceLocation),
    [aircraft, sortBy, referenceLocation],
  );
  const visible = expanded ? sorted : sorted.slice(0, PAGE_SIZE);
  const hiddenCount = sorted.length - visible.length;

  return (
    <div className="aircraft-list-panel">
      <div className="aircraft-list-header">
        <strong>Aircraft ({aircraft.length})</strong>
        <select value={sortBy} onChange={(e) => setSortBy(e.target.value)}>
          <option value="distance">Sort: distance</option>
          <option value="callsign">Sort: callsign</option>
          <option value="altitude">Sort: altitude</option>
          <option value="seen">Sort: last seen</option>
        </select>
      </div>

      <ul className="aircraft-list">
        {visible.map((ac) => {
          const selected = ac.hex === selectedHex;
          // Network aircraft only refresh every 5 s, so give them longer.
          const stale = ac.seen != null && ac.seen > (ac.network ? 15 : 5);
          return (
            <li key={ac.hex}>
              <button
                type="button"
                className={`aircraft-list-btn${selected ? ' selected' : ''}${stale ? ' stale' : ''}`}
                onClick={() => onSelect(ac.hex)}
              >
                <span className="aircraft-list-label">
                  <span className="callsign-text">{identity(ac)}</span>
                  {ac.network && (
                    <span className="net-tag" title="From the online network, not your receiver">
                      NET
                    </span>
                  )}
                  {ac.alt_baro === 'ground' && <span className="chip chip-ground">ground</span>}
                  {stale && <span className="chip chip-stale">stale</span>}
                </span>
                <span className="aircraft-list-metric">{metricLabel(ac, referenceLocation, units)}</span>
              </button>
            </li>
          );
        })}
      </ul>

      {aircraft.length === 0 && <div className="aircraft-list-empty">No aircraft in range.</div>}

      {hiddenCount > 0 && (
        <button type="button" className="header-btn" onClick={() => setExpanded(true)}>
          Show {hiddenCount} more
        </button>
      )}
      {expanded && sorted.length > PAGE_SIZE && (
        <button type="button" className="header-btn" onClick={() => setExpanded(false)}>
          Show less
        </button>
      )}
    </div>
  );
}
