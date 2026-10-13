use std::time::Duration;

use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};
use windows::Foundation::TimeSpan;

use crate::Fix;

pub fn locate(timeout: Duration) -> Result<Fix, String> {
    let fail = |e: windows::core::Error| format!("Windows location failed: {}", e.message());

    let access = Geolocator::RequestAccessAsync().map_err(fail)?.get().map_err(fail)?;
    if access != GeolocationAccessStatus::Allowed {
        return Err(
            "Location access is off. Turn it on in Settings › Privacy & security › Location, \
             including \"Let desktop apps access your location\"."
                .into(),
        );
    }
    let locator = Geolocator::new().map_err(fail)?;
    // TimeSpan counts 100 ns ticks.
    let ticks = (timeout.as_nanos() / 100) as i64;
    let position = locator
        .GetGeopositionAsyncWithAgeAndTimeout(TimeSpan { Duration: 0 }, TimeSpan { Duration: ticks })
        .map_err(fail)?
        .get()
        .map_err(fail)?;
    let coordinate = position.Coordinate().map_err(fail)?;
    let point = coordinate.Point().map_err(fail)?.Position().map_err(fail)?;
    Ok(Fix {
        lat: point.Latitude,
        lon: point.Longitude,
        accuracy_m: coordinate.Accuracy().ok(),
    })
}
