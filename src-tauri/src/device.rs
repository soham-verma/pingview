//! Finds the receiver (a USB bridge or ADSBee, or a Bluetooth LE / Wi-Fi
//! bridge), keeps it connected, and feeds its bytes into the pipeline. Also hosts replay mode
//! (`PINGVIEW_REPLAY=<capture file>`).

use std::collections::HashSet;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serialport::{SerialPortType, UsbPortInfo};

use crate::adsbee::{self, Session};
use crate::ble::{self, BleLink};
use crate::discover;
use crate::pipeline::Pipeline;
use crate::settings::Settings;
use crate::store::{Model, ReceiverStatus};

/// USB IDs of the bridges: ST's generic CDC Virtual COM Port (Black Pill)
/// and the ESP32-C3's built-in USB Serial/JTAG (XIAO ESP32-C3).
pub const USB_IDS: [(u16, u16); 2] = [(0x0483, 0x5740), (0x303A, 0x1001)];
pub const BAUD: u32 = 57_600;
const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_DELAY: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_millis(200);
/// A Ping sends status frames every second even with no traffic, so this
/// long without a byte means a wireless link is dead (TCP and BLE don't
/// always say so).
const WIRELESS_SILENCE: Duration = Duration::from_secs(10);

/// Called with hexes that need a registration lookup.
pub type LookupSink = Arc<dyn Fn(Vec<String>) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PortInfo {
    pub name: String,
    pub description: String,
    pub likely_receiver: bool,
    /// Could be an ADSBee 1090, which needs its output switching on.
    #[serde(skip)]
    pub adsbee: bool,
}

fn is_receiver(usb: &UsbPortInfo) -> bool {
    USB_IDS.contains(&(usb.vid, usb.pid))
}

/// macOS lists every USB serial device twice; only the `cu.` callout
/// device can be opened without waiting for carrier detect.
fn usable_name(name: &str) -> bool {
    !(cfg!(target_os = "macos") && name.starts_with("/dev/tty."))
}

/// Serial ports, then Bluetooth receivers in range, then Wi-Fi receivers
/// found on the network.
pub fn list_ports() -> Vec<PortInfo> {
    let ports = serialport::available_ports().unwrap_or_default();
    let mut list: Vec<PortInfo> = ports
        .into_iter()
        .filter(|p| usable_name(&p.port_name))
        .map(|p| {
            let (description, likely_receiver, adsbee) = match &p.port_type {
                SerialPortType::UsbPort(usb) => {
                    let adsbee = adsbee::is_adsbee(usb);
                    (
                        usb.product.clone().unwrap_or_else(|| "USB serial".into()),
                        is_receiver(usb) || adsbee,
                        adsbee,
                    )
                }
                SerialPortType::BluetoothPort => ("Bluetooth".into(), false, false),
                SerialPortType::PciPort => ("PCI".into(), false, false),
                SerialPortType::Unknown => ("Serial".into(), false, false),
            };
            PortInfo {
                name: p.port_name,
                description,
                likely_receiver,
                adsbee,
            }
        })
        .collect();
    list.extend(ble::receivers().into_iter().map(|name| PortInfo {
        description: "Bluetooth receiver".into(),
        name,
        likely_receiver: true,
        adsbee: false,
    }));
    list.extend(discover::receivers().into_iter().map(|r| PortInfo {
        description: match r.id {
            Some(id) => format!("Wi-Fi receiver {id}"),
            None => "Wi-Fi receiver".into(),
        },
        name: r.address,
        likely_receiver: true,
        adsbee: false,
    }));
    list
}

/// Ports to try, in order: the user's override, else every likely receiver
/// (USB first, then Bluetooth, then Wi-Fi) that hasn't already been rejected, then the
/// bridge's access-point address in case mDNS is blocked.
pub fn candidates(ports: &[PortInfo], override_port: Option<&str>, rejected: &HashSet<String>) -> Vec<String> {
    if let Some(port) = override_port {
        return vec![port.to_string()];
    }
    let mut names: Vec<String> = ports
        .iter()
        .filter(|p| p.likely_receiver && !rejected.contains(&p.name))
        .map(|p| p.name.clone())
        .collect();
    if !names.iter().any(|n| n == discover::AP_ADDRESS) && !rejected.contains(discover::AP_ADDRESS) {
        names.push(discover::AP_ADDRESS.to_string());
    }
    names
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
pub fn spawn_receiver(pipeline: Arc<Pipeline>, settings: Arc<Mutex<Settings>>, lookups: LookupSink) {
    thread::spawn(move || {
        let mut rejected: HashSet<String> = HashSet::new();
        loop {
            let override_port = settings.lock().unwrap().port.clone();
            let ports = list_ports();
            // A rejected port that disappears may come back as something else.
            // The access-point address is retried each round (it's cheap).
            rejected.retain(|name| ports.iter().any(|p| &p.name == name));
            let names = candidates(&ports, override_port.as_deref(), &rejected);
            if names.is_empty() {
                pipeline.set_status(ReceiverStatus::Searching, None, None);
                thread::sleep(RETRY_DELAY);
                continue;
            }
            let mut tried_any = false;
            for name in names {
                let is_adsbee = ports.iter().any(|p| p.name == name && p.adsbee);
                let opened = if ble::is_ble(&name) {
                    match ble::connect(&name, READ_TIMEOUT) {
                        Ok(link) => Ok(Link::Ble(link)),
                        // Usually just out of range; only report it if chosen.
                        Err(_) if override_port.is_none() => continue,
                        Err(e) => Err(format!("Couldn't connect to {}: {e}", &name[ble::SCHEME.len()..])),
                    }
                } else if discover::is_network(&name) {
                    match discover::connect(&name) {
                        Ok(stream) => {
                            let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                            Ok(Link::Network(stream))
                        }
                        // Auto-detect probes addresses that are usually absent;
                        // only report failures for an address the user chose.
                        Err(_) if override_port.is_none() => continue,
                        Err(e) => Err(format!("Couldn't reach {name}: {e}")),
                    }
                } else {
                    serialport::new(&name, BAUD)
                        .timeout(READ_TIMEOUT)
                        // The ADSBee only sends while DTR is up, and Windows
                        // opens with it down. Left alone for the bridges: DTR
                        // changes can reset the XIAO's ESP32-C3.
                        .dtr_on_open(is_adsbee)
                        .open()
                        .map(Link::Serial)
                        .map_err(|e| open_error_detail(&e))
                };
                tried_any = true;
                match opened {
                    Ok(link) => {
                        let session = match &link {
                            Link::Serial(port) if is_adsbee => match start_adsbee(port.as_ref()) {
                                Ok(session) => Some(session),
                                Err(e) => {
                                    let detail = format!("Couldn't talk to the ADSBee: {e}");
                                    pipeline.set_status(ReceiverStatus::Error, Some(&name), Some(detail));
                                    continue;
                                }
                            },
                            _ => None,
                        };
                        let model = if session.is_some() { Model::Adsbee } else { Model::PingRx };
                        pipeline.start_link(&name, model);
                        let outcome = read_loop(link, session, &pipeline, &lookups, || {
                            settings.lock().unwrap().port != override_port
                        });
                        if outcome == LinkEnd::NotAReceiver && override_port.is_none() {
                            rejected.insert(name.clone());
                        }
                        if outcome == LinkEnd::NotAReceiver {
                            pipeline.set_status(
                                ReceiverStatus::Error,
                                Some(&name),
                                Some("This port isn't sending receiver data.".into()),
                            );
                        }
                    }
                    Err(detail) => {
                        pipeline.set_status(ReceiverStatus::Error, Some(&name), Some(detail));
                    }
                }
            }
            if !tried_any {
                pipeline.set_status(ReceiverStatus::Searching, None, None);
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

enum Link {
    Serial(Box<dyn serialport::SerialPort>),
    Network(std::net::TcpStream),
    Ble(BleLink),
}

impl Read for Link {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Link::Serial(port) => port.read(buf),
            Link::Network(stream) => stream.read(buf),
            Link::Ble(link) => link.read(buf),
        }
    }
}

/// Switches an ADSBee's output on; its settings are put back when the
/// session drops, or at exit if the app quits first.
fn start_adsbee(port: &dyn serialport::SerialPort) -> serialport::Result<Session<Box<dyn serialport::SerialPort>>> {
    let session = Session::start(port.try_clone()?, Instant::now())?;
    Ok(session.restore_on_exit_via(Box::new(port.try_clone()?)))
}

fn read_loop<W: io::Write>(
    mut link: Link,
    mut adsbee: Option<Session<W>>,
    pipeline: &Pipeline,
    lookups: &LookupSink,
    settings_changed: impl Fn() -> bool,
) -> LinkEnd {
    let wireless = !matches!(link, Link::Serial(_));
    let opened = Instant::now();
    let mut last_byte = Instant::now();
    let mut battery = None;
    let mut buf = [0u8; 1024];
    loop {
        if let Link::Ble(ble) = &link {
            let now = ble.battery();
            if now != battery {
                battery = now;
                pipeline.set_battery(battery);
            }
        }
        match link.read(&mut buf) {
            // A closed TCP/BLE connection reads as 0 bytes; serial can too, harmlessly.
            Ok(0) if wireless => return LinkEnd::Disconnected,
            Ok(0) => {}
            Ok(n) => {
                last_byte = Instant::now();
                feed(pipeline, lookups, &buf[..n]);
                if let Some(page) = adsbee.as_mut().and_then(|s| s.feed(&buf[..n])) {
                    pipeline.set_settings_page(Some(page));
                }
            }
            // Read timeouts are TimedOut on Windows, WouldBlock on Unix sockets.
            Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {}
            Err(_) => return LinkEnd::Disconnected,
        }
        if let Some(session) = adsbee.as_mut() {
            session.poll(Instant::now());
        }
        if wireless && last_byte.elapsed() > WIRELESS_SILENCE {
            return LinkEnd::Disconnected;
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
            pipeline.start_link(&label, Model::PingRx);
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
            adsbee: false,
        }
    }

    #[test]
    fn candidates_prefer_override_then_receiver_ids() {
        let ports = vec![
            port("/dev/ttyS0", false),
            port("/dev/ttyACM0", true),
            port("/dev/ttyACM1", true),
            port("tcp://10.0.0.7:5760", true),
        ];
        let mut rejected = HashSet::new();
        assert_eq!(
            candidates(&ports, None, &rejected),
            vec!["/dev/ttyACM0", "/dev/ttyACM1", "tcp://10.0.0.7:5760", discover::AP_ADDRESS]
        );
        rejected.insert("/dev/ttyACM0".to_string());
        rejected.insert(discover::AP_ADDRESS.to_string());
        assert_eq!(candidates(&ports, None, &rejected), vec!["/dev/ttyACM1", "tcp://10.0.0.7:5760"]);
        assert_eq!(candidates(&ports, Some("COM7"), &rejected), vec!["COM7"]);
    }

    #[test]
    fn network_link_ends_when_the_bridge_hangs_up() {
        use std::io::Write;
        use std::net::TcpListener;
        use crate::adsb::tests::{payload, sample};
        use crate::mavlink::tests::v1_frame;
        use crate::pipeline::SharedRegistry;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let name = format!("tcp://{}", listener.local_addr().unwrap());
        let bridge = thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.write_all(&v1_frame(0, 246, &payload(&sample()), 184)).unwrap();
            // Dropping the socket closes the connection.
        });
        let stream = discover::connect(&name).unwrap();
        stream.set_read_timeout(Some(READ_TIMEOUT)).unwrap();
        let pipeline = Pipeline::new(None, false, SharedRegistry::default());
        pipeline.start_link(&name, Model::PingRx);
        let lookups: LookupSink = Arc::new(|_| {});
        let outcome = read_loop::<io::Sink>(Link::Network(stream), None, &pipeline, &lookups, || false);
        bridge.join().unwrap();
        assert_eq!(outcome, LinkEnd::Disconnected);
        assert!(pipeline.identified());
        assert_eq!(pipeline.lock().receiver.status, ReceiverStatus::Connected);
    }

/// Needs an ADSBee 1090 plugged in: `cargo test adsbee_smoke -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn adsbee_smoke() {
        use crate::pipeline::SharedRegistry;
        use std::io::Write;

        let ask_protocol = |name: &str| -> String {
            let mut port = serialport::new(name, BAUD).timeout(READ_TIMEOUT).dtr_on_open(true).open().unwrap();
            port.write_all(b"AT+PROTOCOL_OUT?\r\n").unwrap();
            let mut text = Vec::new();
            let mut buf = [0u8; 512];
            let until = Instant::now() + Duration::from_secs(2);
            while Instant::now() < until {
                if let Ok(n) = port.read(&mut buf) {
                    text.extend_from_slice(&buf[..n]);
                }
            }
            let text = String::from_utf8_lossy(&text);
            text.lines().find(|l| l.contains("PROTOCOL_OUT=CONSOLE,")).unwrap_or("").trim().to_string()
        };

        let ports = list_ports();
        let found = ports.iter().find(|p| p.adsbee).expect("no ADSBee plugged in");
        assert!(found.likely_receiver);
        let name = found.name.clone();
        println!("found {name} ({})", found.description);
        let before = ask_protocol(&name);
        println!("before: {before}");

        let port = serialport::new(&name, BAUD).timeout(READ_TIMEOUT).dtr_on_open(true).open().unwrap();
        let session = start_adsbee(port.as_ref()).unwrap();
        let pipeline = Pipeline::new(None, false, SharedRegistry::default());
        pipeline.start_link(&name, Model::Adsbee);
        let lookups: LookupSink = Arc::new(|_| {});
        let stop_at = Instant::now() + Duration::from_secs(12);
        let outcome = read_loop(Link::Serial(port), Some(session), &pipeline, &lookups, || Instant::now() > stop_at);
        let snap = pipeline.tick(Utc::now().timestamp() as f64);
        println!("outcome {outcome:?}, identified {}", pipeline.identified());
        println!("receiver: {}", snap["receiver"]);
        println!("aircraft: {}", snap["aircraft"].as_array().map_or(0, Vec::len));
        assert_eq!(outcome, LinkEnd::SettingsChanged);
        assert!(pipeline.identified());
        assert!(!snap["receiver"]["settings_page"].is_null(), "no settings page found");

        let after = ask_protocol(&name);
        println!("after: {after}");
        assert_eq!(after, before, "console output wasn't put back");
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
