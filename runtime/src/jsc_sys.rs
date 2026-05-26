#[cfg(jsc_available)]
pub use bua_jsc_sys::*;

#[cfg(not(jsc_available))]
#[allow(unused_imports)]
pub use stubs::*;

#[cfg(not(jsc_available))]
#[allow(dead_code, unused_variables)]
pub mod stubs {
    use std::os::raw::{c_char, c_void};

    pub unsafe fn bua_context_create(max_heap_bytes: usize) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_context_destroy(ctx: *mut c_void) {}
    pub unsafe fn bua_context_drain_microtasks(ctx: *mut c_void) {}
    pub unsafe fn bua_eval(
        ctx: *mut c_void,
        source: *const c_char,
        len: usize,
        url: *const c_char,
        ex: *mut *mut c_void,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_eval_module(
        ctx: *mut c_void,
        source: *const c_char,
        len: usize,
        url: *const c_char,
        ex: *mut *mut c_void,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_value_undefined(ctx: *mut c_void) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_value_null(ctx: *mut c_void) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_value_bool(ctx: *mut c_void, v: bool) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_value_number(ctx: *mut c_void, v: f64) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_value_string(ctx: *mut c_void, s: *const c_char, len: usize) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_value_from_json(
        ctx: *mut c_void,
        json: *const c_char,
        len: usize,
        ex: *mut *mut c_void,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn bua_value_type(ctx: *mut c_void, v: *const c_void) -> i32 {
        0
    }
    pub unsafe fn bua_value_to_bool(ctx: *mut c_void, v: *const c_void) -> bool {
        false
    }
    pub unsafe fn bua_value_to_number(ctx: *mut c_void, v: *const c_void) -> f64 {
        0.0
    }
    pub unsafe fn bua_value_to_string_utf8(
        ctx: *mut c_void,
        v: *const c_void,
        out_len: *mut usize,
    ) -> *mut c_char {
        unsafe {
            if !out_len.is_null() {
                *out_len = 0;
            }
        }
        std::ptr::null_mut()
    }
    pub unsafe fn bua_value_to_json(
        ctx: *mut c_void,
        v: *const c_void,
        out_len: *mut usize,
    ) -> *mut c_char {
        unsafe {
            if !out_len.is_null() {
                *out_len = 0;
            }
        }
        std::ptr::null_mut()
    }
    pub unsafe fn bua_string_free(s: *mut c_char) {}
    pub unsafe fn bua_set_native(
        ctx: *mut c_void,
        path: *const c_char,
        func: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut *mut c_void,
            usize,
            *mut c_void,
            *mut *mut c_void,
        ) -> *mut c_void,
        ud: *mut c_void,
    ) -> bool {
        false
    }
    pub unsafe fn bua_snapshot_create(ctx: *mut c_void, out_size: *mut usize) -> *mut u8 {
        unsafe {
            if !out_size.is_null() {
                *out_size = 0;
            }
        }
        std::ptr::null_mut()
    }
    pub unsafe fn bua_snapshot_restore(ctx: *mut c_void, data: *const u8, size: usize) -> bool {
        true
    }
    pub unsafe fn bua_snapshot_free(data: *mut u8) {}
    pub unsafe fn bua_exception_message(ex: *const c_void) -> *const c_char {
        std::ptr::null()
    }
    pub unsafe fn bua_exception_stack(ex: *const c_void) -> *const c_char {
        std::ptr::null()
    }
    pub unsafe fn bua_exception_free(ex: *mut c_void) {}
    pub unsafe fn jsc_value_protect(ctx: *mut c_void, val: *const c_void) {}
    pub unsafe fn jsc_value_unprotect(ctx: *mut c_void, val: *const c_void) {}
    pub unsafe fn jsc_call_as_function(
        ctx: *mut c_void,
        func: *mut c_void,
        this: *mut c_void,
        argc: usize,
        argv: *const *const c_void,
        ex: *mut *mut c_void,
    ) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn jsc_make_deferred_promise(
        ctx: *mut c_void,
        resolve: *mut *mut c_void,
        reject: *mut *mut c_void,
        ex: *mut *mut c_void,
    ) -> *mut c_void {
        unsafe {
            if !resolve.is_null() {
                *resolve = std::ptr::null_mut();
            }
            if !reject.is_null() {
                *reject = std::ptr::null_mut();
            }
        }
        std::ptr::null_mut()
    }

    pub fn with_cstr<T>(s: &str, f: impl FnOnce(*const c_char) -> T) -> T {
        let cstr = std::ffi::CString::new(s).unwrap_or_default();
        f(cstr.as_ptr())
    }
    pub unsafe fn cstr_to_string(ptr: *mut c_char, len: usize) -> String {
        String::new()
    }
    pub unsafe fn read_cstr(ptr: *const c_char) -> String {
        String::new()
    }
}
