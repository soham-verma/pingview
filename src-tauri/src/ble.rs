//! Finds Bluetooth LE receivers (the XIAO ESP32-C3 bridge) and reads them.
//! The bridge advertises the Nordic UART Service as `PingView-xxxx` and
//! sends the Ping's raw MAVLink as notifications on the TX characteristic.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::StreamExt;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Prefix for Bluetooth receivers wherever a serial port name would go.
pub const SCHEME: &str = "ble://";
const NAME_PREFIX: &str = "PingView-";
const NUS_SERVICE: Uuid = Uuid::from_u128(0x6E400001_B5A3_F393_E0A9_E50E24DCCA9E);
const NUS_TX: Uuid = Uuid::from_u128(0x6E400003_B5A3_F393_E0A9_E50E24DCCA9E);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Forget a receiver this long after its last advert.
const SEEN_FOR: Duration = Duration::from_secs(30);

struct Ble {
    rt: Runtime,
    adapter: Adapter,
    /// Advertised name → peripheral and when it was last heard.
    found: Arc<Mutex<BTreeMap<String, (PeripheralId, Instant)>>>,
}

static BLE: OnceLock<Ble> = OnceLock::new();
static STARTED: Once = Once::new();

/// Starts Bluetooth in the background on first use, so a slow or stuck
/// adapter (e.g. waiting on a macOS permission prompt) never holds up USB.
/// `None` until it's ready, or forever if there's no usable adapter.
fn ble() -> Option<&'static Ble> {
    STARTED.call_once(|| {
        std::thread::spawn(|| match start() {
            Ok(ble) => {
                let _ = BLE.set(ble);
            }
            Err(e) => eprintln!("Bluetooth unavailable, Bluetooth receivers won't be found: {e}"),
        });
    });
    BLE.get()
}

fn start() -> Result<Ble, Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("ble")
        .enable_all()
        .build()?;
    let adapter = rt.block_on(async {
        let manager = Manager::new().await?;
        let adapter = manager.adapters().await?.into_iter().next().ok_or("no Bluetooth adapter")?;
        adapter.start_scan(ScanFilter { services: vec![NUS_SERVICE] }).await?;
        Ok::<_, Box<dyn std::error::Error>>(adapter)
    })?;
    let found = Arc::new(Mutex::new(BTreeMap::new()));
    rt.spawn(watch_adverts(adapter.clone(), found.clone()));
    Ok(Ble { rt, adapter, found })
}

async fn watch_adverts(adapter: Adapter, found: Arc<Mutex<BTreeMap<String, (PeripheralId, Instant)>>>) {
    let Ok(mut events) = adapter.events().await else {
        return;
    };
    while let Some(event) = events.next().await {
        let id = match event {
            CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => id,
            _ => continue,
        };
        let Ok(peripheral) = adapter.peripheral(&id).await else {
            continue;
        };
        let Ok(Some(props)) = peripheral.properties().await else {
            continue;
        };
        if let Some(name) = props.local_name.filter(|n| n.starts_with(NAME_PREFIX)) {
            found.lock().unwrap_or_else(|e| e.into_inner()).insert(name, (id, Instant::now()));
        }
    }
}

/// `ble://PingView-xxxx` for each receiver advertising recently.
pub fn receivers() -> Vec<String> {
    let Some(ble) = ble() else {
        return Vec::new();
    };
    let mut found = ble.found.lock().unwrap_or_else(|e| e.into_inner());
    found.retain(|_, (_, seen)| seen.elapsed() < SEEN_FOR);
    found.keys().map(|name| format!("{SCHEME}{name}")).collect()
}

pub fn is_ble(name: &str) -> bool {
    name.starts_with(SCHEME)
}

/// An open connection; reads return the Ping's bytes as they arrive.
pub struct BleLink {
    data: Receiver<Vec<u8>>,
    leftover: Vec<u8>,
    timeout: Duration,
    peripheral: Peripheral,
    reader: JoinHandle<()>,
}

/// Connects to `ble://PingView-xxxx` and subscribes to its data.
pub fn connect(name: &str, read_timeout: Duration) -> io::Result<BleLink> {
    let ble = ble().ok_or_else(|| io::Error::other("Bluetooth is unavailable"))?;
    let local_name = name.strip_prefix(SCHEME).unwrap_or(name);
    let id = ble
        .found
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(local_name)
        .map(|(id, _)| id.clone())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{local_name} isn't in range")))?;
    let (tx, data) = mpsc::channel();
    let (peripheral, reader) = ble
        .rt
        .block_on(async {
            let peripheral = ble.adapter.peripheral(&id).await?;
            tokio::time::timeout(CONNECT_TIMEOUT, peripheral.connect())
                .await
                .map_err(|_| btleplug::Error::TimedOut(CONNECT_TIMEOUT))??;
            peripheral.discover_services().await?;
            let characteristic = peripheral
                .characteristics()
                .into_iter()
                .find(|c| c.uuid == NUS_TX)
                .ok_or_else(|| btleplug::Error::Other("not a PingView receiver (no UART service)".into()))?;
            // Open the stream before subscribing so the first bytes aren't lost.
            let mut notifications = peripheral.notifications().await?;
            peripheral.subscribe(&characteristic).await?;
            let reader = ble.rt.spawn(async move {
                while let Some(n) = notifications.next().await {
                    if n.uuid == NUS_TX && tx.send(n.value).is_err() {
                        break;
                    }
                }
            });
            Ok::<_, btleplug::Error>((peripheral, reader))
        })
        .map_err(io::Error::other)?;
    Ok(BleLink {
        data,
        leftover: Vec::new(),
        timeout: read_timeout,
        peripheral,
        reader,
    })
}

impl Read for BleLink {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.leftover.is_empty() {
            match self.data.recv_timeout(self.timeout) {
                Ok(bytes) => self.leftover = bytes,
                Err(RecvTimeoutError::Timeout) => return Err(io::ErrorKind::TimedOut.into()),
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
            }
        }
        let n = buf.len().min(self.leftover.len());
        buf[..n].copy_from_slice(&self.leftover[..n]);
        self.leftover.drain(..n);
        Ok(n)
    }
}

impl Drop for BleLink {
    fn drop(&mut self) {
        self.reader.abort();
        if let Some(ble) = ble() {
            let peripheral = self.peripheral.clone();
            // Free the receiver for the next connection (it serves one app).
            let _ = ble.rt.block_on(async {
                tokio::time::timeout(Duration::from_secs(2), peripheral.disconnect()).await
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a powered bridge in range: `cargo test ble_smoke -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn ble_smoke() {
        let deadline = Instant::now() + Duration::from_secs(15);
        let name = loop {
            if let Some(name) = receivers().into_iter().next() {
                break name;
            }
            assert!(Instant::now() < deadline, "no PingView receiver advertising");
            std::thread::sleep(Duration::from_millis(500));
        };
        println!("found {name}");
        let mut link = connect(&name, Duration::from_millis(200)).expect("connect");
        println!("connected and subscribed");
        let pipeline = crate::pipeline::Pipeline::new(None, false, Default::default());
        pipeline.start_link(&name);
        let mut buf = [0u8; 256];
        let mut total = 0;
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            match link.read(&mut buf) {
                Ok(n) => {
                    total += n;
                    pipeline.ingest(&buf[..n], 0.0, chrono::Utc::now());
                }
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) => panic!("{e}"),
            }
        }
        let snap = pipeline.tick(0.0);
        println!("{total} bytes in 10 s; identified as a Ping: {}", pipeline.identified());
        println!("receiver: {}", snap["receiver"]);
        println!("aircraft: {}", snap["aircraft"].as_array().map_or(0, Vec::len));
    }
}
