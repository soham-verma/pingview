import { useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { bearingDegrees, haversineMeters } from './geo.js';
import { useAircraftDetails } from './useAircraftDetails.js';
import { AERODROMES, aerodromeCode, airlineFor, nearestAerodrome } from './aviation.js';
import { CloseIcon } from './Icons.jsx';
import {
  airportCodes,
  fmtAltimeter,
  fmtAltitude,
  fmtCoords,
  fmtDistance,
  fmtSpeed,
  fmtTemperature,
  fmtTime,
  fmtVertical,
  useUnits,
  verticalUnit,
} from './units.js';

// MAVLink ADSB_EMITTER_TYPE, as reported by the Ping.
const CATEGORY = {
  1: 'Light',
  2: 'Small',
  3: 'Large',
  4: 'High-vortex large',
  5: 'Heavy',
  6: 'High performance',
  7: 'Rotorcraft',
  9: 'Glider',
  10: 'Lighter than air',
  11: 'Parachutist',
  12: 'Ultralight',
  14: 'UAV',
  15: 'Space vehicle',
  17: 'Emergency vehicle',
  18: 'Service vehicle',
  19: 'Obstacle',
};

const EMERGENCY_SQUAWKS = { 7500: 'Hijack', 7600: 'Radio failure', 7700: 'Emergency' };
const NA = 'n/a';
const NM = 1852;

const LOCAL_AERODROMES = new Map(AERODROMES.filter((a) => a.icao_code).map((a) => [a.icao_code, a]));

function flagEmoji(iso) {
  if (!iso) return '';
  return String.fromCodePoint(...[...iso.toUpperCase()].map((c) => 0x1f1a5 + c.charCodeAt(0)));
}

function num(value, unit, digits = 0) {
  if (value == null || Number.isNaN(value)) return NA;
  const text = Number(value).toLocaleString(undefined, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
  return unit ? `${text} ${unit}` : text;
}

function ago(seconds) {
  if (seconds == null) return NA;
  return seconds < 60 ? `${seconds.toFixed(1)} s` : `${Math.round(seconds / 60)} min`;
}

function compass(deg) {
  return ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'][Math.round(deg / 45) % 8];
}

// City and state for an airport: from the local Australian catalogue when
// we have it ("Sydney (Mascot)", "AU-NSW" → "Sydney, NSW"), else hexdb.
function place(airport) {
  const local = LOCAL_AERODROMES.get(airport.icao);
  if (local?.municipality) {
    const city = local.municipality.replace(/\s*\(.*\)$/, '');
    const state = local.region?.replace(/^AU-/, '');
    return state ? `${city}, ${state}` : city;
  }
  return airport.name ?? airport.icao;
}

function coords(airport) {
  const local = LOCAL_AERODROMES.get(airport.icao);
  const lat = airport.lat ?? local?.latitude;
  const lon = airport.lon ?? local?.longitude;
  return lat != null && lon != null ? { lat, lon } : null;
}

// Progress along the route and ETA, only when the aircraft is plausibly
// flying it (hexdb routes can be years out of date).
function routeProgress(ac, from, to) {
  const a = coords(from);
  const b = coords(to);
  if (!a || !b) return null;
  const done = haversineMeters(a.lat, a.lon, ac.lat, ac.lon);
  const left = haversineMeters(ac.lat, ac.lon, b.lat, b.lon);
  const direct = haversineMeters(a.lat, a.lon, b.lat, b.lon);
  if (done + left > direct * 1.25 + 20_000) return null;
  const progress = done / (done + left);
  let eta = null;
  if (ac.gs > 50) {
    const minutes = Math.round((left / NM / ac.gs) * 60);
    eta = { minutes, at: new Date(Date.now() + minutes * 60_000) };
  }
  return { progress, eta };
}

function Section({ title, note, children }) {
  return (
    <section className="card-section">
      <h3>
        {title}
        {note && <span>{note}</span>}
      </h3>
      <dl>{children}</dl>
    </section>
  );
}

function Row({ label, value, className }) {
  return (
    <>
      <dt>{label}</dt>
      <dd className={className}>{value ?? NA}</dd>
    </>
  );
}

function RouteEnd({ airport, units, className }) {
  const codes = airportCodes(airport, units);
  return (
    <div className={`ac-route-end${className ? ` ${className}` : ''}`}>
      <strong>{codes.main}</strong>
      <span>{place(airport)}</span>
      {codes.other && <small>({codes.other})</small>}
    </div>
  );
}

export default function AircraftCard({ ac, referenceLocation, onClose }) {
  const units = useUnits();
  const [expanded, setExpanded] = useState(false);
  const details = useAircraftDetails(ac.hex, ac.flight);
  const info = details?.info;
  const net = details?.net;
  const route = details?.route;
  const photo = details?.photo;

  // Offline registry first (ac.*), then hexdb (info), then the live network.
  const registration = ac.registration ?? info?.registration ?? net?.registration;
  const typeCode = ac.type_code ?? info?.type_code ?? net?.type_code;
  const typeDesc =
    ac.type_desc ??
    ([info?.manufacturer, info?.type_name].filter(Boolean).join(' ') || net?.type_desc || null);
  const owner = ac.owner_op ?? info?.owner ?? net?.owner_op;
  const airline = airlineFor(ac.flight);
  const emergency = EMERGENCY_SQUAWKS[ac.squawk];
  const nearest = nearestAerodrome(ac.lat, ac.lon);

  const from = route?.airports[0];
  const to = route?.airports[route.airports.length - 1];
  const progress = from && to ? routeProgress(ac, from, to) : null;

  let distance = NA;
  if (referenceLocation) {
    const m = haversineMeters(referenceLocation.lat, referenceLocation.lon, ac.lat, ac.lon);
    const brg = bearingDegrees(referenceLocation.lat, referenceLocation.lon, ac.lat, ac.lon);
    distance = `${fmtDistance(m, units)} · ${Math.round(brg)}° ${compass(brg)}`;
  }

  // The Ping sends one altitude (barometric or GPS); the network fills the other.
  const fromNet = (v) => (v == null ? null : { value: v, net: true });
  const baroAlt = ac.alt_source === 'baro' ? { value: ac.alt_baro } : fromNet(net?.alt_baro);
  const geomAlt = ac.alt_geom != null ? { value: ac.alt_geom } : fromNet(net?.alt_geom);
  const altValue = (alt) =>
    alt ? (
      <>
        {fmtAltitude(alt.value, units)}
        {alt.net && (
          <span className="row-src" title={`From ${net.via}`}>
            net
          </span>
        )}
      </>
    ) : (
      NA
    );

  const trend = ac.baro_rate > 150 ? 'climbing' : ac.baro_rate < -150 ? 'descending' : null;

  return (
    <article className="ac-card">
      <header className="ac-card-head">
        <div className="ac-card-id">
          <div className="ac-callsign">
            {ac.flight || registration || ac.hex.toUpperCase()}
            {ac.military && <span className="chip chip-military">Military</span>}
            {ac.network && (
              <span className="net-tag" title={`From ${ac.source}; your receiver doesn't hear this aircraft`}>
                NET
              </span>
            )}
            {ac.flight_assisted && (
              <span className="chip" title={`Callsign from ${net?.via ?? 'the network'}`}>
                Network
              </span>
            )}
          </div>
          <div className="ac-sub">
            {[airline?.name, typeCode].filter(Boolean).join(' · ') || CATEGORY[ac.emitter_type] || 'Aircraft'}
          </div>
        </div>
        <button type="button" className="sheet-close" onClick={onClose} aria-label="Close">
          <CloseIcon />
        </button>
      </header>

      {emergency && (
        <div className="ac-emergency">
          Squawk {ac.squawk} · {emergency}
        </div>
      )}

      {from && to && (
        <div className="ac-route">
          <RouteEnd airport={from} units={units} />
          <div className="ac-route-bar" aria-hidden="true">
            <span style={{ width: `${Math.round((progress?.progress ?? 0) * 100)}%` }} />
            {progress && <i style={{ left: `${Math.round(progress.progress * 100)}%` }} />}
          </div>
          <RouteEnd airport={to} units={units} className="ac-route-to" />
        </div>
      )}

      <div className="ac-stats">
        <div>
          <span>Altitude</span>
          <strong>{fmtAltitude(ac.alt_baro, units)}</strong>
        </div>
        <div>
          <span>Ground speed</span>
          <strong>{fmtSpeed(ac.gs, units)}</strong>
        </div>
        <div>
          <span>Vertical, {verticalUnit(units)}</span>
          <strong className={trend ?? undefined}>
            {trend === 'climbing' ? '▲ ' : trend === 'descending' ? '▼ ' : ''}
            {fmtVertical(ac.baro_rate, units)}
          </strong>
        </div>
      </div>

      {progress?.eta && (
        <div className="ac-eta">
          ETA <strong>{fmtTime(progress.eta.at, units)}</strong>
          <span>in {progress.eta.minutes} min</span>
        </div>
      )}

      {photo && (
        <figure className="ac-photo">
          <img src={photo.large ?? photo.thumbnail} alt={registration ?? ac.hex} />
          <figcaption>
            <a
              href={photo.link}
              onClick={(e) => {
                e.preventDefault();
                openUrl(photo.link);
              }}
            >
              © {photo.photographer} · planespotters.net
            </a>
          </figcaption>
        </figure>
      )}

      <footer className="ac-card-foot">
        <span>
          {flagEmoji(ac.country_iso)} {registration ?? ac.hex.toUpperCase()}
        </span>
        <span className="ac-type">
          {typeDesc ?? CATEGORY[ac.emitter_type] ?? ''}
          {typeCode && <small> ({typeCode})</small>}
        </span>
      </footer>

      <button type="button" className="ac-more" onClick={() => setExpanded((e) => !e)}>
        {expanded ? 'Less' : 'More details'}
        <span aria-hidden="true">{expanded ? '▴' : '▾'}</span>
      </button>

      {expanded && (
        <div className="ac-details">
          <Section title="Identity">
            <Row label="Hex" value={ac.hex.toUpperCase()} />
            <Row label="Callsign" value={ac.flight} />
            {airline && <Row label="Spoken" value={airline.spoken} />}
            <Row label="Owner / operator" value={owner} />
            <Row label="Year" value={ac.year ?? net?.year} />
            <Row label="Country" value={ac.country} />
            <Row label="Category" value={CATEGORY[ac.emitter_type] ?? NA} />
            <Row label="Squawk" value={ac.squawk} className={emergency ? 'card-alert' : undefined} />
          </Section>

          <Section title="Position" note={ac.network ? `from ${ac.source}` : 'from receiver'}>
            <Row label="Baro. altitude" value={altValue(baroAlt)} />
            <Row label="WGS84 altitude" value={altValue(geomAlt)} />
            <Row
              label="Vert. rate"
              value={
                ac.baro_rate == null ? NA : `${fmtVertical(ac.baro_rate, units, { signed: true })} ${verticalUnit(units)}`
              }
            />
            <Row label="Track" value={num(ac.track, '°')} />
            <Row label="Position" value={fmtCoords(ac.lat, ac.lon, units)} />
            <Row label="From you" value={distance} />
            <Row
              label="Nearest aerodrome"
              value={
                nearest
                  ? `${aerodromeCode(nearest.aerodrome)} · ${fmtDistance(nearest.meters, units)} ${compass(nearest.bearing)}`
                  : NA
              }
            />
          </Section>

          {ac.network ? (
            <Section title="Signal" note={`from ${ac.source}`}>
              <Row label="Source" value={`Network (${ac.source})`} />
              <Row label="Your receiver" value="Not heard" />
              <Row label="Last position" value={ago(ac.seen_pos)} />
              <Row label="Last seen" value={ago(ac.seen)} />
            </Section>
          ) : (
            <Section title="Signal" note="from receiver">
              <Row label="Source" value={ac.source === 'uat' ? 'UAT (978 MHz)' : 'ADS-B (1090 MHz)'} />
              <Row label="Messages" value={ac.messages} />
              <Row label="Msg. rate" value={num(ac.msg_rate, '/s', 1)} />
              <Row label="Last position" value={ago(ac.seen_pos)} />
              <Row label="Last seen" value={ago(ac.seen)} />
            </Section>
          )}

          {route?.updated && (
            <p className="card-note">
              Route data as of{' '}
              {new Date(route.updated * 1000).toLocaleDateString(undefined, {
                day: 'numeric',
                month: 'short',
                year: 'numeric',
              })}{' '}
              (hexdb.io).
            </p>
          )}

          {net && (
            <Section title="Network" note={`via ${net.via}`}>
              <Row label="IAS / TAS" value={`${fmtSpeed(net.ias, units)} / ${fmtSpeed(net.tas, units)}`} />
              <Row label="Mach" value={num(net.mach, '', 3)} />
              <Row
                label="Wind"
                value={net.wind_dir != null ? `${num(net.wind_dir, '°')} at ${fmtSpeed(net.wind_speed, units)}` : NA}
              />
              <Row label="OAT / TAT" value={`${fmtTemperature(net.oat, units)} / ${fmtTemperature(net.tat, units)}`} />
              <Row label="Sel. altitude" value={fmtAltitude(net.nav_altitude, units)} />
              <Row label="Sel. heading" value={num(net.nav_heading, '°')} />
              <Row label="QNH" value={fmtAltimeter(net.nav_qnh, units)} />
              <Row label="NIC / NACp / SIL" value={`${net.nic ?? NA} / ${net.nac_p ?? NA} / ${net.sil ?? NA}`} />
              <Row label="RSSI (network)" value={num(net.rssi, 'dBFS', 1)} />
            </Section>
          )}
        </div>
      )}
    </article>
  );
}
