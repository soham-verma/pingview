use std::thread;
use std::time::{Duration, Instant};

use zbus::blocking::Connection;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

use crate::Fix;

const DESKTOP_ID: &str = "io.github.soham-verma.pingview";
/// GeoClue accuracy levels: 4 = city, 6 = street, 8 = exact.
const ACCURACY_STREET: u32 = 6;

#[zbus::proxy(
    interface = "org.freedesktop.GeoClue2.Manager",
    default_service = "org.freedesktop.GeoClue2",
    default_path = "/org/freedesktop/GeoClue2/Manager"
)]
trait Manager {
    fn get_client(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(interface = "org.freedesktop.GeoClue2.Client", default_service = "org.freedesktop.GeoClue2")]
trait Client {
    fn start(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_desktop_id(&self, id: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_requested_accuracy_level(&self, level: u32) -> zbus::Result<()>;
    #[zbus(property)]
    fn location(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(interface = "org.freedesktop.GeoClue2.Location", default_service = "org.freedesktop.GeoClue2")]
trait Location {
    #[zbus(property)]
    fn latitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn longitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn accuracy(&self) -> zbus::Result<f64>;
}

pub fn locate(timeout: Duration) -> Result<Fix, String> {
    let unavailable = |e: zbus::Error| {
        format!("Location service (GeoClue) unavailable — set your location on the map instead. ({e})")
    };
    let conn = Connection::system().map_err(unavailable)?;
    let client_path = ManagerProxyBlocking::new(&conn)
        .map_err(unavailable)?
        .get_client()
        .map_err(unavailable)?;
    let client = ClientProxyBlocking::builder(&conn)
        .path(client_path)
        .map_err(unavailable)?
        .cache_properties(CacheProperties::No)
        .build()
        .map_err(unavailable)?;
    client.set_desktop_id(DESKTOP_ID).map_err(unavailable)?;
    client
        .set_requested_accuracy_level(ACCURACY_STREET)
        .map_err(unavailable)?;
    client.start().map_err(|e| format!("Location access was refused ({e})."))?;

    let deadline = Instant::now() + timeout;
    let result = loop {
        if let Ok(path) = client.location() {
            if path.as_str() != "/" {
                let location = LocationProxyBlocking::builder(&conn)
                    .path(path)
                    .and_then(|b| b.cache_properties(CacheProperties::No).build())
                    .map_err(unavailable)?;
                break Ok(Fix {
                    lat: location.latitude().map_err(unavailable)?,
                    lon: location.longitude().map_err(unavailable)?,
                    accuracy_m: location.accuracy().ok(),
                });
            }
        }
        if Instant::now() > deadline {
            break Err("Couldn't get a location fix in time.".to_string());
        }
        thread::sleep(Duration::from_millis(250));
    };
    let _ = client.stop();
    result
}
