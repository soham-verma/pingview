//! ADSBee 1090 over USB: recognising its port, switching its console to
//! MAVLink 2 (without saving, so a reboot undoes it) and reading back where
//! its settings web page is from the AT replies mixed into the stream.

use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serialport::UsbPortInfo;

/// Where the ADSBee's settings page can be reached, if anywhere.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingsPage {
    /// On the user's network (Wi-Fi or Ethernet).
    Url { url: String },
    /// Only through the ADSBee's own access point.
    Hotspot { ssid: String, password: String, url: String },
    /// Wi-Fi and Ethernet are both off.
    Off,
}

/// The Raspberry Pi Pico's stock USB serial ID, which the ADSBee keeps.
pub const PICO_USB_ID: (u16, u16) = (0x2E8A, 0x000A);
/// Its USB product string. Windows doesn't pass this through (it reports the
/// driver's name), so the Pico ID alone counts too and the data decides.
const PRODUCT: &str = "ADSBee";
/// Page address on the ADSBee's own access point (the ESP32's default).
const HOTSPOT_URL: &str = "http://192.168.4.1/";
/// Longest console line kept; anything longer is binary, not a reply.
const MAX_LINE: usize = 256;

pub fn is_adsbee(usb: &UsbPortInfo) -> bool {
    (usb.vid, usb.pid) == PICO_USB_ID || usb.product.as_deref().is_some_and(|p| p.contains(PRODUCT))
}

/// `enabled,ssid,password,channel` from `AT+WIFI_AP?` → SSID and password.
fn parse_wifi_ap(reply: &str) -> Option<(String, String)> {
    let (_enabled, rest) = reply.split_once(',')?;
    let (ssid_password, _channel) = rest.rsplit_once(',')?;
    let (ssid, password) = ssid_password.split_once(',')?;
    Some((ssid.to_string(), password.to_string()))
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Section {
    #[default]
    None,
    Ethernet,
    Station,
}

/// Collects the console's text replies from between MAVLink frames.
#[derive(Default)]
pub struct Console {
    line: Vec<u8>,
    section: Section,
    ethernet_ip: Option<Ipv4Addr>,
    station_ip: Option<Ipv4Addr>,
    /// From the last `AT+NETWORK_INFO?` reply; `None` until one is complete.
    ap_enabled: Option<bool>,
    /// SSID and password from `AT+WIFI_AP?`.
    ap: Option<(String, String)>,
    page: Option<SettingsPage>,
    /// The console's output protocol before PingView changed it.
    original_protocol: Option<String>,
}

impl Console {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds raw bytes; returns the settings page when it changes.
    pub fn push(&mut self, bytes: &[u8]) -> Option<SettingsPage> {
        let mut changed = None;
        for &b in bytes {
            match b {
                b'\n' => {
                    let line = String::from_utf8_lossy(&self.line).into_owned();
                    self.line.clear();
                    if self.line_done(&line) {
                        changed = self.page.clone();
                    }
                }
                b'\r' => {}
                b'\t' | 0x20..=0x7E if self.line.len() < MAX_LINE => self.line.push(b),
                // Binary (a MAVLink frame) or an overlong run: not a reply line.
                _ => self.line.clear(),
            }
        }
        changed
    }

    /// Handles one complete line; true if the settings page changed.
    fn line_done(&mut self, line: &str) -> bool {
        if line.contains("Ethernet: ") {
            self.section = Section::Ethernet;
            self.ethernet_ip = None;
        } else if line.contains("WiFi Station: ") {
            self.section = Section::Station;
            self.station_ip = None;
        } else if let Some((_, rest)) = line.split_once("WiFi Access Point: ") {
            // Last block of the AT+NETWORK_INFO? reply.
            self.section = Section::None;
            self.ap_enabled = Some(rest.trim() == "ENABLED");
            return self.resolve();
        } else if let Some((_, rest)) = line.split_once("IP Address: ") {
            let ip = rest.trim().parse::<Ipv4Addr>().ok().filter(|ip| !ip.is_unspecified());
            match self.section {
                Section::Ethernet => self.ethernet_ip = ip,
                Section::Station => self.station_ip = ip,
                Section::None => {}
            }
        } else if let Some((_, rest)) = line.split_once("PROTOCOL_OUT=CONSOLE,") {
            // Only the first answer predates our change.
            let valid = !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
            if self.original_protocol.is_none() && valid {
                self.original_protocol = Some(rest.to_string());
            }
        } else if let Some((_, rest)) = line.split_once("WIFI_AP=") {
            if let Some(ap) = parse_wifi_ap(rest) {
                self.ap = Some(ap);
                return self.resolve();
            }
        }
        false
    }

    fn resolve(&mut self) -> bool {
        let Some(page) = self.current() else {
            return false;
        };
        if self.page.as_ref() == Some(&page) {
            return false;
        }
        self.page = Some(page);
        true
    }

    fn current(&self) -> Option<SettingsPage> {
        let ap_enabled = self.ap_enabled?;
        if let Some(ip) = self.station_ip.or(self.ethernet_ip) {
            return Some(SettingsPage::Url { url: format!("http://{ip}/") });
        }
        if !ap_enabled {
            return Some(SettingsPage::Off);
        }
        let (ssid, password) = self.ap.clone()?;
        Some(SettingsPage::Hotspot { ssid, password, url: HOTSPOT_URL.into() })
    }
}

/// How often to ask again for the settings page while there's no link yet.
pub const QUERY_INTERVAL: Duration = Duration::from_secs(30);

const ASK_PROTOCOL: &[u8] = b"AT+PROTOCOL_OUT?\r\n";
const ASK_NETWORK: &[u8] = b"AT+NETWORK_INFO?\r\nAT+WIFI_AP?\r\n";
/// Not followed by `AT+SETTINGS=SAVE`, so a reboot undoes it.
const MAVLINK_ON: &[u8] = b"AT+PROTOCOL_OUT=CONSOLE,MAVLINK2\r\n";

/// Port handle and restore command for the live session, so quitting the
/// app puts the ADSBee back too (the reader thread doesn't get the chance).
static ON_EXIT: Mutex<Option<ExitRestore>> = Mutex::new(None);
type ExitRestore = (Box<dyn Write + Send>, Vec<u8>);

/// Called as the app exits.
pub fn restore_on_exit() {
    if let Some((mut out, restore)) = ON_EXIT.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = out.write_all(&restore);
    }
}

/// A connected ADSBee: switches its output on and puts it back when dropped.
pub struct Session<W: Write> {
    out: W,
    console: Console,
    queried_at: Instant,
    on_exit: bool,
}

impl<W: Write> Session<W> {
    pub fn start(mut out: W, now: Instant) -> io::Result<Self> {
        out.write_all(&[ASK_PROTOCOL, ASK_NETWORK, MAVLINK_ON].concat())?;
        Ok(Self { out, console: Console::new(), queried_at: now, on_exit: false })
    }

    /// Also restores the ADSBee through `handle` if the app quits mid-session.
    pub fn restore_on_exit_via(mut self, handle: Box<dyn Write + Send>) -> Self {
        *ON_EXIT.lock().unwrap_or_else(|e| e.into_inner()) = Some((handle, self.restore_command()));
        self.on_exit = true;
        self
    }

    /// Feeds bytes read from the port; returns the settings page when it changes.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<SettingsPage> {
        let had_original = self.console.original_protocol.is_some();
        let page = self.console.push(bytes);
        if self.on_exit && !had_original && self.console.original_protocol.is_some() {
            if let Some((_, restore)) = ON_EXIT.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                *restore = self.restore_command();
            }
        }
        page
    }

    /// Asks for the settings page again every `QUERY_INTERVAL` until there's a link.
    pub fn poll(&mut self, now: Instant) {
        if matches!(self.console.page, Some(SettingsPage::Url { .. })) {
            return;
        }
        if now.duration_since(self.queried_at) >= QUERY_INTERVAL {
            self.queried_at = now;
            let _ = self.out.write_all(ASK_NETWORK);
        }
    }

    fn restore_command(&self) -> Vec<u8> {
        let protocol = self.console.original_protocol.as_deref().unwrap_or("NONE");
        format!("AT+PROTOCOL_OUT=CONSOLE,{protocol}\r\n").into_bytes()
    }
}

impl<W: Write> Drop for Session<W> {
    fn drop(&mut self) {
        if self.on_exit {
            ON_EXIT.lock().unwrap_or_else(|e| e.into_inner()).take();
        }
        let _ = self.out.write_all(&self.restore_command());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    fn usb(vid: u16, pid: u16, product: Option<&str>) -> UsbPortInfo {
        UsbPortInfo {
            vid,
            pid,
            serial_number: None,
            manufacturer: None,
            product: product.map(str::to_string),
        }
    }

    const NETWORK_INFO_STA: &str = "IP Mode: IPV4\r\n\
        Ethernet: DISABLED\r\n\
        \tNo IPv4 address assigned.\r\n\
        WiFi Station: ENABLED\r\n\
        \tSSID: home\r\n\
        \tIP Address: 192.168.1.42\r\n\
        \tSubnet Mask: 255.255.255.0\r\n\
        \tGateway: 192.168.1.1\r\n\
        WiFi Access Point: ENABLED\r\n\
        \tNum Clients: 0\r\n";

    const NETWORK_INFO_AP_ONLY: &str = "IP Mode: IPV4\r\n\
        Ethernet: DISABLED\r\n\
        \tNo IPv4 address assigned.\r\n\
        WiFi Station: DISABLED\r\n\
        \tSSID: \r\n\
        \tNo IPv4 address assigned.\r\n\
        WiFi Access Point: ENABLED\r\n\
        \tNum Clients: 1\r\n\
        \t\tClient 0 | IP: 192.168.4.2 MAC: aa:bb:cc:dd:ee:ff\r\n";

    const WIFI_AP: &str = "WIFI_AP=1,ADSBee1090-0123456789ab,yummyflowers,6\r\n";

    #[test]
    fn recognises_adsbee_by_name_or_pico_usb_id() {
        assert!(is_adsbee(&usb(0x2E8A, 0x000A, Some("ADSBee 1090"))));
        // Windows reports the driver's name, not the device's.
        assert!(is_adsbee(&usb(0x2E8A, 0x000A, Some("USB Serial Device (COM5)"))));
        assert!(!is_adsbee(&usb(0x0483, 0x5740, Some("STM32 Virtual ComPort"))));
        assert!(!is_adsbee(&usb(0x303A, 0x1001, None)));
    }

    #[test]
    fn station_ip_gives_a_link() {
        let mut c = Console::new();
        let page = c.push(NETWORK_INFO_STA.as_bytes());
        assert_eq!(page, Some(SettingsPage::Url { url: "http://192.168.1.42/".into() }));
    }

    #[test]
    fn ethernet_ip_gives_a_link() {
        let text = NETWORK_INFO_STA
            .replace("Ethernet: DISABLED\r\n\tNo IPv4 address assigned.", "Ethernet: ENABLED\r\n\tIP Address: 10.0.0.9")
            .replace("\tIP Address: 192.168.1.42\r\n", "\tNo IPv4 address assigned.\r\n");
        let page = Console::new().push(text.as_bytes());
        assert_eq!(page, Some(SettingsPage::Url { url: "http://10.0.0.9/".into() }));
    }

    #[test]
    fn access_point_only_gives_hotspot_details() {
        let mut c = Console::new();
        assert_eq!(c.push(NETWORK_INFO_AP_ONLY.as_bytes()), None, "waits for the hotspot name");
        assert_eq!(
            c.push(WIFI_AP.as_bytes()),
            Some(SettingsPage::Hotspot {
                ssid: "ADSBee1090-0123456789ab".into(),
                password: "yummyflowers".into(),
                url: "http://192.168.4.1/".into(),
            })
        );
    }

    #[test]
    fn everything_off_says_so() {
        let text = NETWORK_INFO_AP_ONLY.replace("WiFi Access Point: ENABLED", "WiFi Access Point: DISABLED");
        assert_eq!(Console::new().push(text.as_bytes()), Some(SettingsPage::Off));
    }

    #[test]
    fn replies_survive_binary_frames_and_split_reads() {
        let mut stream = vec![0xFD, 0x09, 0x00, 0x00, 0x01, 0x01, 0x00, 0x0A, 0x00, 0x00, 0x1B];
        stream.extend_from_slice(NETWORK_INFO_STA.as_bytes());
        stream.extend_from_slice(&[0xFD, 0x26, 0x0D, 0x0A, 0xF6, 0x00]);
        let mut c = Console::new();
        let mut pages = Vec::new();
        for chunk in stream.chunks(7) {
            pages.extend(c.push(chunk));
        }
        assert_eq!(pages, vec![SettingsPage::Url { url: "http://192.168.1.42/".into() }]);
    }

    /// A writer the test can read back after the session drops.
    #[derive(Clone, Default)]
    struct Sent(Arc<Mutex<Vec<u8>>>);

    impl Write for Sent {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Sent {
        fn take(&self) -> String {
            String::from_utf8(std::mem::take(&mut *self.0.lock().unwrap())).unwrap()
        }
    }

    #[test]
    fn session_asks_for_details_then_switches_on_mavlink() {
        let sent = Sent::default();
        let _session = Session::start(sent.clone(), Instant::now()).unwrap();
        assert_eq!(
            sent.take(),
            "AT+PROTOCOL_OUT?\r\nAT+NETWORK_INFO?\r\nAT+WIFI_AP?\r\nAT+PROTOCOL_OUT=CONSOLE,MAVLINK2\r\n"
        );
    }

    #[test]
    fn session_puts_back_the_original_output_when_done() {
        let sent = Sent::default();
        let mut session = Session::start(sent.clone(), Instant::now()).unwrap();
        session.feed(b"PROTOCOL_OUT=CONSOLE,BEAST\r\nPROTOCOL_OUT=COMMS_UART,MAVLINK2\r\nOK\r\n");
        sent.take();
        drop(session);
        assert_eq!(sent.take(), "AT+PROTOCOL_OUT=CONSOLE,BEAST\r\n");
    }

    #[test]
    fn session_switches_output_off_if_original_unknown() {
        let sent = Sent::default();
        let session = Session::start(sent.clone(), Instant::now()).unwrap();
        sent.take();
        drop(session);
        assert_eq!(sent.take(), "AT+PROTOCOL_OUT=CONSOLE,NONE\r\n");
    }

    #[test]
    fn session_asks_again_until_it_has_a_link() {
        let sent = Sent::default();
        let t0 = Instant::now();
        let mut session = Session::start(sent.clone(), t0).unwrap();
        sent.take();
        session.poll(t0 + Duration::from_secs(5));
        assert_eq!(sent.take(), "", "too soon");
        session.poll(t0 + QUERY_INTERVAL);
        assert_eq!(sent.take(), "AT+NETWORK_INFO?\r\nAT+WIFI_AP?\r\n");
        assert!(session.feed(NETWORK_INFO_STA.as_bytes()).is_some());
        session.poll(t0 + QUERY_INTERVAL * 3);
        assert_eq!(sent.take(), "", "has a link now");
    }

    #[test]
    fn only_reports_changes() {
        let mut c = Console::new();
        assert!(c.push(NETWORK_INFO_STA.as_bytes()).is_some());
        assert_eq!(c.push(NETWORK_INFO_STA.as_bytes()), None);
        let joined_later = NETWORK_INFO_STA.replace("192.168.1.42", "192.168.1.77");
        assert_eq!(
            c.push(joined_later.as_bytes()),
            Some(SettingsPage::Url { url: "http://192.168.1.77/".into() })
        );
    }
}
