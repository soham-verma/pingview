//! Decodes MAVLink `ADSB_VEHICLE` (message 246) into a `TrafficReport`.
//!
//! Field layout and flags follow the public MAVLink common message set.
//! Units are converted to what the frontend expects: feet, knots, ft/min.

pub const MSG_ID_ADSB_VEHICLE: u32 = 246;
pub const CRC_EXTRA: u8 = 184;
pub const PAYLOAD_LEN: usize = 38;

const FLAG_VALID_COORDS: u16 = 0x0001;
const FLAG_VALID_ALTITUDE: u16 = 0x0002;
const FLAG_VALID_HEADING: u16 = 0x0004;
const FLAG_VALID_VELOCITY: u16 = 0x0008;
const FLAG_VALID_CALLSIGN: u16 = 0x0010;
const FLAG_VALID_SQUAWK: u16 = 0x0020;
const FLAG_SIMULATED: u16 = 0x0040;
const FLAG_VERTICAL_VELOCITY_VALID: u16 = 0x0080;
const FLAG_SOURCE_UAT: u16 = 0x8000;

const ALTITUDE_TYPE_GEOMETRIC: u8 = 1;

const MM_PER_FOOT: f64 = 304.8;
const CMS_TO_KNOTS: f64 = 0.019_438_444_9;
const CMS_TO_FPM: f64 = 1.968_503_94;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrafficReport {
    pub hex: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub alt_baro: Option<i32>,
    pub alt_geom: Option<i32>,
    pub track: Option<f64>,
    pub gs: Option<f64>,
    pub baro_rate: Option<i32>,
    pub flight: Option<String>,
    pub squawk: Option<String>,
    pub emitter_type: u8,
    /// Heard on 978 MHz UAT rather than 1090 MHz ADS-B.
    pub uat: bool,
    /// Seconds since the Ping last heard this aircraft.
    pub tslc: u8,
}

/// Decodes a message-246 payload. Returns `None` for simulated traffic.
/// MAVLink v2 trims trailing zero bytes, so short payloads are zero-padded.
pub fn decode_adsb_vehicle(payload: &[u8]) -> Option<TrafficReport> {
    let mut p = [0u8; PAYLOAD_LEN];
    let n = payload.len().min(PAYLOAD_LEN);
    p[..n].copy_from_slice(&payload[..n]);

    let u16_at = |i: usize| u16::from_le_bytes([p[i], p[i + 1]]);
    let i32_at = |i: usize| i32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);

    let icao = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
    let lat = i32_at(4);
    let lon = i32_at(8);
    let altitude_mm = i32_at(12);
    let heading_cdeg = u16_at(16);
    let hor_velocity = u16_at(18);
    let ver_velocity = i16::from_le_bytes([p[20], p[21]]);
    let flags = u16_at(22);
    let squawk = u16_at(24);
    let altitude_type = p[26];
    let callsign = &p[27..36];
    let emitter_type = p[36];
    let tslc = p[37];

    if flags & FLAG_SIMULATED != 0 {
        return None;
    }

    let has = |flag: u16| flags & flag != 0;
    let mut r = TrafficReport {
        hex: format!("{:06x}", icao & 0x00FF_FFFF),
        emitter_type,
        uat: flags & FLAG_SOURCE_UAT != 0,
        tslc,
        ..Default::default()
    };

    if has(FLAG_VALID_COORDS) {
        r.lat = Some(lat as f64 / 1e7);
        r.lon = Some(lon as f64 / 1e7);
    }
    if has(FLAG_VALID_ALTITUDE) {
        let feet = (altitude_mm as f64 / MM_PER_FOOT).round() as i32;
        if altitude_type == ALTITUDE_TYPE_GEOMETRIC {
            r.alt_geom = Some(feet);
        } else {
            r.alt_baro = Some(feet);
        }
    }
    if has(FLAG_VALID_HEADING) {
        r.track = Some(heading_cdeg as f64 / 100.0);
    }
    if has(FLAG_VALID_VELOCITY) {
        r.gs = Some(round1(hor_velocity as f64 * CMS_TO_KNOTS));
    }
    if has(FLAG_VERTICAL_VELOCITY_VALID) {
        r.baro_rate = Some((ver_velocity as f64 * CMS_TO_FPM).round() as i32);
    }
    if has(FLAG_VALID_CALLSIGN) {
        let text: String = callsign
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as char)
            .collect();
        let text = text.trim();
        if !text.is_empty() {
            r.flight = Some(text.to_string());
        }
    }
    if has(FLAG_VALID_SQUAWK) {
        r.squawk = Some(format_squawk(squawk));
    }
    Some(r)
}

/// Squawk codes are four octal digits. The Ping sends the code as its
/// decimal digits (7700 → 7700), confirmed against the 2026-10-13 capture.
fn format_squawk(raw: u16) -> String {
    format!("{raw:04}")
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub struct Fields {
        pub icao: u32,
        pub lat: i32,
        pub lon: i32,
        pub altitude_mm: i32,
        pub heading: u16,
        pub hor_velocity: u16,
        pub ver_velocity: i16,
        pub flags: u16,
        pub squawk: u16,
        pub altitude_type: u8,
        pub callsign: &'static str,
        pub emitter_type: u8,
        pub tslc: u8,
    }

    pub fn payload(f: &Fields) -> Vec<u8> {
        let mut p = Vec::with_capacity(PAYLOAD_LEN);
        p.extend(f.icao.to_le_bytes());
        p.extend(f.lat.to_le_bytes());
        p.extend(f.lon.to_le_bytes());
        p.extend(f.altitude_mm.to_le_bytes());
        p.extend(f.heading.to_le_bytes());
        p.extend(f.hor_velocity.to_le_bytes());
        p.extend(f.ver_velocity.to_le_bytes());
        p.extend(f.flags.to_le_bytes());
        p.extend(f.squawk.to_le_bytes());
        p.push(f.altitude_type);
        let mut cs = [0u8; 9];
        cs[..f.callsign.len()].copy_from_slice(f.callsign.as_bytes());
        p.extend(cs);
        p.push(f.emitter_type);
        p.push(f.tslc);
        p
    }

    pub fn sample() -> Fields {
        Fields {
            icao: 0x7C6B2D,
            lat: -339_000_000,
            lon: 1_512_000_000,
            altitude_mm: 3_657_600, // 12000 ft
            heading: 27_000,
            hor_velocity: 12_861, // ~250 kt
            ver_velocity: -254,   // ~-500 ft/min
            flags: 0x00BF,        // everything valid, not simulated
            squawk: 1200,
            altitude_type: 0,
            callsign: "RXA6681",
            emitter_type: 1,
            tslc: 2,
        }
    }

    #[test]
    fn decodes_all_fields_with_unit_conversion() {
        let r = decode_adsb_vehicle(&payload(&sample())).unwrap();
        assert_eq!(r.hex, "7c6b2d");
        assert_eq!(r.lat, Some(-33.9));
        assert_eq!(r.lon, Some(151.2));
        assert_eq!(r.alt_baro, Some(12000));
        assert_eq!(r.alt_geom, None);
        assert_eq!(r.track, Some(270.0));
        assert_eq!(r.gs, Some(250.0));
        assert_eq!(r.baro_rate, Some(-500));
        assert_eq!(r.flight.as_deref(), Some("RXA6681"));
        assert_eq!(r.squawk.as_deref(), Some("1200"));
        assert_eq!((r.emitter_type, r.tslc), (1, 2));
        assert!(!r.uat);
    }

    #[test]
    fn uat_flag_marks_source() {
        let mut f = sample();
        f.flags |= 0x8000;
        assert!(decode_adsb_vehicle(&payload(&f)).unwrap().uat);
    }

    #[test]
    fn geometric_altitude_goes_to_alt_geom() {
        let mut f = sample();
        f.altitude_type = 1;
        let r = decode_adsb_vehicle(&payload(&f)).unwrap();
        assert_eq!((r.alt_baro, r.alt_geom), (None, Some(12000)));
    }

    #[test]
    fn fields_without_validity_flag_are_none() {
        let mut f = sample();
        f.flags = 0;
        let r = decode_adsb_vehicle(&payload(&f)).unwrap();
        assert_eq!(r.hex, "7c6b2d");
        assert!(r.lat.is_none() && r.alt_baro.is_none() && r.track.is_none());
        assert!(r.gs.is_none() && r.baro_rate.is_none());
        assert!(r.flight.is_none() && r.squawk.is_none());
    }

    #[test]
    fn simulated_traffic_is_dropped() {
        let mut f = sample();
        f.flags |= 0x0040;
        assert!(decode_adsb_vehicle(&payload(&f)).is_none());
    }

    #[test]
    fn blank_callsign_is_none() {
        let mut f = sample();
        f.callsign = "        ";
        assert!(decode_adsb_vehicle(&payload(&f)).unwrap().flight.is_none());
    }

    #[test]
    fn short_v2_payload_is_zero_padded() {
        let full = payload(&sample());
        let r = decode_adsb_vehicle(&full[..20]).unwrap();
        assert_eq!(r.hex, "7c6b2d");
        assert!(r.squawk.is_none(), "flags were cut off, so nothing is valid");
    }
}
