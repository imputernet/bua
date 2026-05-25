use anyhow::{Context, Result};
use bua_core::{
    Capability, CapabilitySet, EnvCapability, FsCapability, FsMode, NetCapability,
    SubprocessCapability,
};
use bua_runtime::{
    deterministic::ReplayEngine,
    metrics::RuntimeMetrics,
    runtime::runtime::{Runtime, RuntimeConfig},
    runtime::snapshot_ctx::SnapshotConfig,
    snapshot::LayeredSnapshot,
    tools::default_tool_registry,
    transpiler::Transpiler,
};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(
    name = "bua",
    about = "Bua — AI-native deterministic JavaScript runtime",
    long_about = "An AI-native deterministic JavaScript runtime for autonomous agents.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, global = true, default_value = "info")]
    log: String,

    #[arg(long, global = true, help = "Emit structured JSON logs")]
    json_logs: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Execute a JavaScript or TypeScript file
    Run(RunArgs),
    /// Agent-specific subcommands
    Agent(AgentCommand),
    /// Replay a recorded execution snapshot
    Replay(ReplayArgs),
    /// Check TypeScript syntax
    Check(CheckArgs),
    /// Print runtime info and capability model
    Info,
    /// Emit current runtime metrics as JSON
    Metrics,
}

// ---------------------------------------------------------------------------
// `bua run`
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
struct RunArgs {
    file: PathBuf,

    #[arg(long = "allow-fs", value_name = "PATH[:MODE]")]
    allow_fs: Vec<String>,

    #[arg(long = "allow-net", value_name = "HOST")]
    allow_net: Vec<String>,

    #[arg(long = "allow-run", value_name = "EXECUTABLE")]
    allow_run: Vec<String>,

    #[arg(long)]
    allow_env: bool,

    #[arg(long)]
    allow_all: bool,

    #[arg(long, default_value = "256")]
    max_heap_mib: usize,

    #[arg(long, default_value = "300")]
    timeout_secs: u64,

    #[arg(long, help = "Enable deterministic execution mode")]
    deterministic: bool,

    #[arg(long, value_name = "SEED", help = "RNG seed for deterministic mode")]
    seed: Option<u64>,

    #[arg(long, value_name = "FILE", help = "Write layered snapshot to file")]
    snapshot: Option<PathBuf>,

    #[arg(long, help = "Emit execution trace to stdout (NDJSON)")]
    trace: bool,

    #[arg(long, value_name = "FILE", help = "Write execution trace to file")]
    trace_file: Option<PathBuf>,

    #[arg(long, default_value = "8")]
    max_agents: usize,
}

// ---------------------------------------------------------------------------
// `bua agent`
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
struct AgentCommand {
    #[command(subcommand)]
    cmd: AgentSubcommand,
}

#[derive(Debug, Subcommand)]
enum AgentSubcommand {
    /// Run an agent script
    Run(RunArgs),
    /// List active agents (requires running daemon — Phase 2)
    List,
}

// ---------------------------------------------------------------------------
// `bua replay`
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
struct ReplayArgs {
    snapshot: PathBuf,

    #[arg(long, help = "Verify replay matches recorded trace")]
    verify: bool,

    #[arg(long, help = "Run in strict divergence mode (error on any divergence)")]
    strict: bool,

    #[arg(long, value_name = "LABEL", help = "Restore from a named checkpoint")]
    from: Option<String>,
}

// ---------------------------------------------------------------------------
// `bua check`
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
struct CheckArgs {
    files: Vec<PathBuf>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(&cli.log, cli.json_logs);

    match cli.command {
        Command::Run(args) => cmd_run(args).await,
        Command::Agent(AgentCommand {
            cmd: AgentSubcommand::Run(args),
        }) => cmd_run(args).await,
        Command::Agent(AgentCommand {
            cmd: AgentSubcommand::List,
        }) => cmd_agent_list(),
        Command::Replay(args) => cmd_replay(args).await,
        Command::Check(args) => cmd_check(args),
        Command::Info => cmd_info(),
        Command::Metrics => cmd_metrics(),
    }
}

// ---------------------------------------------------------------------------
// `bua run` / `bua agent run`
// ---------------------------------------------------------------------------

async fn cmd_run(args: RunArgs) -> Result<()> {
    let file = args
        .file
        .canonicalize()
        .with_context(|| format!("cannot find: {}", args.file.display()))?;

    tracing::info!(file = %file.display(), deterministic = args.deterministic, "starting");

    let capabilities = build_capabilities(&args)?;
    let tools = Arc::new(default_tool_registry());
    let metrics = RuntimeMetrics::new();

    // Snapshot config
    let snapshot_config = args.snapshot.as_ref().map(|p| SnapshotConfig {
        dir: p
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf(),
        max_snapshots: 10,
        auto_snapshot_every: None,
    });

    let timeout =
        (args.timeout_secs > 0).then(|| std::time::Duration::from_secs(args.timeout_secs));

    let config = RuntimeConfig {
        entrypoint: file,
        capabilities,
        timeout,
        max_heap_bytes: args.max_heap_mib * 1024 * 1024,
        parent_id: None,
        snapshot_config,
        trace_enabled: true,
    };

    metrics.agent_started();
    let runtime = Runtime::new(config, tools)?;
    let exit_code = runtime.run().await.unwrap_or(1);

    if exit_code == 0 {
        metrics.agent_completed();
    } else {
        metrics.agent_failed();
    }

    // Emit trace if requested
    if args.trace {
        let ndjson = runtime.trace.to_ndjson();
        eprintln!("\n--- execution trace ---");
        eprintln!("{ndjson}");
    }

    if let Some(trace_file) = &args.trace_file {
        let ndjson = runtime.trace.to_ndjson();
        tokio::fs::write(trace_file, ndjson).await?;
        tracing::info!(path = %trace_file.display(), "trace written");
    }

    // Write snapshot if requested
    if let Some(snap_path) = &args.snapshot {
        let heap = runtime.vm.engine.snapshot_heap().await.unwrap_or_default();
        let snap = LayeredSnapshot::new(runtime.execution_id())
            .with_vm(heap)
            .with_capability(runtime.caps.snapshot(), runtime.caps.generation())
            .with_trace(runtime.trace.to_ndjson(), runtime.trace.event_count());
        snap.write_to_file(snap_path).await?;
        println!("Snapshot: {}", snap_path.display());
    }

    tracing::info!(
        exit_code,
        trace_events = runtime.trace.event_count(),
        tool_calls = runtime.tools.call_count(),
        "execution complete"
    );

    std::process::exit(exit_code);
}

// ---------------------------------------------------------------------------
// `bua agent list`
// ---------------------------------------------------------------------------

fn cmd_agent_list() -> Result<()> {
    println!("Agent daemon not running. Start with `bua daemon` (Phase 2).");
    Ok(())
}

// ---------------------------------------------------------------------------
// `bua replay`
// ---------------------------------------------------------------------------

async fn cmd_replay(args: ReplayArgs) -> Result<()> {
    let snap = LayeredSnapshot::read_from_file(&args.snapshot)
        .await
        .with_context(|| format!("failed to load snapshot: {}", args.snapshot.display()))?;

    let header = snap.header.as_ref();
    println!("Snapshot: {}", args.snapshot.display());
    if let Some(h) = header {
        println!("Execution ID : {}", h.execution_id);
        println!("Timestamp    : {} µs", h.timestamp_us);
        println!("Label        : {}", h.label.as_deref().unwrap_or("(none)"));
        println!("Format ver   : {}", h.format_version);
    }

    println!("Strata       : {}", snap.strata_count());
    if let Some(ref t) = snap.trace {
        println!("  trace      : {} events", t.event_count);
    }
    if let Some(ref v) = snap.vm {
        println!("  vm         : {} bytes heap", v.heap_bytes.len());
    }
    if let Some(ref tl) = snap.tool {
        println!("  tool log   : {} calls", tl.call_log.len());
    }
    if let Some(ref m) = snap.memory {
        println!("  memory     : {} keys", m.entries.len());
    }

    if args.verify {
        println!("\nVerifying replay...");

        if snap.tool.is_none() {
            println!("  No tool log in snapshot — nothing to verify.");
            return Ok(());
        }

        let engine = ReplayEngine::from_snapshot(&snap, args.strict);

        // Simulate replaying each recorded tool call
        if let Some(ref tool_log) = snap.tool {
            for record in &tool_log.call_log {
                let args_val: serde_json::Value =
                    serde_json::from_str(&record.args_json).unwrap_or(serde_json::Value::Null);

                let result = engine.intercept_tool_call(&record.name, &args_val);
                match result {
                    Ok(_) => print!("  ✓ call #{}: {}", record.sequence, record.name),
                    Err(ref e) => print!("  ✗ call #{}: DIVERGED — {}", record.sequence, e),
                }
                println!(" ({} µs)", record.duration_us);
            }
        }

        let result = engine.result();
        println!("\nCalls replayed : {}", result.calls_replayed);
        println!("Divergences    : {}", result.divergences.len());
        println!("Deterministic  : {}", result.deterministic);

        if !result.is_clean() {
            anyhow::bail!(
                "Replay verification failed: {} divergences",
                result.divergences.len()
            );
        }

        println!("\n✓ Replay verified — execution is deterministic");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `bua check`
// ---------------------------------------------------------------------------

fn cmd_check(args: CheckArgs) -> Result<()> {
    let t = Transpiler::default();
    let mut errors = 0usize;

    for file in &args.files {
        match std::fs::read_to_string(file) {
            Ok(src) => match t.transpile(&src, &file.to_string_lossy()) {
                Ok(out) => println!("✓ {} ({} µs)", file.display(), out.duration_us),
                Err(e) => {
                    eprintln!("✗ {}: {e}", file.display());
                    errors += 1;
                }
            },
            Err(e) => {
                eprintln!("✗ {}: {e}", file.display());
                errors += 1;
            }
        }
    }

    if errors > 0 {
        anyhow::bail!("{errors} file(s) failed");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `bua info`
// ---------------------------------------------------------------------------

fn cmd_info() -> Result<()> {
    println!("Bua Runtime v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("  JS Engine     : JavaScriptCore (JSC)");
    println!("  Async Runtime : Tokio");
    println!("  Architecture  : {}", std::env::consts::ARCH);
    println!("  OS            : {}", std::env::consts::OS);
    println!();
    println!("Capability flags:");
    println!("  --allow-fs=<path>[:r|rw|rwcd]    Filesystem access");
    println!("  --allow-net=<host|*.domain|*>     Network outbound");
    println!("  --allow-run=<exe>                 Subprocess execution");
    println!("  --allow-env                       Environment variables");
    println!("  --allow-all                       ⚠ Unrestricted (trusted only)");
    println!();
    println!("Deterministic flags:");
    println!("  --deterministic                   Enable deterministic mode");
    println!("  --seed=<N>                        RNG seed");
    println!("  --snapshot=<file>                 Write layered snapshot");
    println!();
    println!("Built-in modules:");
    for name in &[
        "fs", "env", "tools", "agent", "trace", "time", "memory", "random",
    ] {
        println!("  bua:{name}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `bua metrics`
// ---------------------------------------------------------------------------

fn cmd_metrics() -> Result<()> {
    // In a real daemon this would query a running runtime.
    // For now: emit an empty metrics snapshot.
    let m = RuntimeMetrics::new();
    println!("{}", serde_json::to_string_pretty(&m.to_json())?);
    Ok(())
}

// ---------------------------------------------------------------------------
// Capability builder
// ---------------------------------------------------------------------------

fn build_capabilities(args: &RunArgs) -> Result<CapabilitySet> {
    if args.allow_all {
        return Ok(CapabilitySet::unrestricted());
    }

    let mut caps = CapabilitySet::new();

    for fs_arg in &args.allow_fs {
        let (path_str, mode_str) = fs_arg.split_once(':').unwrap_or((fs_arg.as_str(), "rw"));
        let path = PathBuf::from(path_str)
            .canonicalize()
            .with_context(|| format!("--allow-fs path not found: {path_str}"))?;

        let mode = match mode_str {
            "r" => FsMode::READ,
            "rw" => FsMode::READ | FsMode::WRITE | FsMode::CREATE,
            "rwcd" => FsMode::READ | FsMode::WRITE | FsMode::CREATE | FsMode::DELETE,
            other => anyhow::bail!("unknown fs mode '{}' (use r, rw, rwcd)", other),
        };

        caps.grant(Capability::Filesystem(FsCapability {
            allowed_roots: vec![path],
            mode,
        }));
    }

    if !args.allow_net.is_empty() {
        caps.grant(Capability::Network(NetCapability {
            allowed_hosts: args.allow_net.clone(),
            allow_outbound: true,
            allow_inbound: false,
        }));
    }

    if !args.allow_run.is_empty() {
        caps.grant(Capability::Subprocess(SubprocessCapability {
            allowed_executables: args.allow_run.clone(),
        }));
    }

    if args.allow_env {
        caps.grant(Capability::EnvRead(EnvCapability {
            allowed_keys: vec![],
        }));
    }

    Ok(caps)
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

fn init_logging(level: &str, json: bool) {
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    if json {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .compact()
            .init();
    }
}
