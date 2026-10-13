//! User settings, stored as JSON in the app config directory.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tiles::{valid_custom_id, valid_template};

/// The map that ships with the app (Natural Earth): no key, no internet.
pub const BUILTIN_STYLE: &str = "builtin";

/// A map the user added: their own XYZ tile server, or an MBTiles file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomMap {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub source: CustomSource,
    pub attribution: Option<String>,
    pub max_zoom: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CustomSource {
    Url { template: String },
    Mbtiles { path: PathBuf },
}

impl CustomMap {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_custom_id(&self.id) {
            return Err(format!("Invalid map id '{}'.", self.id));
        }
        if self.name.trim().is_empty() {
            return Err("Give the map a name.".into());
        }
        match &self.source {
            CustomSource::Url { template } if !valid_template(template) => Err(
                "The tile URL needs http(s) and {z}, {x} and {y}, e.g. https://example.com/{z}/{x}/{y}.png".into(),
            ),
            CustomSource::Mbtiles { path } if !path.is_file() => {
                Err(format!("Map file not found: {}", path.display()))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Serial port to use instead of auto-detection.
    pub port: Option<String>,
    /// Never touch the internet: receiver, offline map and built-in data only.
    pub local_only: bool,
    /// The user's own MapTiler API key. No key, no map tiles.
    pub maptiler_key: Option<String>,
    /// `builtin`, a MapTiler style id, or `custom-<id>`.
    pub map_style: String,
    pub custom_maps: Vec<CustomMap>,
    pub history_enabled: bool,
    /// Online assist: adsb.lol, adsb.fi, hexdb.io, planespotters.net.
    #[serde(alias = "enrich_enabled")]
    pub assist_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: None,
            local_only: false,
            maptiler_key: None,
            map_style: BUILTIN_STYLE.to_string(),
            custom_maps: Vec::new(),
            history_enabled: true,
            assist_enabled: true,
        }
    }
}

impl Settings {
    pub fn file(config_dir: &Path) -> PathBuf {
        config_dir.join("settings.json")
    }

    /// Missing or unreadable settings fall back to defaults.
    pub fn load(config_dir: &Path) -> Self {
        fs::read_to_string(Self::file(config_dir))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(config_dir)?;
        let text = serde_json::to_string_pretty(self).expect("settings serialize");
        fs::write(Self::file(config_dir), text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_custom_maps() {
        let mut m = CustomMap {
            id: "abc".into(),
            name: "Mine".into(),
            source: CustomSource::Url {
                template: "https://t.example.com/{z}/{x}/{y}.png".into(),
            },
            attribution: None,
            max_zoom: None,
        };
        assert!(m.validate().is_ok());
        m.source = CustomSource::Url { template: "https://t.example.com/tiles.png".into() };
        assert!(m.validate().is_err());
        m.source = CustomSource::Mbtiles { path: "/definitely/missing.mbtiles".into() };
        assert!(m.validate().is_err());
        m.source = CustomSource::Url { template: "https://t.example.com/{z}/{x}/{y}.png".into() };
        m.id = "../x".into();
        assert!(m.validate().is_err());
    }

    #[test]
    fn custom_map_json_shape() {
        let m: CustomMap = serde_json::from_str(
            r#"{"id":"f1","name":"Region","kind":"mbtiles","path":"/maps/region.mbtiles","attribution":null,"max_zoom":14}"#,
        )
        .unwrap();
        assert_eq!(m.source, CustomSource::Mbtiles { path: "/maps/region.mbtiles".into() });
    }

    #[test]
    fn round_trips_and_defaults_missing_fields() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(tmp.path()), Settings::default());

        fs::write(Settings::file(tmp.path()), r#"{"port":"COM3"}"#).unwrap();
        let s = Settings::load(tmp.path());
        assert_eq!(s.port.as_deref(), Some("COM3"));
        assert_eq!(s.map_style, BUILTIN_STYLE);
        assert!(s.history_enabled);

        let mut changed = s.clone();
        changed.maptiler_key = Some("abc".into());
        changed.custom_maps.push(CustomMap {
            id: "home1".into(),
            name: "Home server".into(),
            source: CustomSource::Url {
                template: "http://192.168.1.20/{z}/{x}/{y}.png".into(),
            },
            attribution: None,
            max_zoom: Some(16),
        });
        changed.save(tmp.path()).unwrap();
        assert_eq!(Settings::load(tmp.path()), changed);
    }
}
