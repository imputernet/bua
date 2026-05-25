# Changelog

All notable changes to Bua are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Bua follows [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

### Planned for 0.2.0
- Full JSC eval wired end-to-end (no stub mode required)
- SWC TypeScript transpilation (replaces heuristic strip)
- `Bua.tools.call()` → real Promise bridge
- `Bua.agent.spawn()` → real AgentScheduler dispatch
- `fetch()` global (reqwest-backed)
- `setTimeout` / `setInterval` wired to EventLoop
- Full snapshot restore (`bua replay` executing real JS)
- `bua:memory` KV persistence across runs
- Deterministic RNG (`Bua.random.seed()`)

---

## [0.1.0] — 2026-05-25

### Initial Release

Bua is an AI-native deterministic JavaScript/TypeScript runtime for autonomous agents.

#### Architecture
- Capability-secure execution model (deny by default, revocable, auditable)
- JSC engine on a dedicated OS thread bridged to Tokio via typed channels (`!Send` correctly enforced)
- `JsValue` as the sole cross-boundary type — no raw JSC pointers outside `ffi/`
- Real `JSValueProtect` / `JSValueUnprotect` GC ownership via Rust `Drop`
- Real `JSObjectCallAsFunction` wiring completing the Promise bridge
- Real `JSObjectMakeDeferredPromise` for deferred Promise creation

#### Core Systems
- **CapabilitySet** — granular (fs/net/subprocess/env/agent), revocable with generation counter, wildcard host matching
- **ExecutionTrace** — append-only NDJSON, every tool call + permission check + lifecycle event recorded
- **LayeredSnapshot** — 6 strata (VM/Capability/Trace/Tool/Scheduler/Memory), CRC32 integrity, forward-compatible, binary framed
- **Promise bridge** — `ResolutionQueue` (anti-reentrancy), `PromiseBridge` (Tokio task → JS Promise), `JsPromiseFuture` (JS Promise → Rust Future)
- **Deterministic mode** — `DeterministicClock` (frozen/stepping/live), `ReplayEngine` (tool playback + divergence detection), `IoInterceptor` (write suppression, clock injection)
- **AgentScheduler** — semaphore-bounded concurrency, agent lifecycle state machine, timeout enforcement
- **Runtime context hierarchy** — VM / Agent / Capability / Tool / Trace / Snapshot per agent
- **ModuleGraph** — ESM dependency DAG, topological evaluation order, cycle detection
- **ModuleResolver** — relative/absolute/`bua:*` resolution, 7-extension probing, index files
- **8 built-in modules** — `bua:fs`, `bua:env`, `bua:tools`, `bua:agent`, `bua:trace`, `bua:time`, `bua:memory`, `bua:random`
- **GlobalInjector** — `globalThis.Bua` assembled from native bridge functions + JS bootstrap
- **RuntimeMetrics** — lock-free atomics, latency histograms, NDJSON export

#### CLI
- `bua run app.ts --allow-fs=./workspace --allow-net=api.openai.com`
- `bua agent run research.ts --deterministic --snapshot=run.bsnap`
- `bua replay run.bsnap --verify`
- `bua check file.ts`
- `bua info`
- `bua metrics`

#### Release Targets
- Linux x86_64 (musl — static binary)
- Linux arm64 (musl — static binary)
- macOS x86_64
- macOS arm64 (Apple Silicon)
- Windows x86_64

#### Distribution
- GitHub Releases (binary tarballs + checksums)
- GHCR container (`ghcr.io/imputernet/bua`)
- npm (`bua.js`) with optional platform-specific binary packages

[Unreleased]: https://github.com/imputernet/bua/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/imputernet/bua/releases/tag/v0.1.0
