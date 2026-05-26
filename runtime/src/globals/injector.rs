// runtime/src/globals/injector.rs

use bua_core::BuaResult;
use std::sync::Arc;

use crate::deterministic::clock::DeterministicClock;
use crate::ffi::context::JscContext;
use crate::runtime::capability_ctx::CapabilityContext;
use crate::runtime::tool_ctx::ToolContext;
use crate::runtime::trace_ctx::TraceContext;

/// Injects the full `globalThis.Bua` API surface into a JSC context.
pub struct GlobalInjector {
    tools: ToolContext,
    trace: TraceContext,
    caps: CapabilityContext,
    clock: DeterministicClock,
    agent_id: String,
    parent_id: Option<String>,
    bua_version: &'static str,
}

impl GlobalInjector {
    pub fn new(
        tools: ToolContext,
        trace: TraceContext,
        caps: CapabilityContext,
        clock: DeterministicClock,
        agent_id: String,
        parent_id: Option<String>,
    ) -> Self {
        Self {
            tools,
            trace,
            caps,
            clock,
            agent_id,
            parent_id,
            bua_version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// Inject all native functions and run the JS bootstrap.
    pub fn inject(&self, ctx: &mut JscContext) -> BuaResult<()> {
        // Register native bridge functions
        self.register_tool_natives(ctx)?;
        self.register_trace_natives(ctx)?;
        self.register_time_natives(ctx)?;
        self.register_env_natives(ctx)?;
        self.register_memory_natives(ctx)?;
        self.register_random_natives(ctx)?;

        // Run JS bootstrap to assemble the Bua global object
        let bootstrap = self.build_bootstrap_script();
        ctx.eval(&bootstrap, Some("bua://bootstrap"))
            .map_err(|e| bua_core::BuaError::JsEngineInit(e.to_string()))?;

        tracing::debug!(agent_id = %self.agent_id, "Bua globals injected");
        Ok(())
    }

    fn register_tool_natives(&self, ctx: &mut JscContext) -> BuaResult<()> {
        let _tools = self.tools.clone();

        ctx.register_native(
            "__bua_tools_call__",
            Arc::new(move |_ctx, args| {
                let _name = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| crate::ffi::value::JsException::new("tools.call: missing name"))?
                    .to_string();

                let _js_args = args
                    .get(1)
                    .map(|v| v.to_json())
                    .unwrap_or(serde_json::Value::Object(Default::default()));

                // Return stub value — real impl returns a Promise via PromiseBridge
                Ok(crate::ffi::value::JsValue::Undefined)
            }),
        )?;

        let tools2 = self.tools.clone();
        ctx.register_native(
            "__bua_tools_list__",
            Arc::new(move |_ctx, _args| {
                let list = tools2.list();
                let json = serde_json::to_string(&list)
                    .map_err(|e| crate::ffi::value::JsException::new(e.to_string()))?;
                Ok(crate::ffi::value::JsValue::String(json))
            }),
        )?;

        Ok(())
    }

    fn register_trace_natives(&self, ctx: &mut JscContext) -> BuaResult<()> {
        let trace = self.trace.clone();

        ctx.register_native(
            "__bua_trace_log__",
            Arc::new(move |_ctx, args| {
                use bua_core::trace::LogLevel;
                let level_str = args.first().and_then(|v| v.as_str()).unwrap_or("info");
                let msg = args
                    .get(1)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let level = match level_str {
                    "error" => LogLevel::Error,
                    "warn" => LogLevel::Warn,
                    "debug" => LogLevel::Debug,
                    "trace" => LogLevel::Trace,
                    _ => LogLevel::Info,
                };

                trace.log(level, &msg);
                Ok(crate::ffi::value::JsValue::Undefined)
            }),
        )?;

        Ok(())
    }

    fn register_time_natives(&self, ctx: &mut JscContext) -> BuaResult<()> {
        let clock = self.clock.clone();
        ctx.register_native(
            "__bua_time_now__",
            Arc::new(move |_ctx, _args| Ok(crate::ffi::value::JsValue::Number(clock.now_ms()))),
        )?;

        let clock2 = self.clock.clone();
        ctx.register_native(
            "__bua_time_freeze__",
            Arc::new(move |_ctx, _args| {
                clock2.freeze();
                Ok(crate::ffi::value::JsValue::Undefined)
            }),
        )?;

        let clock3 = self.clock.clone();
        ctx.register_native(
            "__bua_time_advance__",
            Arc::new(move |_ctx, args| {
                let ms = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
                clock3.advance(std::time::Duration::from_millis(ms as u64));
                Ok(crate::ffi::value::JsValue::Undefined)
            }),
        )?;

        Ok(())
    }

    fn register_env_natives(&self, ctx: &mut JscContext) -> BuaResult<()> {
        let caps = self.caps.clone();

        ctx.register_native(
            "__bua_env_get__",
            Arc::new(move |_ctx, args| {
                let key = match args.first().and_then(|v| v.as_str()) {
                    Some(k) => k.to_string(),
                    None => return Ok(crate::ffi::value::JsValue::Undefined),
                };

                // Check capability
                use bua_core::Permission;
                if caps.require(&Permission::EnvRead(key.clone())).is_err() {
                    return Ok(crate::ffi::value::JsValue::Undefined);
                }

                match std::env::var(&key) {
                    Ok(val) => Ok(crate::ffi::value::JsValue::String(val)),
                    Err(_) => Ok(crate::ffi::value::JsValue::Undefined),
                }
            }),
        )?;

        Ok(())
    }

    fn register_memory_natives(&self, ctx: &mut JscContext) -> BuaResult<()> {
        // Phase 2: wire to MemoryStore.
        // For now: no-op stubs so the module surface is consistent.
        ctx.register_native(
            "__bua_memory_put__",
            Arc::new(|_, _| Ok(crate::ffi::value::JsValue::Undefined)),
        )?;
        ctx.register_native(
            "__bua_memory_get__",
            Arc::new(|_, _| Ok(crate::ffi::value::JsValue::Undefined)),
        )?;
        ctx.register_native(
            "__bua_memory_del__",
            Arc::new(|_, _| Ok(crate::ffi::value::JsValue::Undefined)),
        )?;
        ctx.register_native(
            "__bua_memory_list__",
            Arc::new(|_, _| Ok(crate::ffi::value::JsValue::String("[]".into()))),
        )?;
        Ok(())
    }

    fn register_random_natives(&self, ctx: &mut JscContext) -> BuaResult<()> {
        // Phase 2: wire to DeterministicRng.
        ctx.register_native(
            "__bua_random_seed__",
            Arc::new(|_, _| Ok(crate::ffi::value::JsValue::Undefined)),
        )?;
        ctx.register_native(
            "__bua_random_next__",
            Arc::new(|_, _| Ok(crate::ffi::value::JsValue::Number(js_random()))),
        )?;
        Ok(())
    }

    fn build_bootstrap_script(&self) -> String {
        let version = self.bua_version;
        let agent_id = &self.agent_id;
        let parent_id = self
            .parent_id
            .as_deref()
            .map(|s| format!(r#""{s}""#))
            .unwrap_or("undefined".into());

        format!(
            r#"
(function() {{
  'use strict';

  if (typeof globalThis.Bua !== 'undefined') return; // Already initialized

  const _tools = {{
    call: async (name, args) => {{
      // Will be replaced with real Promise bridge in Phase 2
      const result = __bua_tools_call__(name, JSON.stringify(args ?? {{}}));
      return {{ output: result, error: null }};
    }},
    list: () => JSON.parse(__bua_tools_list__() || '[]'),
  }};

  const _trace = {{
    log: (level, msg, _meta) => __bua_trace_log__(level, String(msg)),
    info:  (msg) => __bua_trace_log__('info',  String(msg)),
    warn:  (msg) => __bua_trace_log__('warn',  String(msg)),
    error: (msg) => __bua_trace_log__('error', String(msg)),
    debug: (msg) => __bua_trace_log__('debug', String(msg)),
  }};

  const _time = {{
    now:     () => __bua_time_now__(),
    freeze:  () => __bua_time_freeze__(),
    advance: (ms) => __bua_time_advance__(ms),
  }};

  const _env = {{
    get: (key) => __bua_env_get__(key),
    all: () => ({{}}), // Phase 2: enumerate all permitted keys
  }};

  const _memory = {{
    put:  (k, v) => Promise.resolve(__bua_memory_put__(k, JSON.stringify(v))),
    get:  (k) => Promise.resolve(__bua_memory_get__(k)),
    del:  (k) => Promise.resolve(__bua_memory_del__(k)),
    list: (prefix) => Promise.resolve(JSON.parse(__bua_memory_list__(prefix || '') || '[]')),
  }};

  const _random = {{
    seed:   (s) => __bua_random_seed__(s),
    random: () => __bua_random_next__(),
    randInt: (min, max) => Math.floor(__bua_random_next__() * (max - min)) + min,
    uuid:   () => crypto.randomUUID(),
  }};

  const _agent = {{
    id: "{agent_id}",
    parentId: {parent_id},
    spawn: async (config) => {{
      // Phase 2: wire to AgentScheduler via PromiseBridge
      throw new Error('agent.spawn: not yet implemented in this build');
    }},
  }};

  globalThis.Bua = Object.freeze({{
    version: "{version}",
    tools:   Object.freeze(_tools),
    trace:   Object.freeze(_trace),
    time:    Object.freeze(_time),
    env:     Object.freeze(_env),
    memory:  Object.freeze(_memory),
    random:  Object.freeze(_random),
    agent:   Object.freeze(_agent),
  }});

  // Console → trace bridge
  const _console = {{
    log:   (...a) => __bua_trace_log__('info',  a.map(String).join(' ')),
    info:  (...a) => __bua_trace_log__('info',  a.map(String).join(' ')),
    warn:  (...a) => __bua_trace_log__('warn',  a.map(String).join(' ')),
    error: (...a) => __bua_trace_log__('error', a.map(String).join(' ')),
    debug: (...a) => __bua_trace_log__('debug', a.map(String).join(' ')),
  }};
  globalThis.console = _console;

  // process.exit compatibility
  globalThis.process = {{ exit: (code) => __bua_trace_log__('info', `process.exit(${{code}})`) }};

}})();
"#
        )
    }
}

/// Placeholder random until DeterministicRng is wired in.
fn js_random() -> f64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut h = DefaultHasher::new();
    SystemTime::now().hash(&mut h);
    (h.finish() as f64) / (u64::MAX as f64)
}
