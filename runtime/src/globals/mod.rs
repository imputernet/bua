// runtime/src/globals/mod.rs
//
// Bua global object injector.
//
// Injects `globalThis.Bua` into every JS context with the full API surface:
//
//   Bua.tools.call(name, args)  -> Promise<result>
//   Bua.tools.list()            -> [{name, description, parameters}]
//   Bua.agent.spawn(config)     -> Promise<{id, result}>
//   Bua.agent.id                -> string
//   Bua.trace.log(level, msg)   -> void
//   Bua.time.now()              -> number
//   Bua.time.freeze()           -> void
//   Bua.time.advance(ms)        -> void
//   Bua.env.get(key)            -> string | undefined
//   Bua.memory.put(k, v)        -> Promise<void>
//   Bua.memory.get(k)           -> Promise<unknown>
//   Bua.random.seed(s)          -> void
//   Bua.random.random()         -> number
//   Bua.version                 -> string
//
// The bootstrap sequence:
//   1. JscContext::new() initializes JSC
//   2. GlobalInjector::inject(&mut ctx, runtime_refs) called once
//   3. Each API method is registered via ctx.register_native()
//   4. A JS bootstrap script assembles the Bua object from the natives
//
// The JS bootstrap script approach (vs pure C++ object construction) keeps
// the API definition in JS where it's readable and testable.

pub mod injector;

pub use injector::GlobalInjector;
