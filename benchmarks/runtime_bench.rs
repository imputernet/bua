// benchmarks/runtime_bench.rs
// Run with: cargo bench --package bua-runtime

use bua_core::{CapabilitySet};
use bua_runtime::{
    engine::{JsEngine, JsEngineConfig},
    tools::{default_tool_registry, ToolCall},
};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

fn bench_engine_init(iterations: u32) {
    let start = Instant::now();
    for _ in 0..iterations {
        let engine = JsEngine::new(
            JsEngineConfig::default(),
            CapabilitySet::new(),
        )
        .unwrap();
        black_box(&engine);
        drop(engine);
    }
    let elapsed = start.elapsed();
    println!(
        "engine_init: {} iters in {:.2}ms ({:.1}µs/iter)",
        iterations,
        elapsed.as_millis(),
        elapsed.as_micros() as f64 / iterations as f64
    );
}

fn bench_capability_check(iterations: u32) {
    use bua_core::{Capability, FsCapability, FsMode, Permission};
    use std::path::PathBuf;

    let mut caps = CapabilitySet::new();
    caps.grant(Capability::Filesystem(FsCapability {
        allowed_roots: vec![PathBuf::from("/workspace")],
        mode: FsMode::READ | FsMode::WRITE,
    }));

    let perm = Permission::FsRead(PathBuf::from("/workspace/data.json"));
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(caps.check(black_box(&perm)));
    }
    let elapsed = start.elapsed();
    println!(
        "capability_check: {} iters in {:.2}ms ({:.1}ns/iter)",
        iterations,
        elapsed.as_millis(),
        elapsed.as_nanos() as f64 / iterations as f64
    );
}

#[tokio::main]
async fn bench_tool_dispatch(iterations: u32) {
    let registry = Arc::new(default_tool_registry());
    let caps = CapabilitySet::unrestricted();

    let call = ToolCall {
        name: "bua_read_file".into(),
        args: serde_json::json!({ "path": "/nonexistent_bench_file" }),
        call_id: None,
    };

    let start = Instant::now();
    for _ in 0..iterations {
        let result = registry.dispatch(&call, &caps).await;
        black_box(&result);
    }
    let elapsed = start.elapsed();
    println!(
        "tool_dispatch: {} iters in {:.2}ms ({:.1}µs/iter)",
        iterations,
        elapsed.as_millis(),
        elapsed.as_micros() as f64 / iterations as f64
    );
}

fn main() {
    println!("=== Bua Runtime Benchmarks ===\n");

    bench_engine_init(1_000);
    bench_capability_check(1_000_000);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        bench_tool_dispatch(10_000).await;
    });

    println!("\nDone.");
}
