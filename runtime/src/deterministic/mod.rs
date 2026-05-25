// runtime/src/deterministic/mod.rs
//
// Deterministic execution mode: `bua run app.ts --deterministic`
//
// In deterministic mode:
//   - Time is frozen at a recorded baseline (Bua.time.now() always returns the same value)
//   - All tool call results are played back from the ToolStratum of a snapshot
//   - No live I/O occurs (network, filesystem writes are intercepted)
//   - The execution trace is verified against the recorded trace
//   - Any divergence is a detectable error
//
// This makes Bua executions:
//   - Reproducible across machines
//   - Auditable (trace diff shows exactly what changed)
//   - Debuggable (re-run any past execution identically)
//   - Testable (assertions against recorded behavior)

pub mod clock;
pub mod replay;
pub mod interceptor;

pub use clock::DeterministicClock;
pub use replay::{ReplayEngine, ReplayResult, DivergenceError};
pub use interceptor::IoInterceptor;
