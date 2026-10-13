//! MapTiler raster tiles behind the `pingview://` scheme, cached on disk so
//! areas you've viewed before still render offline.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::net::NetHealth;

pub const CACHE_LIMIT_BYTES: u64 = 500 * 1024 * 1024;
pub const CACHE_TARGET_BYTES: u64 = 400 * 1024 * 1024;
const MAX_ZOOM: u32 = 22;

/// MapTiler map styles offered in Settings, as (id, label).
pub const STYLES: [(&str, &str); 5] = [
    ("dataviz-dark", "Dark"),
    ("streets-v2-dark", "Streets (dark)"),
    ("streets-v2", "Streets"),
    ("topo-v2", "Topographic"),
    ("hybrid", "Satellite"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileId {
    pub z: u32,
    pub x: u32,
    pub y: u32,
    /// High-DPI (`@2x`) tile.
    pub retina: bool,
}

/// Parses `tiles/{style}/{z}/{x}/{y}.png` or `…/{y}@2x.png`. Rejects unknown
/// styles and anything outside the tile grid.
/// Which map a tile request is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleRef {
    /// One of `STYLES` (MapTiler).
    MapTiler(&'static str),
    /// A user-added map, by its id (`custom-<id>` in the URL).
    Custom(String),
}

/// Ids of user-added maps: short, lowercase letters/digits only, so they're
/// safe in URLs and as a cache folder name.
pub fn valid_custom_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

pub fn parse_tile_path(path: &str) -> Option<(StyleRef, TileId)> {
    let rest = path.trim_start_matches('/').strip_prefix("tiles/")?;
    let rest = rest.strip_suffix(".png")?;
    let (rest, retina) = match rest.strip_suffix("@2x") {
        Some(r) => (r, true),
        None => (rest, false),
    };
    let mut parts = rest.split('/');
    let style_id = parts.next()?;
    let style = match style_id.strip_prefix("custom-") {
        Some(id) if valid_custom_id(id) => StyleRef::Custom(id.to_string()),
        Some(_) => return None,
        None => StyleRef::MapTiler(STYLES.iter().find(|(id, _)| *id == style_id)?.0),
    };
    let z: u32 = parts.next()?.parse().ok()?;
    let x: u32 = parts.next()?.parse().ok()?;
    let y: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || z > MAX_ZOOM {
        return None;
    }
    let n = 1u32 << z;
    (x < n && y < n).then_some((style, TileId { z, x, y, retina }))
}

/// MapTiler serves satellite imagery as JPEG and map styles as PNG.
fn extension(style: &str) -> &'static str {
    if style == "hybrid" {
        "jpg"
    } else {
        "png"
    }
}

pub fn tile_url(style: &str, key: &str, t: TileId) -> String {
    let scale = if t.retina { "@2x" } else { "" };
    format!(
        "https://api.maptiler.com/maps/{style}/256/{}/{}/{}{scale}.{}?key={key}",
        t.z,
        t.x,
        t.y,
        extension(style)
    )
}

/// Cache path. The key isn't part of it, so changing keys keeps the cache.
pub fn tile_file(cache_root: &Path, style: &str, t: TileId) -> PathBuf {
    let scale = if t.retina { "@2x" } else { "" };
    cache_root
        .join(style)
        .join(t.z.to_string())
        .join(t.x.to_string())
        .join(format!("{}{scale}.{}", t.y, extension(style)))
}

pub fn content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/png"
    }
}

/// Fills a user's XYZ template: `{z}`, `{x}`, `{y}`, `{-y}` (TMS row),
/// `{s}` (a/b/c subdomain) and `{r}` (dropped).
pub fn fill_template(template: &str, t: TileId) -> String {
    let tms_y = (1u32 << t.z) - 1 - t.y;
    template
        .replace("{s}", ["a", "b", "c"][((t.x + t.y) % 3) as usize])
        .replace("{z}", &t.z.to_string())
        .replace("{x}", &t.x.to_string())
        .replace("{-y}", &tms_y.to_string())
        .replace("{y}", &t.y.to_string())
        .replace("{r}", "")
}

/// A usable XYZ template: http(s) with {z}, {x} and {y} (or {-y}).
pub fn valid_template(template: &str) -> bool {
    let t = template.trim();
    (t.starts_with("https://") || t.starts_with("http://"))
        && t.contains("{z}")
        && t.contains("{x}")
        && (t.contains("{y}") || t.contains("{-y}"))
        && reqwest::Url::parse(&fill_template(t, TileId { z: 0, x: 0, y: 0, retina: false })).is_ok()
}

/// True for servers on this machine or the local network (a tile server you
/// run yourself), which local-only mode still allows.
pub fn is_local_url(url: &str) -> bool {
    use std::net::IpAddr;
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Domain(d)) => d == "localhost" || d.ends_with(".local") || d.ends_with(".lan"),
        Some(url::Host::Ipv4(ip)) => {
            let ip = IpAddr::V4(ip);
            ip.is_loopback() || matches!(ip, IpAddr::V4(v4) if v4.is_private() || v4.is_link_local())
        }
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00,
        None => false,
    }
}

/// A tile from a user's own XYZ server, cached like MapTiler tiles. Servers
/// on the local network are allowed even in local-only mode.
pub async fn get_custom_url_tile(
    client: &reqwest::Client,
    cache_root: &Path,
    id: &str,
    template: &str,
    t: TileId,
    health: &NetHealth,
) -> TileResult {
    let file = cache_root
        .join(format!("custom-{id}"))
        .join(t.z.to_string())
        .join(t.x.to_string())
        .join(format!("{}.tile", t.y));
    if let Ok(bytes) = fs::read(&file) {
        return TileResult::Tile(bytes);
    }
    let url = fill_template(template, t);
    let local = is_local_url(&url);
    if !local && !health.allowed() {
        return TileResult::Unavailable;
    }
    let Ok(resp) = client.get(&url).send().await else {
        if !local {
            health.fail();
        }
        return TileResult::Unavailable;
    };
    if !local {
        health.ok();
    }
    if !resp.status().is_success() {
        return TileResult::Unavailable;
    }
    let Ok(bytes) = resp.bytes().await else {
        return TileResult::Unavailable;
    };
    if let Some(parent) = file.parent() {
        if fs::create_dir_all(parent).is_ok() {
            let _ = fs::write(&file, &bytes);
        }
    }
    TileResult::Tile(bytes.to_vec())
}

#[derive(Debug, PartialEq, Eq)]
pub enum TileResult {
    Tile(Vec<u8>),
    /// No key configured, so nothing was requested.
    NoKey,
    /// MapTiler refused the key (HTTP 401/403).
    BadKey,
    /// Not cached, and offline / local-only / any other failure.
    Unavailable,
}

/// Serves a tile from cache, or fetches and caches it.
pub async fn get_tile(
    client: &reqwest::Client,
    cache_root: &Path,
    key: Option<&str>,
    style: &str,
    t: TileId,
    health: &NetHealth,
) -> TileResult {
    let file = tile_file(cache_root, style, t);
    if let Ok(bytes) = fs::read(&file) {
        return TileResult::Tile(bytes);
    }
    if !health.allowed() {
        return TileResult::Unavailable;
    }
    let Some(key) = key.filter(|k| !k.is_empty()) else {
        return TileResult::NoKey;
    };
    let Ok(resp) = client.get(tile_url(style, key, t)).send().await else {
        health.fail();
        return TileResult::Unavailable;
    };
    health.ok();
    match resp.status().as_u16() {
        200 => {}
        401 | 403 => return TileResult::BadKey,
        _ => return TileResult::Unavailable,
    }
    let Ok(bytes) = resp.bytes().await else {
        return TileResult::Unavailable;
    };
    if let Some(parent) = file.parent() {
        if fs::create_dir_all(parent).is_ok() {
            let _ = fs::write(&file, &bytes);
        }
    }
    TileResult::Tile(bytes.to_vec())
}

/// Deletes the oldest tiles once the cache is over `limit`, down to `target`.
pub fn prune_cache(cache_root: &Path, limit: u64, target: u64) -> u64 {
    let mut files = Vec::new();
    collect_files(cache_root, &mut files);
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= limit {
        return total;
    }
    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in files {
        if total <= target {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
    total
}

fn collect_files(dir: &Path, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect_files(&path, out);
        } else {
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((path, meta.len(), modified));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(z: u32, x: u32, y: u32, retina: bool) -> TileId {
        TileId { z, x, y, retina }
    }

    #[test]
    fn parses_valid_tile_paths_only() {
        assert_eq!(
            parse_tile_path("/tiles/dataviz-dark/3/4/5.png"),
            Some((StyleRef::MapTiler("dataviz-dark"), tile(3, 4, 5, false)))
        );
        assert_eq!(
            parse_tile_path("tiles/hybrid/0/0/0@2x.png"),
            Some((StyleRef::MapTiler("hybrid"), tile(0, 0, 0, true)))
        );
        assert_eq!(
            parse_tile_path("/tiles/custom-myserver1/3/4/5.png"),
            Some((StyleRef::Custom("myserver1".into()), tile(3, 4, 5, false)))
        );
        assert_eq!(parse_tile_path("/tiles/custom-../3/4/5.png"), None);
        assert_eq!(parse_tile_path("/tiles/custom-UPPER/3/4/5.png"), None);
        assert_eq!(parse_tile_path("/tiles/dataviz-dark/3/8/0.png"), None, "x outside grid");
        assert_eq!(parse_tile_path("/tiles/dataviz-dark/23/0/0.png"), None, "zoom too deep");
        assert_eq!(parse_tile_path("/tiles/not-a-style/3/4/5.png"), None);
        assert_eq!(parse_tile_path("/tiles/../3/4/5.png"), None);
        assert_eq!(parse_tile_path("/tiles/dataviz-dark/3/4/5.png/extra"), None);
    }

    #[test]
    fn builds_maptiler_urls() {
        assert_eq!(
            tile_url("dataviz-dark", "KEY", tile(10, 5, 6, false)),
            "https://api.maptiler.com/maps/dataviz-dark/256/10/5/6.png?key=KEY"
        );
        assert_eq!(
            tile_url("hybrid", "KEY", tile(10, 5, 6, true)),
            "https://api.maptiler.com/maps/hybrid/256/10/5/6@2x.jpg?key=KEY"
        );
    }

    #[test]
    fn cache_path_separates_styles_and_scales_but_not_keys() {
        let root = Path::new("/cache");
        assert_eq!(
            tile_file(root, "streets-v2", tile(1, 0, 1, true)),
            Path::new("/cache/streets-v2/1/0/1@2x.png")
        );
        assert_ne!(
            tile_file(root, "streets-v2", tile(1, 0, 1, false)),
            tile_file(root, "topo-v2", tile(1, 0, 1, false))
        );
    }

    #[tokio::test]
    async fn no_key_means_no_request() {
        let tmp = tempfile::tempdir().unwrap();
        let client = reqwest::Client::new();
        let online = NetHealth::new(false);
        let t = tile(1, 0, 0, false);
        assert_eq!(get_tile(&client, tmp.path(), None, "streets-v2", t, &online).await, TileResult::NoKey);
        assert_eq!(get_tile(&client, tmp.path(), Some(""), "streets-v2", t, &online).await, TileResult::NoKey);
    }

    #[tokio::test]
    async fn local_only_serves_cache_and_never_fetches() {
        let tmp = tempfile::tempdir().unwrap();
        let local = NetHealth::new(true);
        let cached = tile(1, 0, 0, false);
        let file = tile_file(tmp.path(), "streets-v2", cached);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"png").unwrap();
        let client = reqwest::Client::new();
        assert_eq!(
            get_tile(&client, tmp.path(), Some("key"), "streets-v2", cached, &local).await,
            TileResult::Tile(b"png".to_vec())
        );
        let missing = tile(1, 1, 1, false);
        assert_eq!(
            get_tile(&client, tmp.path(), Some("key"), "streets-v2", missing, &local).await,
            TileResult::Unavailable
        );
    }

    #[tokio::test]
    async fn cached_tile_is_served_without_a_key() {
        let tmp = tempfile::tempdir().unwrap();
        let t = tile(1, 0, 0, false);
        let file = tile_file(tmp.path(), "streets-v2", t);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"png").unwrap();
        let got = get_tile(&reqwest::Client::new(), tmp.path(), None, "streets-v2", t, &NetHealth::new(false)).await;
        assert_eq!(got, TileResult::Tile(b"png".to_vec()));
    }

    #[test]
    fn sniffs_jpeg_png_and_webp() {
        assert_eq!(content_type(&[0xFF, 0xD8, 0xFF]), "image/jpeg");
        assert_eq!(content_type(b"\x89PNG"), "image/png");
        assert_eq!(content_type(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
    }

    #[test]
    fn fills_custom_templates() {
        let t = tile(3, 4, 1, false);
        assert_eq!(fill_template("https://{s}.t.io/{z}/{x}/{y}.png", t), "https://c.t.io/3/4/1.png");
        assert_eq!(fill_template("http://host/{z}/{x}/{-y}.jpg", t), "http://host/3/4/6.jpg");
    }

    #[test]
    fn validates_templates() {
        assert!(valid_template("https://tiles.example.com/{z}/{x}/{y}.png"));
        assert!(valid_template("http://192.168.1.20:8080/styles/basic/{z}/{x}/{y}.png"));
        assert!(!valid_template("https://tiles.example.com/{z}/{x}.png"), "no y");
        assert!(!valid_template("file:///etc/{z}/{x}/{y}"));
        assert!(!valid_template("tiles/{z}/{x}/{y}.png"));
    }

    #[test]
    fn recognises_local_network_servers() {
        assert!(is_local_url("http://localhost:8080/1/2/3.png"));
        assert!(is_local_url("http://127.0.0.1/1/2/3.png"));
        assert!(is_local_url("http://192.168.1.20/1/2/3.png"));
        assert!(is_local_url("http://10.128.0.4/1/2/3.png"));
        assert!(is_local_url("http://tileserver.local/1/2/3.png"));
        assert!(!is_local_url("https://tiles.example.com/1/2/3.png"));
        assert!(!is_local_url("https://8.8.8.8/1/2/3.png"));
    }

    #[test]
    fn custom_ids_are_safe() {
        assert!(valid_custom_id("abc123"));
        assert!(!valid_custom_id(""));
        assert!(!valid_custom_id("a/b"));
        assert!(!valid_custom_id("Abc"));
        assert!(!valid_custom_id(&"a".repeat(33)));
    }

    #[test]
    fn prune_removes_oldest_tiles_first() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p/1/0");
        fs::create_dir_all(&dir).unwrap();
        for i in 0..5 {
            fs::write(dir.join(format!("{i}.png")), [0u8; 100]).unwrap();
            let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i);
            fs::File::options()
                .write(true)
                .open(dir.join(format!("{i}.png")))
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        assert_eq!(prune_cache(tmp.path(), 1000, 300), 500, "under limit: untouched");
        assert_eq!(prune_cache(tmp.path(), 400, 300), 300);
        assert!(!dir.join("0.png").exists() && !dir.join("1.png").exists());
        assert!(dir.join("4.png").exists());
    }
}
