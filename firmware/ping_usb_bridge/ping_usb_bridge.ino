// USB CDC <-> USART1 bridge for a uAvionix pingRX Pro on a Black Pill F401.
// Ping TX -> PA10 (USART1 RX), Ping RX <- PA9 (USART1 TX), 57600 8N1.
// Build: Generic STM32F4 / BlackPill F401CC, usb=CDCgen, xserial=none.

Uart Ping(PA10, PA9);  // rx, tx

void setup() {
  Serial.begin(115200);  // USB CDC; host baud is ignored
  Ping.begin(57600);
  pinMode(PC13, OUTPUT);  // onboard LED, toggles on Ping traffic
}

void loop() {
  uint8_t buf[64];
  int n = Ping.available();
  if (n > 0) {
    n = Ping.readBytes(buf, min(n, (int)sizeof(buf)));
    Serial.write(buf, n);
    digitalToggle(PC13);
  }
  n = Serial.available();
  if (n > 0) {
    n = Serial.readBytes(buf, min(n, (int)sizeof(buf)));
    Ping.write(buf, n);
  }
}
