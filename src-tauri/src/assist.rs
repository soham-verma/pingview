//! Online assist: fills in what the Ping can't know.
//!
//! - adsb.lol, then adsb.fi: the aircraft's live network record (callsign,
//!   registration, type, airspeed, wind, selected altitude, ...).
//! - hexdb.io: static aircraft info, callsign routes, airport names.
//! - planespotters.net: a photo, credited to its photographer.
//!
//! Everything is cached in memory; nothing is written to disk. The Ping's
//! own data always takes priority over network data.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::net::NetHealth;

const TIMEOUT: Duration = Duration::from_secs(6);
const LIVE_TTL: Duration = Duration::from_secs(5);
const STATIC_TTL: Duration = Duration::from_secs(24 * 3600);
const MISS_TTL: Duration = Duration::from_secs(3600);
/// adsb.fi's open data API allows one request per second.
const ADSB_FI_SPACING: Duration = Duration::from_millis(1100);

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NetRecord {
    /// Which service answered: "adsb.lol" or "adsb.fi".
    pub via: &'static str,
    pub flight: Option<String>,
    pub registration: Option<String>,
    pub type_code: Option<String>,
    pub type_desc: Option<String>,
    pub owner_op: Option<String>,
    pub year: Option<String>,
    pub category: Option<String>,
    /// Barometric and GPS (WGS84) altitude in feet; the Ping only sends one.
    pub alt_baro: Option<f64>,
    pub alt_geom: Option<f64>,
    pub ias: Option<f64>,
    pub tas: Option<f64>,
    pub mach: Option<f64>,
    pub wind_dir: Option<f64>,
    pub wind_speed: Option<f64>,
    pub oat: Option<f64>,
    pub tat: Option<f64>,
    pub nav_altitude: Option<f64>,
    pub nav_heading: Option<f64>,
    pub nav_qnh: Option<f64>,
    pub mag_heading: Option<f64>,
    pub true_heading: Option<f64>,
    pub geom_rate: Option<f64>,
    pub nic: Option<f64>,
    pub nac_p: Option<f64>,
    pub sil: Option<f64>,
    pub rssi: Option<f64>,
    /// Seconds since the network last heard it.
    pub seen: Option<f64>,
}

/// One aircraft from an area (point + radius) query.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AreaAircraft {
    pub hex: String,
    pub flight: Option<String>,
    pub registration: Option<String>,
    pub type_code: Option<String>,
    pub type_desc: Option<String>,
    pub alt_baro: Option<i32>,
    pub alt_geom: Option<i32>,
    pub gs: Option<f64>,
    pub track: Option<f64>,
    pub baro_rate: Option<i32>,
    pub squawk: Option<String>,
    pub lat: f64,
    pub lon: f64,
    pub military: bool,
    /// Seconds since the network last heard it / last had its position.
    pub seen: f64,
    pub seen_pos: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StaticInfo {
    pub registration: Option<String>,
    pub type_code: Option<String>,
    pub manufacturer: Option<String>,
    pub type_name: Option<String>,
    pub owner: Option<String>,
    pub operator_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Airport {
    pub icao: String,
    pub iata: Option<String>,
    pub name: Option<String>,
    pub country_code: Option<String>,
    /// Region/state, e.g. "New South Wales".
    pub region: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Route {
    pub flight: String,
    pub airports: Vec<Airport>,
    /// Unix seconds when hexdb last updated this route (can be years old).
    pub updated: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Photo {
    pub thumbnail: String,
    pub large: Option<String>,
    pub link: String,
    pub photographer: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Details {
    pub info: Option<StaticInfo>,
    pub net: Option<NetRecord>,
    pub route: Option<Route>,
    pub photo: Option<Photo>,
}

enum Fetched {
    Json(Value),
    NotFound,
    Failed,
}

struct Cache<T> {
    entries: Mutex<HashMap<String, (Instant, Option<T>)>>,
    hit_ttl: Duration,
    miss_ttl: Duration,
}

impl<T: Clone> Cache<T> {
    fn new(hit_ttl: Duration, miss_ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            hit_ttl,
            miss_ttl,
        }
    }

    /// `Some(value)` when cached and fresh (the value itself may be a miss).
    fn get(&self, key: &str) -> Option<Option<T>> {
        let entries = self.entries.lock().unwrap();
        let (at, value) = entries.get(key)?;
        let ttl = if value.is_some() { self.hit_ttl } else { self.miss_ttl };
        (at.elapsed() < ttl).then(|| value.clone())
    }

    fn put(&self, key: &str, value: Option<T>) {
        self.entries
            .lock()
            .unwrap()
            .insert(key.to_string(), (Instant::now(), value));
    }
}

pub struct Assist {
    client: reqwest::Client,
    health: Arc<NetHealth>,
    live: Cache<NetRecord>,
    info: Cache<StaticInfo>,
    routes: Cache<Route>,
    airports: Cache<Airport>,
    photos: Cache<Photo>,
    adsb_fi_next: tokio::sync::Mutex<Instant>,
}

impl Assist {
    pub fn new(client: reqwest::Client, health: Arc<NetHealth>) -> Self {
        Self {
            client,
            health,
            live: Cache::new(LIVE_TTL, LIVE_TTL),
            info: Cache::new(STATIC_TTL, MISS_TTL),
            routes: Cache::new(Duration::from_secs(3600), MISS_TTL),
            airports: Cache::new(Duration::MAX, MISS_TTL),
            photos: Cache::new(STATIC_TTL, MISS_TTL),
            adsb_fi_next: tokio::sync::Mutex::new(Instant::now()),
        }
    }

    async fn fetch(&self, url: &str) -> Fetched {
        // Local-only mode, or backing off after losing the connection.
        if !self.health.allowed() {
            return Fetched::Failed;
        }
        let Ok(resp) = self.client.get(url).timeout(TIMEOUT).send().await else {
            self.health.fail();
            return Fetched::Failed;
        };
        self.health.ok();
        match resp.status().as_u16() {
            200 => resp.json().await.map_or(Fetched::Failed, Fetched::Json),
            404 => Fetched::NotFound,
            // 403/429/5xx are usually rate limits or a firewall hiccup.
            _ => Fetched::Failed,
        }
    }

    /// Fetches and parses, caching definitive answers only: a parsed value
    /// or a clean "not found". Transient failures aren't cached, so the
    /// next refresh tries again.
    async fn lookup<T: Clone>(
        &self,
        cache: &Cache<T>,
        key: &str,
        url: &str,
        parse: impl Fn(&Value) -> Option<T>,
    ) -> Option<T> {
        if let Some(cached) = cache.get(key) {
            return cached;
        }
        let value = match self.fetch(url).await {
            Fetched::Json(v) => parse(&v),
            Fetched::NotFound => None,
            Fetched::Failed => return None,
        };
        cache.put(key, value.clone());
        value
    }

    /// Live network record: adsb.lol first, adsb.fi if that has nothing.
    pub async fn live(&self, hex: &str) -> Option<NetRecord> {
        if let Some(cached) = self.live.get(hex) {
            return cached;
        }
        if self.health.local_only() {
            return None;
        }
        let mut record = match self.fetch(&format!("https://api.adsb.lol/v2/hex/{hex}")).await {
            Fetched::Json(v) => parse_readsb(&v, "adsb.lol"),
            _ => None,
        };
        if record.is_none() {
            {
                let mut next = self.adsb_fi_next.lock().await;
                let now = Instant::now();
                if *next > now {
                    tokio::time::sleep(*next - now).await;
                }
                *next = Instant::now() + ADSB_FI_SPACING;
            }
            record = match self.fetch(&format!("https://opendata.adsb.fi/api/v2/hex/{hex}")).await {
                Fetched::Json(v) => parse_readsb(&v, "adsb.fi"),
                _ => None,
            };
        }
        self.live.put(hex, record.clone());
        record
    }

    /// Every aircraft the network sees within `radius_nm` of a point:
    /// adsb.lol first, adsb.fi (max 250 nm, 1 req/s) if that fails.
    pub async fn area(&self, lat: f64, lon: f64, radius_nm: f64) -> Option<(Vec<AreaAircraft>, &'static str)> {
        if self.health.local_only() {
            return None;
        }
        let url = format!("https://api.adsb.lol/v2/point/{lat:.4}/{lon:.4}/{radius_nm:.0}");
        if let Fetched::Json(v) = self.fetch(&url).await {
            if let Some(list) = parse_readsb_list(&v) {
                return Some((list, "adsb.lol"));
            }
        }
        {
            let mut next = self.adsb_fi_next.lock().await;
            let now = Instant::now();
            if *next > now {
                tokio::time::sleep(*next - now).await;
            }
            *next = Instant::now() + ADSB_FI_SPACING;
        }
        let dist = radius_nm.min(250.0);
        let url = format!("https://opendata.adsb.fi/api/v3/lat/{lat:.4}/lon/{lon:.4}/dist/{dist:.0}");
        match self.fetch(&url).await {
            Fetched::Json(v) => parse_readsb_list(&v).map(|list| (list, "adsb.fi")),
            _ => None,
        }
    }

    pub async fn info(&self, hex: &str) -> Option<StaticInfo> {
        let url = format!("https://hexdb.io/api/v1/aircraft/{hex}");
        self.lookup(&self.info, hex, &url, parse_hexdb_aircraft).await
    }

    pub async fn route(&self, flight: &str) -> Option<Route> {
        let flight = flight.trim().to_uppercase();
        if flight.is_empty() || !flight.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        if let Some(cached) = self.routes.get(&flight) {
            return cached;
        }
        let url = format!("https://hexdb.io/api/v1/route/icao/{flight}");
        let mut route = match self.fetch(&url).await {
            Fetched::Json(v) => parse_hexdb_route(&v),
            Fetched::NotFound => None,
            Fetched::Failed => return None,
        };
        if let Some(route) = route.as_mut() {
            for airport in route.airports.iter_mut() {
                if let Some(full) = self.airport(&airport.icao).await {
                    *airport = full;
                }
            }
        }
        self.routes.put(&flight, route.clone());
        route
    }

    async fn airport(&self, icao: &str) -> Option<Airport> {
        let url = format!("https://hexdb.io/api/v1/airport/icao/{icao}");
        self.lookup(&self.airports, icao, &url, parse_hexdb_airport).await
    }

    pub async fn photo(&self, hex: &str) -> Option<Photo> {
        // Planespotters' firewall is stricter with lowercase hex.
        let url = format!(
            "https://api.planespotters.net/pub/photos/hex/{}",
            hex.to_uppercase()
        );
        self.lookup(&self.photos, hex, &url, parse_planespotters).await
    }

    /// Everything the detail panel shows for one aircraft. `flight` is the
    /// callsign currently known, used for the route if the network has none.
    pub async fn details(&self, hex: &str, flight: Option<&str>) -> Details {
        let (net, info, photo) = tokio::join!(self.live(hex), self.info(hex), self.photo(hex));
        let callsign = flight
            .map(str::to_string)
            .or_else(|| net.as_ref().and_then(|n| n.flight.clone()));
        let route = match callsign {
            Some(cs) => self.route(&cs).await,
            None => None,
        };
        Details {
            info,
            net,
            route,
            photo,
        }
    }
}

fn text(v: &Value) -> Option<String> {
    let s = v.as_str()?.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Parses a readsb-format `{"ac":[{...}]}` response (adsb.lol, adsb.fi).
pub fn parse_readsb(v: &Value, via: &'static str) -> Option<NetRecord> {
    let ac = v["ac"].as_array()?.first()?;
    let num = |key: &str| ac[key].as_f64();
    Some(NetRecord {
        via,
        flight: text(&ac["flight"]),
        registration: text(&ac["r"]),
        type_code: text(&ac["t"]),
        type_desc: text(&ac["desc"]),
        owner_op: text(&ac["ownOp"]),
        year: text(&ac["year"]),
        category: text(&ac["category"]),
        // "ground" comes through as a string, so it's simply absent here.
        alt_baro: num("alt_baro"),
        alt_geom: num("alt_geom"),
        ias: num("ias"),
        tas: num("tas"),
        mach: num("mach"),
        wind_dir: num("wd"),
        wind_speed: num("ws"),
        oat: num("oat"),
        tat: num("tat"),
        nav_altitude: num("nav_altitude_mcp").or_else(|| num("nav_altitude_fms")),
        nav_heading: num("nav_heading"),
        nav_qnh: num("nav_qnh"),
        mag_heading: num("mag_heading"),
        true_heading: num("true_heading"),
        geom_rate: num("geom_rate"),
        nic: num("nic"),
        nac_p: num("nac_p"),
        sil: num("sil"),
        rssi: num("rssi"),
        seen: num("seen"),
    })
}

/// Parses an area response. `None` means the response wasn't a list at all.
pub fn parse_readsb_list(v: &Value) -> Option<Vec<AreaAircraft>> {
    let list = v["ac"].as_array().or_else(|| v["aircraft"].as_array())?;
    Some(
        list.iter()
            .filter_map(|ac| {
                let hex = text(&ac["hex"])?.to_lowercase();
                // Skip non-ICAO addresses ("~" prefix: TIS-B/anonymous) and,
                // below, entries without a position.
                if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return None;
                }
                let int = |key: &str| ac[key].as_f64().map(|v| v.round() as i32);
                Some(AreaAircraft {
                    lat: ac["lat"].as_f64()?,
                    lon: ac["lon"].as_f64()?,
                    flight: text(&ac["flight"]),
                    registration: text(&ac["r"]),
                    type_code: text(&ac["t"]),
                    type_desc: text(&ac["desc"]),
                    // "ground" comes through as a string; leave altitude unset.
                    alt_baro: int("alt_baro"),
                    alt_geom: int("alt_geom"),
                    gs: ac["gs"].as_f64(),
                    track: ac["track"].as_f64(),
                    baro_rate: int("baro_rate").or_else(|| int("geom_rate")),
                    squawk: text(&ac["squawk"]),
                    military: ac["dbFlags"].as_u64().is_some_and(|f| f & 1 != 0),
                    seen: ac["seen"].as_f64().unwrap_or(0.0),
                    seen_pos: ac["seen_pos"].as_f64().unwrap_or(0.0),
                    hex,
                })
            })
            .collect(),
    )
}

pub fn parse_hexdb_aircraft(v: &Value) -> Option<StaticInfo> {
    let info = StaticInfo {
        registration: text(&v["Registration"]),
        type_code: text(&v["ICAOTypeCode"]),
        manufacturer: text(&v["Manufacturer"]),
        type_name: text(&v["Type"]),
        owner: text(&v["RegisteredOwners"]),
        operator_code: text(&v["OperatorFlagCode"]),
    };
    (info != StaticInfo::default()).then_some(info)
}

pub fn parse_hexdb_route(v: &Value) -> Option<Route> {
    let flight = text(&v["flight"])?;
    let airports: Vec<Airport> = text(&v["route"])?
        .split('-')
        .filter(|code| code.len() == 4 && code.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|icao| Airport {
            icao: icao.to_string(),
            iata: None,
            name: None,
            country_code: None,
            region: None,
            lat: None,
            lon: None,
        })
        .collect();
    (airports.len() >= 2).then(|| Route {
        flight,
        airports,
        updated: v["updatetime"].as_i64(),
    })
}

pub fn parse_hexdb_airport(v: &Value) -> Option<Airport> {
    Some(Airport {
        icao: text(&v["icao"])?,
        iata: text(&v["iata"]),
        name: text(&v["airport"]),
        country_code: text(&v["country_code"]),
        region: text(&v["region_name"]),
        lat: v["latitude"].as_f64(),
        lon: v["longitude"].as_f64(),
    })
}

pub fn parse_planespotters(v: &Value) -> Option<Photo> {
    let p = v["photos"].as_array()?.first()?;
    Some(Photo {
        thumbnail: text(&p["thumbnail"]["src"])?,
        large: text(&p["thumbnail_large"]["src"]),
        link: text(&p["link"])?,
        photographer: text(&p["photographer"]).unwrap_or_else(|| "unknown".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Trimmed real responses from 2026-10-13.
    fn adsb_fi() -> Value {
        json!({"ac": [{"hex": "7c6b2d", "flight": "JST446  ", "r": "VH-VGF", "t": "A320",
            "desc": "AIRBUS A-320", "ownOp": "JETSTAR AIRWAYS PTY LIMITED", "year": "2010",
            "alt_baro": 27200, "alt_geom": 27625, "ias": 308, "tas": 454, "mach": 0.764, "wd": 259, "ws": 33, "oat": -41, "tat": -13,
            "category": "A0", "nav_qnh": 1026.0, "nav_altitude_mcp": 35008, "nic": 8,
            "nac_p": 8, "sil": 2, "rssi": -16.0, "seen": 0.0}]})
    }

    #[test]
    fn parses_readsb_record() {
        let r = parse_readsb(&adsb_fi(), "adsb.fi").unwrap();
        assert_eq!(r.via, "adsb.fi");
        assert_eq!(r.flight.as_deref(), Some("JST446"));
        assert_eq!(r.registration.as_deref(), Some("VH-VGF"));
        assert_eq!(r.owner_op.as_deref(), Some("JETSTAR AIRWAYS PTY LIMITED"));
        assert_eq!((r.ias, r.mach, r.wind_dir), (Some(308.0), Some(0.764), Some(259.0)));
        assert_eq!(r.nav_altitude, Some(35008.0));
        assert_eq!((r.alt_baro, r.alt_geom), (Some(27200.0), Some(27625.0)));
        assert_eq!(r.nav_heading, None);
    }

    #[test]
    fn empty_readsb_response_is_none() {
        assert_eq!(parse_readsb(&json!({"ac": [], "msg": "No error"}), "adsb.lol"), None);
        assert_eq!(parse_readsb(&json!({"msg": "error"}), "adsb.lol"), None);
    }

    #[test]
    fn parses_area_list() {
        let v = json!({"ac": [
            {"hex": "7c6b2d", "flight": "JST446  ", "r": "VH-VGF", "t": "A320", "alt_baro": 27200,
             "gs": 478.9, "track": 39.4, "baro_rate": 1056, "lat": -36.6, "lon": 145.7, "seen": 0.2, "seen_pos": 0.4},
            {"hex": "7c1234", "alt_baro": "ground", "lat": -33.9, "lon": 151.2, "dbFlags": 1},
            {"hex": "~2a5c13", "lat": -33.0, "lon": 150.0},
            {"hex": "7cabcd", "flight": "NOPOS"}
        ]});
        let list = parse_readsb_list(&v).unwrap();
        assert_eq!(list.len(), 2, "TIS-B (~) and position-less entries are skipped");
        assert_eq!(list[0].flight.as_deref(), Some("JST446"));
        assert_eq!((list[0].alt_baro, list[0].baro_rate), (Some(27200), Some(1056)));
        assert_eq!(list[1].alt_baro, None, "ground isn't a number");
        assert!(list[1].military);
        assert!(parse_readsb_list(&json!({"msg": "error"})).is_none());
    }

    #[test]
    fn parses_hexdb_aircraft() {
        let v = json!({"ModeS": "7C6B2D", "Registration": "VH-VGF", "Manufacturer": "Airbus",
            "ICAOTypeCode": "A320", "Type": "A320 232", "RegisteredOwners": "Jetstar Airways",
            "OperatorFlagCode": "JST"});
        let info = parse_hexdb_aircraft(&v).unwrap();
        assert_eq!(info.registration.as_deref(), Some("VH-VGF"));
        assert_eq!(info.owner.as_deref(), Some("Jetstar Airways"));
        assert_eq!(parse_hexdb_aircraft(&json!({"status": "404"})), None);
    }

    #[test]
    fn parses_hexdb_route_and_airport() {
        let route =
            parse_hexdb_route(&json!({"flight": "VOZ670", "route": "YSSY-YSCB", "updatetime": 1534681875}))
                .unwrap();
        let codes: Vec<_> = route.airports.iter().map(|a| a.icao.as_str()).collect();
        assert_eq!(codes, ["YSSY", "YSCB"]);
        assert_eq!(route.updated, Some(1534681875));
        assert_eq!(parse_hexdb_route(&json!({"flight": "X", "route": "unknown"})), None);

        let airport = parse_hexdb_airport(&json!({"country_code": "AU", "region_name": "New South Wales",
            "iata": "SYD", "icao": "YSSY", "airport": "Sydney Airport (Kingsford Smith Airport)",
            "latitude": -33.9461, "longitude": 151.177}))
        .unwrap();
        assert_eq!(airport.iata.as_deref(), Some("SYD"));
        assert_eq!(airport.region.as_deref(), Some("New South Wales"));
        assert_eq!((airport.lat, airport.lon), (Some(-33.9461), Some(151.177)));
    }

    #[test]
    fn parses_planespotters_photo() {
        let v = json!({"photos": [{"id": "1981348",
            "thumbnail": {"src": "https://t.plnspttrs.net/42588/1981348_7db5b82212_t.jpg"},
            "thumbnail_large": {"src": "https://t.plnspttrs.net/42588/1981348_7db5b82212_280.jpg"},
            "link": "https://www.planespotters.net/photo/1981348/vh-vgf", "photographer": "Tom_YA"}]});
        let photo = parse_planespotters(&v).unwrap();
        assert_eq!(photo.photographer, "Tom_YA");
        assert!(photo.large.unwrap().ends_with("_280.jpg"));
        assert_eq!(parse_planespotters(&json!({"photos": []})), None);
    }

    /// Hits the real services. Run with `cargo test -- --ignored live_`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs network"]
    async fn live_details_for_a_known_aircraft() {
        let client = reqwest::Client::builder()
            .user_agent("PingView/0.1.0 (+https://github.com/soham-verma/pingview)")
            .build()
            .unwrap();
        let assist = Assist::new(client, Arc::new(NetHealth::new(false)));
        let d = assist.details("7c6b2d", Some("VOZ670")).await;
        assert_eq!(d.info.unwrap().registration.as_deref(), Some("VH-VGF"));
        assert!(d.photo.is_some(), "planespotters has photos of VH-VGF");
        let route = d.route.unwrap();
        assert_eq!(route.airports[0].iata.as_deref(), Some("SYD"));
        eprintln!("net: {:?}", d.net.map(|n| (n.via, n.flight)));

        let (list, via) = assist.area(-33.9, 151.2, 100.0).await.expect("area query answers");
        assert!(!list.is_empty(), "Sydney always has traffic within 100 nm");
        assert!(list.iter().all(|a| a.hex.len() == 6));
        eprintln!("area: {} aircraft via {via}", list.len());
    }

    #[tokio::test]
    async fn local_only_makes_no_requests() {
        // An unroutable address: if a request were attempted it would fail
        // slowly; local-only must short-circuit before that.
        let assist = Assist::new(reqwest::Client::new(), Arc::new(NetHealth::new(true)));
        let started = std::time::Instant::now();
        assert!(assist.live("7c6b2d").await.is_none());
        assert!(assist.area(-33.9, 151.2, 50.0).await.is_none());
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn cache_respects_hit_and_miss_ttls() {
        let cache: Cache<String> = Cache::new(Duration::from_secs(60), Duration::ZERO);
        assert_eq!(cache.get("a"), None);
        cache.put("a", Some("x".into()));
        assert_eq!(cache.get("a"), Some(Some("x".into())));
        cache.put("b", None);
        assert_eq!(cache.get("b"), None, "misses expire immediately with a zero TTL");
    }
}
