// jsc/include/bua_jsc.h
// C API bridge between Rust and JavaScriptCore.
// All functions are thread-safe via the per-context lock.

#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// ---------------------------------------------------------------------------
// Opaque handles
// ---------------------------------------------------------------------------

typedef struct BuaContext    BuaContext;
typedef struct BuaValue      BuaValue;
typedef struct BuaException  BuaException;

// ---------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------

typedef enum {
    BUA_TYPE_UNDEFINED = 0,
    BUA_TYPE_NULL      = 1,
    BUA_TYPE_BOOLEAN   = 2,
    BUA_TYPE_NUMBER    = 3,
    BUA_TYPE_STRING    = 4,
    BUA_TYPE_OBJECT    = 5,
    BUA_TYPE_SYMBOL    = 6,
    BUA_TYPE_BIGINT    = 7,
} BuaValueType;

// ---------------------------------------------------------------------------
// Native function callback
// Returning NULL signals an exception; set *out_exception.
// ---------------------------------------------------------------------------

typedef BuaValue* (*BuaNativeFunction)(
    BuaContext* ctx,
    BuaValue*   this_val,
    BuaValue**  args,
    size_t      argc,
    void*       user_data,
    BuaException** out_exception
);

// ---------------------------------------------------------------------------
// Context lifecycle
// ---------------------------------------------------------------------------

/// Create a new JSC global context. Thread: any, but single-thread from then on.
BuaContext* bua_context_create(size_t max_heap_bytes);

/// Destroy a context and free all associated memory.
void bua_context_destroy(BuaContext* ctx);

/// Drain the microtask queue (call after every top-level eval).
void bua_context_drain_microtasks(BuaContext* ctx);

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Evaluate a null-terminated JS source string.
/// Returns NULL on exception (inspect *out_exception).
BuaValue* bua_eval(
    BuaContext*    ctx,
    const char*    source,
    size_t         source_len,
    const char*    source_url,
    BuaException** out_exception
);

/// Load and evaluate an ES module from a source string.
BuaValue* bua_eval_module(
    BuaContext*    ctx,
    const char*    source,
    size_t         source_len,
    const char*    module_url,
    BuaException** out_exception
);

// ---------------------------------------------------------------------------
// Value constructors
// ---------------------------------------------------------------------------

BuaValue* bua_value_undefined(BuaContext* ctx);
BuaValue* bua_value_null(BuaContext* ctx);
BuaValue* bua_value_bool(BuaContext* ctx, bool v);
BuaValue* bua_value_number(BuaContext* ctx, double v);
BuaValue* bua_value_string(BuaContext* ctx, const char* utf8, size_t len);
BuaValue* bua_value_from_json(BuaContext* ctx, const char* json, size_t len,
                              BuaException** out_exception);

// ---------------------------------------------------------------------------
// Value accessors
// ---------------------------------------------------------------------------

BuaValueType bua_value_type(BuaContext* ctx, const BuaValue* v);
bool         bua_value_to_bool(BuaContext* ctx, const BuaValue* v);
double       bua_value_to_number(BuaContext* ctx, const BuaValue* v);

/// Caller must free the returned buffer with bua_string_free().
char* bua_value_to_string_utf8(BuaContext* ctx, const BuaValue* v, size_t* out_len);

/// Serialize any value to JSON. Returns NULL on non-serializable values.
char* bua_value_to_json(BuaContext* ctx, const BuaValue* v, size_t* out_len);

void bua_string_free(char* s);

// ---------------------------------------------------------------------------
// Native function registration
// ---------------------------------------------------------------------------

/// Set a native function at path (dot-separated), creating intermediate objects.
/// e.g. bua_set_native(ctx, "Bua.tools.call", fn, data)
bool bua_set_native(
    BuaContext*        ctx,
    const char*        dotted_path,
    BuaNativeFunction  fn,
    void*              user_data
);

// ---------------------------------------------------------------------------
// Heap snapshot
// ---------------------------------------------------------------------------

/// Serialize the current heap to a byte buffer.
/// Caller must free with bua_snapshot_free().
uint8_t* bua_snapshot_create(BuaContext* ctx, size_t* out_size);

/// Restore a context from a previously captured snapshot.
/// The context must be freshly created.
bool bua_snapshot_restore(BuaContext* ctx, const uint8_t* data, size_t size);

void bua_snapshot_free(uint8_t* data);

// ---------------------------------------------------------------------------
// Exception inspection
// ---------------------------------------------------------------------------

const char* bua_exception_message(const BuaException* ex);
const char* bua_exception_stack(const BuaException* ex);
void        bua_exception_free(BuaException* ex);

#ifdef __cplusplus
} // extern "C"
#endif
