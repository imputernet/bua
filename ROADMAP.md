# Bua Roadmap

## Phase 1 — Foundation (Complete)

Architecture and core systems.

- [x] Capability security model (`CapabilitySet`, revocation, audit)
- [x] Typed error taxonomy
- [x] Execution trace (NDJSON, append-only)
- [x] Agent scheduler (Tokio, semaphore-bounded)
- [x] Tool registry (async, permission-gated)
- [x] Runtime context hierarchy (VM/Agent/Capability/Tool/Trace/Snapshot)
- [x] Promise bridge pipeline (JS Promise ↔ Rust Future ↔ Tokio)
- [x] Layered snapshot format (6 strata, CRC32, forward-compatible)
- [x] Deterministic clock (frozen/stepping/live modes)
- [x] Replay engine (tool call playback, divergence detection)
- [x] I/O interceptor (write suppression, clock injection)
- [x] JSC C++ bridge (full C API surface)
- [x] `build.rs` (platform-aware JSC detection)
- [x] Raw FFI bindings (`bua_jsc_sys.rs`)
- [x] ESM module graph (cycle detection, topological eval order)
- [x] Module resolver (relative, absolute, `bua:*` builtins)
- [x] 8 built-in `bua:*` modules
- [x] `globalThis.Bua` injector + JS bootstrap
- [x] Runtime metrics (lock-free, histogram, NDJSON export)
- [x] CLI (`bua run`, `bua check`, `bua info`, `bua replay`)

---

## Phase 2 — Real Execution

Make `bua run hello.ts` work end-to-end with real JSC.

- [ ] Real JSC eval (connect `build.rs` to `JscContext::eval`)
- [ ] SWC TypeScript transpilation (replace heuristic strip)
- [ ] Module loader wired to JSC module callbacks
- [ ] Promise microtask drain after every eval
- [ ] `Bua.tools.call()` → real `PromiseBridge::spawn_tool_call()`
- [ ] `Bua.agent.spawn()` → real `AgentScheduler::spawn()`
- [ ] `fetch()` global (reqwest-backed)
- [ ] `setTimeout` / `setInterval` wired to `EventLoop`
- [ ] `TextEncoder` / `TextDecoder`
- [ ] `crypto.subtle` (ring-backed subset)
- [ ] Source maps for TypeScript stack traces
- [ ] Unhandled Promise rejection tracking + hooks

**Target:** `bua run hello.ts` executes real JavaScript.

---

## Phase 3 — Agent Features

Make autonomous agents viable.

- [ ] `bua:memory` persistence (MemoryStratum survives across runs)
- [ ] Full snapshot restore (VM heap + all strata)
- [ ] `bua replay execution.bsnap` working end-to-end
- [ ] Agent IPC (structured message passing between agents)
- [ ] Deterministic RNG (`Bua.random.seed()`)
- [ ] Tool rate limiting (calls/second per agent)
- [ ] Agent capability delegation (explicit grant API)
- [ ] Child agent result collection (`spawnAll`)
- [ ] Async stack traces
- [ ] Promise debugging instrumentation

**Target:** `bua agent run autonomous_research.ts` works.

---

## Phase 4 — Hardening

Production-grade security and reliability.

- [ ] Snapshot deserialization depth limit (anti-DoS)
- [ ] Symlink resolution in FsCapability
- [ ] JSC heap memory limit enforcement (private API)
- [ ] Agent watchdog (detects stalled agents)
- [ ] Structured error recovery (agent crash → restart)
- [ ] Fuzz testing (snapshot format, capability checks)
- [ ] Security audit
- [ ] `bua run --verify` (capability audit mode — shows what would be used)

---

## Phase 5 — Distribution

Scale beyond a single machine.

- [ ] Remote agent execution protocol
- [ ] Agent state migration
- [ ] Distributed tool registry
- [ ] OTLP trace export
- [ ] Prometheus metrics endpoint
- [ ] Cloud checkpoint storage (S3/GCS)
- [ ] Agent supervision tree

---

## Non-Goals (Permanent)

These will never be in Bua's scope:

- Full Node.js API compatibility
- npm package management
- Browser DOM emulation
- Bundler / build tool
- Legacy CommonJS (CJS) support
- `node_modules` resolution
