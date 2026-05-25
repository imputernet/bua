# Contributing to Bua

## Architecture First

Before writing code, read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). Bua has strong architectural invariants that must be preserved:

1. **No unsafe outside `runtime/src/ffi/`** — All JSC interaction is isolated here.
2. **No reentrancy** — Never call JSC while an eval is in progress.
3. **Capability deny-by-default** — New operations must require explicit capabilities.
4. **Child ⊆ Parent** — Child agents can never exceed parent capabilities.
5. **`JsValue` is the only cross-boundary type** — Raw JSC pointers never leave `ffi/`.

## Development Setup

```bash
git clone https://github.com/imputernet/bua
cd bua

# Build (stub mode — no JSC required)
cargo build

# Run tests
cargo test

# Run with real JSC (macOS)
cargo build --release  # build.rs auto-detects JSC on macOS

# Lint
cargo clippy -- -D warnings
cargo fmt --check
```

## Project Structure

```
core/           Shared primitives (CapabilitySet, BuaError, ExecutionTrace)
runtime/
  src/
    ffi/        JSC FFI boundary — ALL unsafe lives here
    runtime/    Context hierarchy (VM/Agent/Capability/Tool/Trace/Snapshot)
    promise/    Promise bridge pipeline
    deterministic/  Deterministic execution (clock, replay, interceptor)
    modules/    ESM graph, resolver, builtins
    globals/    Bua global object injection
    metrics/    Runtime observability
cli/            CLI binary (bua run, bua replay, etc.)
jsc/            C++ JSC bridge + raw bindings
  src/bua_jsc.cpp    Bridge implementation
  include/bua_jsc.h  C API header
  bindings/          Rust FFI declarations
```

## Adding a Tool

1. Implement the `Tool` trait in `runtime/src/tools.rs`
2. Register it in `default_tool_registry()`
3. Add required permissions to `required_permissions()`
4. Write a test that verifies capability enforcement

```rust
struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &str { "bua_my_tool" }
    fn schema(&self) -> &ToolSchema { /* ... */ }
    fn required_permissions(&self) -> Vec<Permission> {
        vec![Permission::FsRead(PathBuf::from("/"))] // example
    }
    fn call(&self, args: Value) -> BoxFuture<BuaResult<Value>> {
        Box::pin(async move { /* ... */ })
    }
}
```

## Adding a Builtin Module

Add the module source string to `runtime/src/modules/builtins.rs` and register it in `BuiltinRegistry::register_all()`.

All builtin modules must:
- Use `Bua.*` native functions only (no direct `__bua_*__` calls from user code)
- Export a default object
- Document capability requirements in JSDoc

## Testing

- Unit tests live next to the code (`#[cfg(test)]` modules)
- Integration tests: `tests/integration/`
- E2E tests: `tests/e2e/` (require JSC)
- Benchmarks: `benchmarks/`

Tests that require JSC: gate with `#[cfg(jsc_available)]`.

All tests must pass in stub mode (no JSC) on CI.

## Pull Request Checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo fmt` applied
- [ ] No new `unsafe` outside `runtime/src/ffi/`
- [ ] New capabilities documented
- [ ] Architectural invariants preserved
- [ ] Tests added for new functionality
