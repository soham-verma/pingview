//! Reads raster tiles from an MBTiles file (SQLite), for maps users bring
//! themselves. Works with no network at all.
//!
//! MBTiles stores rows in TMS order (row 0 at the bottom), so the y from the
//! web map is flipped on lookup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

use crate::tiles::TileId;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MbInfo {
    pub name: Option<String>,
    /// "png", "jpg", "webp" … or "pbf" for vector tiles (not supported).
    pub format: Option<String>,
    pub minzoom: Option<u32>,
    pub maxzoom: Option<u32>,
    /// [west, south, east, north]
    pub bounds: Option<[f64; 4]>,
    pub attribution: Option<String>,
}

impl MbInfo {
    pub fn is_vector(&self) -> bool {
        matches!(self.format.as_deref(), Some("pbf" | "mvt"))
    }
}

fn open(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
}

/// Reads the metadata table (name/format/zoom range/bounds/attribution).
pub fn info(path: &Path) -> Result<MbInfo, String> {
    let conn = open(path).map_err(|e| format!("Couldn't open the file: {e}"))?;
    let mut stmt = conn
        .prepare("SELECT name, value FROM metadata")
        .map_err(|_| "Not an MBTiles file (no metadata table).".to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    let meta: HashMap<String, String> = rows.filter_map(Result::ok).collect();
    let bounds = meta.get("bounds").and_then(|b| {
        let v: Vec<f64> = b.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
    });
    let mut info = MbInfo {
        name: meta.get("name").cloned(),
        format: meta.get("format").map(|f| f.to_lowercase()),
        minzoom: meta.get("minzoom").and_then(|z| z.parse().ok()),
        maxzoom: meta.get("maxzoom").and_then(|z| z.parse().ok()),
        bounds,
        attribution: meta.get("attribution").cloned(),
    };
    // Fall back to the tiles table when metadata doesn't say.
    if info.maxzoom.is_none() || info.minzoom.is_none() {
        if let Ok((min, max)) = conn.query_row("SELECT MIN(zoom_level), MAX(zoom_level) FROM tiles", [], |r| {
            Ok((r.get::<_, Option<u32>>(0)?, r.get::<_, Option<u32>>(1)?))
        }) {
            info.minzoom = info.minzoom.or(min);
            info.maxzoom = info.maxzoom.or(max);
        }
    }
    Ok(info)
}

/// Open MBTiles connections, kept per file.
#[derive(Default)]
pub struct MbTilesPool {
    conns: Mutex<HashMap<PathBuf, Connection>>,
}

impl MbTilesPool {
    pub fn tile(&self, path: &Path, t: TileId) -> Option<Vec<u8>> {
        let mut conns = self.conns.lock().unwrap_or_else(|e| e.into_inner());
        if !conns.contains_key(path) {
            conns.insert(path.to_path_buf(), open(path).ok()?);
        }
        let conn = conns.get(path)?;
        let tms_row = (1u32 << t.z) - 1 - t.y;
        conn.query_row(
            "SELECT tile_data FROM tiles WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3",
            (t.z, t.x, tms_row),
            |r| r.get::<_, Vec<u8>>(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Forget a file (it was removed from the map list or replaced).
    pub fn close(&self, path: &Path) {
        self.conns.lock().unwrap_or_else(|e| e.into_inner()).remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(z: u32, x: u32, y: u32) -> TileId {
        TileId { z, x, y, retina: false }
    }

    /// Writes a tiny MBTiles file with one tile at z=2, x=1, y=0 (web/XYZ).
    fn sample(dir: &Path, with_zoom_meta: bool) -> PathBuf {
        let path = dir.join("sample.mbtiles");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);",
        )
        .unwrap();
        let mut meta = vec![
            ("name", "Test region"),
            ("format", "png"),
            ("bounds", "150.0,-35.0,152.0,-33.0"),
            ("attribution", "© Test"),
        ];
        if with_zoom_meta {
            meta.extend([("minzoom", "0"), ("maxzoom", "12")]);
        }
        for (k, v) in meta {
            conn.execute("INSERT INTO metadata VALUES (?1, ?2)", (k, v)).unwrap();
        }
        // XYZ y=0 at z=2 is TMS row 3.
        conn.execute("INSERT INTO tiles VALUES (2, 1, 3, ?1)", [b"tile-bytes".to_vec()]).unwrap();
        path
    }

    #[test]
    fn reads_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let info = info(&sample(tmp.path(), true)).unwrap();
        assert_eq!(info.name.as_deref(), Some("Test region"));
        assert_eq!((info.minzoom, info.maxzoom), (Some(0), Some(12)));
        assert_eq!(info.bounds, Some([150.0, -35.0, 152.0, -33.0]));
        assert!(!info.is_vector());
    }

    #[test]
    fn falls_back_to_tile_table_for_zoom_range() {
        let tmp = tempfile::tempdir().unwrap();
        let info = info(&sample(tmp.path(), false)).unwrap();
        assert_eq!((info.minzoom, info.maxzoom), (Some(2), Some(2)));
    }

    #[test]
    fn flips_y_to_tms_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let path = sample(tmp.path(), true);
        let pool = MbTilesPool::default();
        assert_eq!(pool.tile(&path, tile(2, 1, 0)), Some(b"tile-bytes".to_vec()));
        assert_eq!(pool.tile(&path, tile(2, 1, 3)), None, "TMS row 3 is XYZ y=0, not y=3");
    }

    /// Checks a real file: `PINGVIEW_TEST_MBTILES=/path/to/file.mbtiles
    /// cargo test -- --ignored real_mbtiles --nocapture`.
    #[test]
    #[ignore = "needs a real .mbtiles file"]
    fn real_mbtiles_file() {
        let path = std::env::var("PINGVIEW_TEST_MBTILES").expect("set PINGVIEW_TEST_MBTILES");
        let info = info(Path::new(&path)).unwrap();
        eprintln!("{info:?} vector={}", info.is_vector());
    }

    #[test]
    fn rejects_files_that_are_not_mbtiles() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("not.mbtiles");
        std::fs::write(&path, b"hello").unwrap();
        assert!(info(&path).is_err());
        assert!(info(&tmp.path().join("missing.mbtiles")).is_err());
    }
}
