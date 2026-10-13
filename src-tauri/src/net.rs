//! Connection health and the local-only switch.
//!
//! Every outbound request (tiles, adsb.lol/adsb.fi, hexdb, planespotters)
//! asks `allowed()` first and reports back with `ok()` / `fail()`. That
//! gives three guarantees:
//! - local-only mode makes zero network requests;
//! - after a connection failure, requests pause for `RETRY_MS` instead of
//!   piling up while offline (one probe gets through each window);
//! - the UI can show whether online extras are currently available.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// How long to back off after a connection failure before trying again.
const RETRY_MS: i64 = 15_000;
/// Report "offline" only once failures have outlasted any recent success.
const OFFLINE_AFTER_MS: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetStatus {
    Online,
    Offline,
    LocalOnly,
}

#[derive(Default)]
pub struct NetHealth {
    local_only: AtomicBool,
    last_ok: AtomicI64,
    last_fail: AtomicI64,
    last_probe: AtomicI64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl NetHealth {
    pub fn new(local_only: bool) -> Self {
        let h = Self::default();
        h.set_local_only(local_only);
        h
    }

    pub fn set_local_only(&self, on: bool) {
        self.local_only.store(on, Ordering::Relaxed);
    }

    pub fn local_only(&self) -> bool {
        self.local_only.load(Ordering::Relaxed)
    }

    pub fn allowed(&self) -> bool {
        self.allowed_at(now_ms())
    }

    pub fn ok(&self) {
        self.last_ok.store(now_ms(), Ordering::Relaxed);
    }

    /// Call only for connection-level failures (timeout, DNS, refused),
    /// not for HTTP errors: a 404 still means we're online.
    pub fn fail(&self) {
        self.last_fail.store(now_ms(), Ordering::Relaxed);
    }

    pub fn status(&self) -> NetStatus {
        self.status_at(now_ms())
    }

    fn failing(&self) -> bool {
        self.last_fail.load(Ordering::Relaxed) > self.last_ok.load(Ordering::Relaxed)
    }

    pub fn allowed_at(&self, now: i64) -> bool {
        if self.local_only() {
            return false;
        }
        if !self.failing() {
            return true;
        }
        // Backing off: let one probe through per retry window.
        let last = self
            .last_probe
            .load(Ordering::Relaxed)
            .max(self.last_fail.load(Ordering::Relaxed));
        if now - last < RETRY_MS {
            return false;
        }
        self.last_probe
            .compare_exchange(self.last_probe.load(Ordering::Relaxed), now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    }

    pub fn status_at(&self, now: i64) -> NetStatus {
        if self.local_only() {
            NetStatus::LocalOnly
        } else if self.failing() && now - self.last_ok.load(Ordering::Relaxed) > OFFLINE_AFTER_MS {
            NetStatus::Offline
        } else {
            NetStatus::Online
        }
    }

    #[cfg(test)]
    fn mark(&self, ok_at: i64, fail_at: i64) {
        self.last_ok.store(ok_at, Ordering::Relaxed);
        self.last_fail.store(fail_at, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only_blocks_everything() {
        let h = NetHealth::new(true);
        assert!(!h.allowed_at(1_000_000));
        assert_eq!(h.status_at(1_000_000), NetStatus::LocalOnly);
        h.set_local_only(false);
        assert!(h.allowed_at(1_000_000));
        assert_eq!(h.status_at(1_000_000), NetStatus::Online);
    }

    #[test]
    fn backs_off_after_a_failure_then_probes_once_per_window() {
        let h = NetHealth::new(false);
        h.mark(0, 100_000);
        assert!(!h.allowed_at(100_000 + 1_000), "inside the back-off window");
        let probe_at = 100_000 + RETRY_MS + 1;
        assert!(h.allowed_at(probe_at), "one probe after the window");
        assert!(!h.allowed_at(probe_at + 10), "but only one");
        assert!(h.allowed_at(probe_at + RETRY_MS + 1), "next window, next probe");
    }

    #[test]
    fn a_success_clears_the_failure() {
        let h = NetHealth::new(false);
        h.mark(0, 100_000);
        h.mark(100_500, 100_000);
        assert!(h.allowed_at(100_600));
        assert_eq!(h.status_at(100_600), NetStatus::Online);
    }

    #[test]
    fn reports_offline_once_failures_outlast_the_last_success() {
        let h = NetHealth::new(false);
        h.mark(100_000, 105_000);
        assert_eq!(h.status_at(106_000), NetStatus::Online, "a blip right after a success");
        assert_eq!(h.status_at(100_000 + OFFLINE_AFTER_MS + 1), NetStatus::Offline);
    }
}
