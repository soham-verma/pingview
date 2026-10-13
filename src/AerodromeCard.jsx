import { fmtAltitude, useUnits } from './units.js';

// Popup card for an aerodrome: who it is, then its radio frequencies,
// which is what a pilot or listener actually needs.

const KINDS = {
  CTAF: { label: 'CTAF', kind: 'ctaf' },
  UNIC: { label: 'UNICOM', kind: 'ctaf' },
  Multicom: { label: 'Multicom', kind: 'ctaf' },
  TWR: { label: 'Tower', kind: 'tower' },
  'A/D': { label: 'Tower', kind: 'tower' },
  GND: { label: 'Ground', kind: 'tower' },
  SMC: { label: 'Ground', kind: 'tower' },
  CLD: { label: 'Clearance', kind: 'tower' },
  ACD: { label: 'Clearance', kind: 'tower' },
  ATIS: { label: 'ATIS', kind: 'info' },
  AWIS: { label: 'AWIS', kind: 'info' },
  APP: { label: 'Approach', kind: 'radar' },
  DEP: { label: 'Departures', kind: 'radar' },
  'APP/DEP': { label: 'Approach', kind: 'radar' },
  FSS: { label: 'Centre', kind: 'centre' },
  FIA: { label: 'Centre', kind: 'centre' },
};

// VHF airband plus military UHF; anything else in the data is a typo.
const inBand = (mhz) => mhz >= 108 && mhz <= 400;

function titleCase(s) {
  return s
    .toLowerCase()
    .replace(/\s+/g, ' ')
    .replace(/\bcntr\b/, 'centre')
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

function freqText(mhz) {
  return Number(mhz).toFixed(3).replace(/0{1,2}$/, '');
}

function frequencies(a) {
  const list = [];
  const add = (type, mhz, description) => {
    if (mhz == null || !inBand(mhz)) return;
    const known = KINDS[type] ?? (/radar/i.test(description ?? '') ? { label: 'Radar', kind: 'radar' } : null);
    const { label, kind } = known ?? { label: /cent|cntr/i.test(type) ? 'Centre' : 'Other', kind: 'centre' };
    const caption =
      description && !['MISC', type].includes(description.trim()) && titleCase(description) !== label
        ? titleCase(description)
        : null;
    // One row per frequency; a frequency shared by several services (e.g.
    // Tower, and CTAF when the tower is closed) gets one pill per service.
    const row = list.find((f) => f.mhz === mhz);
    if (!row) list.push({ mhz, caption, services: [{ label, kind }] });
    else if (!row.services.some((x) => x.label === label)) row.services.push({ label, kind });
  };
  add(a.primary_comm_type ?? (a.controlled ? 'TWR' : 'CTAF'), a.primary_freq_mhz);
  add('TWR', a.tower_freq_mhz);
  for (const f of a.other_frequencies) add(f.type, f.frequency_mhz, f.description);
  return list;
}

const TYPE_TEXT = {
  large_airport: 'Major airport',
  medium_airport: 'Airport',
  small_airport: 'Airstrip',
  heliport: 'Heliport',
  seaplane_base: 'Seaplane base',
};

export default function AerodromeCard({ a }) {
  const units = useUnits();
  const code = a.icao_code || a.ident;
  const freqs = frequencies(a);
  return (
    <div className="ad-card">
      <div className="ad-head">
        <span className="ad-code">{code}</span>
        {a.iata_code && <span className="ad-iata">{a.iata_code}</span>}
      </div>
      <div className="ad-name">{a.name}</div>
      {a.municipality && <div className="ad-place">{a.municipality}</div>}

      <div className="ad-tags">
        {a.controlled && <span className="ad-tag ad-tag-controlled">Controlled</span>}
        <span className="ad-tag">{TYPE_TEXT[a.airport_type] ?? a.airport_type}</span>
        {a.elevation_ft != null && <span className="ad-tag">{fmtAltitude(a.elevation_ft, units)}</span>}
      </div>

      {freqs.length > 0 ? (
        <ul className="ad-freqs">
          {freqs.map((f) => (
            <li key={f.mhz}>
              <span className="ad-pills">
                {f.services.map((x) => (
                  <span key={x.label} className={`ad-pill ad-${x.kind}`}>
                    {x.label}
                  </span>
                ))}
              </span>
              <span className="ad-caption">{f.caption}</span>
              <span className="ad-mhz">{freqText(f.mhz)}</span>
            </li>
          ))}
        </ul>
      ) : (
        <div className="ad-none">No published frequencies</div>
      )}
    </div>
  );
}
