# Bua 

**An AI-native JavaScript runtime for ai agents.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build](https://github.com/imputernet/bua/actions/workflows/ci.yml/badge.svg)](https://github.com/bua-runtime/bua/actions)

---

## What is Bua?

Bua is not another JavaScript runtime.

Bua is a **capability-secure, deterministic execution engine** designed from the ground up for autonomous AI agents. It uses JavaScript as its execution language — but its core design priorities are:

- **Determinism** — every execution can be recorded and replayed byte-for-byte
- **Capability security** — deny-by-default permissions, revocable at runtime
- **AI-native tooling** — first-class tool calling, agent spawning, structured traces
- **Replayability** — any execution can be snapshotted and restored
- **Observability** — every tool call, permission check, and agent lifecycle event is traced

Bua is **not** a Node.js clone, a Bun alternative, or a browser runtime. It is an autonomous agent substrate.

---

## Quick Start

```bash
# Run a TypeScript agent
bua run agent.ts --allow-fs=./workspace --allow-net=api.openai.com

# Run in deterministic mode (reproducible)
bua run agent.ts --deterministic --snapshot=./run.bsnap

# Replay a recorded execution
bua replay ./run.bsnap

# Run an autonomous agent
bua agent run research.ts --allow-net=* --allow-fs=./output
```

---

## Core Features

### Capability Security

Every operation requires an explicit capability. Deny by default.

```bash
bua run app.ts \
  --allow-fs=./workspace:rw \    # Filesystem: read+write to ./workspace only
  --allow-net=api.openai.com \   # Network: single host only
  --allow-run=git \              # Subprocess: only 'git'
  --allow-env                    # Environment variables
```

Capabilities are:
- **Granular** — scoped to specific paths, hosts, executables
- **Revocable** — can be removed mid-execution
- **Auditable** — every check is logged to the execution trace
- **Non-escalatable** — child agents cannot exceed parent capabilities

### Deterministic Execution

```bash
bua run research.ts --deterministic --seed=42
```

In deterministic mode:
- Time is frozen at a fixed baseline (`Bua.time.now()` returns the same value)
- All tool call results are recorded for replay
- Random number generation is seeded
- Module loading order is deterministic
- Any re-execution produces an identical trace

### Tool Calling

```typescript
import { call, list } from 'bua:tools';

// Call any registered tool
const content = await call('bua_read_file', { path: './data.json' });
const response = await call('bua_http_get', { url: 'https://api.example.com/data' });

// List available tools
const tools = list();
```

### Agent Spawning

```typescript
import { spawn, spawnAll } from 'bua:agent';

// Spawn a child agent (capability-constrained)
const result = await spawn({
  entrypoint: './worker.ts',
  allowNet: ['api.openai.com'],
  timeout: 30_000,
});

// Fan out to multiple agents
const results = await spawnAll([
  { entrypoint: './worker.ts', allowFs: ['./data/chunk1'] },
  { entrypoint: './worker.ts', allowFs: ['./data/chunk2'] },
]);
```

### Snapshots & Replay

```typescript
import { checkpoint } from 'bua:trace';

// Checkpoint current execution state
await checkpoint('after-research-phase');

// Restore later:
// bua replay execution.bsnap --from=after-research-phase
```

### Structured Tracing

```typescript
import * as trace from 'bua:trace';

trace.info('Starting research phase');
trace.warn('Rate limit approaching', { calls: 95, limit: 100 });

// Export trace as NDJSON
const ndjson = trace.exportNdjson();
```

---

## Architecture

```
┌──────────────────────────────────────────────┐
│                  bua CLI                     │
├──────────────────────────────────────────────┤
│              Agent Scheduler                 │
│         (Tokio async, semaphore)             │
├──────────────────────────────────────────────┤
│           Runtime (per agent)                │
│  VM | Agent | Capability | Tool | Trace | Snapshot
├──────────────────────────────────────────────┤
│         Promise Bridge Pipeline              │
│    JS Promise ↔ Rust Future ↔ Tokio Task    │
├──────────────────────────────────────────────┤
│           JSC Engine (thread-safe)           │
│   Dedicated JS thread ← channel → Tokio     │
├──────────────────────────────────────────────┤
│        JavaScriptCore (JSC)                  │
│     C++ bridge / Zig platform layer          │
└──────────────────────────────────────────────┘
```

**Key design decisions:**

- **JSC on a dedicated thread** — JSC is `!Send`. One OS thread per agent, bridged to Tokio via typed channels.
- **No reentrancy** — JS never calls back into JSC during an active eval. All async results queue and drain at safe points.
- **`JsValue` as the FFI boundary** — No raw JSC pointers escape `runtime/src/ffi/`. All ownership is explicit.
- **Capability calculus** — Child agents inherit a strict subset of parent capabilities. No escalation possible.
- **Layered snapshots** — 6 independent strata: VM heap, capabilities, trace, tool log, scheduler state, agent memory. Each is independently restorable.

---

## Built-in Modules

| Module | Description |
|--------|-------------|
| `bua:fs` | Filesystem access (capability-gated) |
| `bua:env` | Environment variables (capability-gated) |
| `bua:tools` | Tool calling interface |
| `bua:agent` | Agent spawning and coordination |
| `bua:trace` | Structured execution tracing |
| `bua:time` | Deterministic time control |
| `bua:memory` | Persistent agent memory (KV) |
| `bua:random` | Deterministic RNG |

---

## Examples

See [`examples/`](examples/) for full working examples:

- [`hello_agent.ts`](examples/hello_agent.ts) — Basic agent with tool calls
- [`multi_agent.ts`](examples/multi_agent.ts) — Fan-out to parallel child agents
- [`deterministic_replay.ts`](examples/deterministic_replay.ts) — Record and replay
- [`capability_sandbox.ts`](examples/capability_sandbox.ts) — Capability isolation demo
- [`autonomous_research.ts`](examples/autonomous_research.ts) — Full autonomous research agent
- [`snapshot_restore.ts`](examples/snapshot_restore.ts) — Snapshot and resume execution

---

## Tech Stack

| Component | Technology |
|-----------|-----------|
| JS Engine | JavaScriptCore (JSC) |
| Async Runtime | Tokio (Rust) |
| Core Runtime | Rust |
| JSC Bridge | C++17 |
| Platform Layer | Zig |
| TypeScript | SWC (strip-only, no bundling) |

---

## Building

### Prerequisites

- Rust 1.78+
- Clang/LLVM (C++17)
- **macOS**: Xcode command line tools (JSC ships with the SDK)
- **Linux**: `libwebkit2gtk-4.1-dev` or `libjavascriptcoregtk-4.1-dev`

```bash
git clone https://github.com/imputernet/bua
cd bua
cargo build --release
```

The build script auto-detects JSC. Without JSC, Bua builds in **stub mode** — all Rust/architecture tests pass but JS evaluation returns `undefined`.

### Environment Variables

```bash
BUA_JSC_PATH=/path/to/jsc/lib  # Override JSC library path
BUA_LOG=debug                   # Log level (trace/debug/info/warn/error)
```

---

## Security

Bua is designed security-first. See [SECURITY.md](SECURITY.md) for the threat model and responsible disclosure process.

---

## License

MIT — see [LICENSE](LICENSE).
