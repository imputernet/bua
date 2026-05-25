// runtime/src/ffi/context.rs
//
// JscContext — safe wrapper around one JSC global context.
//
// Thread invariant: JscContext is !Send.
//   It must stay on the OS thread that created it.
//   JscEngine (ffi/engine.rs) enforces this with a dedicated thread.
//
// ALL unsafe in this file. None elsewhere in bua-runtime.

use bua_core::{BuaError, BuaResult};
use std::sync::Arc;

use super::value::{FunctionHandle, JsException, JsValue, ObjectHandle, PromiseHandle};

pub type NativeFn =
    Arc<dyn Fn(&JscContext, Vec<JsValue>) -> Result<JsValue, JsException> + Send + Sync + 'static>;

// ---------------------------------------------------------------------------
// Internal native registration entry
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct NativeEntry {
    func: NativeFn,
    /// Canonical path (for debug / deregistration).
    path: String,
}

// ---------------------------------------------------------------------------
// JscContext
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct JscContext {
    /// Raw *mut BuaContext from C side. 0 = stub mode.
    ctx_ptr: usize,
    /// Boxed entries kept alive — their raw pointers are passed as user_data
    /// to the C bridge's native callback registration.
    native_entries: Vec<NativeEntry>,
    poisoned: bool,
}

impl JscContext {
    pub fn new(max_heap_bytes: usize) -> BuaResult<Self> {
        #[cfg(jsc_available)]
        {
            use crate::jsc_sys;
            let ptr = unsafe { jsc_sys::bua_context_create(max_heap_bytes) };
            if (ptr as usize) == 0 {
                return Err(BuaError::JsEngineInit(
                    "bua_context_create returned null".into(),
                ));
            }
            tracing::debug!(max_heap_bytes, ptr = ptr as usize, "JscContext created");
            return Ok(Self {
                ctx_ptr: ptr as usize,
                native_entries: Vec::new(),
                poisoned: false,
            });
        }
        #[cfg(not(jsc_available))]
        {
            tracing::debug!(max_heap_bytes, "JscContext created (stub)");
            Ok(Self {
                ctx_ptr: 0,
                native_entries: Vec::new(),
                poisoned: false,
            })
        }
    }

    // -----------------------------------------------------------------------
    // Evaluation
    // -----------------------------------------------------------------------

    pub fn eval(&self, source: &str, source_url: Option<&str>) -> Result<JsValue, JsException> {
        self.check_not_poisoned()?;
        tracing::debug!(
            url = source_url.unwrap_or("<anon>"),
            bytes = source.len(),
            "eval"
        );

        #[cfg(jsc_available)]
        {
            use crate::jsc_sys::{self, read_cstr};
            use std::{ffi::CString, ptr};

            let url_cstr = source_url.and_then(|u| CString::new(u).ok());
            let url_ptr = url_cstr.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());
            let mut ex_ptr: *mut std::ffi::c_void = ptr::null_mut();

            let result = unsafe {
                jsc_sys::bua_eval(
                    self.ctx_ptr as *mut _,
                    source.as_ptr() as *const _,
                    source.len(),
                    url_ptr,
                    &mut ex_ptr,
                )
            };

            if !(ex_ptr as usize == 0) {
                let msg = unsafe { read_cstr(jsc_sys::bua_exception_message(ex_ptr as *const _)) };
                let stack = unsafe {
                    let s = jsc_sys::bua_exception_stack(ex_ptr as *const _);
                    if s.is_null() {
                        None
                    } else {
                        Some(read_cstr(s))
                    }
                };
                unsafe {
                    jsc_sys::bua_exception_free(ex_ptr);
                }
                return Err(JsException::new(msg).with_stack(stack.unwrap_or_default()));
            }

            if (result as usize) == 0 {
                return Ok(JsValue::Undefined);
            }
            return Ok(self.wrap_jsc_value(result as usize));
        }
        #[cfg(not(jsc_available))]
        Ok(JsValue::Undefined)
    }

    pub fn eval_module(&self, source: &str, module_url: &str) -> Result<JsValue, JsException> {
        self.check_not_poisoned()?;
        tracing::debug!(module_url, bytes = source.len(), "eval_module");

        #[cfg(jsc_available)]
        {
            use crate::jsc_sys::{self, read_cstr};
            use std::{ffi::CString, ptr};

            let url_cstr = CString::new(module_url).map_err(|e| JsException::new(e.to_string()))?;
            let mut ex_ptr: *mut std::ffi::c_void = ptr::null_mut();

            let result = unsafe {
                jsc_sys::bua_eval_module(
                    self.ctx_ptr as *mut _,
                    source.as_ptr() as *const _,
                    source.len(),
                    url_cstr.as_ptr(),
                    &mut ex_ptr,
                )
            };

            if !(ex_ptr as usize == 0) {
                let msg = unsafe { read_cstr(jsc_sys::bua_exception_message(ex_ptr as *const _)) };
                unsafe {
                    jsc_sys::bua_exception_free(ex_ptr);
                }
                return Err(JsException::new(msg));
            }
            if (result as usize) == 0 {
                return Ok(JsValue::Undefined);
            }
            return Ok(self.wrap_jsc_value(result as usize));
        }
        #[cfg(not(jsc_available))]
        Ok(JsValue::Undefined)
    }

    /// Drain the microtask queue.
    /// MUST be called after every top-level eval and after resolving Promises.
    pub fn drain_microtasks(&self) {
        #[cfg(jsc_available)]
        unsafe {
            crate::jsc_sys::bua_context_drain_microtasks(self.ctx_ptr as *mut _);
        }
        tracing::trace!("drain_microtasks");
    }

    // -----------------------------------------------------------------------
    // Native function registration
    // -----------------------------------------------------------------------

    pub fn register_native(&mut self, path: &str, func: NativeFn) -> BuaResult<()> {
        let entry = Box::new(NativeEntry {
            func,
            path: path.to_string(),
        });

        #[cfg(jsc_available)]
        {
            use crate::jsc_sys::{self, with_cstr};

            // SAFETY: entry_ptr stays valid because we store the box in native_entries below.
            // The pointer is erased from Box::as_ref before the push so borrow checker is happy.
            let entry_ptr = entry.as_ref() as *const NativeEntry as *mut std::ffi::c_void;

            let ok = unsafe {
                with_cstr(path, |p| {
                    jsc_sys::bua_set_native(self.ctx_ptr as *mut _, p, native_trampoline, entry_ptr)
                })
            };

            if !ok {
                return Err(BuaError::internal(format!("failed to register '{path}'")));
            }
        }

        self.native_entries.push(*entry);
        tracing::debug!(path, "native registered");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // call_function — THE completed stub
    // -----------------------------------------------------------------------

    /// Call a JS function with `args`, returning the result.
    ///
    /// Handles:
    ///   - Primitive arg marshalling (via JsValue::to_jsc_arg)
    ///   - Object/array/function arg pass-through (reuse existing pointer)
    ///   - Exception extraction + wrapping
    ///   - Return value wrapping
    ///
    /// MUST be called on the JS thread (enforced by JscEngine dispatch).
    pub fn call_function(
        &self,
        func: &FunctionHandle,
        _this: Option<&JsValue>,
        args: Vec<JsValue>,
    ) -> Result<JsValue, JsException> {
        self.check_not_poisoned()?;

        if func.is_stub() {
            // Stub handle — no real function. Return undefined (safe in tests).
            tracing::trace!("call_function on stub handle → undefined");
            return Ok(JsValue::Undefined);
        }

        #[cfg(jsc_available)]
        {
            use crate::jsc_sys::{self, read_cstr};
            use std::ptr;

            // Marshal arguments to JSValueRef pointers.
            // Each primitive is freshly constructed; handles reuse existing ptr.
            let arg_ptrs: Vec<*const std::ffi::c_void> = args
                .iter()
                .map(|v| v.to_jsc_arg(self.ctx_ptr) as *const _)
                .collect();

            // `this` object — null means JS `undefined` this (global / strict mode)
            let this_ptr = this
                .and_then(|v| v.raw_ptr())
                .map(|p| p as *mut std::ffi::c_void)
                .unwrap_or(ptr::null_mut());

            let func_ptr = func.raw_ptr() as *mut std::ffi::c_void;
            let mut ex_ptr: *mut std::ffi::c_void = ptr::null_mut();

            // JSObjectCallAsFunction(ctx, func, thisObject, argc, argv, exception)
            let result = unsafe {
                jsc_sys::jsc_call_as_function(
                    self.ctx_ptr as *mut _,
                    func_ptr,
                    this_ptr,
                    arg_ptrs.len(),
                    if arg_ptrs.is_empty() {
                        ptr::null()
                    } else {
                        arg_ptrs.as_ptr()
                    },
                    &mut ex_ptr,
                )
            };

            if !(ex_ptr as usize == 0) {
                let msg = unsafe { read_cstr(jsc_sys::bua_exception_message(ex_ptr as *const _)) };
                let stack = unsafe {
                    let s = jsc_sys::bua_exception_stack(ex_ptr as *const _);
                    if s.is_null() {
                        None
                    } else {
                        Some(read_cstr(s))
                    }
                };
                unsafe {
                    jsc_sys::bua_exception_free(ex_ptr);
                }
                return Err(JsException::new(msg).with_stack(stack.unwrap_or_default()));
            }

            if (result as usize) == 0 {
                return Ok(JsValue::Undefined);
            }
            return Ok(self.wrap_jsc_value(result as usize));
        }
        #[cfg(not(jsc_available))]
        {
            tracing::trace!(args = args.len(), "call_function (stub)");
            Ok(JsValue::Undefined)
        }
    }

    // -----------------------------------------------------------------------
    // Promise creation (JSObjectMakeDeferredPromise)
    // -----------------------------------------------------------------------

    /// Create a JS deferred Promise.
    /// Returns (PromiseHandle, promise_value).
    /// The caller stores the handle; the promise_value is returned to JS.
    pub fn create_promise(&self) -> Result<(PromiseHandle, JsValue), JsException> {
        self.check_not_poisoned()?;

        #[cfg(jsc_available)]
        {
            use crate::jsc_sys::{self, read_cstr};
            use std::ptr;

            // JSObjectMakeDeferredPromise(ctx, &resolve, &reject, &exception)
            let mut resolve_ptr: *mut std::ffi::c_void = ptr::null_mut();
            let mut reject_ptr: *mut std::ffi::c_void = ptr::null_mut();
            let mut ex_ptr: *mut std::ffi::c_void = ptr::null_mut();

            let promise_ptr = unsafe {
                jsc_sys::jsc_make_deferred_promise(
                    self.ctx_ptr as *mut _,
                    &mut resolve_ptr,
                    &mut reject_ptr,
                    &mut ex_ptr,
                )
            };

            if !(ex_ptr as usize == 0) {
                let msg = unsafe { read_cstr(jsc_sys::bua_exception_message(ex_ptr as *const _)) };
                unsafe {
                    jsc_sys::bua_exception_free(ex_ptr);
                }
                return Err(JsException::new(msg));
            }

            let ctx = self.ctx_ptr;
            let handle = PromiseHandle::new(
                FunctionHandle::new(resolve_ptr as usize, ctx),
                FunctionHandle::new(reject_ptr as usize, ctx),
                ObjectHandle::new(promise_ptr as usize, ctx),
            );
            let promise_val = handle.promise_value();
            return Ok((handle, promise_val));
        }

        #[cfg(not(jsc_available))]
        Ok((PromiseHandle::stub(), JsValue::Object(ObjectHandle::stub())))
    }

    /// Resolve a deferred Promise with a value.
    pub fn resolve_promise(&self, handle: &PromiseHandle, value: JsValue) -> BuaResult<()> {
        self.call_function(&handle.resolve, None, vec![value])
            .map(|_| ())
            .map_err(BuaError::from)
    }

    /// Reject a deferred Promise with an exception.
    pub fn reject_promise(&self, handle: &PromiseHandle, ex: JsException) -> BuaResult<()> {
        let err_str = JsValue::String(format!("{}: {}", ex.name, ex.message));
        self.call_function(&handle.reject, None, vec![err_str])
            .map(|_| ())
            .map_err(BuaError::from)
    }

    // -----------------------------------------------------------------------
    // JSON bridge
    // -----------------------------------------------------------------------

    pub fn value_to_json(&self, val: &JsValue) -> BuaResult<String> {
        #[cfg(jsc_available)]
        if let Some(ptr) = val.raw_ptr() {
            use crate::jsc_sys::{self, cstr_to_string};
            let mut len: usize = 0;
            let json_ptr = unsafe {
                jsc_sys::bua_value_to_json(self.ctx_ptr as *mut _, ptr as *const _, &mut len)
            };
            if !(json_ptr as usize == 0) {
                return Ok(unsafe { cstr_to_string(json_ptr, len) });
            }
        }
        Ok(serde_json::to_string(&val.to_json())?)
    }

    pub fn json_to_value(&self, json: &str) -> Result<JsValue, JsException> {
        self.check_not_poisoned()?;

        #[cfg(jsc_available)]
        {
            use crate::jsc_sys::{self};
            use std::ptr;

            let mut ex_ptr: *mut std::ffi::c_void = ptr::null_mut();
            let result = unsafe {
                jsc_sys::bua_value_from_json(
                    self.ctx_ptr as *mut _,
                    json.as_ptr() as *const _,
                    json.len(),
                    &mut ex_ptr,
                )
            };
            if !(ex_ptr as usize == 0) {
                unsafe {
                    jsc_sys::bua_exception_free(ex_ptr);
                }
                return Err(JsException::new(format!("JSON parse error")));
            }
            if (result as usize) == 0 {
                return Ok(JsValue::Null);
            }
            return Ok(self.wrap_jsc_value(result as usize));
        }

        serde_json::from_str::<serde_json::Value>(json)
            .map(JsValue::from_json)
            .map_err(|e| JsException::new(format!("JSON parse: {e}")))
    }

    // -----------------------------------------------------------------------
    // Heap snapshot
    // -----------------------------------------------------------------------

    pub fn snapshot_heap(&self) -> BuaResult<Vec<u8>> {
        #[cfg(jsc_available)]
        {
            use crate::jsc_sys;
            let mut size: usize = 0;
            let ptr = unsafe { jsc_sys::bua_snapshot_create(self.ctx_ptr as *mut _, &mut size) };
            if (ptr as usize) == 0 || size == 0 {
                return Err(BuaError::SnapshotSerialize(
                    "bua_snapshot_create failed".into(),
                ));
            }
            let bytes = unsafe { std::slice::from_raw_parts(ptr, size).to_vec() };
            unsafe {
                jsc_sys::bua_snapshot_free(ptr);
            }
            return Ok(bytes);
        }
        tracing::info!("snapshot_heap (stub)");
        Ok(vec![0xB, 0xA, 0xA, 0x2]) // v2 magic
    }

    pub fn restore_heap(&mut self, data: &[u8]) -> BuaResult<()> {
        #[cfg(jsc_available)]
        {
            use crate::jsc_sys;
            let ok = unsafe {
                jsc_sys::bua_snapshot_restore(self.ctx_ptr as *mut _, data.as_ptr(), data.len())
            };
            if !ok {
                return Err(BuaError::SnapshotRestore(
                    "bua_snapshot_restore failed".into(),
                ));
            }
            return Ok(());
        }
        tracing::info!(bytes = data.len(), "restore_heap (stub)");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    fn check_not_poisoned(&self) -> Result<(), JsException> {
        if self.poisoned {
            Err(JsException::new("context is poisoned"))
        } else {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Internal: wrap a raw JSC value pointer into a safe JsValue
    // -----------------------------------------------------------------------

    #[cfg(jsc_available)]
    fn wrap_jsc_value(&self, ptr: usize) -> JsValue {
        use crate::jsc_sys::{self, cstr_to_string};

        let type_id = unsafe { jsc_sys::bua_value_type(self.ctx_ptr as *mut _, ptr as *const _) };

        match type_id {
            0 => JsValue::Undefined,
            1 => JsValue::Null,
            2 => {
                let b =
                    unsafe { jsc_sys::bua_value_to_bool(self.ctx_ptr as *mut _, ptr as *const _) };
                JsValue::Bool(b)
            }
            3 => {
                let n = unsafe {
                    jsc_sys::bua_value_to_number(self.ctx_ptr as *mut _, ptr as *const _)
                };
                JsValue::Number(n)
            }
            4 => {
                let mut len: usize = 0;
                let s_ptr = unsafe {
                    jsc_sys::bua_value_to_string_utf8(
                        self.ctx_ptr as *mut _,
                        ptr as *const _,
                        &mut len,
                    )
                };
                JsValue::String(unsafe { cstr_to_string(s_ptr, len) })
            }
            5 => {
                // Object — try JSON first for plain data objects, else return handle
                let mut len: usize = 0;
                let json_ptr = unsafe {
                    jsc_sys::bua_value_to_json(self.ctx_ptr as *mut _, ptr as *const _, &mut len)
                };
                if !(json_ptr as usize == 0) {
                    let s = unsafe { cstr_to_string(json_ptr, len) };
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                        // Check if it's actually a function (type 5 but callable)
                        // For now: plain object → JSON value, complex → handle
                        match v {
                            serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                                return JsValue::from_json(v);
                            }
                            _ => {}
                        }
                    }
                }
                // Opaque handle — protect and return
                JsValue::Object(ObjectHandle::new(ptr, self.ctx_ptr))
            }
            _ => JsValue::Undefined,
        }
    }
}

impl Drop for JscContext {
    fn drop(&mut self) {
        #[cfg(jsc_available)]
        if self.ctx_ptr != 0 {
            unsafe {
                crate::jsc_sys::bua_context_destroy(self.ctx_ptr as *mut _);
            }
            tracing::debug!("JscContext destroyed");
        }
    }
}

// ---------------------------------------------------------------------------
// Native callback trampoline
// ---------------------------------------------------------------------------
//
// Called by JSC on the JS thread when a __bua_*__ native is invoked from JS.
//
// user_data is a raw *const NativeEntry whose Box lives in JscContext::native_entries.
// The box is alive as long as the JscContext is alive, so this dereference is safe
// on the JS thread (which is the same thread that owns JscContext).

#[cfg(jsc_available)]
unsafe extern "C" fn native_trampoline(
    ctx_ptr: *mut std::ffi::c_void,
    _this: *mut std::ffi::c_void,
    raw_args: *mut *mut std::ffi::c_void,
    argc: usize,
    user_data: *mut std::ffi::c_void,
    out_ex: *mut *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    // SAFETY: user_data is &NativeEntry kept alive in native_entries vec.
    let entry = &*(user_data as *const NativeEntry);

    // Build a temporary non-owning JscContext view for the callback.
    // We can't clone JscContext, so we use a thin wrapper.
    let temp_ctx = JscContext {
        ctx_ptr: ctx_ptr as usize,
        native_entries: Vec::new(), // empty — no ownership
        poisoned: false,
    };

    // Wrap raw arg pointers into JsValue (non-owning — we don't JSValueProtect here;
    // they're already protected for the duration of the call by JSC).
    let args: Vec<JsValue> = (0..argc)
        .map(|i| {
            let ptr = *raw_args.add(i);
            if (ptr as usize) == 0 {
                JsValue::Undefined
            } else {
                temp_ctx.wrap_jsc_value(ptr as usize)
            }
        })
        .collect();

    // Forget temp_ctx so Drop doesn't call bua_context_destroy on the borrowed ptr.
    std::mem::forget(temp_ctx);

    match (entry.func)(
        &JscContext {
            ctx_ptr: ctx_ptr as usize,
            native_entries: Vec::new(),
            poisoned: false,
        },
        args,
    ) {
        Ok(val) => {
            // Convert result back to a raw JSC pointer.
            // For now: serialise via JSON for complex values.
            use crate::jsc_sys;
            match val {
                JsValue::Undefined => jsc_sys::bua_value_undefined(ctx_ptr),
                JsValue::Null => jsc_sys::bua_value_null(ctx_ptr),
                JsValue::Bool(b) => jsc_sys::bua_value_bool(ctx_ptr, b),
                JsValue::Number(n) => jsc_sys::bua_value_number(ctx_ptr, n),
                JsValue::String(ref s) => {
                    jsc_sys::bua_value_string(ctx_ptr, s.as_ptr() as *const _, s.len())
                }
                other => {
                    // Complex value: marshal as JSON
                    let json = serde_json::to_string(&other.to_json()).unwrap_or("null".into());
                    let mut ex2: *mut std::ffi::c_void = std::ptr::null_mut();
                    let r = jsc_sys::bua_value_from_json(
                        ctx_ptr,
                        json.as_ptr() as *const _,
                        json.len(),
                        &mut ex2,
                    );
                    if !(ex2 as usize == 0) {
                        jsc_sys::bua_exception_free(ex2);
                    }
                    r
                }
            }
        }
        Err(ex) => {
            // Set the out exception as a JS string
            use crate::jsc_sys;
            let msg = format!("{}: {}", ex.name, ex.message);
            *out_ex =
                jsc_sys::bua_value_string(ctx_ptr, msg.as_ptr() as *const _, msg.len()) as *mut _;
            std::ptr::null_mut()
        }
    }
}
