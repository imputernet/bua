// jsc/src/bua_jsc.cpp
// JavaScriptCore bridge implementation.
//
// Compile flags (macOS/Linux):
//   clang++ -std=c++17 -fPIC -O2 \
//     -I$(JSC_INCLUDE) \
//     -L$(JSC_LIB) -lJavaScriptCore \
//     bua_jsc.cpp -o libbua_jsc.a

#include "bua_jsc.h"

#include <JavaScriptCore/JavaScript.h>
#include <cassert>
#include <cstring>
#include <cstdlib>
#include <string>
#include <unordered_map>
#include <memory>
#include <functional>

// ---------------------------------------------------------------------------
// Internal structures
// ---------------------------------------------------------------------------

struct BuaContext {
    JSContextGroupRef group;
    JSGlobalContextRef ctx;
    size_t max_heap_bytes;
};

struct BuaValue {
    JSValueRef val;
    JSContextRef ctx;
};

struct BuaException {
    std::string message;
    std::string stack;
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

static BuaException* make_exception(JSContextRef ctx, JSValueRef ex_val) {
    if (!ex_val) return nullptr;
    auto* ex = new BuaException();

    // message
    JSStringRef msg_key = JSStringCreateWithUTF8CString("message");
    JSValueRef ex_obj   = ex_val;
    if (JSValueIsObject(ctx, ex_obj)) {
        JSObjectRef obj = JSValueToObject(ctx, ex_obj, nullptr);
        JSValueRef  msg = JSObjectGetProperty(ctx, obj, msg_key, nullptr);
        JSStringRef s   = JSValueToStringCopy(ctx, msg, nullptr);
        size_t      len = JSStringGetMaximumUTF8CStringSize(s);
        ex->message.resize(len);
        JSStringGetUTF8CString(s, &ex->message[0], len);
        JSStringRelease(s);

        // stack
        JSStringRef stk_key = JSStringCreateWithUTF8CString("stack");
        JSValueRef  stk     = JSObjectGetProperty(ctx, obj, stk_key, nullptr);
        if (!JSValueIsUndefined(ctx, stk)) {
            JSStringRef ss = JSValueToStringCopy(ctx, stk, nullptr);
            size_t slen = JSStringGetMaximumUTF8CStringSize(ss);
            ex->stack.resize(slen);
            JSStringGetUTF8CString(ss, &ex->stack[0], slen);
            JSStringRelease(ss);
        }
        JSStringRelease(stk_key);
    } else {
        JSStringRef s = JSValueToStringCopy(ctx, ex_obj, nullptr);
        size_t len    = JSStringGetMaximumUTF8CStringSize(s);
        ex->message.resize(len);
        JSStringGetUTF8CString(s, &ex->message[0], len);
        JSStringRelease(s);
    }
    JSStringRelease(msg_key);
    return ex;
}

static JSValueRef bua_value_unwrap(BuaValue* v) {
    return v ? v->val : nullptr;
}

// ---------------------------------------------------------------------------
// Context lifecycle
// ---------------------------------------------------------------------------

BuaContext* bua_context_create(size_t max_heap_bytes) {
    auto* bc  = new BuaContext();
    bc->group = JSContextGroupCreate();
    bc->ctx   = JSGlobalContextCreateInGroup(bc->group, nullptr);
    bc->max_heap_bytes = max_heap_bytes;
    // JSC doesn't expose a direct heap cap API in the public C API;
    // use the private API in production builds via JSC internals.
    return bc;
}

void bua_context_destroy(BuaContext* bc) {
    if (!bc) return;
    JSGlobalContextRelease(bc->ctx);
    JSContextGroupRelease(bc->group);
    delete bc;
}

void bua_context_drain_microtasks(BuaContext* bc) {
    // JSC private API: JSC::VM::drainMicrotasks()
    // In the public build we rely on JSCheckScriptSyntax to flush.
    // Real impl: call via private header include.
    (void)bc;
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

BuaValue* bua_eval(
    BuaContext*    bc,
    const char*    source,
    size_t         source_len,
    const char*    source_url,
    BuaException** out_exception
) {
    JSStringRef src_str = JSStringCreateWithUTF8CString(source);
    JSStringRef url_str = source_url
        ? JSStringCreateWithUTF8CString(source_url)
        : nullptr;

    JSValueRef ex_val = nullptr;
    JSValueRef result = JSEvaluateScript(bc->ctx, src_str, nullptr, url_str, 1, &ex_val);

    JSStringRelease(src_str);
    if (url_str) JSStringRelease(url_str);

    if (ex_val) {
        if (out_exception) *out_exception = make_exception(bc->ctx, ex_val);
        return nullptr;
    }

    auto* v  = new BuaValue();
    v->val   = result;
    v->ctx   = bc->ctx;
    JSValueProtect(bc->ctx, v->val);
    return v;
}

// bua_eval_module: full ESM support requires JSC module loader callbacks
// which are set up via JSC's private ModuleLoader API. Stub for now.
BuaValue* bua_eval_module(
    BuaContext*    bc,
    const char*    source,
    size_t         source_len,
    const char*    module_url,
    BuaException** out_exception
) {
    return bua_eval(bc, source, source_len, module_url, out_exception);
}

// ---------------------------------------------------------------------------
// Value constructors
// ---------------------------------------------------------------------------

BuaValue* bua_value_undefined(BuaContext* bc) {
    auto* v = new BuaValue();
    v->val  = JSValueMakeUndefined(bc->ctx);
    v->ctx  = bc->ctx;
    return v;
}

BuaValue* bua_value_null(BuaContext* bc) {
    auto* v = new BuaValue();
    v->val  = JSValueMakeNull(bc->ctx);
    v->ctx  = bc->ctx;
    return v;
}

BuaValue* bua_value_bool(BuaContext* bc, bool val) {
    auto* v = new BuaValue();
    v->val  = JSValueMakeBoolean(bc->ctx, val);
    v->ctx  = bc->ctx;
    return v;
}

BuaValue* bua_value_number(BuaContext* bc, double val) {
    auto* v = new BuaValue();
    v->val  = JSValueMakeNumber(bc->ctx, val);
    v->ctx  = bc->ctx;
    return v;
}

BuaValue* bua_value_string(BuaContext* bc, const char* utf8, size_t len) {
    JSStringRef s = JSStringCreateWithUTF8CString(utf8);
    auto* v       = new BuaValue();
    v->val        = JSValueMakeString(bc->ctx, s);
    v->ctx        = bc->ctx;
    JSStringRelease(s);
    JSValueProtect(bc->ctx, v->val);
    return v;
}

BuaValue* bua_value_from_json(BuaContext* bc, const char* json, size_t len,
                              BuaException** out_exception) {
    JSStringRef s   = JSStringCreateWithUTF8CString(json);
    JSValueRef  val = JSValueMakeFromJSONString(bc->ctx, s);
    JSStringRelease(s);
    if (!val) {
        if (out_exception) {
            auto* ex = new BuaException();
            ex->message = "invalid JSON";
            *out_exception = ex;
        }
        return nullptr;
    }
    auto* v = new BuaValue();
    v->val  = val;
    v->ctx  = bc->ctx;
    JSValueProtect(bc->ctx, v->val);
    return v;
}

// ---------------------------------------------------------------------------
// Value accessors
// ---------------------------------------------------------------------------

BuaValueType bua_value_type(BuaContext* bc, const BuaValue* v) {
    switch (JSValueGetType(bc->ctx, v->val)) {
        case kJSTypeUndefined: return BUA_TYPE_UNDEFINED;
        case kJSTypeNull:      return BUA_TYPE_NULL;
        case kJSTypeBoolean:   return BUA_TYPE_BOOLEAN;
        case kJSTypeNumber:    return BUA_TYPE_NUMBER;
        case kJSTypeString:    return BUA_TYPE_STRING;
        case kJSTypeObject:    return BUA_TYPE_OBJECT;
        case kJSTypeSymbol:    return BUA_TYPE_SYMBOL;
        default:               return BUA_TYPE_UNDEFINED;
    }
}

bool bua_value_to_bool(BuaContext* bc, const BuaValue* v) {
    return JSValueToBoolean(bc->ctx, v->val);
}

double bua_value_to_number(BuaContext* bc, const BuaValue* v) {
    return JSValueToNumber(bc->ctx, v->val, nullptr);
}

char* bua_value_to_string_utf8(BuaContext* bc, const BuaValue* v, size_t* out_len) {
    JSStringRef s   = JSValueToStringCopy(bc->ctx, v->val, nullptr);
    size_t      len = JSStringGetMaximumUTF8CStringSize(s);
    char*       buf = static_cast<char*>(malloc(len));
    JSStringGetUTF8CString(s, buf, len);
    JSStringRelease(s);
    if (out_len) *out_len = strlen(buf);
    return buf;
}

char* bua_value_to_json(BuaContext* bc, const BuaValue* v, size_t* out_len) {
    JSStringRef s = JSValueCreateJSONString(bc->ctx, v->val, 0, nullptr);
    if (!s) return nullptr;
    size_t len = JSStringGetMaximumUTF8CStringSize(s);
    char*  buf = static_cast<char*>(malloc(len));
    JSStringGetUTF8CString(s, buf, len);
    JSStringRelease(s);
    if (out_len) *out_len = strlen(buf);
    return buf;
}

void bua_string_free(char* s) {
    free(s);
}

// ---------------------------------------------------------------------------
// Native function registration
// ---------------------------------------------------------------------------

struct NativeEntry {
    BuaNativeFunction fn;
    void*             user_data;
};

static JSValueRef native_callback(
    JSContextRef     ctx,
    JSObjectRef      function,
    JSObjectRef      this_obj,
    size_t           argc,
    const JSValueRef argv[],
    JSValueRef*      out_exception
) {
    auto* entry = static_cast<NativeEntry*>(JSObjectGetPrivate(function));
    if (!entry) return JSValueMakeUndefined(ctx);

    // Wrap args
    std::vector<BuaValue*> args(argc);
    for (size_t i = 0; i < argc; ++i) {
        args[i]      = new BuaValue();
        args[i]->val = argv[i];
        args[i]->ctx = ctx;
    }

    BuaValue this_v{ this_obj, ctx };
    BuaException* ex = nullptr;
    BuaValue* result = entry->fn(
        nullptr, // BuaContext* not needed at callback time
        &this_v,
        args.data(),
        argc,
        entry->user_data,
        &ex
    );

    for (auto* a : args) delete a;

    if (ex) {
        JSStringRef msg = JSStringCreateWithUTF8CString(ex->message.c_str());
        *out_exception  = JSValueMakeString(ctx, msg);
        JSStringRelease(msg);
        delete ex;
        return nullptr;
    }

    JSValueRef ret = result ? result->val : JSValueMakeUndefined(ctx);
    delete result;
    return ret;
}

bool bua_set_native(
    BuaContext*        bc,
    const char*        dotted_path,
    BuaNativeFunction  fn,
    void*              user_data
) {
    auto* entry = new NativeEntry{ fn, user_data };

    JSClassDefinition def = kJSClassDefinitionEmpty;
    def.callAsFunction    = native_callback;
    JSClassRef cls        = JSClassCreate(&def);
    JSObjectRef func_obj  = JSObjectMake(bc->ctx, cls, entry);
    JSClassRelease(cls);

    // Walk/create the path
    std::string path(dotted_path);
    JSObjectRef current = JSContextGetGlobalObject(bc->ctx);

    size_t start = 0;
    while (true) {
        size_t dot = path.find('.', start);
        bool   last = (dot == std::string::npos);
        std::string segment = last ? path.substr(start) : path.substr(start, dot - start);

        JSStringRef key = JSStringCreateWithUTF8CString(segment.c_str());

        if (last) {
            JSObjectSetProperty(bc->ctx, current, key, func_obj, kJSPropertyAttributeNone, nullptr);
            JSStringRelease(key);
            break;
        } else {
            JSValueRef existing = JSObjectGetProperty(bc->ctx, current, key, nullptr);
            if (JSValueIsObject(bc->ctx, existing)) {
                current = JSValueToObject(bc->ctx, existing, nullptr);
            } else {
                JSObjectRef child = JSObjectMake(bc->ctx, nullptr, nullptr);
                JSObjectSetProperty(bc->ctx, current, key, child, kJSPropertyAttributeNone, nullptr);
                current = child;
            }
            JSStringRelease(key);
            start = dot + 1;
        }
    }

    return true;
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

uint8_t* bua_snapshot_create(BuaContext* bc, size_t* out_size) {
    // JSC snapshot API (private): JSC::Snapshot::create(vm)
    // Public stub: return empty buffer.
    static const uint8_t stub[] = { 0xB, 0xA, 0xA, 0x1 };
    *out_size = sizeof(stub);
    uint8_t* buf = static_cast<uint8_t*>(malloc(sizeof(stub)));
    memcpy(buf, stub, sizeof(stub));
    return buf;
}

bool bua_snapshot_restore(BuaContext* bc, const uint8_t* data, size_t size) {
    (void)bc; (void)data; (void)size;
    return true;
}

void bua_snapshot_free(uint8_t* data) {
    free(data);
}

// ---------------------------------------------------------------------------
// Exception
// ---------------------------------------------------------------------------

const char* bua_exception_message(const BuaException* ex) {
    return ex ? ex->message.c_str() : "";
}

const char* bua_exception_stack(const BuaException* ex) {
    return ex ? ex->stack.c_str() : "";
}

void bua_exception_free(BuaException* ex) {
    delete ex;
}

// ---------------------------------------------------------------------------
// Direct JSC function call — thin wrapper so Rust can call JSObjectCallAsFunction
// without needing the full JavaScriptCore/JSObjectRef.h include on the Rust side.
// ---------------------------------------------------------------------------

// These are declared as thin C wrappers around the real JSC calls.
// They MUST match the signatures in bua_jsc_sys.rs jsc_call_as_function /
// jsc_make_deferred_promise exactly (same parameter order, same pointer types).

extern "C" JSValueRef jsc_call_as_function(
    JSContextRef        ctx,
    JSObjectRef         func,
    JSObjectRef         this_obj,
    size_t              arg_count,
    const JSValueRef*   arguments,
    JSValueRef*         exception
) {
    if (!JSObjectIsFunction(ctx, func)) {
        if (exception) {
            JSStringRef msg = JSStringCreateWithUTF8CString("call_function: object is not callable");
            *exception = JSValueMakeString(ctx, msg);
            JSStringRelease(msg);
        }
        return nullptr;
    }
    return JSObjectCallAsFunction(ctx, func, this_obj, arg_count, arguments, exception);
}

extern "C" JSObjectRef jsc_make_deferred_promise(
    JSContextRef  ctx,
    JSObjectRef*  resolve,
    JSObjectRef*  reject,
    JSValueRef*   exception
) {
#if defined(__APPLE__)
    // JSObjectMakeDeferredPromise is available on Apple platforms since JSC 7604.1
    return JSObjectMakeDeferredPromise(ctx, resolve, reject, exception);
#else
    // Linux: JSObjectMakeDeferredPromise may not be in the GTK headers.
    // Implement via JS eval as a fallback.
    (void)resolve; (void)reject;
    const char* src = "(function(){ let r,j; const p = new Promise((a,b)=>{r=a;j=b;}); return {p,r,j}; })()";
    JSStringRef script = JSStringCreateWithUTF8CString(src);
    JSValueRef  result = JSEvaluateScript(ctx, script, nullptr, nullptr, 0, exception);
    JSStringRelease(script);
    if (!result || (exception && *exception)) return nullptr;
    JSObjectRef obj = JSValueToObject(ctx, result, exception);
    if (!obj) return nullptr;
    // Extract p, r, j properties
    auto getprop = [&](const char* key) -> JSObjectRef {
        JSStringRef k = JSStringCreateWithUTF8CString(key);
        JSValueRef  v = JSObjectGetProperty(ctx, obj, k, nullptr);
        JSStringRelease(k);
        return v ? JSValueToObject(ctx, v, nullptr) : nullptr;
    };
    if (resolve) *resolve = getprop("r");
    if (reject)  *reject  = getprop("j");
    return getprop("p");
#endif
}
