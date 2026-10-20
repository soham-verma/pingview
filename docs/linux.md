# Linux: serial port permissions

Linux only lets privileged users open serial ports by default. The `.deb`
package installs a udev rule that gives the logged-in user access to the
PingView receiver automatically. If you use the `.AppImage`, or PingView
shows "Permission denied", do one of these once:

**Option 1: udev rule (recommended)**

```sh
printf '%s\n' \
  'SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="5740", TAG+="uaccess"' \
  'SUBSYSTEM=="tty", ATTRS{idVendor}=="2e8a", ATTRS{idProduct}=="000a", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/60-pingview.rules
sudo udevadm control --reload-rules
```

Then unplug and replug the receiver.

**Option 2: dialout group**

```sh
sudo usermod -aG dialout "$USER"
```

Then log out and back in.

Note: `0483:5740` is ST's generic USB serial ID and `2e8a:000a` (the
ADSBee 1090) is the Raspberry Pi Pico's, so the rule also covers other
STM32 and Pico USB-serial boards. PingView still checks a port's data before
using it.
