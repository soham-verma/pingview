//! Finds the receiver, keeps it connected, and feeds its bytes into the
//! pipeline. Also hosts replay mode (`PINGVIEW_REPLAY=<capture file>`).

use std::collections::HashSet;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serialport::{SerialPortType, UsbPortInfo};

use crate::pipeline::Pipeline;
use crate::settings::Settings;
use crate::store::ReceiverStatus;

/// ST's generic USB CDC Virtual COM Port ID, which the Black Pill bridge uses.
pub const USB_VID: u16 = 0x0483;
pub const USB_PID: u16 = 0x5740;
pub const BAUD: u32 = 57_600;
const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_DELAY: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_millis(200);

/// Called with hexes that need a registration lookup.
pub type LookupSink = Arc<dyn Fn(Vec<String>) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PortInfo {
    pub name: String,
    pub description: String,
    pub likely_receiver: bool,
}

fn is_receiver(usb: &UsbPortInfo) -> bool {
    usb.vid == USB_VID && usb.pid == USB_PID
}

/// macOS lists every USB serial device twice; only the `cu.` callout
/// device can be opened without waiting for carrier detect.
fn usable_name(name: &str) -> bool {
    !(cfg!(target_os = "macos") && name.starts_with("/dev/tty."))
}

pub fn list_ports() -> Vec<PortInfo> {
    let ports = serialport::available_ports().unwrap_or_default();
    ports
        .into_iter()
        .filter(|p| usable_name(&p.port_name))
        .map(|p| {
            let (description, likely_receiver) = match &p.port_type {
                SerialPortType::UsbPort(usb) => (
                    usb.product.clone().unwrap_or_else(|| "USB serial".into()),
                    is_receiver(usb),
                ),
                SerialPortType::BluetoothPort => ("Bluetooth".into(), false),
                SerialPortType::PciPort => ("PCI".into(), false),
                SerialPortType::Unknown => ("Serial".into(), false),
            };
            PortInfo {
                name: p.port_name,
                description,
                likely_receiver,
            }
        })
        .collect()
}

/// Ports to try, in order: the user's override, else every port with the
/// receiver's USB ID that hasn't already been rejected.
pub fn candidates(ports: &[PortInfo], override_port: Option<&str>, rejected: &HashSet<String>) -> Vec<String> {
    if let Some(port) = override_port {
        return vec![port.to_string()];
    }
    ports
        .iter()
        .filter(|p| p.likely_receiver && !rejected.contains(&p.name))
        .map(|p| p.name.clone())
        .collect()
}

pub fn open_error_detail(err: &serialport::Error) -> String {
    let denied = matches!(err.kind(), serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied))
        || err.to_string().to_lowercase().contains("permission denied");
    if denied && cfg!(target_os = "linux") {
        "Permission denied. Install the PingView udev rule (see docs/linux.md) \
         or add yourself to the dialout group, then replug the receiver."
            .into()
    } else if denied {
        "Permission denied — another app may have the receiver open.".into()
    } else {
        format!("Couldn't open the receiver: {err}")
    }
}

/// Runs forever on its own thread: discover, open, read, reconnect.
pub fn spawn_serial(pipeline: Arc<Pipeline>, settings: Arc<Mutex<Settings>>, lookups: LookupSink) {
    thread::spawn(move || {
        let mut rejected: HashSet<String> = HashSet::new();
        loop {
            let override_port = settings.lock().unwrap().port.clone();
            let ports = list_ports();
            // A rejected port that disappears may come back as something else.
            rejected.retain(|name| ports.iter().any(|p| &p.name == name));
            let names = candidates(&ports, override_port.as_deref(), &rejected);
            if names.is_empty() {
                pipeline.set_status(ReceiverStatus::Searching, None, None);
                thread::sleep(RETRY_DELAY);
                continue;
            }
            for name in names {
                match serialport::new(&name, BAUD).timeout(READ_TIMEOUT).open() {
                    Ok(port) => {
                        pipeline.start_link(&name);
                        let outcome = read_loop(port, &pipeline, &lookups, || {
                            settings.lock().unwrap().port != override_port
                        });
                        if outcome == LinkEnd::NotAReceiver && override_port.is_none() {
                            rejected.insert(name.clone());
                        }
                        if outcome == LinkEnd::NotAReceiver {
                            pipeline.set_status(
                                ReceiverStatus::Error,
                                Some(&name),
                                Some("This port isn't sending pingRX data.".into()),
                            );
                        }
                    }
                    Err(e) => {
                        pipeline.set_status(ReceiverStatus::Error, Some(&name), Some(open_error_detail(&e)));
                    }
                }
            }
            thread::sleep(RETRY_DELAY);
        }
    });
}

#[derive(Debug, PartialEq, Eq)]
enum LinkEnd {
    Disconnected,
    NotAReceiver,
    SettingsChanged,
}

fn read_loop(
    mut port: Box<dyn serialport::SerialPort>,
    pipeline: &Pipeline,
    lookups: &LookupSink,
    settings_changed: impl Fn() -> bool,
) -> LinkEnd {
    let opened = Instant::now();
    let mut buf = [0u8; 1024];
    loop {
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => feed(pipeline, lookups, &buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
            Err(_) => return LinkEnd::Disconnected,
        }
        if !pipeline.identified() && opened.elapsed() > IDENTIFY_TIMEOUT {
            return LinkEnd::NotAReceiver;
        }
        if settings_changed() {
            return LinkEnd::SettingsChanged;
        }
    }
}

fn feed(pipeline: &Pipeline, lookups: &LookupSink, bytes: &[u8]) {
    let now_utc = Utc::now();
    let now = now_utc.timestamp_millis() as f64 / 1000.0;
    let hexes = pipeline.ingest(bytes, now, now_utc);
    if !hexes.is_empty() {
        lookups(hexes);
    }
}

/// Replays a raw capture at the receiver's real byte rate, looping forever.
pub fn spawn_replay(pipeline: Arc<Pipeline>, file: PathBuf, lookups: LookupSink) {
    thread::spawn(move || {
        let data = match std::fs::read(&file) {
            Ok(d) if !d.is_empty() => d,
            Ok(_) | Err(_) => {
                let detail = format!("Replay file unreadable or empty: {}", file.display());
                pipeline.set_status(ReceiverStatus::Error, None, Some(detail));
                return;
            }
        };
        let label = format!("replay:{}", file.display());
        // 57600 baud, 8N1 → 5760 bytes/s; 64-byte chunks every ~11 ms.
        let chunk = 64;
        let pause = Duration::from_secs_f64(chunk as f64 / (BAUD as f64 / 10.0));
        loop {
            pipeline.start_link(&label);
            for part in data.chunks(chunk) {
                feed(&pipeline, &lookups, part);
                thread::sleep(pause);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, likely: bool) -> PortInfo {
        PortInfo {
            name: name.into(),
            description: String::new(),
            likely_receiver: likely,
        }
    }

    #[test]
    fn candidates_prefer_override_then_receiver_ids() {
        let ports = vec![port("/dev/ttyS0", false), port("/dev/ttyACM0", true), port("/dev/ttyACM1", true)];
        let mut rejected = HashSet::new();
        assert_eq!(candidates(&ports, None, &rejected), vec!["/dev/ttyACM0", "/dev/ttyACM1"]);
        rejected.insert("/dev/ttyACM0".to_string());
        assert_eq!(candidates(&ports, None, &rejected), vec!["/dev/ttyACM1"]);
        assert_eq!(candidates(&ports, Some("COM7"), &rejected), vec!["COM7"]);
    }

    #[test]
    fn linux_permission_error_explains_the_fix() {
        let err = serialport::Error::new(
            serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied),
            "Permission denied",
        );
        let detail = open_error_detail(&err);
        assert!(detail.starts_with("Permission denied"));
        if cfg!(target_os = "linux") {
            assert!(detail.contains("udev"));
        }
    }
}
