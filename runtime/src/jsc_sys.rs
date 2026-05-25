// runtime/src/jsc_sys.rs
//
// Bridges jsc/bindings/bua_jsc_sys.rs into the runtime crate namespace.
// ffi/context.rs and ffi/value.rs use `crate::jsc_sys::*`.
//
// When cfg(jsc_available) is set by build.rs:
//   - The real bua-jsc-sys crate is in scope (via the `jsc` feature)
//   - All unsafe FFI calls go through the real C bridge
//
// When cfg(jsc_available) is NOT set:
//   - Stub no-ops are used so the codebase compiles everywhere
//   - Tests exercise the Rust architecture without a JSC install

#[cfg(jsc_available)]
pub use bua_jsc_sys::*;

#[cfg(not(jsc_available))]
#[allow(dead_code)]
mod stubs {
    use std::os::raw::{c_char, c_void};

    // --- Context ---
    #[inline]
    pub unsafe fn bua_context_create(_max: usize) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_context_destroy(_ctx: *mut c_void) {}
    #[inline]
    pub unsafe fn bua_context_drain_microtasks(_ctx: *mut c_void) {}

    // --- Eval ---
    #[inline]
    pub unsafe fn bua_eval(
        _ctx: *mut c_void,
        _src: *const c_char,
        _len: usize,
        _url: *const c_char,
        _ex: *mut *mut c_void,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_eval_module(
        _ctx: *mut c_void,
        _src: *const c_char,
        _len: usize,
        _url: *const c_char,
        _ex: *mut *mut c_void,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }

    // --- Value constructors ---
    #[inline]
    pub unsafe fn bua_value_undefined(_ctx: *mut c_void) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_value_null(_ctx: *mut c_void) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_value_bool(_ctx: *mut c_void, _v: bool) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_value_number(_ctx: *mut c_void, _v: f64) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_value_string(
        _ctx: *mut c_void,
        _s: *const c_char,
        _len: usize,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_value_from_json(
        _ctx: *mut c_void,
        _j: *const c_char,
        _len: usize,
        _ex: *mut *mut c_void,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }

    // --- Value accessors ---
    #[inline]
    pub unsafe fn bua_value_type(_ctx: *mut c_void, _v: *const c_void) -> i32 {
        0
    }
    #[inline]
    pub unsafe fn bua_value_to_bool(_ctx: *mut c_void, _v: *const c_void) -> bool {
        false
    }
    #[inline]
    pub unsafe fn bua_value_to_number(_ctx: *mut c_void, _v: *const c_void) -> f64 {
        0.0
    }
    #[inline]
    pub unsafe fn bua_value_to_string_utf8(
        _ctx: *mut c_void,
        _v: *const c_void,
        out_len: *mut usize,
    ) -> *mut c_char {
        unsafe {
            *out_len = 0;
        }
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_value_to_json(
        _ctx: *mut c_void,
        _v: *const c_void,
        out_len: *mut usize,
    ) -> *mut c_char {
        unsafe {
            *out_len = 0;
        }
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_string_free(_s: *mut c_char) {}

    // --- Natives ---
    #[inline]
    pub unsafe fn bua_set_native(
        _ctx: *mut c_void,
        _path: *const c_char,
        _f: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut *mut c_void,
            usize,
            *mut c_void,
            *mut *mut c_void,
        ) -> *mut c_void,
        _ud: *mut c_void,
    ) -> bool {
        false
    }

    // --- Snapshots ---
    #[inline]
    pub unsafe fn bua_snapshot_create(_ctx: *mut c_void, out_size: *mut usize) -> *mut u8 {
        unsafe {
            *out_size = 0;
        }
        std::ptr::null_mut()
    }
    #[inline]
    pub unsafe fn bua_snapshot_restore(_ctx: *mut c_void, _data: *const u8, _size: usize) -> bool {
        true
    }
    #[inline]
    pub unsafe fn bua_snapshot_free(_data: *mut u8) {}

    // --- Exceptions ---
    #[inline]
    pub unsafe fn bua_exception_message(_ex: *const c_void) -> *const c_char {
        std::ptr::null()
    }
    #[inline]
    pub unsafe fn bua_exception_stack(_ex: *const c_void) -> *const c_char {
        std::ptr::null()
    }
    #[inline]
    pub unsafe fn bua_exception_free(_ex: *mut c_void) {}

    // --- GC protect ---
    #[inline]
    pub unsafe fn jsc_value_protect(_ctx: *mut c_void, _val: *const c_void) {}
    #[inline]
    pub unsafe fn jsc_value_unprotect(_ctx: *mut c_void, _val: *const c_void) {}

    // --- Function call + deferred Promise ---
    #[inline]
    pub unsafe fn jsc_call_as_function(
        _ctx: *mut c_void,
        _func: *mut c_void,
        _this: *mut c_void,
        _argc: usize,
        _argv: *const *const c_void,
        _ex: *mut *mut c_void,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }

    #[inline]
    pub unsafe fn jsc_make_deferred_promise(
        _ctx: *mut c_void,
        resolve: *mut *mut c_void,
        reject: *mut *mut c_void,
        _ex: *mut *mut c_void,
    ) -> *mut c_void {
        unsafe {
            *resolve = std::ptr::null_mut();
            *reject = std::ptr::null_mut();
        }
        std::ptr::null_mut()
    }

    // --- String helpers ---
    pub fn with_cstr<T>(s: &str, f: impl FnOnce(*const c_char) -> T) -> T {
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        f(cstr.as_ptr())
    }
    pub unsafe fn cstr_to_string(ptr: *mut c_char, len: usize) -> String {
        if ptr.is_null() || len == 0 {
            return String::new();
        }
        let slice = std::slice::from_raw_parts(ptr as *const u8, len);
        String::from_utf8_lossy(slice).into_owned()
    }
    pub unsafe fn read_cstr(ptr: *const c_char) -> String {
        if ptr.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}
