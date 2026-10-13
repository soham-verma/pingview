# Receiver bridge firmware

`ping_usb_bridge/` turns an STM32F401 "Black Pill" into a USB serial
device that passes the pingRX Pro's UART through unchanged:

- USART1 at 57600 8N1: PA10 = RX (from Ping TX), PA9 = TX (to Ping RX)
- USB CDC: shows up as `/dev/cu.usbmodem…` (macOS), `COMn` (Windows),
  `/dev/ttyACM…` (Linux), USB ID `0483:5740`
- The onboard LED (PC13) toggles on every burst of Ping data

## Build

Arduino IDE or `arduino-cli`, with the STM32 core installed. Add this URL
under *Additional boards manager URLs*:
`https://github.com/stm32duino/BoardManagerFiles/raw/main/package_stmicroelectronics_index.json`.

```sh
arduino-cli compile \
  --fqbn 'STMicroelectronics:stm32:GenF4:pnum=BLACKPILL_F401CC,usb=CDCgen,xserial=none' \
  --output-dir build ping_usb_bridge
```

## Flash (DFU over USB)

1. Disconnect the wire on PA10 (Ping data on that pin can stop the
   bootloader entering DFU).
2. Hold **BOOT0**, tap **NRST**, release BOOT0.
3. Run `dfu-util -a 0 -s 0x08000000:leave -D build/ping_usb_bridge.ino.bin`.
4. Reconnect PA10, then tap **NRST** or replug USB. `dfu-util`'s `:leave`
   can leave the USB peripheral half-configured until a real reset.
