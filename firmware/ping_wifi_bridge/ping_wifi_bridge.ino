// Wireless bridge for a uAvionix pingRX Pro on a Seeed XIAO ESP32-C3.
// Ping UART bytes go unchanged to a Bluetooth LE app (Nordic UART Service),
// every TCP client on port 5760, and USB; bytes from TCP clients or USB go
// back to the Ping. Wi-Fi is advertised over mDNS as _pingview._tcp so
// PingView finds it. Status and Wi-Fi setup at http://<ip>/.
// Build: board XIAO_ESP32C3, USB CDC On Boot enabled (default), Partition Scheme
// Minimal SPIFFS (see firmware/README.md).

#include <BLEDevice.h>
#include <BLEServer.h>
#include <ESPmDNS.h>
#include <esp_sleep.h>
#include <Preferences.h>
#include <WebServer.h>
#include <WiFi.h>

#include "config.h"

#define FW_VERSION "1"

HardwareSerial Ping(1);
WiFiServer server(TCP_PORT);
WebServer http(80);
Preferences prefs;
WiFiClient clients[MAX_CLIENTS];

String deviceId;  // "a1b2", from the MAC
String savedSsid, savedPass;
bool apMode = false;
uint32_t lastStaRetry = 0, staAttemptAt = 0;
uint32_t bytesFromPing = 0, bytesToPing = 0, lastPingByteAt = 0;

// Nordic UART Service: apps write to RX, we notify on TX.
#define NUS_SERVICE "6E400001-B5A3-F393-E0A9-E50E24DCCA9E"
#define NUS_RX "6E400002-B5A3-F393-E0A9-E50E24DCCA9E"
#define NUS_TX "6E400003-B5A3-F393-E0A9-E50E24DCCA9E"
BLEServer *bleServer = nullptr;
BLECharacteristic *bleTx = nullptr;
volatile bool bleConnected = false;
volatile uint16_t bleConnHandle = 0;
int batteryMv = -1;

// Standard Battery Service (level in %), plus the measured voltage in mV
// (uint16 little-endian) since the % is only an estimate from voltage.
#define BATTERY_SERVICE "180F"
#define BATTERY_LEVEL "2A19"
#define BATTERY_MV "B536A774-8C8D-4422-8924-83BC6496632D"
BLECharacteristic *bleBatteryLevel = nullptr;
BLECharacteristic *bleBatteryMv = nullptr;

// Ping bytes are batched so a burst goes out as one TCP segment.
uint8_t pending[512];
size_t pendingLen = 0;
uint32_t pendingSince = 0;
const uint32_t FLUSH_MS = 20;

// ---------------------------------------------------------------- Wi-Fi

bool joinNetwork(uint32_t timeoutMs) {
  if (savedSsid.isEmpty()) return false;
  WiFi.begin(savedSsid.c_str(), savedPass.c_str());
  uint32_t start = millis();
  while (WiFi.status() != WL_CONNECTED && millis() - start < timeoutMs) {
    delay(100);
  }
  return WiFi.status() == WL_CONNECTED;
}

void startWifi() {
  WiFi.persistent(false);  // credentials live in our own Preferences
  WiFi.setHostname(("pingview-" + deviceId).c_str());
  WiFi.mode(WIFI_STA);
  if (joinNetwork(WIFI_CONNECT_TIMEOUT_MS)) {
    WiFi.setAutoReconnect(true);
    apMode = false;
    return;
  }
  WiFi.disconnect(true);
  WiFi.mode(savedSsid.isEmpty() ? WIFI_AP : WIFI_AP_STA);
  WiFi.softAP(("PingView-" + deviceId).c_str(), AP_PASSWORD);
  apMode = true;
  lastStaRetry = millis();
}

// In access-point mode, keep trying the saved network (e.g. a phone hotspot
// switched on after the receiver) while nobody is using the access point.
// Non-blocking, so the bridge keeps running during the attempt.
void retrySavedNetwork() {
  if (!apMode || savedSsid.isEmpty()) return;
  if (staAttemptAt != 0) {
    if (WiFi.status() == WL_CONNECTED) {
      WiFi.mode(WIFI_STA);
      WiFi.setAutoReconnect(true);
      apMode = false;
      staAttemptAt = 0;
      MDNS.end();
      startMdns();
    } else if (millis() - staAttemptAt > 10000) {
      WiFi.disconnect(false);
      staAttemptAt = 0;
      lastStaRetry = millis();
    }
    return;
  }
  if (WiFi.softAPgetStationNum() > 0) lastStaRetry = millis();
  if (millis() - lastStaRetry < WIFI_RETRY_MS) return;
  WiFi.begin(savedSsid.c_str(), savedPass.c_str());
  staAttemptAt = millis();
}

void startMdns() {
  if (!MDNS.begin(("pingview-" + deviceId).c_str())) return;
  MDNS.addService("pingview", "tcp", TCP_PORT);
  MDNS.addServiceTxt("pingview", "tcp", "id", deviceId);
  MDNS.addServiceTxt("pingview", "tcp", "fw", FW_VERSION);
  MDNS.addService("http", "tcp", 80);
}

void forgetWifiIfBootHeld() {
  static uint32_t pressedAt = 0;
  if (digitalRead(BOOT_BUTTON_PIN) == HIGH) {
    pressedAt = 0;
    return;
  }
  if (pressedAt == 0) pressedAt = millis();
  if (millis() - pressedAt > 3000) {
    prefs.remove("ssid");
    prefs.remove("pass");
    delay(100);
    ESP.restart();
  }
}

// ---------------------------------------------------------------- BLE

class BleServerEvents : public BLEServerCallbacks {
  void onConnect(BLEServer *server, ble_gap_conn_desc *desc) override {
    bleConnHandle = desc->conn_handle;
    bleConnected = true;
    // 15-30 ms interval: plenty for the Ping's few kB/s.
    server->requestConnParams(desc->conn_handle, 12, 24, 0, 400);
  }
  void onDisconnect(BLEServer *, ble_gap_conn_desc *) override {
    bleConnected = false;
    BLEDevice::startAdvertising();
  }
};

class BleRxEvents : public BLECharacteristicCallbacks {
  void onWrite(BLECharacteristic *ch) override {
    if (!BLE_ALLOW_WRITES) return;
    String data = ch->getValue();
    Ping.write((const uint8_t *)data.c_str(), data.length());
    bytesToPing += data.length();
  }
};

void startBle() {
  String name = "PingView-" + deviceId;
  BLEDevice::init(name);
  BLEDevice::setMTU(247);
  bleServer = BLEDevice::createServer();
  bleServer->setCallbacks(new BleServerEvents());
  BLEService *service = bleServer->createService(NUS_SERVICE);
  bleTx = service->createCharacteristic(NUS_TX, BLECharacteristic::PROPERTY_NOTIFY);
  BLECharacteristic *rx =
      service->createCharacteristic(NUS_RX, BLECharacteristic::PROPERTY_WRITE | BLECharacteristic::PROPERTY_WRITE_NR);
  rx->setCallbacks(new BleRxEvents());
  service->start();

  if (BATTERY_ADC_PIN >= 0) {
    BLEService *battery = bleServer->createService(BATTERY_SERVICE);
    bleBatteryLevel = battery->createCharacteristic(
        BATTERY_LEVEL, BLECharacteristic::PROPERTY_READ | BLECharacteristic::PROPERTY_NOTIFY);
    bleBatteryMv = battery->createCharacteristic(
        BATTERY_MV, BLECharacteristic::PROPERTY_READ | BLECharacteristic::PROPERTY_NOTIFY);
    battery->start();
  }

  // The 128-bit UUID and the name don't both fit in one 31-byte advert, so
  // the name goes in the scan response.
  BLEAdvertising *adv = BLEDevice::getAdvertising();
  BLEAdvertisementData advData, scanData;
  advData.setFlags(0x06);  // general discoverable, no classic Bluetooth
  advData.setCompleteServices(BLEUUID(NUS_SERVICE));
  scanData.setName(name);
  adv->setAdvertisementData(advData);
  adv->setScanResponseData(scanData);
  BLEDevice::startAdvertising();
}

void sendBle(const uint8_t *data, size_t len) {
  if (!bleConnected) return;
  size_t chunk = bleServer->getPeerMTU(bleConnHandle) - 3;  // ATT header
  if (chunk < 20) chunk = 20;
  for (size_t off = 0; off < len; off += chunk) {
    size_t n = min(chunk, len - off);
    bleTx->setValue((uint8_t *)data + off, n);
    bleTx->notify();
  }
}

// ---------------------------------------------------------------- TCP

void acceptClients() {
  WiFiClient incoming = server.accept();
  if (!incoming) return;
  incoming.setNoDelay(true);
  for (auto &c : clients) {
    if (!c.connected()) {
      c.stop();
      c = incoming;
      return;
    }
  }
  incoming.stop();  // full
}

int clientCount() {
  int n = 0;
  for (auto &c : clients) n += c.connected() ? 1 : 0;
  return n;
}

void flushToClients() {
  if (pendingLen == 0) return;
  for (auto &c : clients) {
    if (c.connected() && c.write(pending, pendingLen) != pendingLen) c.stop();
  }
  if (ENABLE_BLE) sendBle(pending, pendingLen);
  // Wired fallback: also stream over USB when a host has the port open.
  if (Serial) Serial.write(pending, pendingLen);
  pendingLen = 0;
}

void pumpPingToClients() {
  while (Ping.available() && pendingLen < sizeof(pending)) {
    int n = Ping.read(pending + pendingLen, sizeof(pending) - pendingLen);
    if (n <= 0) break;
    if (pendingLen == 0) pendingSince = millis();
    pendingLen += n;
    bytesFromPing += n;
    lastPingByteAt = millis();
  }
  if (pendingLen == sizeof(pending) || (pendingLen > 0 && millis() - pendingSince >= FLUSH_MS)) {
    flushToClients();
  }
}

void pumpClientsToPing() {
  uint8_t buf[128];
  for (auto &c : clients) {
    while (c.connected() && c.available()) {
      int n = c.read(buf, sizeof(buf));
      if (n <= 0) break;
      Ping.write(buf, n);
      bytesToPing += n;
    }
  }
  while (Serial.available()) {
    int n = Serial.read(buf, sizeof(buf));
    if (n <= 0) break;
    Ping.write(buf, n);
    bytesToPing += n;
  }
}

// ---------------------------------------------------------------- Battery

// Rough single-cell LiPo state of charge under light load. Voltage is flat
// through the middle of the curve, so treat the % as an estimate.
uint8_t batteryPercent(int mv) {
  static const int16_t curve[][2] = {{4200, 100}, {4100, 90}, {4000, 80}, {3920, 70}, {3870, 60}, {3830, 50},
                                     {3790, 40},  {3750, 30}, {3710, 20}, {3670, 10}, {3600, 5},  {3300, 0}};
  const int n = sizeof(curve) / sizeof(curve[0]);
  if (mv >= curve[0][0]) return 100;
  for (int i = 1; i < n; i++) {
    if (mv >= curve[i][0]) {
      return curve[i][1] + (mv - curve[i][0]) * (curve[i - 1][1] - curve[i][1]) / (curve[i - 1][0] - curve[i][0]);
    }
  }
  return 0;
}

// The mV characteristic carries the raw divider reading, so a missing or
// miswired divider shows up as a low number rather than silence. The level
// is only published for a plausible battery voltage.
void publishBattery(int measuredMv) {
  static int lastMv = -1;
  if (!bleBatteryMv || measuredMv == lastMv) return;
  lastMv = measuredMv;
  uint8_t mv[2] = {(uint8_t)(measuredMv & 0xFF), (uint8_t)(measuredMv >> 8)};
  bleBatteryMv->setValue(mv, 2);
  if (bleConnected) bleBatteryMv->notify();
  if (batteryMv < 0) return;
  uint8_t pct = batteryPercent(batteryMv);
  bleBatteryLevel->setValue(&pct, 1);
  if (bleConnected) bleBatteryLevel->notify();
}

void readBattery() {
  static uint32_t lastRead = 0, lowSince = 0;
  if (BATTERY_ADC_PIN < 0 || (lastRead != 0 && millis() - lastRead < 2000)) return;
  lastRead = millis();
  uint32_t sum = 0;
  for (int i = 0; i < 16; i++) sum += analogReadMilliVolts(BATTERY_ADC_PIN);
  int mv = 2 * sum / 16;
  batteryMv = mv > 2500 ? mv : -1;  // nothing on the divider
  publishBattery(mv);
  if (batteryMv < 0 || batteryMv >= BATTERY_CUTOFF_MV) {
    lowSince = 0;
    return;
  }
  if (lowSince == 0) lowSince = millis();
  // Sustained low voltage: protect the cell. Only useful if we can also cut
  // the Ping's boost; otherwise it keeps draining the battery regardless.
  if (PING_POWER_PIN >= 0 && millis() - lowSince > 10000) {
    flushToClients();
    Ping.end();
    digitalWrite(PING_POWER_PIN, LOW);
    WiFi.mode(WIFI_OFF);
    esp_deep_sleep_start();  // until reset or power cycle
  }
}

// ---------------------------------------------------------------- HTTP

String jsonEscape(const String &s) {
  String out;
  for (char ch : s) {
    if (ch == '"' || ch == '\\') out += '\\';
    if ((uint8_t)ch >= 0x20) out += ch;
  }
  return out;
}

String htmlEscape(const String &s) {
  String out;
  for (char ch : s) {
    if (ch == '<') out += "&lt;";
    else if (ch == '>') out += "&gt;";
    else if (ch == '&') out += "&amp;";
    else if (ch == '"') out += "&quot;";
    else out += ch;
  }
  return out;
}

String ipAddress() {
  return (apMode ? WiFi.softAPIP() : WiFi.localIP()).toString();
}

void handleStatus() {
  uint32_t quietMs = lastPingByteAt ? millis() - lastPingByteAt : UINT32_MAX;
  String j = "{";
  j += "\"id\":\"" + deviceId + "\",";
  j += "\"fw\":\"" FW_VERSION "\",";
  j += "\"mode\":\"" + String(apMode ? "ap" : "sta") + "\",";
  j += "\"ssid\":\"" + jsonEscape(apMode ? "PingView-" + deviceId : WiFi.SSID()) + "\",";
  j += "\"ip\":\"" + ipAddress() + "\",";
  j += "\"rssi\":" + String(apMode ? 0 : WiFi.RSSI()) + ",";
  j += "\"clients\":" + String(clientCount()) + ",";
  j += "\"ble_connected\":" + String(bleConnected ? "true" : "false") + ",";
  j += "\"ping_active\":" + String(quietMs < 5000 ? "true" : "false") + ",";
  j += "\"bytes_from_ping\":" + String(bytesFromPing) + ",";
  j += "\"bytes_to_ping\":" + String(bytesToPing) + ",";
  j += "\"battery_mv\":" + (batteryMv > 0 ? String(batteryMv) : String("null")) + ",";
  j += "\"uptime_s\":" + String(millis() / 1000);
  j += "}";
  http.send(200, "application/json", j);
}

void handleRoot() {
  String h =
      "<!doctype html><meta name=viewport content='width=device-width'>"
      "<title>PingView receiver</title>"
      "<style>body{font:16px system-ui;margin:2em auto;max-width:28em;padding:0 1em}"
      "td{padding:.2em 1em .2em 0}input,button{font:inherit;margin:.2em 0;width:100%}</style>"
      "<h2>PingView receiver " + deviceId + "</h2><table>";
  auto row = [&](const String &k, const String &v) { h += "<tr><td>" + k + "<td>" + v; };
  row("Wi-Fi", apMode ? "Access point PingView-" + deviceId : "Joined " + htmlEscape(WiFi.SSID()));
  row("Address", ipAddress() + ":" + String(TCP_PORT));
  row("Apps connected", String(clientCount()) + " Wi-Fi, " + String(bleConnected ? 1 : 0) + " Bluetooth");
  row("Ping data", lastPingByteAt && millis() - lastPingByteAt < 5000 ? "Receiving" : "None");
  row("Battery", batteryMv > 0 ? String(batteryMv / 1000.0, 2) + " V" : "—");
  h += "</table><h3>Join a network</h3>"
       "<p>For example a phone hotspot, so your computer keeps its internet."
       "<form method=post action=/wifi>"
       "<input name=ssid placeholder='Network name' value='" + htmlEscape(savedSsid) + "' required>"
       "<input name=pass type=password placeholder='Password'>"
       "<button>Save and restart</button></form>";
  if (!savedSsid.isEmpty()) {
    h += "<form method=post action=/forget><button>Forget network (access point only)</button></form>";
  }
  http.send(200, "text/html", h);
}

void handleWifi() {
  String ssid = http.arg("ssid");
  if (ssid.isEmpty() || ssid.length() > 32 || http.arg("pass").length() > 63) {
    http.send(400, "text/plain", "Network name 1-32 characters, password up to 63.");
    return;
  }
  prefs.putString("ssid", ssid);
  prefs.putString("pass", http.arg("pass"));
  http.send(200, "text/html", "Saved. Restarting and joining " + htmlEscape(ssid) + "&hellip;");
  delay(500);
  ESP.restart();
}

void handleForget() {
  prefs.remove("ssid");
  prefs.remove("pass");
  http.send(200, "text/html", "Forgotten. Restarting as access point PingView-" + deviceId + "&hellip;");
  delay(500);
  ESP.restart();
}

// ---------------------------------------------------------------- Main

void setup() {
  setCpuFrequencyMhz(80);  // plenty for 57600 baud; saves battery
  Serial.begin(115200);    // USB CDC; host baud is ignored
  Serial.setTxTimeoutMs(0);  // never stall the bridge when no USB host
  pinMode(BOOT_BUTTON_PIN, INPUT_PULLUP);
  if (PING_POWER_PIN >= 0) {
    pinMode(PING_POWER_PIN, OUTPUT);
    digitalWrite(PING_POWER_PIN, HIGH);
  }

  Ping.setRxBufferSize(8192);  // rides out a slow TCP write
  Ping.begin(PING_BAUD, SERIAL_8N1, PING_RX_PIN, PING_TX_PIN);

  uint64_t mac = ESP.getEfuseMac();
  char id[5];
  snprintf(id, sizeof(id), "%02x%02x", (uint8_t)(mac >> 32), (uint8_t)(mac >> 40));
  deviceId = id;

  prefs.begin("pingview", false);
  savedSsid = prefs.getString("ssid", WIFI_SSID);
  savedPass = prefs.getString("pass", WIFI_PASSWORD);

  if (ENABLE_BLE) startBle();
  if (ENABLE_WIFI) {
    startWifi();
    startMdns();
    server.begin();
    server.setNoDelay(true);
    http.on("/", HTTP_GET, handleRoot);
    http.on("/status", HTTP_GET, handleStatus);
    http.on("/wifi", HTTP_POST, handleWifi);
    http.on("/forget", HTTP_POST, handleForget);
    http.begin();
  }
}

void loop() {
  if (ENABLE_WIFI) acceptClients();
  pumpPingToClients();
  pumpClientsToPing();
  if (ENABLE_WIFI) {
    http.handleClient();
    retrySavedNetwork();
    forgetWifiIfBootHeld();
  }
  readBattery();
  delay(1);  // lets the idle task run (and the CPU sleep between bytes)
}
