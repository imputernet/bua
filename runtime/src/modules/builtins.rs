// runtime/src/modules/builtins.rs
//
// Built-in Bua modules — accessible via `import { ... } from 'bua:*'`.
//
// These are synthetic modules: their source is generated at runtime from
// Rust state rather than loaded from disk. They provide the official
// Bua API surface to JS code.

use std::collections::HashMap;

/// Registry of built-in module sources.
pub struct BuiltinRegistry {
    modules: HashMap<String, String>,
}

impl BuiltinRegistry {
    pub fn new() -> Self {
        let mut reg = Self {
            modules: HashMap::new(),
        };
        reg.register_all();
        reg
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.modules.get(name).map(String::as_str)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }

    fn register(&mut self, name: &str, source: impl Into<String>) {
        self.modules.insert(name.to_string(), source.into());
    }

    fn register_all(&mut self) {
        self.register("fs", BUA_FS_MODULE);
        self.register("env", BUA_ENV_MODULE);
        self.register("trace", BUA_TRACE_MODULE);
        self.register("agent", BUA_AGENT_MODULE);
        self.register("tools", BUA_TOOLS_MODULE);
        self.register("time", BUA_TIME_MODULE);
        self.register("memory", BUA_MEMORY_MODULE);
        self.register("random", BUA_RANDOM_MODULE);
    }
}

impl Default for BuiltinRegistry {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------------
// bua:fs — filesystem access (capability-gated)
// ---------------------------------------------------------------------------

const BUA_FS_MODULE: &str = r#"
/**
 * bua:fs — Capability-gated filesystem access.
 * Requires --allow-fs capability.
 */

const _native = globalThis.Bua?.native ?? {};

/** Read a UTF-8 text file. */
export async function readFile(path) {
  const result = await Bua.tools.call('bua_read_file', { path });
  if (result.error) throw new Error(`bua:fs readFile: ${result.error}`);
  return result.output?.content ?? '';
}

/** Write a UTF-8 text file. */
export async function writeFile(path, content) {
  const result = await Bua.tools.call('bua_write_file', { path, content });
  if (result.error) throw new Error(`bua:fs writeFile: ${result.error}`);
}

/** Check if a path exists. */
export async function exists(path) {
  try {
    await Bua.tools.call('bua_stat', { path });
    return true;
  } catch {
    return false;
  }
}

/** Read a directory's entries. */
export async function readDir(path) {
  const result = await Bua.tools.call('bua_read_dir', { path });
  if (result.error) throw new Error(`bua:fs readDir: ${result.error}`);
  return result.output?.entries ?? [];
}

export default { readFile, writeFile, exists, readDir };
"#;

// ---------------------------------------------------------------------------
// bua:env — environment variable access (capability-gated)
// ---------------------------------------------------------------------------

const BUA_ENV_MODULE: &str = r#"
/**
 * bua:env — Capability-gated environment variable access.
 * Requires --allow-env capability.
 */

/** Get an environment variable. Returns undefined if not set or not permitted. */
export function get(key) {
  return Bua.env?.get(key);
}

/** Get an environment variable or throw if missing. */
export function require(key) {
  const val = get(key);
  if (val === undefined) throw new Error(`Required environment variable '${key}' is not set`);
  return val;
}

/** Get all allowed environment variables as an object. */
export function all() {
  return Bua.env?.all() ?? {};
}

export default { get, require, all };
"#;

// ---------------------------------------------------------------------------
// bua:trace — structured execution tracing
// ---------------------------------------------------------------------------

const BUA_TRACE_MODULE: &str = r#"
/**
 * bua:trace — Structured execution tracing.
 * Events are written to the agent's ExecutionTrace.
 */

const _t = globalThis.Bua?.trace;

export const log = (level, msg, meta) => _t?.log(level, msg, meta);
export const info  = (msg, meta) => log('info', msg, meta);
export const warn  = (msg, meta) => log('warn', msg, meta);
export const error = (msg, meta) => log('error', msg, meta);
export const debug = (msg, meta) => log('debug', msg, meta);

/** Export the trace as NDJSON string. */
export const exportNdjson = () => _t?.exportNdjson?.() ?? '';

/** Annotate the current execution with a label for snapshot identification. */
export const checkpoint = (label) => _t?.checkpoint?.(label);

export default { log, info, warn, error, debug, exportNdjson, checkpoint };
"#;

// ---------------------------------------------------------------------------
// bua:agent — agent spawning and coordination
// ---------------------------------------------------------------------------

const BUA_AGENT_MODULE: &str = r#"
/**
 * bua:agent — Spawn and coordinate child agents.
 * Requires AgentSpawn capability.
 */

/**
 * Spawn a child agent.
 * @param {object} config
 * @param {string} config.entrypoint - Path to the agent script
 * @param {string[]} [config.allowFs] - Filesystem paths to grant
 * @param {string[]} [config.allowNet] - Network hosts to grant
 * @param {number} [config.timeout] - Timeout in milliseconds
 */
export async function spawn(config) {
  return await Bua.agent.spawn(config);
}

/**
 * Spawn multiple agents in parallel and collect results.
 * @param {object[]} configs - Array of spawn configs
 */
export async function spawnAll(configs) {
  return Promise.all(configs.map(spawn));
}

/**
 * Current agent ID.
 */
export const id = () => Bua.agent?.id;

/**
 * Current agent's parent ID (undefined for root agents).
 */
export const parentId = () => Bua.agent?.parentId;

export default { spawn, spawnAll, id, parentId };
"#;

// ---------------------------------------------------------------------------
// bua:tools — tool calling interface
// ---------------------------------------------------------------------------

const BUA_TOOLS_MODULE: &str = r#"
/**
 * bua:tools — Direct tool calling interface.
 */

/**
 * Call a registered tool by name.
 * @param {string} name - Tool name
 * @param {object} args - Tool arguments (JSON-serializable)
 * @returns {Promise<unknown>} Tool result
 */
export async function call(name, args = {}) {
  const result = await Bua.tools.call(name, args);
  if (result.error) throw Object.assign(new Error(result.error), { toolName: name });
  return result.output;
}

/**
 * List all registered tools with their schemas.
 */
export function list() {
  return Bua.tools.list() ?? [];
}

/**
 * Check if a tool is registered.
 */
export function has(name) {
  return list().some(t => t.name === name);
}

export default { call, list, has };
"#;

// ---------------------------------------------------------------------------
// bua:time — deterministic time control
// ---------------------------------------------------------------------------

const BUA_TIME_MODULE: &str = r#"
/**
 * bua:time — Deterministic time control.
 * In --deterministic mode, these functions control the frozen clock.
 */

/** Current time in milliseconds (like Date.now() but determinism-aware). */
export function now() {
  return Bua.time?.now() ?? Date.now();
}

/** Freeze time at the current value. No-op in live mode. */
export function freeze() {
  Bua.time?.freeze();
}

/** Advance virtual time by delta milliseconds. No-op in live mode. */
export function advance(deltaMs) {
  Bua.time?.advance(deltaMs);
}

/** Set virtual time to an absolute timestamp. */
export function set(timestampMs) {
  Bua.time?.set(timestampMs);
}

export default { now, freeze, advance, set };
"#;

// ---------------------------------------------------------------------------
// bua:memory — persistent agent memory (KV store)
// ---------------------------------------------------------------------------

const BUA_MEMORY_MODULE: &str = r#"
/**
 * bua:memory — Persistent agent memory (survives across snapshots).
 * Data is included in the MemoryStratum of snapshots.
 */

/** Store a value under a key. */
export async function put(key, value) {
  return Bua.memory?.put(key, value);
}

/** Retrieve a value by key. Returns undefined if not set. */
export async function get(key) {
  return Bua.memory?.get(key);
}

/** Delete a key. */
export async function del(key) {
  return Bua.memory?.del(key);
}

/** List all keys matching an optional prefix. */
export async function list(prefix = '') {
  return Bua.memory?.list(prefix) ?? [];
}

export default { put, get, del, list };
"#;

// ---------------------------------------------------------------------------
// bua:random — deterministic random number generation
// ---------------------------------------------------------------------------

const BUA_RANDOM_MODULE: &str = r#"
/**
 * bua:random — Deterministic RNG.
 * In --deterministic mode, uses a seeded PRNG.
 * In live mode, uses crypto.getRandomValues().
 */

/** Seed the deterministic RNG. Only has effect in --deterministic mode. */
export function seed(s) {
  Bua.random?.seed(s);
}

/** Generate a random float in [0, 1). Deterministic if seeded. */
export function random() {
  return Bua.random?.random() ?? Math.random();
}

/** Generate a random integer in [min, max). */
export function randInt(min, max) {
  return Math.floor(random() * (max - min)) + min;
}

/** Generate a random UUID v4 string. Deterministic if seeded. */
export function uuid() {
  return Bua.random?.uuid() ?? crypto.randomUUID();
}

export default { seed, random, randInt, uuid };
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_registered() {
        let reg = BuiltinRegistry::new();
        for name in &["fs", "env", "trace", "agent", "tools", "time", "memory", "random"] {
            assert!(reg.get(name).is_some(), "missing builtin: bua:{name}");
        }
    }

    #[test]
    fn builtin_source_is_valid_js() {
        let reg = BuiltinRegistry::new();
        // Every builtin source should at least contain 'export'
        for name in reg.names() {
            let src = reg.get(name).unwrap();
            assert!(
                src.contains("export"),
                "bua:{name} source doesn't contain exports"
            );
        }
    }
}
