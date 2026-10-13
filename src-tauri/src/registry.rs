//! Offline aircraft registry: ICAO hex → registration, type, owner, from the
//! bundled tar1090-db dump (`aircraft.csv.gz`, ~620k rows of
//! `hex;reg;type;flags;desc;year;ownop;`).
//!
//! The decompressed text is kept as one buffer with a sorted index of line
//! offsets, so lookups are a binary search and memory stays near the file size.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use flate2::read::GzDecoder;
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RegistryEntry {
    pub registration: Option<String>,
    pub type_code: Option<String>,
    pub type_desc: Option<String>,
    pub year: Option<String>,
    pub owner_op: Option<String>,
    pub military: bool,
    pub interesting: bool,
    /// FAA Privacy ICAO Address: the hex isn't the aircraft's real one.
    pub pia: bool,
    /// FAA Limiting Aircraft Data Displayed.
    pub ladd: bool,
}

pub struct Registry {
    text: String,
    /// (ICAO address, byte offset of its line), sorted by address.
    index: Vec<(u32, u32)>,
}

impl Registry {
    pub fn load_gz(path: &Path) -> io::Result<Self> {
        let mut text = String::new();
        GzDecoder::new(File::open(path)?).read_to_string(&mut text)?;
        Ok(Self::from_text(text))
    }

    pub fn from_text(text: String) -> Self {
        let mut index = Vec::with_capacity(text.len() / 50);
        let mut offset = 0usize;
        for line in text.split_inclusive('\n') {
            if let Some(hex) = line.split(';').next() {
                if let Ok(addr) = u32::from_str_radix(hex.trim(), 16) {
                    index.push((addr, offset as u32));
                }
            }
            offset += line.len();
        }
        index.sort_unstable_by_key(|&(addr, _)| addr);
        Self { text, index }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn get(&self, hex: &str) -> Option<RegistryEntry> {
        let addr = u32::from_str_radix(hex, 16).ok()?;
        let i = self.index.binary_search_by_key(&addr, |&(a, _)| a).ok()?;
        let start = self.index[i].1 as usize;
        let line = self.text[start..].lines().next()?;
        let f: Vec<&str> = line.split(';').collect();
        let field = |n: usize| {
            f.get(n)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let flag = |n: usize| f.get(3).and_then(|s| s.as_bytes().get(n)) == Some(&b'1');
        Some(RegistryEntry {
            registration: field(1),
            type_code: field(2),
            type_desc: field(4),
            year: field(5),
            owner_op: field(6),
            military: flag(0),
            interesting: flag(1),
            pia: flag(2),
            ladd: flag(3),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
A00001;N1;C680;0001;CESSNA 680 Citation Sovereign;;FEDERAL AVIATION ADMINISTRATION;
7C6B2D;VH-VGF;A320;00;AIRBUS A-320;2010;;
7C52F9;VH-QOB;DH8D;00;DE HAVILLAND DHC-8-400 Dash 8;;;
7CF800;TEST-MIL;A332;10;SYNTHETIC MILITARY ROW;;EXAMPLE AIR FORCE;
not a row
";

    #[test]
    fn looks_up_by_hex_case_insensitively() {
        let r = Registry::from_text(SAMPLE.to_string());
        assert_eq!(r.len(), 4);
        let e = r.get("7c6b2d").unwrap();
        assert_eq!(e.registration.as_deref(), Some("VH-VGF"));
        assert_eq!(e.type_code.as_deref(), Some("A320"));
        assert_eq!(e.type_desc.as_deref(), Some("AIRBUS A-320"));
        assert_eq!(e.year.as_deref(), Some("2010"));
        assert_eq!(e.owner_op, None);
        assert!(!e.military);
        assert_eq!(r.get("7C52F9").unwrap().registration.as_deref(), Some("VH-QOB"));
        assert_eq!(r.get("123456"), None);
        assert_eq!(r.get("zz"), None);
    }

    #[test]
    fn decodes_flag_bits() {
        let r = Registry::from_text(SAMPLE.to_string());
        let mil = r.get("7cf800").unwrap();
        assert!(mil.military && !mil.interesting);
        assert_eq!(mil.owner_op.as_deref(), Some("EXAMPLE AIR FORCE"));
        let faa = r.get("a00001").unwrap();
        assert!(faa.ladd && !faa.military && !faa.pia);
    }

    #[test]
    fn loads_the_bundled_dump() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../aviation-data/aircraft.csv.gz");
        let r = Registry::load_gz(&path).unwrap();
        assert!(r.len() > 600_000, "{}", r.len());
        assert_eq!(r.get("7c6b2d").unwrap().registration.as_deref(), Some("VH-VGF"));
    }
}
