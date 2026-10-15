# PingView

A live ADS-B traffic map for a uAvionix **pingRX Pro** receiver. Plug the
receiver into a Mac, Windows or Linux computer, open PingView, and nearby
aircraft appear on the map. You don't need drivers, a decoder or any config.

The map and UI come from [adsb-tar1090](https://github.com/soham-verma/adsb-tar1090):
altitude-coloured aircraft, tar1090-style segmented trails, an aircraft
list, a detail card, a reference location with distances, and history.

## Hardware

```
pingRX Pro ──UART 57600──► STM32F401 "Black Pill" (USB CDC bridge) ──USB──► computer
```

| pingRX Pro | Black Pill |
|---|---|
| VCC | 5V |
| TX  | PA10 (USART1 RX) |
| RX  | PA9 (USART1 TX) |
| GND | GND |

**Wireless:** a Seeed XIAO ESP32-C3 on a battery replaces the Black Pill
and streams the same data over Bluetooth LE (your computer keeps its Wi-Fi
for internet) or Wi-Fi. PingView finds it on its own:

```
pingRX Pro ──UART 57600──► XIAO ESP32-C3 ──Bluetooth LE, Wi-Fi (TCP 5760) or USB──► computer
```

Wiring, power and flashing for both bridges: see [`firmware/README.md`](firmware/README.md).

## Install

Download the installer for your OS from
[Releases](https://github.com/soham-verma/pingview/releases):

- **macOS**: `.dmg` (universal: Apple silicon and Intel)
- **Windows**: `.msi` or `-setup.exe`. Windows SmartScreen may warn
  because the build is unsigned. Choose *More info → Run anyway*.
- **Linux**: `.deb` (installs a udev rule so the receiver works without
  `sudo`) or `.AppImage` (see [`docs/linux.md`](docs/linux.md) for the
  one-time permission setup)

## Using it

The map fills the window. Bottom-left, the dock opens **Map Options**: map
style and brightness, then collapsible sections for overlays (network
traffic), airports (heliports, labels, icon size), aircraft (trails, icon
size), aircraft labels, and units (time zone, airport codes, altitude,
speed, coordinates, distance, temperature, altimeter setting), with a reset
button. **More** holds **History** and **Settings**; click the status pill
for **Receiver** details (port, message rates, frame and CRC counters). Bottom-right: full screen, your location (use the device's
location or pick a point on the map) and zoom. A pill top-left shows the
receiver state and aircraft count.

**Network traffic** (Map Options, on by default) adds aircraft your receiver
can't hear, from adsb.lol / adsb.fi, within a radius you choose (25–250 nm)
of your location or the map centre. They're tagged **NET**. adsb.fi's open
data is for personal, non-commercial use only.

Aerodromes (2,700+ in Australia) show by zoom level; click one for its
CTAF, tower and other frequencies.

Click an aircraft (on the map or in the list) for its card: route with
progress and ETA, altitude, speed, vertical rate, photo, registration and
type. **More details** adds identity, the receiver's own position and
signal data, and, when online, live data from the adsb.lol/adsb.fi network
(airspeed, wind, selected altitude, …), labelled with where it came from.

**Works offline first.** Aircraft from the receiver, the aircraft database
(registration, type, owner), airlines, airports and history all work with
no internet. A built-in offline map (Natural Earth coastlines, borders,
highways and towns) is always underneath, so the map is never blank.
Settings → **Local only** guarantees PingView never touches the internet.
Online extras (detailed map tiles, photos, routes, network traffic) are a
bonus: if the connection drops, the status pill shows **Offline**, they
pause, and they come back on their own.

**Maps.** Map Options → Map lets you pick:
- **Offline**: the built-in world map (OpenStreetMap via Protomaps,
  zoom 0–7: roads, towns, water, borders everywhere), free, no key, no
  internet (the default). If a machine can't run it (no WebGL), a small
  Natural Earth map is used instead.
- **MapTiler** styles (Dark, Streets, Topographic, Satellite): add a free key
  from [cloud.maptiler.com](https://cloud.maptiler.com/account/keys/) in
  Settings. Tiles you've viewed are cached on disk (up to 500 MB) and keep
  working offline.
- **Your own map**: an XYZ tile URL (`https://…/{z}/{x}/{y}.png`; a server
  on your local network keeps working in Local-only mode) or a raster
  **.mbtiles** file on disk (PNG/JPEG/WebP; fully offline). Vector MBTiles
  (`pbf`, e.g. OpenMapTiles) can be used by serving them as raster with
  [tileserver-gl](https://github.com/maptiler/tileserver-gl) and adding its
  `http://localhost:8080/styles/<style>/{z}/{x}/{y}.png` URL.

**Offline:** aircraft come from the receiver, and registration/type/owner
come from the bundled aircraft database, so both work with no internet.
Online assist (callsigns the receiver hasn't heard yet, routes, photos,
network data) quietly does nothing while offline, and can be turned off
in Settings.

**History:** positions are recorded to one JSONL file per UTC day (same
format as adsb-tar1090). Load a day onto the map, save it elsewhere, or open
the folder from the History panel.

## Development

The built-in world map (~189 MB) is too big for git. Fetch it once before
building (needs the [`pmtiles` CLI](https://github.com/protomaps/go-pmtiles)):

```sh
scripts/fetch_basemap.sh   # writes basemap-data/world.pmtiles, checksum-verified
```

Set `PINGVIEW_DEBUG_BASEMAP=1` to log the app's map-file and tile requests.

Requirements: Rust (stable), Node 22+, and on Linux the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) plus `libudev-dev` and `libdbus-1-dev` (Bluetooth).

```sh
npm install
npx tauri dev                       # run against a plugged-in receiver
PINGVIEW_REPLAY=src-tauri/tests/fixtures/capture-2026-10-13.bin npx tauri dev
                                    # no hardware: replay a recorded capture
cd src-tauri && cargo test          # backend tests
```

How it works: the Rust backend (`src-tauri/src/`) reads the serial port,
splits MAVLink frames (`mavlink.rs`), decodes `ADSB_VEHICLE` traffic
reports (`adsb.rs`), keeps per-aircraft state (`store.rs`) and pushes a
snapshot to the React frontend once a second as an `aircraft` event. Map
tiles go through a `pingview://` scheme so they can be cached on disk
(`tiles.rs`). The design is in
[`docs/superpowers/specs/2026-10-13-pingview-design.md`](docs/superpowers/specs/2026-10-13-pingview-design.md).

## Releasing

Push a `v*` tag. The Release workflow builds every platform and creates a
draft GitHub Release. macOS builds are signed and notarized when these
repo secrets are set (same Apple account and names as Pingnet):
`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
`APPLE_TEAM_ID`, `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_CONTENT`.

## Credits

- Built-in world map © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors (ODbL), via [Protomaps](https://protomaps.com); fonts Noto Sans (OFL), icons from tangrams (MIT).
- Fallback map from [Natural Earth](https://www.naturalearthdata.com) (public domain).
- Map tiles © [MapTiler](https://www.maptiler.com/copyright/), map data ©
  [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors.
- Aircraft database (`aviation-data/aircraft.csv.gz`) from
  [tar1090-db](https://github.com/wiedehopf/tar1090-db).
- Airline prefixes and telephony from [OpenFlights](https://openflights.org/data)
  (ODbL).
- Aerodromes (`aviation-data/australia_aerodromes.json`) compiled from public
  aerodrome and frequency listings.
- Live network data from [adsb.lol](https://adsb.lol) and
  [adsb.fi](https://adsb.fi); aircraft info and routes from
  [hexdb.io](https://hexdb.io); photos from
  [planespotters.net](https://www.planespotters.net), credited to each
  photographer.

MIT licensed.
