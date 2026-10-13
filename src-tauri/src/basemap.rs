//! Serves the built-in world map (a PMTiles archive, ~189 MB) to the
//! webview in byte ranges: the map library asks for the header, directory
//! and individual tiles with HTTP `Range` requests, so the file is never
//! read whole.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// Largest single range served (the PMTiles reader asks for ≤ a few hundred KB).
const MAX_RANGE: u64 = 16 * 1024 * 1024;

/// Parses `bytes=start-end` / `bytes=start-` / `bytes=-suffix` against a
/// file of `len` bytes. Returns an inclusive (start, end).
pub fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    // Only single ranges; multi-range requests aren't used by the reader.
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (s, "") => (s.parse().ok()?, len - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len && end - start < MAX_RANGE).then_some((start, end))
}

pub fn read_range(path: &Path, start: u64, end: u64) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0; (end - start + 1) as usize];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(parse_range("bytes=0-16383", 100_000), Some((0, 16383)));
        assert_eq!(parse_range("bytes=99990-", 100_000), Some((99990, 99999)));
        assert_eq!(parse_range("bytes=-10", 100_000), Some((99990, 99999)));
        assert_eq!(parse_range("bytes=99990-200000", 100_000), Some((99990, 99999)), "end clamps to the file");
    }

    #[test]
    fn rejects_bad_ranges() {
        assert_eq!(parse_range("bytes=500-100", 1000), None);
        assert_eq!(parse_range("bytes=2000-2100", 1000), None);
        assert_eq!(parse_range("bytes=0-10,20-30", 1000), None);
        assert_eq!(parse_range("items=0-10", 1000), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
        assert_eq!(parse_range("bytes=0-99999999", 200_000_000), None, "over the size cap");
    }

    #[test]
    fn reads_exact_slices() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f.bin");
        std::fs::write(&path, (0u8..=255).collect::<Vec<_>>()).unwrap();
        assert_eq!(read_range(&path, 10, 13).unwrap(), vec![10, 11, 12, 13]);
        assert!(read_range(&path, 250, 300).is_err(), "past the end");
    }
}
