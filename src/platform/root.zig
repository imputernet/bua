// src/platform/root.zig
// Bua platform layer: custom allocators, OS-specific primitives.

const std = @import("std");

pub const allocator = @import("allocator.zig");
pub const mmap      = @import("mmap.zig");
pub const clock     = @import("clock.zig");

// Re-export the primary runtime allocator for use by Rust via C FFI.
pub export fn bua_alloc(size: usize, alignment: usize) ?*anyopaque {
    const mem = allocator.runtime_allocator.rawAlloc(size, @intCast(alignment), @returnAddress()) orelse return null;
    return mem.ptr;
}

pub export fn bua_free(ptr: *anyopaque, size: usize, alignment: usize) void {
    const slice = @as([*]u8, @ptrCast(ptr))[0..size];
    allocator.runtime_allocator.rawFree(slice, @intCast(alignment), @returnAddress());
}

pub export fn bua_realloc(ptr: *anyopaque, old_size: usize, new_size: usize, alignment: usize) ?*anyopaque {
    const old_slice = @as([*]u8, @ptrCast(ptr))[0..old_size];
    const mem = allocator.runtime_allocator.rawResize(old_slice, @intCast(alignment), new_size, @returnAddress());
    if (mem) return ptr;
    // Fallback: alloc new, copy, free old
    const new_mem = allocator.runtime_allocator.rawAlloc(new_size, @intCast(alignment), @returnAddress()) orelse return null;
    const copy_len = @min(old_size, new_size);
    @memcpy(new_mem[0..copy_len], old_slice[0..copy_len]);
    allocator.runtime_allocator.rawFree(old_slice, @intCast(alignment), @returnAddress());
    return new_mem.ptr;
}

/// High-resolution monotonic timestamp in nanoseconds.
pub export fn bua_clock_ns() u64 {
    return clock.monotonic_ns();
}

test "bua_alloc roundtrip" {
    const ptr = bua_alloc(64, 8) orelse unreachable;
    bua_free(ptr, 64, 8);
}
