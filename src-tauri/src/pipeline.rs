//! Bytes in, aircraft state out. Shared by the serial reader, replay mode
//! and the 1 Hz ticker; holds no I/O itself apart from history writes.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use chrono::{DateTime, Utc};

use crate::adsb::{decode_adsb_vehicle, MSG_ID_ADSB_VEHICLE};
use crate::adsbee::SettingsPage;
use crate::assist::AreaAircraft;
use crate::history::History;
use crate::mavlink::{Frame, Framer};
use crate::registry::Registry;
use crate::store::{Aircraft, Battery, Model, ReceiverInfo, ReceiverStatus, Store, STALE_SECS};

/// System and component ID the pingRX Pro stamps on every frame.
pub const PING_SYSID: u8 = 1;
pub const PING_COMPID: u8 = 0x9C;
/// Unverified Ping-shaped frames needed in a row to accept a port.
const IDENTIFY_RUN: u32 = 3;
/// With no message 246 for this long, status drops to `no_traffic`.
const NO_TRAFFIC_SECS: f64 = 60.0;
const RATE_WINDOW_SECS: f64 = 5.0;
/// Ask the network again after this long if it had no callsign either.
const ASSIST_RETRY_SECS: f64 = 120.0;

/// Decides whether a port is really a receiver: a Ping (see spec, Device)
/// or anything sending ADS-B heartbeats, like the ADSBee.
#[derive(Default)]
pub struct Identifier {
    run: u32,
    confirmed: bool,
}

impl Identifier {
    pub fn observe(&mut self, frame: &Frame) -> bool {
        if self.confirmed {
            return true;
        }
        if (frame.verified && frame.msgid == MSG_ID_ADSB_VEHICLE) || frame.is_adsb_heartbeat() {
            self.confirmed = true;
        } else if frame.sysid == PING_SYSID && frame.compid == PING_COMPID {
            self.run += 1;
            self.confirmed = self.run >= IDENTIFY_RUN;
        } else {
            self.run = 0;
        }
        self.confirmed
    }

    pub fn confirmed(&self) -> bool {
        self.confirmed
    }
}

pub struct Inner {
    pub store: Store,
    pub receiver: ReceiverInfo,
    pub history: Option<History>,
    pub assist_enabled: bool,
    framer: Framer,
    identifier: Identifier,
    last_report_at: Option<f64>,
    report_times: VecDeque<f64>,
    assist_requested: HashMap<String, f64>,
    /// Aircraft from the last network area query (not heard by the receiver).
    network: Vec<Aircraft>,
}

/// Filled in by a background thread once the dump has loaded.
pub type SharedRegistry = Arc<OnceLock<Registry>>;

#[derive(Default)]
pub struct Pipeline {
    inner: Mutex<Inner>,
    registry: SharedRegistry,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            store: Store::new(),
            receiver: ReceiverInfo::default(),
            history: None,
            assist_enabled: false,
            framer: Framer::new(),
            identifier: Identifier::default(),
            last_report_at: None,
            report_times: VecDeque::new(),
            assist_requested: HashMap::new(),
            network: Vec::new(),
        }
    }
}

impl Pipeline {
    pub fn new(history: Option<History>, assist_enabled: bool, registry: SharedRegistry) -> Self {
        let inner = Inner {
            history,
            assist_enabled,
            ..Default::default()
        };
        Self {
            inner: Mutex::new(inner),
            registry,
        }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A port was just opened: start framing from scratch.
    pub fn start_link(&self, port: &str, model: Model) {
        let mut s = self.lock();
        s.framer = Framer::new();
        s.identifier = Identifier::default();
        s.last_report_at = None;
        s.receiver = ReceiverInfo {
            status: ReceiverStatus::Connecting,
            port: Some(port.to_string()),
            model: Some(model),
            ..Default::default()
        };
    }

    pub fn set_status(&self, status: ReceiverStatus, port: Option<&str>, detail: Option<String>) {
        let mut s = self.lock();
        s.receiver.status = status;
        s.receiver.port = port.map(str::to_string);
        s.receiver.detail = detail;
        s.receiver.msg_rate = 0.0;
        s.receiver.battery = None;
        s.receiver.model = None;
        s.receiver.settings_page = None;
    }

    pub fn set_settings_page(&self, page: Option<SettingsPage>) {
        self.lock().receiver.settings_page = page;
    }

    pub fn set_battery(&self, battery: Option<Battery>) {
        self.lock().receiver.battery = battery;
    }

    pub fn identified(&self) -> bool {
        self.lock().identifier.confirmed()
    }

    /// Feeds raw serial bytes. Returns hexes that need a network assist
    /// lookup (no callsign from the Ping yet).
    pub fn ingest(&self, bytes: &[u8], now: f64, now_utc: DateTime<Utc>) -> Vec<String> {
        let mut s = self.lock();
        let s = &mut *s;
        let mut lookups = Vec::new();
        for frame in s.framer.push(bytes) {
            if s.identifier.observe(&frame) && s.receiver.status == ReceiverStatus::Connecting {
                s.receiver.status = ReceiverStatus::NoTraffic;
            }
            if !(frame.verified && frame.msgid == MSG_ID_ADSB_VEHICLE) {
                continue;
            }
            let Some(report) = decode_adsb_vehicle(&frame.payload) else {
                continue;
            };
            s.last_report_at = Some(now);
            s.report_times.push_back(now);
            s.receiver.status = ReceiverStatus::Connected;
            if let Some(ac) = s.store.apply(&report, now) {
                if let Some(history) = s.history.as_mut() {
                    if let Err(e) = history.record(ac, now_utc) {
                        eprintln!("history write failed, turning history off: {e}");
                        s.history = None;
                    }
                }
            }
            if let Some(registry) = self.registry.get() {
                if !s.store.get(&report.hex).expect("just applied").registry_checked {
                    s.store.apply_registry(&report.hex, registry.get(&report.hex));
                }
            }
            let ac = s.store.get(&report.hex).expect("just applied");
            if s.assist_enabled && ac.flight.is_none() {
                let due = s
                    .assist_requested
                    .get(&report.hex)
                    .is_none_or(|t| now - t > ASSIST_RETRY_SECS);
                if due {
                    s.assist_requested.insert(report.hex.clone(), now);
                    lookups.push(report.hex.clone());
                }
            }
        }
        s.receiver.stats = s.framer.stats;
        lookups
    }

    /// Runs once a second: ages out aircraft, refreshes receiver status and
    /// returns the snapshot to send to the frontend.
    pub fn tick(&self, now: f64) -> serde_json::Value {
        let mut s = self.lock();
        let s = &mut *s;
        s.store.prune(now);
        while s.report_times.front().is_some_and(|t| now - t > RATE_WINDOW_SECS) {
            s.report_times.pop_front();
        }
        let live = matches!(
            s.receiver.status,
            ReceiverStatus::Connected | ReceiverStatus::NoTraffic
        );
        if live {
            s.receiver.msg_rate = (s.report_times.len() as f64 / RATE_WINDOW_SECS * 10.0).round() / 10.0;
            let quiet = s.last_report_at.is_none_or(|t| now - t > NO_TRAFFIC_SECS);
            s.receiver.status = if quiet {
                ReceiverStatus::NoTraffic
            } else {
                ReceiverStatus::Connected
            };
        }
        s.network.retain(|ac| now - ac.last_seen < STALE_SECS);
        serde_json::to_value(s.store.snapshot(now, &s.receiver, &s.network)).expect("snapshot serializes")
    }

    /// Replaces the network aircraft with a fresh area query result,
    /// filling identity from the offline registry like receiver aircraft.
    pub fn set_network(&self, list: Vec<AreaAircraft>, via: &'static str, now: f64) {
        let registry = self.registry.get();
        let network = list
            .into_iter()
            .map(|n| {
                let mut ac = Aircraft::new(&n.hex, now - n.seen);
                if let Some(entry) = registry.and_then(|r| r.get(&n.hex)) {
                    ac.type_desc = entry.type_desc;
                    ac.owner_op = entry.owner_op;
                    ac.year = entry.year;
                    ac.military = entry.military;
                    ac.registration = entry.registration;
                    ac.type_code = entry.type_code;
                }
                ac.network = true;
                ac.source = via;
                ac.flight = n.flight;
                ac.registration = ac.registration.or(n.registration);
                ac.type_code = ac.type_code.or(n.type_code);
                ac.type_desc = ac.type_desc.or(n.type_desc);
                ac.military |= n.military;
                ac.alt_baro = n.alt_baro;
                ac.alt_geom = n.alt_geom;
                ac.gs = n.gs;
                ac.track = n.track;
                ac.baro_rate = n.baro_rate;
                ac.squawk = n.squawk;
                ac.lat = Some(n.lat);
                ac.lon = Some(n.lon);
                ac.last_seen_pos = Some(now - n.seen_pos);
                ac
            })
            .collect();
        self.lock().network = network;
    }

    pub fn clear_network(&self) {
        self.lock().network.clear();
    }

    pub fn apply_assist(&self, hex: &str, flight: Option<String>, registration: Option<String>) {
        self.lock().store.apply_assist(hex, flight, registration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adsb::tests::{payload, sample};
    use crate::mavlink::tests::{v1_frame, v2_frame_from};

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/capture-2026-10-13.bin");

    fn now_utc() -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000, 0).unwrap()
    }

    #[test]
    fn identifies_ping_from_one_traffic_report() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("test", Model::PingRx);
        p.ingest(&v1_frame(0, 246, &payload(&sample()), 184), 0.0, now_utc());
        assert!(p.identified());
        assert_eq!(p.lock().receiver.status, ReceiverStatus::Connected);
    }

    #[test]
    fn identifies_idle_ping_from_status_frames() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("test", Model::PingRx);
        let status: Vec<u8> = (0..4).flat_map(|i| v1_frame(i, 203, &[0], 0)).collect();
        p.ingest(&status, 0.0, now_utc());
        assert!(p.identified());
        assert_eq!(p.lock().receiver.status, ReceiverStatus::NoTraffic);
    }

    /// MAVLink HEARTBEAT payload: custom_mode, type, autopilot, base_mode,
    /// system_status, mavlink_version.
    fn heartbeat(mav_type: u8) -> Vec<u8> {
        vec![0, 0, 0, 0, mav_type, 8, 0, 4, 3]
    }

    #[test]
    fn identifies_idle_adsbee_from_its_heartbeat() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("/dev/cu.usbmodem1101", Model::Adsbee);
        p.ingest(&v2_frame_from(1, 1, 0, &heartbeat(27), 50), 0.0, now_utc());
        assert!(p.identified());
        assert_eq!(p.lock().receiver.status, ReceiverStatus::NoTraffic);
    }

    #[test]
    fn other_mavlink_heartbeats_are_not_identified() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("test", Model::Adsbee);
        let quadcopter: Vec<u8> = (0..4).flat_map(|_| v2_frame_from(1, 1, 0, &heartbeat(2), 50)).collect();
        p.ingest(&quadcopter, 0.0, now_utc());
        assert!(!p.identified());
    }

    #[test]
    fn snapshot_names_the_receiver_and_its_settings_page() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("/dev/cu.usbmodem1101", Model::Adsbee);
        p.set_settings_page(Some(SettingsPage::Url { url: "http://192.168.1.42/".into() }));
        let snap = p.tick(1.0);
        assert_eq!(snap["receiver"]["model"], "ADSBee 1090");
        assert_eq!(snap["receiver"]["settings_page"]["url"], "http://192.168.1.42/");

        p.start_link("ble://PingView-f7c4", Model::PingRx);
        let snap = p.tick(2.0);
        assert_eq!(snap["receiver"]["model"], "pingRX");
        assert!(snap["receiver"]["settings_page"].is_null(), "a new link starts without one");
    }

    #[test]
    fn other_devices_are_not_identified() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("test", Model::PingRx);
        p.ingest(b"hello from some other STM32 board\r\n".repeat(20).as_slice(), 0.0, now_utc());
        assert!(!p.identified());
    }

    #[test]
    fn status_drops_to_no_traffic_after_a_quiet_minute() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("test", Model::PingRx);
        p.ingest(&v1_frame(0, 246, &payload(&sample()), 184), 0.0, now_utc());
        p.tick(1.0);
        assert_eq!(p.lock().receiver.status, ReceiverStatus::Connected);
        p.tick(NO_TRAFFIC_SECS + 1.0);
        assert_eq!(p.lock().receiver.status, ReceiverStatus::NoTraffic);
    }

    #[test]
    fn battery_belongs_to_the_current_link() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        let battery = Battery { mv: 3910, percent: Some(68) };
        p.start_link("ble://PingView-f7c4", Model::PingRx);
        p.set_battery(Some(battery));
        assert_eq!(p.tick(1.0)["receiver"]["battery"]["percent"], 68);

        p.set_status(ReceiverStatus::Searching, None, None);
        assert_eq!(p.lock().receiver.battery, None, "a lost link has no battery");

        p.start_link("ble://PingView-f7c4", Model::PingRx);
        p.set_battery(Some(battery));
        p.start_link("/dev/ttyACM0", Model::PingRx);
        assert_eq!(p.lock().receiver.battery, None, "a new link starts without one");
    }

    #[test]
    fn requests_assist_for_aircraft_without_callsign() {
        let p = Pipeline::new(None, true, SharedRegistry::default());
        p.start_link("test", Model::PingRx);
        let mut f = sample();
        f.flags &= !0x0010; // no callsign
        let frame = v1_frame(0, 246, &payload(&f), 184);
        assert_eq!(p.ingest(&frame, 0.0, now_utc()), vec!["7c6b2d"]);
        assert!(p.ingest(&frame, 1.0, now_utc()).is_empty());
        assert_eq!(p.ingest(&frame, ASSIST_RETRY_SECS + 2.0, now_utc()), vec!["7c6b2d"]);
    }

    #[test]
    fn registry_fills_identity_once_loaded() {
        let registry = SharedRegistry::default();
        let _ = registry.set(Registry::from_text("7C6B2D;VH-VGF;A320;00;AIRBUS A-320;2010;;\n".into()));
        let p = Pipeline::new(None, false, registry);
        p.start_link("test", Model::PingRx);
        let mut f = sample();
        f.flags &= !0x0010; // no callsign from the Ping
        p.ingest(&v1_frame(0, 246, &payload(&f), 184), 0.0, now_utc());
        let s = p.lock();
        let ac = s.store.get("7c6b2d").unwrap();
        assert_eq!(ac.registration.as_deref(), Some("VH-VGF"));
        assert_eq!(ac.type_code.as_deref(), Some("A320"));
    }

    #[test]
    fn network_aircraft_are_tagged_and_merged() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("test", Model::PingRx);
        p.ingest(&v1_frame(0, 246, &payload(&sample()), 184), 100.0, now_utc());
        let net = |hex: &str| AreaAircraft {
            hex: hex.into(),
            lat: -33.0,
            lon: 151.0,
            ..Default::default()
        };
        p.set_network(vec![net("7c6b2d"), net("7c0001")], "adsb.lol", 100.0);
        let snap = p.tick(100.0);
        let list = snap["aircraft"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        let own = list.iter().find(|a| a["hex"] == "7c6b2d").unwrap();
        assert_eq!(own["network"], false, "the receiver's copy wins");
        let other = list.iter().find(|a| a["hex"] == "7c0001").unwrap();
        assert_eq!((other["network"].as_bool(), other["source"].as_str()), (Some(true), Some("adsb.lol")));
        assert!(p.tick(100.0 + STALE_SECS + 1.0)["aircraft"].as_array().unwrap().iter().all(|a| a["network"] == false));
    }

    #[test]
    fn real_capture_yields_aircraft() {
        let p = Pipeline::new(None, false, SharedRegistry::default());
        p.start_link("fixture", Model::PingRx);
        p.ingest(FIXTURE, 0.0, now_utc());
        let s = p.lock();
        assert!(s.identifier.confirmed());
        assert!(s.receiver.stats.verified > 10, "{:?}", s.receiver.stats);
        let crc_rate = s.receiver.stats.crc_errors as f64 / s.receiver.stats.verified as f64;
        assert!(crc_rate < 0.05, "{:?}", s.receiver.stats);
        drop(s);
        let snap = p.tick(0.0);
        let aircraft = snap["aircraft"].as_array().unwrap();
        assert!(!aircraft.is_empty());
        for ac in aircraft {
            if let (Some(lat), Some(lon)) = (ac["lat"].as_f64(), ac["lon"].as_f64()) {
                assert!((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon));
            }
            if let Some(sq) = ac["squawk"].as_str() {
                assert!(sq.len() == 4 && sq.chars().all(|c| ('0'..='7').contains(&c)), "{sq}");
            }
        }
    }
}
