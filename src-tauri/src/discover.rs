//! Finds Wi-Fi receivers (the XIAO ESP32-C3 bridge) on the local network.
//! They advertise `_pingview._tcp` over mDNS and stream raw MAVLink over TCP.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};

const SERVICE: &str = "_pingview._tcp.local.";
/// Prefix for network receivers wherever a serial port name would go.
pub const SCHEME: &str = "tcp://";
/// Where the bridge is in access-point mode (ESP32 soft-AP default).
pub const AP_ADDRESS: &str = "tcp://192.168.4.1:5760";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `tcp://ip:port`
    pub address: String,
    /// The bridge's short ID (from its MAC), e.g. `a1b2`.
    pub id: Option<String>,
}

/// Fullname → receiver, filled by the browse thread.
static FOUND: OnceLock<Mutex<BTreeMap<String, Found>>> = OnceLock::new();

fn found() -> &'static Mutex<BTreeMap<String, Found>> {
    FOUND.get_or_init(|| {
        start_browsing();
        Mutex::new(BTreeMap::new())
    })
}

fn start_browsing() {
    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => return eprintln!("mDNS unavailable, Wi-Fi receivers need a manual address: {e}"),
    };
    let events = match daemon.browse(SERVICE) {
        Ok(rx) => rx,
        Err(e) => return eprintln!("mDNS browse failed: {e}"),
    };
    thread::spawn(move || {
        let _daemon = daemon; // keep it running
        while let Ok(event) = events.recv() {
            let mut map = found().lock().unwrap_or_else(|e| e.into_inner());
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    // Prefer IPv4: link-local IPv6 needs a scope ID to connect.
                    let Some(ip) = info.get_addresses_v4().into_iter().min() else {
                        continue;
                    };
                    let addr = SocketAddr::new(IpAddr::V4(ip), info.get_port());
                    map.insert(
                        info.get_fullname().to_string(),
                        Found {
                            address: format!("{SCHEME}{addr}"),
                            id: info.get_property_val_str("id").map(str::to_string),
                        },
                    );
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    map.remove(&fullname);
                }
                _ => {}
            }
        }
    });
}

/// Receivers seen on the network so far. The first call starts browsing,
/// so it returns nothing until the next poll.
pub fn receivers() -> Vec<Found> {
    let mut list: Vec<Found> = found().lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect();
    list.dedup_by(|a, b| a.address == b.address);
    list
}

pub fn is_network(name: &str) -> bool {
    name.starts_with(SCHEME)
}

/// Connects to `tcp://host:port` (host may be `pingview-a1b2.local`).
pub fn connect(name: &str) -> std::io::Result<TcpStream> {
    let target = name.strip_prefix(SCHEME).unwrap_or(name);
    let mut last_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no address");
    for addr in target.to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                return Ok(stream);
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn connects_to_a_tcp_address() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let name = format!("{SCHEME}{}", listener.local_addr().unwrap());
        assert!(is_network(&name));
        let mut stream = connect(&name).unwrap();
        stream.write_all(b"x").unwrap();
        assert!(listener.accept().is_ok());
    }

    #[test]
    fn serial_names_are_not_network() {
        assert!(!is_network("/dev/cu.usbmodem1101"));
        assert!(!is_network("COM3"));
        assert!(is_network(AP_ADDRESS));
    }
}
