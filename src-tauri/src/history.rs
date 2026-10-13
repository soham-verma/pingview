//! Flat-file position history: one JSONL file per UTC day, same row format
//! adsb-tar1090 writes, so files move freely between the two apps.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::json;

use crate::store::Aircraft;

pub struct History {
    dir: PathBuf,
    last: HashMap<String, (f64, f64)>,
}

impl History {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            last: HashMap::new(),
        }
    }

    /// Appends a row for `ac` unless its position hasn't moved since the
    /// last row written for it.
    pub fn record(&mut self, ac: &Aircraft, at: DateTime<Utc>) -> io::Result<()> {
        let (Some(lat), Some(lon)) = (ac.lat, ac.lon) else {
            return Ok(());
        };
        if self.last.get(&ac.hex) == Some(&(lat, lon)) {
            return Ok(());
        }
        let row = json!({
            "icao": ac.hex,
            "ts": at.to_rfc3339_opts(SecondsFormat::Millis, true),
            "lat": lat,
            "lon": lon,
            // The frontend colours history by alt_baro; fall back to
            // geometric altitude when that's all the Ping reported.
            "alt_baro": ac.alt_baro.or(ac.alt_geom),
            "alt_geom": ac.alt_geom,
            "gs": ac.gs,
            "track": ac.track,
            "baro_rate": ac.baro_rate,
            "on_ground": null,
            "flight": ac.flight,
            "squawk": ac.squawk,
        });
        fs::create_dir_all(&self.dir)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(&at.format("%Y-%m-%d").to_string()))?;
        writeln!(file, "{row}")?;
        self.last.insert(ac.hex.clone(), (lat, lon));
        Ok(())
    }

    /// Dates with recordings, newest first.
    pub fn list(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut dates: Vec<String> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter_map(|name| {
                let date = name.strip_prefix("adsb-")?.strip_suffix(".jsonl")?;
                is_date(date).then(|| date.to_string())
            })
            .collect();
        dates.sort_unstable_by(|a, b| b.cmp(a));
        dates
    }

    pub fn read(&self, date: &str) -> io::Result<String> {
        if !is_date(date) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad date"));
        }
        fs::read_to_string(self.path(date))
    }

    pub fn path(&self, date: &str) -> PathBuf {
        self.dir.join(format!("adsb-{date}.jsonl"))
    }
}

/// Strict `YYYY-MM-DD`, so a date can never become a path traversal.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn aircraft(hex: &str, lat: f64, lon: f64) -> Aircraft {
        let mut ac = Aircraft::new(hex, 0.0);
        ac.flight = Some("QFA1".into());
        ac.alt_geom = Some(3000);
        ac.gs = Some(120.0);
        ac.lat = Some(lat);
        ac.lon = Some(lon);
        ac
    }

    #[test]
    fn writes_rows_and_dedups_unmoved_positions() {
        let tmp = tempfile::tempdir().unwrap();
        let mut h = History::new(tmp.path().to_path_buf());
        let t = Utc.with_ymd_and_hms(2026, 10, 13, 9, 0, 0).unwrap();
        h.record(&aircraft("abc123", -33.9, 151.2), t).unwrap();
        h.record(&aircraft("abc123", -33.9, 151.2), t).unwrap();
        h.record(&aircraft("abc123", -33.8, 151.2), t).unwrap();

        let text = h.read("2026-10-13").unwrap();
        let rows: Vec<serde_json::Value> =
            text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["icao"], "abc123");
        assert_eq!(rows[0]["ts"], "2026-10-13T09:00:00.000Z");
        assert_eq!(rows[0]["alt_baro"], 3000, "falls back to geometric altitude");
    }

    #[test]
    fn rolls_over_to_a_new_file_at_utc_midnight() {
        let tmp = tempfile::tempdir().unwrap();
        let mut h = History::new(tmp.path().to_path_buf());
        let before = Utc.with_ymd_and_hms(2026, 10, 13, 23, 59, 59).unwrap();
        let after = Utc.with_ymd_and_hms(2026, 10, 14, 0, 0, 1).unwrap();
        h.record(&aircraft("abc123", 1.0, 1.0), before).unwrap();
        h.record(&aircraft("abc123", 2.0, 2.0), after).unwrap();
        assert_eq!(h.list(), vec!["2026-10-14", "2026-10-13"]);
    }

    #[test]
    fn rejects_dates_that_are_not_yyyy_mm_dd() {
        let tmp = tempfile::tempdir().unwrap();
        let h = History::new(tmp.path().to_path_buf());
        assert!(h.read("../../etc/passwd").is_err());
        assert!(h.read("2026-1-13").is_err());
        assert!(is_date("2026-10-13"));
    }
}
