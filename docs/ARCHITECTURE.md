# Bua Runtime — Architecture

## Overview

Bua is an AI-native JavaScript/TypeScript runtime. It uses JavaScriptCore (JSC) as the JS engine, Rust for the async runtime and systems layer, C++ for the JSC bridge, and Zig for platform primitives and the build system.

## Layer Diagram

```
┌─────────────────────────────────────────────────────┐
│                    bua CLI                          │  Rust (clap)
├─────────────────────────────────────────────────────┤
│                 Agent Scheduler                     │  Rust (tokio)
│          AgentHandle | ToolRegistry                 │
├────────────────────────┬────────────────────────────┤
│    Permission Guard    │     Execution Trace        │  bua-core (Rust)
│    CapabilitySet       │     Snapshot/Replay        │
├────────────────────────┴────────────────────────────┤
│                   Event Loop                        │  Rust (tokio channels)
│        Timers | I/O callbacks | Microtasks          │
├─────────────────────────────────────────────────────┤
│                  JSC Engine                         │  C++ (JavaScriptCore)
│      eval | eval_module | native functions          │
├─────────────────────────────────────────────────────┤
│               Platform Layer                        │  Zig
│       Allocator | Clock | mmap | OS primitives      │
└─────────────────────────────────────────────────────┘
```

## Key Design Decisions

### 1. JSC over V8
- Ships with macOS/iOS (zero-dependency on Apple targets)
- Lower memory overhead than V8 for embedded use
- Strong JIT: DFG + FTL
- Easier to snapshot heap state (private API)

### 2. Capability-based permissions (deny by default)
Every operation that touches the OS requires an explicit `Capability` in the agent's `CapabilitySet`. Capabilities are:
- **Granular**: scoped to specific paths, hosts, executables
- **Revocable**: can be removed mid-execution (generation counter tracks revocations)
- **Auditable**: every check is logged to the execution trace
- **Non-delegatable**: child agents cannot exceed parent's capability set

### 3. Tokio as outer executor
JSC contexts are not thread-safe. Each agent runs JSC on a dedicated thread, bridged to the Tokio I/O reactor via channels. This gives us:
- True async I/O without blocking the JS thread
- Natural backpressure via channel capacity
- Agent isolation at the OS-thread level

### 4. Tool calling as first-class primitive
Tools are Rust async functions registered in a `ToolRegistry`. From JS they're called via `Bua.tools.call(name, args)`. The bridge:
1. JS calls native function (synchronous JSC callback)
2. Rust sends tool call to Tokio via channel
3. Tokio executes the async tool
4. Result sent back; JS `Promise` resolves

### 5. Deterministic replay
Every tool call's inputs and outputs are recorded in the `ExecutionTrace`. A snapshot captures the JSC heap bytecode + trace. Replay restores the heap and re-feeds recorded tool results instead of making live calls, producing identical output deterministically.

## Module Responsibilities

| Crate/Module | Responsibility |
|---|---|
| `bua-core` | Shared types: `CapabilitySet`, `BuaError`, `AgentId`, `ExecutionTrace` |
| `bua-runtime/engine` | JSC wrapper: eval, native registration, snapshot |
| `bua-runtime/agent` | Agent lifecycle: spawn, message pump, timeout |
| `bua-runtime/scheduler` | Multi-agent concurrency: semaphore, registry |
| `bua-runtime/tools` | Tool trait, registry, built-in tools |
| `bua-runtime/loader` | ESM resolution, TypeScript transpilation, cache |
| `bua-runtime/event_loop` | Timer management, I/O bridge, microtask drain |
| `bua-runtime/snapshot` | Serialize/deserialize execution state |
| `bua-runtime/transpiler` | TypeScript → JavaScript via SWC |
| `bua-runtime/permissions` | Runtime permission enforcement + audit |
| `bua` (cli) | Argument parsing, capability builder, log init |
| `jsc/` (C++) | JSC C API bridge: `BuaContext`, eval, native callbacks |
| `src/platform/` (Zig) | Allocator, clock, mmap, OS ABI |

## Execution Lifecycle

```
bua run app.ts --allow-fs=./workspace --allow-net=api.openai.com
     │
     ├─ Parse CLI flags → CapabilitySet
     ├─ Build ToolRegistry (built-ins + user tools)
     ├─ Create AgentScheduler
     └─ AgentScheduler::spawn(AgentConfig)
          │
          ├─ Acquire semaphore slot
          ├─ JsEngine::new(config, caps)  → JSC context init
          ├─ Loader::load(entrypoint)     → resolve + transpile + cache
          ├─ JsEngine::eval_module()      → JSEvaluateScript()
          │    │
          │    ├─ JS calls Bua.tools.call("bua_read_file", {...})
          │    │    └─ native callback → channel → ToolRegistry::dispatch()
          │    │         ├─ PermissionGuard::require(FsRead(path)) ✓
          │    │         └─ ReadFileTool::call(args) → tokio::fs::read
          │    │              └─ result → channel → Promise.resolve()
          │    │
          │    └─ JS returns / throws
          │
          ├─ ExecutionTrace → NDJSON
          ├─ Snapshot::write_to_file() (if --snapshot flag)
          └─ AgentStatus::Completed { exit_code: 0 }
```

## Roadmap

### Phase 1 — MVP (current)
- [x] Project structure + build system
- [x] Capability model (core)
- [x] Error taxonomy
- [x] Execution trace
- [x] JSC C++ bridge header + implementation
- [x] JS Engine abstraction
- [x] TypeScript transpiler (SWC stub → full SWC)
- [x] ESM module loader + cache
- [x] Tool registry + built-in tools (fs, http)
- [x] Agent lifecycle
- [x] Agent scheduler
- [x] Event loop
- [x] Snapshot/replay (structure)
- [x] CLI (run, check, info, replay)
- [x] Zig platform layer (allocator, clock)

### Phase 2 — Engine
- [ ] Full JSC C++ bridge (real JSEvaluateScript + module loader)
- [ ] SWC integration (replace stub transpiler)
- [ ] Source maps
- [ ] Full ESM dynamic import
- [ ] `fetch()` global (reqwest-backed)
- [ ] `setTimeout` / `setInterval` wired to event loop
- [ ] `TextEncoder` / `TextDecoder`
- [ ] `crypto.subtle` (ring-backed)

### Phase 3 — Agent features
- [ ] Agent IPC (structured inter-agent messaging)
- [ ] Memory persistence (agent state across runs)
- [ ] Full snapshot/restore with JSC heap
- [ ] Replay verification
- [ ] Agent capability delegation (child ⊆ parent)
- [ ] Resource quotas (CPU time, memory, tool calls/sec)

### Phase 4 — Distribution
- [ ] Remote agent execution
- [ ] Distributed tool registry
- [ ] Agent checkpointing
- [ ] Observability export (OTLP traces, metrics)
