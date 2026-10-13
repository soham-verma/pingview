use std::cell::RefCell;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};

use crate::{Fix, RunOnMain};

const POLL: Duration = Duration::from_millis(250);

thread_local! {
    // CoreLocation managers must live on the thread with a run loop (the
    // main thread), and must stay alive while updates arrive.
    static MANAGER: RefCell<Option<Retained<CLLocationManager>>> = const { RefCell::new(None) };
}

enum Poll {
    Fix(Fix),
    Denied,
    Waiting,
}

fn start() {
    MANAGER.with(|m| {
        let mut m = m.borrow_mut();
        let manager = m.get_or_insert_with(|| unsafe { CLLocationManager::new() });
        unsafe {
            manager.requestWhenInUseAuthorization();
            manager.startUpdatingLocation();
        }
    });
}

fn poll() -> Poll {
    MANAGER.with(|m| {
        let m = m.borrow();
        let Some(manager) = m.as_ref() else {
            return Poll::Waiting;
        };
        let status = unsafe { manager.authorizationStatus() };
        if status == CLAuthorizationStatus::Denied || status == CLAuthorizationStatus::Restricted {
            return Poll::Denied;
        }
        match unsafe { manager.location() } {
            Some(location) => {
                let c = unsafe { location.coordinate() };
                let accuracy = unsafe { location.horizontalAccuracy() };
                unsafe { manager.stopUpdatingLocation() };
                Poll::Fix(Fix {
                    lat: c.latitude,
                    lon: c.longitude,
                    accuracy_m: (accuracy >= 0.0).then_some(accuracy),
                })
            }
            None => Poll::Waiting,
        }
    })
}

fn stop() {
    MANAGER.with(|m| {
        if let Some(manager) = m.borrow().as_ref() {
            unsafe { manager.stopUpdatingLocation() };
        }
    });
}

/// Runs `f` on the main thread and waits for its result.
fn on_main<T: Send + 'static>(run_on_main: &RunOnMain, f: fn() -> T) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    run_on_main(Box::new(move || {
        let _ = tx.send(f());
    }));
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

pub fn locate(run_on_main: &RunOnMain, timeout: Duration) -> Result<Fix, String> {
    on_main(run_on_main, start).ok_or("Couldn't start location services.")?;
    let deadline = Instant::now() + timeout;
    loop {
        match on_main(run_on_main, poll) {
            Some(Poll::Fix(fix)) => return Ok(fix),
            Some(Poll::Denied) => {
                return Err("Location access is off for PingView. Turn it on in System Settings › \
                            Privacy & Security › Location Services."
                    .into())
            }
            Some(Poll::Waiting) | None => {}
        }
        if Instant::now() > deadline {
            on_main(run_on_main, stop);
            return Err("Couldn't get a location fix in time.".into());
        }
        std::thread::sleep(POLL);
    }
}
