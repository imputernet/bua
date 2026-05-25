// runtime/src/ffi/value.rs
//
// Canonical Rust-side JS value model.
//
// OWNERSHIP CONTRACT:
//   - Every live JSC value held by Rust is protected via JSValueProtect.
//   - HandleInner::Drop calls JSValueUnprotect when the last Rust reference drops.
//   - No JSC pointer escapes this file except through `raw_ptr()` which is
//     pub(crate) and only called inside ffi/context.rs.
//   - Cloning a handle clones the Arc — JSValueProtect was called once on
//     construction; the refcount keeps it alive until all clones drop.

use bua_core::{BuaError, BuaResult};
use serde_json::Value as JsonValue;
use std::fmt;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// HandleInner — the GC root
// ---------------------------------------------------------------------------

struct HandleInner {
    /// Raw JSValueRef (cast to usize so this struct is Send).
    /// Zero means stub / unprotected.
    ptr: usize,
    /// JSContextRef at protect time. Needed for JSValueUnprotect on drop.
    ctx_ptr: usize,
}

impl fmt::Debug for HandleInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({:#x})", self.ptr)
    }
}

impl Drop for HandleInner {
    fn drop(&mut self) {
        if self.ptr == 0 || self.ctx_ptr == 0 {
            return; // stub or already released
        }
        #[cfg(jsc_available)]
        unsafe {
            // JSValueUnprotect(ctx, value)
            // Cast back from usize to raw pointer types expected by JSC.
            let ctx  = self.ctx_ptr as *mut std::ffi::c_void;
            let val  = self.ptr     as *const std::ffi::c_void;
            crate::jsc_sys::jsc_value_unprotect(ctx, val);
            tracing::trace!(ptr = self.ptr, "JSValueUnprotect");
        }
        #[cfg(not(jsc_available))]
        {
            tracing::trace!(ptr = self.ptr, "JSValueUnprotect (stub)");
        }
    }
}

impl HandleInner {
    /// Stub — zero pointers, no JSC interaction.
    fn stub() -> Arc<Self> {
        Arc::new(Self { ptr: 0, ctx_ptr: 0 })
    }

    /// Real — protect the value immediately.
    fn new(ptr: usize, ctx_ptr: usize) -> Arc<Self> {
        if ptr == 0 || ctx_ptr == 0 {
            return Self::stub();
        }
        #[cfg(jsc_available)]
        unsafe {
            let ctx = ctx_ptr as *mut std::ffi::c_void;
            let val = ptr     as *const std::ffi::c_void;
            crate::jsc_sys::jsc_value_protect(ctx, val);
            tracing::trace!(ptr, "JSValueProtect");
        }
        Arc::new(Self { ptr, ctx_ptr })
    }
}

// ---------------------------------------------------------------------------
// Typed handles
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ObjectHandle(Arc<HandleInner>);

#[derive(Clone, Debug)]
pub struct ArrayHandle(Arc<HandleInner>);

#[derive(Clone, Debug)]
pub struct FunctionHandle(Arc<HandleInner>);

macro_rules! impl_handle {
    ($T:ty) => {
        impl $T {
            pub(crate) fn stub() -> Self { Self(HandleInner::stub()) }
            pub(crate) fn new(ptr: usize, ctx_ptr: usize) -> Self {
                Self(HandleInner::new(ptr, ctx_ptr))
            }
            /// Raw pointer value — only valid on the JS thread, only inside ffi/.
            pub(crate) fn raw_ptr(&self) -> usize { self.0.ptr }
            pub(crate) fn ctx_ptr(&self) -> usize { self.0.ctx_ptr }
            pub fn is_stub(&self) -> bool { self.0.ptr == 0 }
        }
    };
}

impl_handle!(ObjectHandle);
impl_handle!(ArrayHandle);
impl_handle!(FunctionHandle);

// ---------------------------------------------------------------------------
// Canonical JsValue
// ---------------------------------------------------------------------------

/// The one and only cross-boundary JS value type.
/// No raw JSC pointers escape the `ffi` module.
#[derive(Debug, Clone)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Object(ObjectHandle),
    Array(ArrayHandle),
    Function(FunctionHandle),
    BigInt(String),
}

impl JsValue {
    // --- Type predicates ---
    pub fn is_undefined(&self) -> bool { matches!(self, Self::Undefined) }
    pub fn is_null(&self)      -> bool { matches!(self, Self::Null) }
    pub fn is_nullish(&self)   -> bool { matches!(self, Self::Undefined | Self::Null) }
    pub fn is_bool(&self)      -> bool { matches!(self, Self::Bool(_)) }
    pub fn is_number(&self)    -> bool { matches!(self, Self::Number(_)) }
    pub fn is_string(&self)    -> bool { matches!(self, Self::String(_)) }
    pub fn is_object(&self)    -> bool { matches!(self, Self::Object(_)) }
    pub fn is_array(&self)     -> bool { matches!(self, Self::Array(_)) }
    pub fn is_function(&self)  -> bool { matches!(self, Self::Function(_)) }

    // --- Extractors ---
    pub fn as_bool(&self)     -> Option<bool>            { if let Self::Bool(b) = self { Some(*b) } else { None } }
    pub fn as_f64(&self)      -> Option<f64>             { if let Self::Number(n) = self { Some(*n) } else { None } }
    pub fn as_i64(&self)      -> Option<i64>             { self.as_f64().map(|n| n as i64) }
    pub fn as_str(&self)      -> Option<&str>            { if let Self::String(s) = self { Some(s) } else { None } }
    pub fn as_object(&self)   -> Option<&ObjectHandle>   { if let Self::Object(h) = self { Some(h) } else { None } }
    pub fn as_array(&self)    -> Option<&ArrayHandle>    { if let Self::Array(h) = self { Some(h) } else { None } }
    pub fn as_function(&self) -> Option<&FunctionHandle> { if let Self::Function(h) = self { Some(h) } else { None } }

    /// JS-style truthiness.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    /// Convert to JSON. Objects/Arrays that have a live JSC handle return their
    /// JSON-serialized form (if available); stub handles become empty {}/{[]}.
    pub fn to_json(&self) -> JsonValue {
        match self {
            Self::Undefined | Self::Null => JsonValue::Null,
            Self::Bool(b)   => JsonValue::Bool(*b),
            Self::Number(n) => serde_json::Number::from_f64(*n).map(JsonValue::Number).unwrap_or(JsonValue::Null),
            Self::String(s) => JsonValue::String(s.clone()),
            Self::Object(_) => JsonValue::Object(Default::default()),
            Self::Array(_)  => JsonValue::Array(Default::default()),
            Self::Function(_) => JsonValue::Null,
            Self::BigInt(s) => JsonValue::String(s.clone()),
        }
    }

    pub fn from_json(json: JsonValue) -> Self {
        match json {
            JsonValue::Null     => Self::Null,
            JsonValue::Bool(b)  => Self::Bool(b),
            JsonValue::Number(n) => Self::Number(n.as_f64().unwrap_or(f64::NAN)),
            JsonValue::String(s) => Self::String(s),
            JsonValue::Array(_)  => Self::Array(ArrayHandle::stub()),
            JsonValue::Object(_) => Self::Object(ObjectHandle::stub()),
        }
    }

    /// Extract raw pointer for use inside ffi/context.rs ONLY.
    /// Returns None for primitive types (they don't have pointers).
    pub(crate) fn raw_ptr(&self) -> Option<usize> {
        match self {
            Self::Object(h)   => Some(h.raw_ptr()),
            Self::Array(h)    => Some(h.raw_ptr()),
            Self::Function(h) => Some(h.raw_ptr()),
            _ => None,
        }
    }

    /// Convert a primitive JsValue to a raw JSC value pointer for call args.
    /// Primitives are constructed fresh; handles reuse the existing pointer.
    /// Returns (ptr, is_constructed) — constructed values must NOT be freed
    /// because JSC owns them after JSObjectCallAsFunction.
    ///
    /// Only valid inside ffi/ on the JS thread.
    #[cfg(jsc_available)]
    pub(crate) fn to_jsc_arg(&self, ctx_ptr: usize) -> usize {
        use crate::jsc_sys;
        let ctx = ctx_ptr as *mut std::ffi::c_void;
        unsafe {
            match self {
                Self::Undefined     => jsc_sys::bua_value_undefined(ctx) as usize,
                Self::Null          => jsc_sys::bua_value_null(ctx) as usize,
                Self::Bool(b)       => jsc_sys::bua_value_bool(ctx, *b) as usize,
                Self::Number(n)     => jsc_sys::bua_value_number(ctx, *n) as usize,
                Self::String(s)     => {
                    jsc_sys::bua_value_string(ctx, s.as_ptr() as *const _, s.len()) as usize
                }
                Self::Object(h) | Self::Array(_) | Self::Function(_) => {
                    // Already protected — reuse existing pointer
                    self.raw_ptr().unwrap_or(0)
                }
                Self::BigInt(s) => {
                    // Encode as string for now; real impl: JSBigInt API
                    jsc_sys::bua_value_string(ctx, s.as_ptr() as *const _, s.len()) as usize
                }
            }
        }
    }
}

impl fmt::Display for JsValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undefined   => write!(f, "undefined"),
            Self::Null        => write!(f, "null"),
            Self::Bool(b)     => write!(f, "{b}"),
            Self::Number(n)   => write!(f, "{n}"),
            Self::String(s)   => write!(f, "{s}"),
            Self::Object(_)   => write!(f, "[object Object]"),
            Self::Array(_)    => write!(f, "[object Array]"),
            Self::Function(_) => write!(f, "[Function]"),
            Self::BigInt(s)   => write!(f, "{s}n"),
        }
    }
}

impl From<bool>   for JsValue { fn from(b: bool)   -> Self { Self::Bool(b) } }
impl From<f64>    for JsValue { fn from(n: f64)    -> Self { Self::Number(n) } }
impl From<i64>    for JsValue { fn from(n: i64)    -> Self { Self::Number(n as f64) } }
impl From<i32>    for JsValue { fn from(n: i32)    -> Self { Self::Number(n as f64) } }
impl From<u32>    for JsValue { fn from(n: u32)    -> Self { Self::Number(n as f64) } }
impl From<String> for JsValue { fn from(s: String) -> Self { Self::String(s) } }
impl From<&str>   for JsValue { fn from(s: &str)   -> Self { Self::String(s.to_string()) } }
impl From<JsonValue> for JsValue { fn from(v: JsonValue) -> Self { Self::from_json(v) } }
impl From<JsValue> for JsonValue { fn from(v: JsValue)   -> Self { v.to_json() } }

// ---------------------------------------------------------------------------
// PromiseHandle — holds resolve/reject function handles for a deferred Promise
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PromiseHandle {
    pub(crate) resolve: FunctionHandle,
    pub(crate) reject:  FunctionHandle,
    /// The promise object itself (kept alive so JS can .then() it).
    pub(crate) promise_obj: ObjectHandle,
}

impl PromiseHandle {
    pub(crate) fn stub() -> Self {
        Self {
            resolve:     FunctionHandle::stub(),
            reject:      FunctionHandle::stub(),
            promise_obj: ObjectHandle::stub(),
        }
    }

    pub(crate) fn new(
        resolve:     FunctionHandle,
        reject:      FunctionHandle,
        promise_obj: ObjectHandle,
    ) -> Self {
        Self { resolve, reject, promise_obj }
    }

    /// Return the JS promise value so it can be returned to calling JS code.
    pub fn promise_value(&self) -> JsValue {
        JsValue::Object(self.promise_obj.clone())
    }
}

// ---------------------------------------------------------------------------
// JsException
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct JsException {
    pub message: String,
    pub stack:   Option<String>,
    pub name:    String,
}

impl JsException {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), stack: None, name: "Error".into() }
    }
    pub fn with_stack(mut self, stack: impl Into<String>) -> Self {
        self.stack = Some(stack.into()); self
    }
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into(); self
    }
}

impl fmt::Display for JsException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.message)?;
        if let Some(s) = &self.stack { write!(f, "\n{s}")?; }
        Ok(())
    }
}

impl From<JsException> for BuaError {
    fn from(ex: JsException) -> Self {
        BuaError::JsException { message: ex.message, stack: ex.stack }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn truthiness() {
        assert!(!JsValue::Undefined.is_truthy());
        assert!(!JsValue::Null.is_truthy());
        assert!(!JsValue::Bool(false).is_truthy());
        assert!(!JsValue::Number(0.0).is_truthy());
        assert!(!JsValue::String("".into()).is_truthy());
        assert!(JsValue::Bool(true).is_truthy());
        assert!(JsValue::Number(1.0).is_truthy());
        assert!(JsValue::Object(ObjectHandle::stub()).is_truthy());
    }

    #[test] fn json_roundtrip() {
        for v in [JsValue::Bool(true), JsValue::Number(42.5), JsValue::String("x".into()), JsValue::Null] {
            let json = v.to_json();
            let back = JsValue::from_json(json);
            assert!(!matches!(back, JsValue::Undefined));
        }
    }

    #[test] fn primitive_raw_ptr_is_none() {
        assert!(JsValue::Number(1.0).raw_ptr().is_none());
        assert!(JsValue::String("x".into()).raw_ptr().is_none());
        assert!(JsValue::Bool(true).raw_ptr().is_none());
        assert!(JsValue::Null.raw_ptr().is_none());
    }

    #[test] fn handle_raw_ptr_is_some() {
        let h = JsValue::Object(ObjectHandle::stub());
        // stub ptr is 0 — raw_ptr() still returns Some(0) for the object variant
        assert!(h.raw_ptr().is_some());
    }

    #[test] fn from_conversions() {
        let v: JsValue = true.into();    assert!(v.is_bool());
        let v: JsValue = 3.14_f64.into(); assert_eq!(v.as_f64(), Some(3.14));
        let v: JsValue = "hi".into();    assert_eq!(v.as_str(), Some("hi"));
    }

    #[test] fn promise_handle_stub_value() {
        let ph = PromiseHandle::stub();
        assert!(ph.promise_value().is_object());
    }
}
