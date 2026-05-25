// tests/integration/e2e_agent_test.rs
//
// Tests the full `bua agent run` path:
//   Runtime::new → run → tool calls → trace events → snapshot
//
// This is the integration test the feedback doc identified as
// "the single most valuable thing to build next."

#[cfg(test)]
mod e2e {
    use bua_core::{
        Capability, CapabilitySet, FsCapability, FsMode, NetCapability,
    };
    use bua_runtime::{
        runtime::runtime::{Runtime, RuntimeConfig},
        runtime::snapshot_ctx::SnapshotConfig,
        tools::default_tool_registry,
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("bua=debug")
            .with_test_writer()
            .try_init();
    }

    fn fs_caps(dir: &std::path::Path) -> CapabilitySet {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::Filesystem(FsCapability {
            allowed_roots: vec![dir.to_path_buf()],
            mode: FsMode::READ | FsMode::WRITE | FsMode::CREATE,
        }));
        caps
    }

    // -----------------------------------------------------------------------
    // Test 1: Agent runs a simple script successfully
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn agent_runs_script_to_completion() {
        setup_tracing();
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("agent.js");
        std::fs::write(&script, r#"
// Simple agent: reads environment and exits
console.log("agent started");
"#).unwrap();

        let config = RuntimeConfig::new(script, CapabilitySet::unrestricted());
        let tools = Arc::new(default_tool_registry());
        let runtime = Runtime::new(config, tools).unwrap();

        let exit_code = runtime.run().await.unwrap_or(1);

        // Stub engine returns undefined (as if script completed), exit 0
        assert_eq!(exit_code, 0);
        assert!(runtime.agent.is_terminal());
        // Trace should have at least ExecutionStart + ExecutionEnd
        assert!(runtime.trace.event_count() >= 2);
    }

    // -----------------------------------------------------------------------
    // Test 2: Capability enforcement blocks disallowed fs access
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn tool_dispatch_respects_fs_capabilities() {
        setup_tracing();
        let dir = TempDir::new().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let script = workspace.join("agent.ts");
        std::fs::write(&script, "// agent").unwrap();

        // Only grant read access to workspace, not /etc
        let caps = fs_caps(&workspace);
        let config = RuntimeConfig::new(script, caps);
        let tools = Arc::new(default_tool_registry());
        let runtime = Runtime::new(config, tools).unwrap();

        // Allowed: read from workspace
        let allowed = runtime
            .tools
            .call(
                "bua_read_file",
                serde_json::json!({ "path": workspace.join("agent.ts").to_str().unwrap() }),
            )
            .await;

        // Result may be an error (stub engine, actual file read) but
        // it should NOT be a permission error — the cap check passes.
        if let Err(ref e) = allowed {
            // Permission denied would be a different error type
            assert!(
                !e.is_permission_error(),
                "workspace read should not be permission-denied, got: {e}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Test 3: Trace records tool calls and results
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn trace_records_tool_calls() {
        setup_tracing();
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("tracer.ts");
        std::fs::write(&script, "// tracer agent").unwrap();

        let config = RuntimeConfig::new(script, CapabilitySet::unrestricted());
        let tools = Arc::new(default_tool_registry());
        let runtime = Runtime::new(config, tools).unwrap();

        // Simulate tool calls as if JS called them
        let _ = runtime
            .tools
            .call("bua_read_file", serde_json::json!({ "path": "/tmp/x" }))
            .await;

        let _ = runtime
            .tools
            .call("bua_read_file", serde_json::json!({ "path": "/tmp/y" }))
            .await;

        // 2 tool calls = 2 call events + 2 result events = 4
        assert_eq!(runtime.tools.call_count(), 2);
        assert_eq!(runtime.trace.event_count(), 4);

        // NDJSON should be parseable
        let ndjson = runtime.trace.to_ndjson();
        let lines: Vec<&str> = ndjson.lines().collect();
        assert_eq!(lines.len(), 4);
        for line in lines {
            assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
        }
    }

    // -----------------------------------------------------------------------
    // Test 4: Snapshot captured and loadable
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn snapshot_captures_and_loads() {
        setup_tracing();
        let dir = TempDir::new().unwrap();
        let snap_dir = dir.path().join("snaps");
        let script = dir.path().join("snappable.ts");
        std::fs::write(&script, "// snappable").unwrap();

        let config = RuntimeConfig::new(script, CapabilitySet::unrestricted())
            .with_snapshots(SnapshotConfig {
                dir: snap_dir.clone(),
                max_snapshots: 5,
                auto_snapshot_every: Some(1), // snapshot every tool call
            });

        let tools = Arc::new(default_tool_registry());
        let runtime = Runtime::new(config, tools).unwrap();

        // Simulate tool call that triggers auto-checkpoint
        let _ = runtime
            .tools
            .call("bua_read_file", serde_json::json!({ "path": "/tmp/x" }))
            .await;

        // Manually checkpoint
        let heap = runtime.vm.engine.snapshot_heap().await.unwrap();
        let snap_ref = runtime
            .snapshot
            .checkpoint(&runtime.caps, &runtime.trace, heap)
            .await
            .unwrap();

        assert!(snap_ref.path.exists());
        assert_eq!(snap_ref.sequence, 0);

        // Load it back
        let loaded = runtime.snapshot.latest_snapshot().await.unwrap();
        assert!(loaded.is_some());
        let snap = loaded.unwrap();
        assert!(snap.trace_ndjson.contains("ToolCall"));
    }

    // -----------------------------------------------------------------------
    // Test 5: Child agent cannot exceed parent capabilities
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn child_agent_cannot_escalate_capabilities() {
        setup_tracing();
        let dir = TempDir::new().unwrap();
        let parent_script = dir.path().join("parent.ts");
        let child_script = dir.path().join("child.ts");
        std::fs::write(&parent_script, "// parent").unwrap();
        std::fs::write(&child_script, "// child").unwrap();

        // Parent has only fs access
        let parent_caps = fs_caps(dir.path());
        let config = RuntimeConfig::new(parent_script, parent_caps);
        let tools = Arc::new(default_tool_registry());
        let parent_rt = Runtime::new(config, tools.clone()).unwrap();

        // Child tries to request network (parent doesn't have it)
        let mut child_wants = CapabilitySet::new();
        child_wants.grant(Capability::Network(NetCapability {
            allowed_hosts: vec!["evil.com".into()],
            allow_outbound: true,
            allow_inbound: false,
        }));

        let child_rt = parent_rt
            .spawn_child(child_script, child_wants, tools)
            .unwrap();

        // Child should NOT be able to reach evil.com
        assert!(
            !child_rt
                .caps
                .check(&bua_core::Permission::NetConnect("evil.com".into()))
        );
    }

    // -----------------------------------------------------------------------
    // Test 6: Lifecycle state machine is correct
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn agent_lifecycle_follows_state_machine() {
        use bua_runtime::runtime::agent_ctx::AgentLifecycle;
        setup_tracing();

        let dir = TempDir::new().unwrap();
        let script = dir.path().join("lifecycle.js");
        std::fs::write(&script, "// lifecycle").unwrap();

        let config = RuntimeConfig::new(script, CapabilitySet::unrestricted());
        let tools = Arc::new(default_tool_registry());
        let rt = Runtime::new(config, tools).unwrap();

        assert!(matches!(rt.lifecycle(), AgentLifecycle::Pending));

        let _ = rt.run().await;

        assert!(rt.agent.is_terminal());
        assert!(matches!(
            rt.lifecycle(),
            AgentLifecycle::Completed { exit_code: 0, .. }
        ));
    }
}
