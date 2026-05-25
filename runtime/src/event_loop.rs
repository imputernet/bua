/// event_loop.rs — Bua event loop
///
/// Bridges the Tokio async executor with JSC's microtask queue.
/// The event loop drives:
///   1. JS microtasks (Promise resolution, queueMicrotask)
///   2. Timers (setTimeout, setInterval, clearTimeout)
///   3. I/O callbacks bridged from Tokio
///   4. Tool call responses bridged back into JS
///
/// Architecture: Tokio is the outer executor. JSC is single-threaded per
/// context. We run JSC on a dedicated `tokio::task::spawn_blocking` thread
/// and communicate via channels.

use bua_core::BuaResult;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

/// A task queued on the event loop.
#[derive(Debug)]
pub enum LoopTask {
    /// JS microtask to drain.
    Microtask,
    /// Timer callback ready to fire.
    Timer { id: u64 },
    /// I/O event from Tokio ready for JS callback.
    IoReady { handle: u64, data: Vec<u8> },
    /// Shut down the event loop.
    Shutdown,
}

/// Handle to a registered timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerId(u64);

static NEXT_TIMER_ID: AtomicU64 = AtomicU64::new(1);

/// Timer entry stored in the loop.
struct TimerEntry {
    deadline: Instant,
    interval: Option<Duration>,
    /// False after clearTimeout/clearInterval.
    active: bool,
}

/// The Bua event loop state, running on a single JS thread.
pub struct EventLoop {
    tx: mpsc::Sender<LoopTask>,
    timers: BTreeMap<u64, TimerEntry>,
}

impl EventLoop {
    /// Create the event loop and return (loop, sender for external wakeups).
    pub fn new() -> (Self, mpsc::Receiver<LoopTask>) {
        let (tx, rx) = mpsc::channel(1024);
        (
            Self {
                tx,
                timers: BTreeMap::new(),
            },
            rx,
        )
    }

    /// Register a setTimeout-style timer.
    pub fn set_timeout(&mut self, delay: Duration) -> TimerId {
        let id = NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed);
        self.timers.insert(
            id,
            TimerEntry {
                deadline: Instant::now() + delay,
                interval: None,
                active: true,
            },
        );
        TimerId(id)
    }

    /// Register a setInterval-style timer.
    pub fn set_interval(&mut self, period: Duration) -> TimerId {
        let id = NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed);
        self.timers.insert(
            id,
            TimerEntry {
                deadline: Instant::now() + period,
                interval: Some(period),
                active: true,
            },
        );
        TimerId(id)
    }

    /// Cancel a timer.
    pub fn clear_timer(&mut self, id: TimerId) {
        if let Some(entry) = self.timers.get_mut(&id.0) {
            entry.active = false;
        }
    }

    /// Drive the event loop until shutdown.
    /// In the real build, this calls JSCDrainMicrotasks() before each wait.
    pub async fn run(&mut self, mut rx: mpsc::Receiver<LoopTask>) -> BuaResult<()> {
        loop {
            // Find the earliest active timer deadline.
            let next_deadline = self
                .timers
                .values()
                .filter(|e| e.active)
                .map(|e| e.deadline)
                .min();

            let task = if let Some(deadline) = next_deadline {
                let now = Instant::now();
                if now >= deadline {
                    // Timer already expired — synthesize the task.
                    Some(LoopTask::Microtask) // drain first
                } else {
                    tokio::select! {
                        msg = rx.recv() => msg,
                        _ = tokio::time::sleep_until(deadline) => Some(LoopTask::Microtask),
                    }
                }
            } else {
                rx.recv().await
            };

            match task {
                Some(LoopTask::Shutdown) | None => {
                    tracing::debug!("event loop shutting down");
                    break;
                }
                Some(LoopTask::Microtask) => {
                    // Real: JSCDrainMicrotasks(ctx)
                    self.fire_expired_timers().await;
                }
                Some(LoopTask::Timer { id }) => {
                    if let Some(entry) = self.timers.get_mut(&id) {
                        if entry.active {
                            if let Some(period) = entry.interval {
                                entry.deadline = Instant::now() + period;
                            } else {
                                entry.active = false;
                            }
                        }
                    }
                }
                Some(LoopTask::IoReady { handle, data }) => {
                    tracing::trace!(handle, bytes = data.len(), "io ready");
                    // Real: invoke registered JS callback for this handle.
                }
            }
        }
        Ok(())
    }

    async fn fire_expired_timers(&mut self) {
        let now = Instant::now();
        let expired: Vec<u64> = self
            .timers
            .iter()
            .filter(|(_, e)| e.active && now >= e.deadline)
            .map(|(id, _)| *id)
            .collect();

        for id in expired {
            let tx = self.tx.clone();
            let _ = tx.send(LoopTask::Timer { id }).await;

            if let Some(entry) = self.timers.get_mut(&id) {
                if let Some(period) = entry.interval {
                    entry.deadline = now + period;
                } else {
                    entry.active = false;
                }
            }
        }
    }

    pub fn sender(&self) -> mpsc::Sender<LoopTask> {
        self.tx.clone()
    }
}
