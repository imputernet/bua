// runtime/src/deterministic/clock.rs
//
// A deterministic clock that can be frozen, stepped, or replayed.
//
// In normal mode: returns real wall time.
// In deterministic mode: returns the frozen baseline time.
// In step mode: advances by a fixed delta on each call (useful for testing).
//
// Exposed to JS as Bua.time.now(), Bua.time.freeze(), Bua.time.step().

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockMode {
    /// Real wall-clock time.
    Live,
    /// Returns a fixed timestamp. Does not advance.
    Frozen,
    /// Advances by `step_us` microseconds on each call.
    Stepping { step_us: u64 },
}

/// Injectable, deterministic time source.
#[derive(Clone, Debug)]
pub struct DeterministicClock {
    inner: Arc<RwLock<ClockInner>>,
}

#[derive(Debug)]
struct ClockInner {
    mode: ClockMode,
    /// Current virtual time in microseconds since UNIX epoch.
    /// In Live mode, this is ignored.
    virtual_us: u64,
    /// Number of times now() has been called.
    call_count: u64,
}

impl DeterministicClock {
    /// Live clock — uses real wall time.
    pub fn live() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ClockInner {
                mode: ClockMode::Live,
                virtual_us: 0,
                call_count: 0,
            })),
        }
    }

    /// Frozen clock — always returns `frozen_at_us`.
    pub fn frozen(frozen_at_us: u64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ClockInner {
                mode: ClockMode::Frozen,
                virtual_us: frozen_at_us,
                call_count: 0,
            })),
        }
    }

    /// Stepping clock — starts at `start_us`, advances by `step_us` each call.
    pub fn stepping(start_us: u64, step_us: u64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ClockInner {
                mode: ClockMode::Stepping { step_us },
                virtual_us: start_us,
                call_count: 0,
            })),
        }
    }

    /// Freeze at the current real time (capture-then-freeze).
    pub fn freeze_now() -> Self {
        let now = real_now_us();
        Self::frozen(now)
    }

    /// Return current time in microseconds since UNIX epoch.
    pub fn now_us(&self) -> u64 {
        let mut inner = self.inner.write();
        inner.call_count += 1;

        match inner.mode {
            ClockMode::Live => real_now_us(),
            ClockMode::Frozen => inner.virtual_us,
            ClockMode::Stepping { step_us } => {
                let t = inner.virtual_us;
                inner.virtual_us += step_us;
                t
            }
        }
    }

    /// Return current time in milliseconds (JS-style Date.now()).
    pub fn now_ms(&self) -> f64 {
        self.now_us() as f64 / 1000.0
    }

    /// Manually advance the virtual clock by `delta`.
    /// No-op in Live mode.
    pub fn advance(&self, delta: Duration) {
        let mut inner = self.inner.write();
        match inner.mode {
            ClockMode::Frozen | ClockMode::Stepping { .. } => {
                inner.virtual_us += delta.as_micros() as u64;
            }
            ClockMode::Live => {}
        }
    }

    /// Set the virtual clock to an absolute time.
    pub fn set_time(&self, timestamp_us: u64) {
        let mut inner = self.inner.write();
        inner.virtual_us = timestamp_us;
    }

    /// Switch to frozen mode at the current virtual time.
    pub fn freeze(&self) {
        let mut inner = self.inner.write();
        let current = match inner.mode {
            ClockMode::Live => real_now_us(),
            _ => inner.virtual_us,
        };
        inner.mode = ClockMode::Frozen;
        inner.virtual_us = current;
    }

    pub fn mode(&self) -> ClockMode {
        self.inner.read().mode
    }

    pub fn is_deterministic(&self) -> bool {
        !matches!(self.inner.read().mode, ClockMode::Live)
    }

    pub fn call_count(&self) -> u64 {
        self.inner.read().call_count
    }

    pub fn current_us(&self) -> u64 {
        self.inner.read().virtual_us
    }
}

fn real_now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_clock_never_advances() {
        let clock = DeterministicClock::frozen(1_000_000);
        assert_eq!(clock.now_us(), 1_000_000);
        assert_eq!(clock.now_us(), 1_000_000);
        assert_eq!(clock.now_us(), 1_000_000);
        assert_eq!(clock.call_count(), 3);
    }

    #[test]
    fn stepping_clock_advances_each_call() {
        let clock = DeterministicClock::stepping(0, 100);
        assert_eq!(clock.now_us(), 0);
        assert_eq!(clock.now_us(), 100);
        assert_eq!(clock.now_us(), 200);
    }

    #[test]
    fn live_clock_is_nondeterministic() {
        let clock = DeterministicClock::live();
        assert!(!clock.is_deterministic());
        let a = clock.now_us();
        let b = clock.now_us();
        assert!(b >= a);
    }

    #[test]
    fn manual_advance() {
        let clock = DeterministicClock::frozen(0);
        clock.advance(Duration::from_secs(1));
        assert_eq!(clock.now_us(), 1_000_000);
    }

    #[test]
    fn freeze_live_clock() {
        let clock = DeterministicClock::live();
        assert!(!clock.is_deterministic());
        clock.freeze();
        assert!(clock.is_deterministic());
        let t1 = clock.now_us();
        let t2 = clock.now_us();
        assert_eq!(t1, t2);
    }
}
