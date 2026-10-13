//! Asks the operating system for the device's location, once.
//!
//! - macOS: CoreLocation (the app bundle needs `NSLocationUsageDescription`)
//! - Windows: Windows.Devices.Geolocation
//! - Linux: GeoClue2 over D-Bus (not installed on every distro)
//!
//! Blocking; call it off the UI thread.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    /// Horizontal accuracy radius in metres, when the OS reports one.
    pub accuracy_m: Option<f64>,
}

/// Runs a closure on the app's main thread (macOS needs CoreLocation there;
/// other platforms ignore it).
pub type RunOnMain = dyn Fn(Box<dyn FnOnce() + Send + 'static>) + Send + Sync;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub fn locate(run_on_main: &RunOnMain, timeout: Duration) -> Result<Fix, String> {
    macos::locate(run_on_main, timeout)
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub fn locate(_run_on_main: &RunOnMain, timeout: Duration) -> Result<Fix, String> {
    windows::locate(timeout)
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub fn locate(_run_on_main: &RunOnMain, timeout: Duration) -> Result<Fix, String> {
    linux::locate(timeout)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn locate(_run_on_main: &RunOnMain, _timeout: Duration) -> Result<Fix, String> {
    Err("Location isn't supported on this platform.".into())
}
