# Receiver bridge firmware

Two bridges, both pass the pingRX Pro's MAVLink through unchanged:

- `ping_wifi_bridge/`: **Seeed XIAO ESP32-C3**, battery powered, streams over
  Bluetooth LE and USB, plus Wi-Fi (TCP) if enabled. PingView finds it on its own.
- `ping_usb_bridge/`: STM32F401 Black Pill, USB only.

## Wi-Fi bridge (XIAO ESP32-C3)

### Wiring

| From | To | Notes |
|---|---|---|
| Ping TX | XIAO **D7** (GPIO20) | Ping UART is 3.3 V. The ESP32-C3 is **not** 5 V tolerant: check Ping TX idles at ~3.3 V before connecting |
| Ping RX | XIAO **D10** (GPIO10) | Not D6: the ESP32-C3 ROM prints boot messages on D6 at every reset |
| Ping GND | XIAO GND | |
| Ping VCC | 5 V boost output | The Ping needs 4–6 V, a LiPo gives 3.0–4.2 V |
| LiPo + | power switch → XIAO **BAT+** pad (underside) and boost input | |
| LiPo − | XIAO **BAT−** pad and boost GND | |
| Battery sense (optional) | 200 kΩ from BAT+ (after the switch) to **D1**, 200 kΩ from D1 to GND | Shows battery voltage on the status page |
| Boost enable (optional) | **D2** (GPIO4), set `PING_POWER_PIN 4` | Lets the low-battery cutoff switch the Ping off |

```
LiPo+ ──switch──┬── XIAO BAT+      (charges from the XIAO's USB-C, 380 mA,
                │                   only while the switch is on)
                └── 5 V boost ──── Ping VCC
LiPo− ──────────┴── XIAO BAT− ──── Ping GND
```

- On battery the XIAO's 5V pin is **not** powered (it's USB VBUS), so the
  Ping needs its own boost converter, e.g. a Pololu U3V16F5 (has an enable
  pin) or a TPS61023 module. Budget ~150 mA at 5 V for the Ping plus
  ~80–100 mA for the XIAO with Wi-Fi on.
- Use a **protected** cell (built-in PCM). The XIAO has no undervoltage
  cutoff of its own, and the firmware's cutoff only stops the Ping if the
  boost has an enable pin wired up.
- Fit the antenna that came with the XIAO (U.FL); range is poor without it.
- Avoid D0 (GPIO2), D8 (GPIO8) and D9 (GPIO9): they're boot strapping pins.

### Connecting

**Bluetooth (recommended):** nothing to set up. The bridge advertises as
**PingView-xxxx** and PingView connects to it on its own, so your computer
stays on its normal Wi-Fi for internet. It serves one app at a time and
uses the standard Nordic UART Service, so nRF Connect or similar can read it
too. BLE apps can't write to the Ping unless `BLE_ALLOW_WRITES` is set in
`config.h` (there's no pairing). On macOS, allow PingView under System
Settings → Privacy & Security → Bluetooth.

PingView tries USB first, then Bluetooth, then Wi-Fi.

**Wi-Fi (off by default):** set `ENABLE_WIFI 1` in `config.h` and reflash.
It costs up to ~80 mA, roughly 29 h → 21 h of battery on a 6 Ah cell. On
power-up the bridge then joins the saved network (20 s timeout). With none
saved, or if it isn't there, it starts its own access point
**PingView-xxxx**, password `pingview`. In access-point mode it keeps
retrying the saved network every minute while nobody's connected, so a phone
hotspot switched on after the receiver still works.

PingView connects over Wi-Fi automatically either way: it finds the bridge via mDNS
(`_pingview._tcp`), or tries `192.168.4.1:5760` when you're on its access
point. Settings → Port lists Wi-Fi receivers too; USB still works and is
preferred when plugged in.

- **Access point**: simplest; your computer loses internet while joined
  (PingView's offline features all keep working).
- **Phone hotspot / other network**: open `http://192.168.4.1/` while on
  the access point (or `http://pingview-xxxx.local/` once joined), enter the
  network name and password, save. The page also shows battery voltage,
  connected apps and whether Ping data is arriving; `/status` is the same as
  JSON.
- Hold **BOOT** for 3 s while running to forget the network.

The stream is plain MAVLink on TCP 5760, so QGroundControl or Mission
Planner can connect to it as well (up to 4 apps at once). Pin choices and
defaults live in `ping_wifi_bridge/config.h`.

### Build and flash

Install the ESP32 core (`esp32` by Espressif, board manager URL
`https://espressif.github.io/arduino-esp32/package_esp32_index.json`), then:

```sh
arduino-cli compile --fqbn esp32:esp32:XIAO_ESP32C3:PartitionScheme=min_spiffs \
  --output-dir build ping_wifi_bridge
arduino-cli upload --fqbn esp32:esp32:XIAO_ESP32C3:PartitionScheme=min_spiffs \
  -p /dev/cu.usbmodem… --input-dir build ping_wifi_bridge
```

In the Arduino IDE: board **XIAO_ESP32C3**, *USB CDC On Boot: Enabled*
(the default), *Partition Scheme: Minimal SPIFFS* (Bluetooth plus Wi-Fi
doesn't fit the default 1.2 MB app partition). If
upload can't find the board, hold **BOOT**, plug in USB, release BOOT.

## USB bridge (Black Pill)

`ping_usb_bridge/` turns an STM32F401 "Black Pill" into a USB serial
device that passes the pingRX Pro's UART through unchanged:

- USART1 at 57600 8N1: PA10 = RX (from Ping TX), PA9 = TX (to Ping RX)
- USB CDC: shows up as `/dev/cu.usbmodem…` (macOS), `COMn` (Windows),
  `/dev/ttyACM…` (Linux), USB ID `0483:5740`
- The onboard LED (PC13) toggles on every burst of Ping data

### Build

Arduino IDE or `arduino-cli`, with the STM32 core installed. Add this URL
under *Additional boards manager URLs*:
`https://github.com/stm32duino/BoardManagerFiles/raw/main/package_stmicroelectronics_index.json`.

```sh
arduino-cli compile \
  --fqbn 'STMicroelectronics:stm32:GenF4:pnum=BLACKPILL_F401CC,usb=CDCgen,xserial=none' \
  --output-dir build ping_usb_bridge
```

### Flash (DFU over USB)

1. Disconnect the wire on PA10 (Ping data on that pin can stop the
   bootloader entering DFU).
2. Hold **BOOT0**, tap **NRST**, release BOOT0.
3. Run `dfu-util -a 0 -s 0x08000000:leave -D build/ping_usb_bridge.ino.bin`.
4. Reconnect PA10, then tap **NRST** or replug USB. `dfu-util`'s `:leave`
   can leave the USB peripheral half-configured until a real reset.
