//! In-memory aircraft state, keyed by ICAO hex.
//!
//! The snapshot keeps the JSON shape adsb-tar1090's bridge served (plus a
//! few extra fields), so the frontend components work unchanged.

use std::collections::{HashMap, VecDeque};

use serde::Serialize;

use crate::adsb::TrafficReport;
use crate::country;
use crate::mavlink::FramerStats;
use crate::registry::RegistryEntry;

/// Aircraft not heard from in this long are dropped.
pub const STALE_SECS: f64 = 60.0;
/// Window for the per-aircraft message rate.
const RATE_WINDOW_SECS: f64 = 10.0;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Aircraft {
    pub hex: String,
    pub flight: Option<String>,
    /// True when `flight` came from adsb.lol/adsb.fi, not the Ping.
    pub flight_assisted: bool,
    pub registration: Option<String>,
    /// From the offline registry (aircraft.csv.gz).
    pub type_code: Option<String>,
    pub type_desc: Option<String>,
    pub owner_op: Option<String>,
    pub year: Option<String>,
    pub military: bool,
    pub alt_baro: Option<i32>,
    pub alt_geom: Option<i32>,
    pub gs: Option<f64>,
    pub track: Option<f64>,
    pub baro_rate: Option<i32>,
    pub squawk: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub emitter_type: u8,
    /// "adsb" (1090 MHz) or "uat" (978 MHz) for the receiver; for network
    /// aircraft, the service they came from ("adsb.lol" / "adsb.fi").
    pub source: &'static str,
    /// Not heard by the receiver; added from the online network.
    pub network: bool,
    pub country: Option<&'static str>,
    pub country_iso: Option<&'static str>,
    pub messages: u64,
    #[serde(skip)]
    pub last_seen: f64,
    #[serde(skip)]
    pub last_seen_pos: Option<f64>,
    #[serde(skip)]
    recent: VecDeque<f64>,
    #[serde(skip)]
    pub registry_checked: bool,
}

impl Aircraft {
    pub fn new(hex: &str, heard_at: f64) -> Self {
        let country = country::from_icao(hex);
        Self {
            hex: hex.to_string(),
            flight: None,
            flight_assisted: false,
            registration: None,
            type_code: None,
            type_desc: None,
            owner_op: None,
            year: None,
            military: false,
            alt_baro: None,
            alt_geom: None,
            gs: None,
            track: None,
            baro_rate: None,
            squawk: None,
            lat: None,
            lon: None,
            emitter_type: 0,
            source: "adsb",
            network: false,
            country: country.map(|c| c.name),
            country_iso: country.map(|c| c.iso),
            messages: 0,
            last_seen: heard_at,
            last_seen_pos: None,
            recent: VecDeque::new(),
            registry_checked: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiverStatus {
    Searching,
    Connecting,
    Connected,
    NoTraffic,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReceiverInfo {
    pub status: ReceiverStatus,
    pub port: Option<String>,
    /// Human-readable reason, set for `error` (and for rejected ports).
    pub detail: Option<String>,
    /// Message-246 reports per second over the last few seconds.
    pub msg_rate: f64,
    pub stats: FramerStats,
}

impl Default for ReceiverInfo {
    fn default() -> Self {
        Self {
            status: ReceiverStatus::Searching,
            port: None,
            detail: None,
            msg_rate: 0.0,
            stats: FramerStats::default(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SnapshotAircraft<'a> {
    #[serde(flatten)]
    pub aircraft: &'a Aircraft,
    pub seen: f64,
    pub seen_pos: Option<f64>,
    pub msg_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct Snapshot<'a> {
    pub now: f64,
    pub messages: u64,
    pub receiver: &'a ReceiverInfo,
    pub aircraft: Vec<SnapshotAircraft<'a>>,
}

#[derive(Default)]
pub struct Store {
    aircraft: HashMap<String, Aircraft>,
    total_messages: u64,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merges one report. Fields missing from the report keep their old
    /// value. Returns the updated aircraft if the report carried a position.
    pub fn apply(&mut self, r: &TrafficReport, now: f64) -> Option<&Aircraft> {
        self.total_messages += 1;
        let heard_at = now - r.tslc as f64;
        let ac = self
            .aircraft
            .entry(r.hex.clone())
            .or_insert_with(|| Aircraft::new(&r.hex, heard_at));
        ac.messages += 1;
        ac.recent.push_back(now);
        while ac.recent.front().is_some_and(|t| now - t > RATE_WINDOW_SECS) {
            ac.recent.pop_front();
        }
        ac.last_seen = ac.last_seen.max(heard_at);
        ac.emitter_type = r.emitter_type;
        ac.source = if r.uat { "uat" } else { "adsb" };
        if r.flight.is_some() {
            ac.flight.clone_from(&r.flight);
            ac.flight_assisted = false;
        }
        merge(&mut ac.alt_baro, &r.alt_baro);
        merge(&mut ac.alt_geom, &r.alt_geom);
        merge(&mut ac.gs, &r.gs);
        merge(&mut ac.track, &r.track);
        merge(&mut ac.baro_rate, &r.baro_rate);
        merge(&mut ac.squawk, &r.squawk);
        if let (Some(lat), Some(lon)) = (r.lat, r.lon) {
            ac.lat = Some(lat);
            ac.lon = Some(lon);
            ac.last_seen_pos = Some(heard_at);
            return Some(ac);
        }
        None
    }

    pub fn prune(&mut self, now: f64) {
        self.aircraft.retain(|_, ac| now - ac.last_seen < STALE_SECS);
    }

    pub fn get(&self, hex: &str) -> Option<&Aircraft> {
        self.aircraft.get(hex)
    }

    /// Fills identity the Ping hasn't provided from a network lookup. The
    /// Ping's own callsign always wins.
    pub fn apply_assist(&mut self, hex: &str, flight: Option<String>, registration: Option<String>) {
        let Some(ac) = self.aircraft.get_mut(hex) else {
            return;
        };
        if ac.flight.is_none() {
            if let Some(flight) = flight {
                ac.flight = Some(flight);
                ac.flight_assisted = true;
            }
        }
        if registration.is_some() {
            ac.registration = registration;
        }
    }

    /// Fills identity from the offline registry, once per aircraft.
    pub fn apply_registry(&mut self, hex: &str, entry: Option<RegistryEntry>) {
        let Some(ac) = self.aircraft.get_mut(hex) else {
            return;
        };
        ac.registry_checked = true;
        if let Some(e) = entry {
            if e.registration.is_some() {
                ac.registration = e.registration;
            }
            ac.type_code = e.type_code;
            ac.type_desc = e.type_desc;
            ac.owner_op = e.owner_op;
            ac.year = e.year;
            ac.military = e.military;
        }
    }

    /// `network` aircraft are added unless the receiver already has them.
    pub fn snapshot<'a>(
        &'a self,
        now: f64,
        receiver: &'a ReceiverInfo,
        network: &'a [Aircraft],
    ) -> Snapshot<'a> {
        let heard = network.iter().filter(|ac| !self.aircraft.contains_key(&ac.hex));
        let mut aircraft: Vec<_> = self
            .aircraft
            .values()
            .chain(heard)
            .map(|ac| SnapshotAircraft {
                aircraft: ac,
                seen: round1((now - ac.last_seen).max(0.0)),
                seen_pos: ac.last_seen_pos.map(|t| round1((now - t).max(0.0))),
                msg_rate: round1(
                    ac.recent.iter().filter(|t| now - **t <= RATE_WINDOW_SECS).count() as f64
                        / RATE_WINDOW_SECS,
                ),
            })
            .collect();
        aircraft.sort_by(|a, b| a.aircraft.hex.cmp(&b.aircraft.hex));
        Snapshot {
            now,
            messages: self.total_messages,
            receiver,
            aircraft,
        }
    }
}

fn merge<T: Clone>(slot: &mut Option<T>, update: &Option<T>) {
    if update.is_some() {
        slot.clone_from(update);
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(hex: &str) -> TrafficReport {
        TrafficReport {
            hex: hex.into(),
            ..Default::default()
        }
    }

    #[test]
    fn merge_keeps_fields_missing_from_later_reports() {
        let mut store = Store::new();
        let mut first = report("abc123");
        first.flight = Some("QFA1".into());
        first.alt_baro = Some(5000);
        store.apply(&first, 100.0);

        let mut second = report("abc123");
        second.gs = Some(200.0);
        store.apply(&second, 101.0);

        let ac = store.get("abc123").unwrap();
        assert_eq!(ac.flight.as_deref(), Some("QFA1"));
        assert_eq!(ac.alt_baro, Some(5000));
        assert_eq!(ac.gs, Some(200.0));
        assert_eq!(ac.messages, 2);
    }

    #[test]
    fn apply_returns_aircraft_only_for_position_reports() {
        let mut store = Store::new();
        assert!(store.apply(&report("abc123"), 100.0).is_none());
        let mut pos = report("abc123");
        pos.lat = Some(-33.9);
        pos.lon = Some(151.2);
        let ac = store.apply(&pos, 101.0).unwrap();
        assert_eq!((ac.lat, ac.lon), (Some(-33.9), Some(151.2)));
    }

    #[test]
    fn tslc_backdates_last_seen() {
        let mut store = Store::new();
        let mut r = report("abc123");
        r.tslc = 5;
        store.apply(&r, 100.0);
        let info = ReceiverInfo::default();
        let snap = store.snapshot(100.0, &info, &[]);
        assert_eq!(snap.aircraft[0].seen, 5.0);
    }

    #[test]
    fn prune_drops_aircraft_after_stale_window() {
        let mut store = Store::new();
        store.apply(&report("old"), 100.0);
        store.apply(&report("new"), 150.0);
        store.prune(100.0 + STALE_SECS);
        assert!(store.get("old").is_none());
        assert!(store.get("new").is_some());
    }

    #[test]
    fn assisted_callsign_fills_gap_until_the_ping_sends_one() {
        let mut store = Store::new();
        store.apply(&report("7c6b2d"), 100.0);
        store.apply_assist("7c6b2d", Some("JST446".into()), Some("VH-VGF".into()));
        let ac = store.get("7c6b2d").unwrap();
        assert_eq!((ac.flight.as_deref(), ac.flight_assisted), (Some("JST446"), true));
        assert_eq!(ac.registration.as_deref(), Some("VH-VGF"));

        let mut own = report("7c6b2d");
        own.flight = Some("JST447".into());
        store.apply(&own, 101.0);
        let ac = store.get("7c6b2d").unwrap();
        assert_eq!((ac.flight.as_deref(), ac.flight_assisted), (Some("JST447"), false));

        store.apply_assist("7c6b2d", Some("OTHER".into()), None);
        assert_eq!(store.get("7c6b2d").unwrap().flight.as_deref(), Some("JST447"));
    }

    #[test]
    fn message_rate_counts_the_last_ten_seconds() {
        let mut store = Store::new();
        for t in 0..20 {
            store.apply(&report("abc123"), t as f64);
        }
        let info = ReceiverInfo::default();
        let snap = store.snapshot(19.0, &info, &[]);
        assert_eq!(snap.aircraft[0].msg_rate, 1.1); // t = 9..=19
    }

    #[test]
    fn receiver_aircraft_win_over_network_copies() {
        let mut store = Store::new();
        let mut own = report("7c6b2d");
        own.alt_baro = Some(1000);
        store.apply(&own, 100.0);
        let mut net_same = Aircraft::new("7c6b2d", 100.0);
        net_same.network = true;
        net_same.alt_baro = Some(9999);
        let mut net_other = Aircraft::new("7c0001", 100.0);
        net_other.network = true;
        let info = ReceiverInfo::default();
        let network = [net_same, net_other];
        let snap = store.snapshot(100.0, &info, &network);
        assert_eq!(snap.aircraft.len(), 2);
        let mine = snap.aircraft.iter().find(|a| a.aircraft.hex == "7c6b2d").unwrap();
        assert_eq!((mine.aircraft.network, mine.aircraft.alt_baro), (false, Some(1000)));
        assert!(snap.aircraft.iter().any(|a| a.aircraft.hex == "7c0001" && a.aircraft.network));
    }

    #[test]
    fn snapshot_matches_bridge_json_shape() {
        let mut store = Store::new();
        let mut r = report("7c6b2d");
        r.flight = Some("RXA6681".into());
        r.alt_baro = Some(12000);
        r.gs = Some(250.0);
        r.track = Some(270.0);
        r.baro_rate = Some(-500);
        r.squawk = Some("1200".into());
        r.lat = Some(-33.9);
        r.lon = Some(151.2);
        r.emitter_type = 1;
        store.apply(&r, 1000.0);
        let info = ReceiverInfo {
            status: ReceiverStatus::Connected,
            port: Some("/dev/ttyACM0".into()),
            ..Default::default()
        };
        let json = serde_json::to_value(store.snapshot(1000.5, &info, &[])).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "now": 1000.5,
                "messages": 1,
                "receiver": {
                    "status": "connected", "port": "/dev/ttyACM0", "detail": null,
                    "msg_rate": 0.0,
                    "stats": { "verified": 0, "other_frames": 0, "crc_errors": 0 }
                },
                "aircraft": [{
                    "hex": "7c6b2d", "flight": "RXA6681", "flight_assisted": false,
                    "registration": null,
                    "type_code": null, "type_desc": null, "owner_op": null, "year": null,
                    "military": false,
                    "alt_baro": 12000, "alt_geom": null, "gs": 250.0, "track": 270.0,
                    "baro_rate": -500, "squawk": "1200", "lat": -33.9, "lon": 151.2,
                    "emitter_type": 1, "source": "adsb", "network": false,
                    "country": "Australia", "country_iso": "AU",
                    "messages": 1, "seen": 0.5, "seen_pos": 0.5, "msg_rate": 0.1
                }]
            })
        );
    }
}
