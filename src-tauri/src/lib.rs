mod adsb;
mod basemap;
mod ble;
mod assist;
mod country;
mod device;
mod discover;
mod history;
mod mavlink;
mod mbtiles;
mod net;
mod pipeline;
mod registry;
mod settings;
mod store;
mod tiles;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use tauri::http::{header, Response, StatusCode};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use assist::{Assist, Details};
use device::{LookupSink, PortInfo};
use history::History;
use net::NetHealth;
use pipeline::{Pipeline, SharedRegistry};
use registry::Registry;
use mbtiles::{MbInfo, MbTilesPool};
use settings::{CustomSource, Settings};
use tiles::{StyleRef, TileResult};

const USER_AGENT: &str = concat!(
    "PingView/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/soham-verma/pingview)"
);
/// Concurrent assist lookups, to stay polite to free public APIs.
const MAX_LOOKUPS_IN_FLIGHT: usize = 2;
const TILE_PRUNE_INTERVAL: Duration = Duration::from_secs(600);
const LOCATE_TIMEOUT: Duration = Duration::from_secs(15);
/// How often network traffic is refreshed (well inside adsb.fi's 1 req/s).
const NETWORK_POLL: Duration = Duration::from_secs(5);
const MAX_NETWORK_RADIUS_NM: f64 = 250.0;

struct AppState {
    pipeline: Arc<Pipeline>,
    settings: Arc<Mutex<Settings>>,
    config_dir: PathBuf,
    history_dir: PathBuf,
    tile_root: PathBuf,
    client: reqwest::Client,
    assist: Arc<Assist>,
    health: Arc<NetHealth>,
    mbtiles: MbTilesPool,
    /// The bundled world map (PMTiles), served in byte ranges.
    basemap_path: PathBuf,
    /// Area to show network traffic for; `None` when the overlay is off.
    network_area: Arc<Mutex<Option<NetworkArea>>>,
    /// Last tile outcome sent to the frontend (TILES_*), so the `tiles`
    /// event only fires on change.
    tile_status: AtomicU8,
}

const TILES_OK: u8 = 0;
const TILES_NO_KEY: u8 = 1;
const TILES_BAD_KEY: u8 = 2;

type CmdResult<T> = Result<T, String>;

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(state: State<AppState>, settings: Settings) -> CmdResult<()> {
    for map in &settings.custom_maps {
        map.validate()?;
    }
    settings.save(&state.config_dir).map_err(|e| e.to_string())?;
    // Reopen MBTiles files on next use (a map may have been removed or replaced).
    for map in &state.settings.lock().unwrap().custom_maps {
        if let CustomSource::Mbtiles { path } = &map.source {
            state.mbtiles.close(path);
        }
    }
    {
        let mut p = state.pipeline.lock();
        p.assist_enabled = settings.assist_enabled && !settings.local_only;
        state.health.set_local_only(settings.local_only);
        match (settings.history_enabled, p.history.is_some()) {
            (true, false) => p.history = Some(History::new(state.history_dir.clone())),
            (false, true) => p.history = None,
            _ => {}
        }
    }
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[derive(serde::Serialize)]
struct MapStyle {
    id: &'static str,
    label: &'static str,
}

#[tauri::command]
fn map_styles() -> Vec<MapStyle> {
    tiles::STYLES
        .iter()
        .map(|&(id, label)| MapStyle { id, label })
        .collect()
}

/// Checks an MBTiles file before it's added as a map.
#[tauri::command]
fn mbtiles_info(path: PathBuf) -> CmdResult<MbInfo> {
    let info = mbtiles::info(&path)?;
    if info.is_vector() {
        return Err("This MBTiles file has vector tiles; PingView needs raster (PNG/JPEG/WebP) tiles.".into());
    }
    Ok(info)
}

#[tauri::command]
fn list_ports() -> Vec<PortInfo> {
    device::list_ports()
}

#[tauri::command]
fn history_list(state: State<AppState>) -> Vec<String> {
    History::new(state.history_dir.clone()).list()
}

#[tauri::command]
fn history_read(state: State<AppState>, date: String) -> CmdResult<String> {
    History::new(state.history_dir.clone())
        .read(&date)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn history_save(state: State<AppState>, date: String, path: PathBuf) -> CmdResult<()> {
    let text = history_read(state, date)?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
fn history_open_folder(app: AppHandle, state: State<AppState>) -> CmdResult<()> {
    std::fs::create_dir_all(&state.history_dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(state.history_dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
struct NetworkArea {
    lat: f64,
    lon: f64,
    radius_nm: f64,
}

/// The map tells us where to fetch network traffic (or `None` to stop).
#[tauri::command]
fn set_network_area(state: State<AppState>, area: Option<NetworkArea>) {
    let area = area.filter(|a| a.lat.is_finite() && a.lon.is_finite()).map(|a| NetworkArea {
        radius_nm: a.radius_nm.clamp(5.0, MAX_NETWORK_RADIUS_NM),
        ..a
    });
    if area.is_none() {
        state.pipeline.clear_network();
    }
    *state.network_area.lock().unwrap() = area;
}

/// Refreshes network traffic for the current area every few seconds.
fn spawn_network_poller(
    pipeline: Arc<Pipeline>,
    assist: Arc<Assist>,
    settings: Arc<Mutex<Settings>>,
    area: Arc<Mutex<Option<NetworkArea>>>,
) {
    // `assist` refuses requests itself in local-only mode or while offline.
    tauri::async_runtime::spawn(async move {
        loop {
            let current = *area.lock().unwrap();
            let enabled = {
                let s = settings.lock().unwrap();
                s.assist_enabled && !s.local_only
            };
            match current {
                Some(a) if enabled => {
                    if let Some((list, via)) = assist.area(a.lat, a.lon, a.radius_nm).await {
                        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
                        pipeline.set_network(list, via, now);
                    }
                }
                _ => pipeline.clear_network(),
            }
            tokio::time::sleep(NETWORK_POLL).await;
        }
    });
}

#[derive(serde::Serialize)]
struct LocationFix {
    lat: f64,
    lon: f64,
    accuracy_m: Option<f64>,
}

/// Asks the OS for the device's location (prompts for permission once).
#[tauri::command]
async fn locate_device(app: AppHandle) -> CmdResult<LocationFix> {
    tauri::async_runtime::spawn_blocking(move || {
        let run_on_main = move |f: Box<dyn FnOnce() + Send + 'static>| {
            let _ = app.run_on_main_thread(f);
        };
        pingview_locate::locate(&run_on_main, LOCATE_TIMEOUT)
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|f| LocationFix {
        lat: f.lat,
        lon: f.lon,
        accuracy_m: f.accuracy_m,
    })
}

/// Everything the detail panel shows beyond the Ping's own data.
#[tauri::command]
async fn aircraft_details(
    state: State<'_, AppState>,
    hex: String,
    flight: Option<String>,
) -> CmdResult<Details> {
    {
        let s = state.settings.lock().unwrap();
        if !s.assist_enabled || s.local_only {
            return Ok(Details::default());
        }
    }
    Ok(state.assist.details(&hex, flight.as_deref()).await)
}

/// Fills callsigns and registrations for aircraft the Ping hasn't named yet.
fn lookup_sink(pipeline: Arc<Pipeline>, assist: Arc<Assist>) -> LookupSink {
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_LOOKUPS_IN_FLIGHT));
    Arc::new(move |hexes: Vec<String>| {
        for hex in hexes {
            let (pipeline, assist, permits) = (pipeline.clone(), assist.clone(), permits.clone());
            tauri::async_runtime::spawn(async move {
                let Ok(_permit) = permits.acquire().await else { return };
                let net = assist.live(&hex).await;
                let mut registration = net.as_ref().and_then(|n| n.registration.clone());
                if registration.is_none() {
                    registration = assist.info(&hex).await.and_then(|i| i.registration);
                }
                pipeline.apply_assist(&hex, net.and_then(|n| n.flight), registration);
            });
        }
    })
}

fn spawn_ticker(app: AppHandle, pipeline: Arc<Pipeline>, health: Arc<NetHealth>) {
    thread::spawn(move || loop {
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        let mut snapshot = pipeline.tick(now);
        snapshot["net"] = serde_json::json!(health.status());
        let _ = app.emit("aircraft", snapshot);
        thread::sleep(Duration::from_secs(1));
    });
}

/// Loads the offline aircraft registry without holding up startup.
fn spawn_registry_loader(path: PathBuf, registry: SharedRegistry) {
    thread::spawn(move || match Registry::load_gz(&path) {
        Ok(r) => {
            let _ = registry.set(r);
        }
        Err(e) => eprintln!("aircraft registry unavailable ({}): {e}", path.display()),
    });
}

fn spawn_tile_pruner(tile_root: PathBuf) {
    thread::spawn(move || loop {
        tiles::prune_cache(&tile_root, tiles::CACHE_LIMIT_BYTES, tiles::CACHE_TARGET_BYTES);
        thread::sleep(TILE_PRUNE_INTERVAL);
    });
}

/// Tells the frontend when tiles start or stop working because of the key.
fn report_tile_status(app: &AppHandle, state: &AppState, status: u8) {
    if state.tile_status.swap(status, Ordering::Relaxed) != status {
        let name = match status {
            TILES_NO_KEY => "no_key",
            TILES_BAD_KEY => "bad_key",
            _ => "ok",
        };
        let _ = app.emit("tiles", name);
    }
}

/// Byte-range responses for the bundled world map (see `basemap.rs`).
fn basemap_response(path: &std::path::Path, range: Option<&str>) -> Response<Vec<u8>> {
    let cors = |b: tauri::http::response::Builder| {
        b.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
            .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Range, Content-Length")
            .header(header::ACCEPT_RANGES, "bytes")
    };
    let fail = |status: StatusCode| cors(Response::builder().status(status)).body(Vec::new()).unwrap();
    let Ok(len) = std::fs::metadata(path).map(|m| m.len()) else {
        return fail(StatusCode::NOT_FOUND);
    };
    let Some((start, end)) = range.and_then(|r| basemap::parse_range(r, len)) else {
        return fail(StatusCode::RANGE_NOT_SATISFIABLE);
    };
    match basemap::read_range(path, start, end) {
        Ok(bytes) => cors(Response::builder().status(StatusCode::PARTIAL_CONTENT))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
            .body(bytes)
            .unwrap(),
        Err(_) => fail(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn tile_response(status: StatusCode, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, tiles::content_type(&body))
        .header(header::CACHE_CONTROL, "max-age=86400")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .expect("static response parts are valid")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("pingview", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            if std::env::var_os("PINGVIEW_DEBUG_BASEMAP").is_some() {
                eprintln!("scheme: {} {} range={:?}", request.method(), request.uri(), request.headers().get(header::RANGE));
            }
            // convertFileSrc percent-encodes the slash in "basemap/world.pmtiles".
            if request.uri().path().replace("%2F", "/").replace("%2f", "/") == "/basemap/world.pmtiles" {
                let state = app.state::<AppState>();
                if request.method() == tauri::http::Method::OPTIONS {
                    responder.respond(basemap_response(&state.basemap_path, Some("bytes=0-0")));
                    return;
                }
                let range = request
                    .headers()
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                let path = state.basemap_path.clone();
                if std::env::var_os("PINGVIEW_DEBUG_BASEMAP").is_some() {
                    eprintln!("basemap request: {range:?}");
                }
                // File reads off the UI thread.
                tauri::async_runtime::spawn_blocking(move || {
                    responder.respond(basemap_response(&path, range.as_deref()));
                });
                return;
            }
            let Some((style, tile)) = tiles::parse_tile_path(request.uri().path()) else {
                responder.respond(tile_response(StatusCode::NOT_FOUND, Vec::new()));
                return;
            };
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let (key, custom) = {
                    let s = state.settings.lock().unwrap();
                    let custom = match &style {
                        StyleRef::Custom(id) => s.custom_maps.iter().find(|m| &m.id == id).cloned(),
                        StyleRef::MapTiler(_) => None,
                    };
                    (s.maptiler_key.clone(), custom)
                };
                let result = match (&style, custom) {
                    (StyleRef::MapTiler(st), _) => {
                        tiles::get_tile(&state.client, &state.tile_root, key.as_deref(), st, tile, &state.health).await
                    }
                    (StyleRef::Custom(id), Some(map)) => match &map.source {
                        CustomSource::Url { template } => {
                            tiles::get_custom_url_tile(&state.client, &state.tile_root, id, template, tile, &state.health)
                                .await
                        }
                        CustomSource::Mbtiles { path } => state
                            .mbtiles
                            .tile(path, tile)
                            .map_or(TileResult::Unavailable, TileResult::Tile),
                    },
                    (StyleRef::Custom(_), None) => TileResult::Unavailable,
                };
                // Key problems only concern MapTiler maps.
                let resp = match result {
                    tiles::TileResult::Tile(bytes) => {
                        report_tile_status(&app, &state, TILES_OK);
                        tile_response(StatusCode::OK, bytes)
                    }
                    tiles::TileResult::NoKey => {
                        report_tile_status(&app, &state, TILES_NO_KEY);
                        tile_response(StatusCode::NOT_FOUND, Vec::new())
                    }
                    tiles::TileResult::BadKey => {
                        report_tile_status(&app, &state, TILES_BAD_KEY);
                        tile_response(StatusCode::NOT_FOUND, Vec::new())
                    }
                    tiles::TileResult::Unavailable => tile_response(StatusCode::NOT_FOUND, Vec::new()),
                };
                responder.respond(resp);
            });
        })
        .setup(|app| {
            let paths = app.path();
            let config_dir = paths.app_config_dir()?;
            let history_dir = paths.app_data_dir()?.join("history");
            let tile_root = paths.app_cache_dir()?.join("tiles");
            let settings = Settings::load(&config_dir);

            let history = settings
                .history_enabled
                .then(|| History::new(history_dir.clone()));
            let registry: SharedRegistry = Arc::new(OnceLock::new());
            let dump = app
                .path()
                .resolve("aviation-data/aircraft.csv.gz", BaseDirectory::Resource)?;
            spawn_registry_loader(dump, registry.clone());
            let basemap_path = app
                .path()
                .resolve("basemap/world.pmtiles", BaseDirectory::Resource)?;
            let pipeline = Arc::new(Pipeline::new(
                history,
                settings.assist_enabled && !settings.local_only,
                registry,
            ));
            let health = Arc::new(NetHealth::new(settings.local_only));
            // Fail fast when offline instead of hanging tile/assist requests.
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(12))
                .build()?;
            let assist = Arc::new(Assist::new(client.clone(), health.clone()));
            let lookups = lookup_sink(pipeline.clone(), assist.clone());
            let settings = Arc::new(Mutex::new(settings));

            match std::env::var_os("PINGVIEW_REPLAY") {
                Some(file) => device::spawn_replay(pipeline.clone(), file.into(), lookups),
                None => device::spawn_receiver(pipeline.clone(), settings.clone(), lookups),
            }
            let network_area = Arc::new(Mutex::new(None));
            spawn_network_poller(pipeline.clone(), assist.clone(), settings.clone(), network_area.clone());
            spawn_ticker(app.handle().clone(), pipeline.clone(), health.clone());
            spawn_tile_pruner(tile_root.clone());

            app.manage(AppState {
                pipeline,
                settings,
                config_dir,
                history_dir,
                tile_root,
                client,
                assist,
                health,
                mbtiles: MbTilesPool::default(),
                basemap_path,
                network_area,
                tile_status: AtomicU8::new(TILES_OK),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            map_styles,
            mbtiles_info,
            aircraft_details,
            locate_device,
            set_network_area,
            list_ports,
            history_list,
            history_read,
            history_save,
            history_open_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PingView");
}
