# PingView — design

Date: 2026-10-13
Status: approved; amended the same day (see Amendments)

## Purpose

A portable ADS-B receiver you can plug into any computer and get a live
traffic map. Hardware is a uAvionix pingRX Pro wired to an STM32F401
"Black Pill" running a USB CDC ↔ UART bridge; PingView is the cross-platform
desktop app that reads it.

Success looks like: someone downloads PingView for macOS, Windows or Linux,
plugs the receiver in, opens the app, and sees nearby aircraft on the map
within a few seconds — no drivers, no decoder, no config.

### Decisions already made

| Topic | Decision |
|---|---|
| Stack | Rust + Tauri 2 backend, React + Leaflet frontend |
| Frontend | Whole adsb-tar1090 frontend copied in (map, segmented trails, list, detail card, reference location, history, registration lookup) |
| Data source | The USB Ping receiver only — no SBS/TCP input |
| Platforms | macOS, Windows, Linux desktops. Phones/tablets out of scope |
| Distribution | Installers on GitHub Releases, built by CI |
| Signing | macOS signed + notarized; Windows and Linux unsigned |
| Offline | Online tiles with an on-disk tile cache; aircraft always work offline |
| Repo | New repo `~/Documents/Dev/pingview`; adsb-tar1090 left untouched |
| Frontend ↔ backend | Tauri events (push) + commands; no HTTP server, no open port |

### Provenance

The receiver decoder is written from the public MAVLink `ADSB_VEHICLE`
(message 246) definition. It is not ported from any employer codebase. The
frontend is copied from the author's own adsb-tar1090 (MIT).

## Hardware and firmware

- pingRX Pro → Black Pill: Ping TX → PA10, Ping RX ← PA9, 5V, GND.
- Firmware: the existing STM32duino sketch (USART1 57600 8N1 ↔ USB CDC,
  PC13 LED toggles on traffic), copied into `firmware/ping_usb_bridge/`
  with a README that documents wiring, the arduino-cli build line, and DFU
  flashing with `dfu-util`. The firmware is not changed.
- On USB it appears as VID:PID `0483:5740`. That is ST's generic Virtual
  COM Port ID, which other STM32 boards also use, so the app detects the
  receiver by both the ID and the traffic it sends (see Device).
- CDC-ACM needs no driver on macOS, Windows 10+ or Linux.

## Architecture

```
USB serial ─► device ─► mavlink (framer + CRC) ─► adsb (msg 246 → TrafficReport)
                                                    │
                    ┌───────────────────────────────┘
                    ▼
              store (per-ICAO state, staleness, prune)
                ├─ ticker: every 1 s emit "aircraft" snapshot ─► frontend
                ├─ history (daily JSONL)
                └─ enrich (registration lookup, cached)

tiles: pingview:// URI scheme ─► disk cache ─► online tile provider
```

Rust crate layout (`src-tauri/src/`), one responsibility per module:

| Module | Responsibility | Depends on |
|---|---|---|
| `device.rs` | Find, open and supervise the serial port; reconnect loop; publish receiver status | `serialport`, `mavlink` |
| `mavlink.rs` | Turn a byte stream into checksum-verified frames (v1 and v2) | nothing |
| `adsb.rs` | Decode message 246 into `TrafficReport`, with unit conversion and validity flags | `mavlink` types |
| `store.rs` | Merge reports per aircraft, compute `seen`, prune, build the snapshot | `adsb` types |
| `history.rs` | Append positions to daily JSONL; list and read days | `store` types |
| `enrich.rs` | Look up registrations for aircraft without a callsign | `reqwest` |
| `tiles.rs` | Handle the `pingview://` scheme: serve tiles from cache or fetch, store and serve | `reqwest` |
| `settings.rs` | Load and save user settings in the app config dir | `serde_json` |
| `lib.rs` | Tauri builder: wires modules together, registers commands, starts tasks | all |

`mavlink`, `adsb`, `store` and `history` are pure (no I/O or I/O behind a
trait) so they can be unit-tested without hardware.

## Data flow

### Device

1. List serial ports. Candidates are USB ports with VID:PID `0483:5740`,
   plus any port the user picked manually (stored in settings).
2. Open the candidate at 57600 baud (CDC ignores baud, but keep it explicit).
3. **Confirm it's a Ping:** within 5 s the port must produce either one
   checksum-verified message 246, or three consecutive well-formed frames
   (each frame's declared length lands exactly on the next start byte,
   with system ID 1 and component ID `0x9C`, which is what the Ping sends,
   seen in the 2026-10-13 capture). Otherwise close the port and mark it
   "not a receiver". The second rule is needed because with no aircraft
   in range the Ping only sends its own status messages, and those can't
   be checksum-verified.
4. Read in a dedicated thread; push bytes into the framer.
5. On read error or unplug: status → `disconnected`, retry discovery every
   2 s. Re-plugging needs no user action.

Receiver status, emitted as a `receiver` event on every change and
included in each snapshot:

| Status | Meaning |
|---|---|
| `searching` | No candidate port found |
| `connecting` | Port open, waiting for the first valid frame |
| `connected` | Valid frames arriving |
| `no_traffic` | Connected, but no message 246 in the last 60 s |
| `error` | Port present but can't be opened, with the reason (e.g. Linux permission denied) |

### MAVLink framing

- Scan for `0xFE` (v1) or `0xFD` (v2). Read the header, payload and
  checksum. Verify the X.25 checksum using the message's CRC extra byte.
- Only message IDs with a known CRC extra can be verified. The only one
  PingView decodes is 246 (`ADSB_VEHICLE`, CRC extra 184, payload 38
  bytes). Frames with other IDs (the Ping's status messages, e.g. 66, 202,
  203) are counted as `other_frames` and skipped by their declared length.
- On a bad checksum, skip one byte and rescan.
- v2: handle truncated payloads (zero-pad to 38 bytes), ignore signed frames' signature.

### Message 246 decode

Payload, little-endian, in MAVLink wire order:

| Field | Type | Unit | Output |
|---|---|---|---|
| ICAO_address | u32 | — | `hex` (6 lowercase hex digits) |
| lat, lon | i32 | degE7 | degrees |
| altitude | i32 | mm | feet |
| heading | u16 | cdeg | `track` degrees |
| hor_velocity | u16 | cm/s | `gs` knots |
| ver_velocity | i16 | cm/s | `baro_rate` ft/min |
| flags | u16 | bitmask | gates each field |
| squawk | u16 | — | `squawk` 4-digit string |
| altitude_type | u8 | 0 = baro, 1 = geometric | selects `alt_baro` or `alt_geom` |
| callsign | char[9] | — | `flight` (trimmed) |
| emitter_type | u8 | MAV `ADSB_EMITTER_TYPE` | `emitter_type` (passed through, unused in v1 UI) |
| tslc | u8 | s | backdates `last_seen` |

A field is only set when its validity flag is set: coords 0x01,
altitude 0x02, heading 0x04, velocity 0x08, callsign 0x10, squawk 0x20,
vertical velocity 0x80. Reports flagged simulated (0x40) are dropped.

**Squawk encoding** isn't clear from the spec (it could be raw octal or
decimal-coded digits). The implementation settles it against the captured
fixture: a squawk must decode to four digits 0–7, cross-checked with a live
aircraft on a public tracker.

### Store and snapshot

- Keyed by `hex`. Each report merges into the existing entry; fields not
  present in the report keep their previous value.
- `last_seen = now - tslc`. `last_seen_pos` updates only when coords are valid.
- Pruned 60 s after `last_seen` (same as adsb-tar1090).
- Every 1 s the ticker emits `aircraft` with the exact shape
  `bridge.js` serves today, so the copied components stay unchanged:

```json
{ "now": 1760000000.0, "messages": 1234,
  "receiver": { "status": "connected", "port": "/dev/cu.usbmodem…", "msg_rate": 5.1 },
  "aircraft": [ { "hex": "7c6b2d", "flight": "RXA6681", "registration": null,
                  "alt_baro": 12000, "alt_geom": null, "gs": 250.0, "track": 270.0,
                  "baro_rate": -500, "squawk": "1200", "lat": -33.9, "lon": 151.2,
                  "emitter_type": 1, "messages": 42, "seen": 0.4, "seen_pos": 0.4 } ] }
```

- Frontend colour and altitude display use `alt_baro ?? alt_geom` (small
  change in `planeIcon.js`, `segments.js`, `AircraftDetailCard.jsx`, the
  list), because the Ping may report only geometric altitude.
- No on-ground flag exists in message 246, so `alt_baro: "ground"` is never sent.

### Frontend changes (from adsb-tar1090)

Kept as-is: map, trails (`segments.js`), list, detail card, legend,
reference location, styling (`index.css`).

Changed:
- `useAircraft.js`: `listen("aircraft")` replaces the fetch poll; exposes `receiver`.
- `HistoryPanel.jsx`: `invoke("history_list")` / `invoke("history_read", {date})`
  replace the `/data/history` fetches; "Download" uses the Tauri save dialog,
  because `<a download>` is unreliable in webviews.
- `AircraftMap.jsx`: tile URL becomes `pingview://tiles/{z}/{x}/{y}.png`.
- Header: receiver status pill (searching / connecting / connected /
  no traffic / error) and message rate. The `error` state shows the reason
  and, on Linux permission errors, the udev fix.
- "Use my location": webview geolocation varies by OS. If it fails, show
  "Location unavailable — set it on the map instead". No native location
  plugin in v1.
- Settings popover: serial port override, tile provider, history on/off,
  registration lookup on/off.

### Tiles

- The webview requests `pingview://tiles/{z}/{x}/{y}.png`. Rust serves
  `<cache_dir>/tiles/{z}/{x}/{y}.png` if present. Otherwise it fetches from
  the provider (with a `PingView/<version>` User-Agent), stores the tile,
  and serves it. Offline with no cached tile, it returns 404 and Leaflet
  shows the dark background.
- Default provider: CARTO Voyager raster
  (`https://{a-d}.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}.png`)
  with attribution "© OpenStreetMap contributors © CARTO". It's closest
  to the OSM look the existing CSS dimming (`brightness(0.55)
  saturate(0.9)`) was tuned for. OSM's own tile servers aren't used,
  because their usage policy discourages use as the default for
  distributed apps.
- Cache limit 500 MB. When it's exceeded, delete the oldest tiles by
  modification time until under 400 MB, checked at startup and every 10 minutes.
- The tile URL template can be changed in settings, for users with a
  keyed provider.

### History

- Same row format and daily file naming as adsb-tar1090
  (`adsb-YYYY-MM-DD.jsonl`, UTC), written to `<app_data_dir>/history/`.
  Deduplicated against each aircraft's last recorded position. On by default.
- Commands: `history_list() -> [date]`, `history_read(date) -> string`,
  `history_save(date)` (opens the save dialog), `history_open_folder()`.

### Registration lookup

- For aircraft without a callsign: try hexdb.io, then adsb.lol. One
  attempt per hex per session. Failures are cached for 1 h, so being
  offline doesn't cause repeated lookups. At most 2 lookups in flight.
- Can be turned off in settings. Off means no network calls besides tiles.

## Error handling

| Situation | Behaviour |
|---|---|
| No receiver plugged in | Status `searching`; map works (cached tiles); header explains "Plug in the PingView receiver" |
| Linux: permission denied on the port | Status `error`; header shows the udev rule / `dialout` fix |
| Port belongs to another STM32 device | Fails the 5 s check, marked "not a receiver", discovery continues |
| Unplugged mid-session | Status `searching`; aircraft age out normally; reconnects automatically |
| Corrupt or partial bytes | Framer rescans; `crc_errors` counter shown in a debug tooltip |
| Offline | Aircraft unaffected; uncached tiles blank; lookups fail quietly and are cached as failed |
| History dir unwritable | History turns itself off for the session and logs once; live map unaffected |

## Packaging and release

- GitHub Actions with `tauri-apps/tauri-action` on tag `v*`, building:
  - macOS universal (`aarch64` + `x86_64`) `.dmg`, signed and notarized
  - Windows `.msi` and NSIS `.exe` (unsigned; SmartScreen will warn)
  - Linux `.AppImage` and `.deb` (Ubuntu 22.04 runner)
- macOS signing uses the existing Apple Developer account (the one
  Pingnet ships under) and copies Pingnet's `release.yml` setup: a
  Developer ID Application certificate for signing, and an App Store
  Connect API key for notarization. Repo secrets, same names as Pingnet:
  `APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID`, `APPLE_API_ISSUER`,
  `APPLE_API_KEY`, `APPLE_API_KEY_CONTENT` (base64 .p8, written to
  `$RUNNER_TEMP/private_keys/` and passed as `APPLE_API_KEY_PATH`).
  GitHub secrets are per-repo, so the owner has to add the same values to
  the pingview repo. Until they're added, CI still builds an unsigned `.dmg`.
- The `.deb` installs `/lib/udev/rules.d/60-pingview.rules`
  (`SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="5740",
  TAG+="uaccess"`). AppImage users get the rule text from the in-app hint
  and `docs/linux.md`.
- Bundle identifier: `io.github.soham-verma.pingview` (matches the
  `soham-verma` GitHub account adsb-tar1090 lives under). No auto-updater in v1.
- CI also runs `cargo test`, `cargo clippy -D warnings` and
  `npm run build` on every push.

## Testing

- **Fixture:** a ≥ 60 s raw capture from the real receiver
  (`src-tauri/tests/fixtures/capture-2026-10-13.bin`) recorded during
  implementation, plus a few hand-built frames (bad checksum, split
  across reads, v2, truncated v2, simulated flag).
- **`mavlink`:** frames found and verified in the fixture; resync after
  garbage; frames split at every byte boundary give the same result.
- **`adsb`:** known values decoded from fixture frames (hex, callsign,
  lat/lon in range, unit conversions); validity flags respected.
- **`store`:** merge keeps old fields, tslc backdating, prune at 60 s,
  snapshot JSON matches the shape above (compared against a golden file).
- **`history`:** dedup, UTC day rollover, list/read.
- **Replay mode:** `PINGVIEW_REPLAY=<file>` feeds a capture through the
  pipeline at real-time pace in place of a serial port. It's used for
  development without hardware and in CI as a smoke test that builds the
  app, runs replay for 10 s and checks that the snapshot has aircraft.
- **Manual (per release):** plug in on macOS, Windows and Linux; unplug
  and replug; offline with cached tiles; Linux permission hint.

## Out of scope for v1

SBS/TCP input, phones/tablets, Windows signing, auto-updater, per-category
icons (`emitter_type` is passed through so they can be added later),
offline map region files, Ping ownship/GPS display, configuring the Ping
from the app.

## Amendments (2026-10-13, during implementation)

These supersede the sections above where they conflict.

- **Tiles: MapTiler only.** CARTO now requires an API key, and AirNav
  Radar's own tile server can't be used by a distributed app. Users enter
  their own MapTiler key in Settings (stored in `settings.json`, never shown
  again once saved). Styles: Dark (`dataviz-dark`, default), Streets (dark),
  Streets, Topographic, Satellite (`hybrid`, JPEG). No key → no tile
  requests and an in-map prompt; HTTP 401/403 → "key rejected" prompt. Cache
  path `<cache>/tiles/<style>/<z>/<x>/<y>[@2x].<ext>`, key-independent.
- **Aircraft panel (ADSBx-style sidebar)** replaces the popup card. Shows
  only fields that can be filled: identity, route, photo, receiver data
  (spatial + signal) and an optional "Network" section. Fields the Ping
  can't provide (RSSI, FMS SEL, wind, IAS/TAS/Mach) come only from the
  network section, labelled with their source.
- **Online assist** (`assist.rs`, replaces `enrich.rs`): adsb.lol then
  adsb.fi (1 req/s) for live records and missing callsigns (marked
  "assisted"; the Ping's own callsign always wins), hexdb.io for static info
  and routes (shown with their "as of" date, which can be years old),
  planespotters.net for photos. Only definitive answers are cached;
  403/429/5xx/timeouts are retried. One setting turns all of it off.
- **Offline data** (`aviation-data/`): tar1090-db `aircraft.csv.gz`
  (bundled resource, indexed in Rust by `registry.rs`) for registration,
  type, owner and military flag; `airlines.json` (OpenFlights) for airline
  name and spoken callsign; `australia_aerodromes.json` for the aerodrome
  layer and "nearest aerodrome". Country comes from the ICAO address block
  (`country.rs`).
- **Location** uses the OS, not the webview: CoreLocation (macOS, with
  `NSLocationUsageDescription` and the hardened-runtime location
  entitlement), Windows.Devices.Geolocation, GeoClue2 over D-Bus (Linux), in
  the `src-tauri/locate` crate. Failures fall back to "pick on map".
- **Layout (AirNav Radar-inspired, blue accent):** no header bar. A compact
  aircraft card top-left (callsign, route with progress and ETA when the
  route is plausible, altitude/speed/vertical strip, photo, registration and
  type, "More details" for the full data). A status pill when nothing is
  selected. Bottom dock: Map Options, History, Receiver, Settings, each
  opening a right-hand drawer. Map Options holds style cards with real tile
  thumbnails, brightness, and overlay toggles (aerodromes, aircraft labels,
  trails). Round fullscreen and location buttons sit above the zoom control.
  Aerodrome popups lead with colour-coded frequencies. Errors show as toasts.
- **Network traffic (reverses "receiver only"):** an overlay, on by default,
  adds aircraft the receiver can't hear from adsb.lol `/v2/point` (fallback
  adsb.fi `/v3/lat/lon/dist`, max 250 nm, 1 req/s), polled every 5 s around
  the user's location (or the map centre), radius 25–250 nm (default 100).
  They look like receiver aircraft but carry a NET tag; the receiver's copy
  of an aircraft always wins; history records receiver aircraft only.
  Needs Online assist on. Terms: adsb.lol data is ODbL (credited in the map
  attribution); adsb.fi open data is personal, non-commercial use only and
  must be credited with a link (also in the attribution).
- **Offline first.** Everything that can work without internet does: the
  receiver, offline registry, airlines, aerodromes, history, OS location.
  A bundled Natural Earth basemap (`src/basemap/basemap.json`, ~1.5 MB,
  built by `scripts/build_basemap.py`: world 1:50m land/lakes/borders,
  Australia 1:10m coastline/states/lakes/highways/towns) is drawn on a
  canvas *under* the tile layer, so offline areas and failed tiles show it.
  `net.rs` (`NetHealth`) gates every outbound request: **Local only**
  (Settings) blocks all of them; after a connection failure requests back
  off for 15 s with one probe per window; the snapshot carries
  `net: online | offline | local_only`, shown on the status pill. The HTTP
  client has connect (4 s) and total (12 s) timeouts. A missing MapTiler
  key is a dismissible hint, not a blocker.
- **Map sources.** `map_style` is `builtin` (default: the Natural Earth
  map, no tile layer), a MapTiler style (needs a key; saving a key while on
  `builtin` switches to `dataviz-dark`, removing it switches back), or
  `custom-<id>` for a user map in `settings.custom_maps`: an XYZ URL
  template (cached under `tiles/custom-<id>/`; local-network hosts are
  allowed in local-only mode) or a raster MBTiles file read with rusqlite
  (`mbtiles.rs`, TMS row flip; vector `pbf` files are rejected with an
  explanation). `set_settings` validates custom maps.
- **Built-in world map (size budget: whole app ≤ 250 MB).** The `builtin`
  style is an OpenStreetMap vector extract of the whole world at zoom 0–7
  (Protomaps build 20261013, 189 MB, `basemap-data/world.pmtiles`, fetched
  and checksum-verified by `scripts/fetch_basemap.sh`, bundled as a Tauri
  resource) drawn with MapLibre GL inside Leaflet (`WorldMap.jsx`, Protomaps
  "dark" flavour, glyphs and sprites in `public/basemap-assets`). Rust
  serves the archive over `pingview://localhost/basemap/world.pmtiles`
  with HTTP byte ranges (`basemap.rs`). If WebGL is missing or the archive
  fails to load, the Natural Earth canvas map is used instead. Release app
  ≈ 215–230 MB.
