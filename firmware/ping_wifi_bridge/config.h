// Build-time settings for the XIAO ESP32-C3 Wi-Fi bridge.
#pragma once

// Ping UART. D7 = GPIO20 (Ping TX -> here), D10 = GPIO10 (here -> Ping RX).
// D6/GPIO21 is avoided for TX: the ROM bootloader prints to it at reset.
#define PING_RX_PIN 20
#define PING_TX_PIN 10
#define PING_BAUD 57600

// Bluetooth LE: Nordic UART Service, advertised as "PingView-XXXX". The
// computer stays on its normal Wi-Fi. One app at a time.
#define ENABLE_BLE 1
// Pass bytes written by a BLE app on to the Ping. Off by default: BLE has no
// pairing here, so anyone in range could otherwise reconfigure the Ping.
#define BLE_ALLOW_WRITES 0

// Wi-Fi (TCP, access point, status page). Off by default: it costs up to
// ~80 mA (about 29 h -> 21 h on a 6 Ah battery) and Bluetooth covers PingView.
// Set to 1 for Wi-Fi, e.g. to use QGroundControl or Mission Planner.
#define ENABLE_WIFI 0

// Raw MAVLink over TCP. 5760 is the usual MAVLink TCP port, so QGroundControl
// and Mission Planner can connect too.
#define TCP_PORT 5760
#define MAX_CLIENTS 4

// Network to join (e.g. a phone hotspot). Anything saved from the web page
// overrides these. Leave empty to start in access-point mode.
#define WIFI_SSID ""
#define WIFI_PASSWORD ""
#define WIFI_CONNECT_TIMEOUT_MS 20000
// While in access-point mode with a saved network, retry it this often
// (only when nobody is connected to the access point).
#define WIFI_RETRY_MS 60000

// Access point: "PingView-XXXX", where XXXX comes from the MAC address.
// WPA2 needs at least 8 characters.
#define AP_PASSWORD "pingview"

// Battery divider (2 x 200k-220k from BAT+ to GND, midpoint here). A0 = GPIO2,
// as in Seeed's wiring guide. GPIO2 is a boot strapping pin and the divider
// holds it near 2 V at power-up; if the board ever fails to start, move the
// midpoint to D1 and set 3. -1 if not fitted.
#define BATTERY_ADC_PIN 2
// Below this (with the divider fitted), switch Ping off and sleep.
#define BATTERY_CUTOFF_MV 3300

// Enable pin of the Ping's 5 V boost converter, high = on. -1 if the boost
// has no enable pin (then low-battery cutoff can't stop the Ping's draw).
#define PING_POWER_PIN -1

// BOOT button: hold for 3 s while running to forget Wi-Fi.
#define BOOT_BUTTON_PIN 9
