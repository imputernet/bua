// src/platform/allocator.zig
// Production allocator: mimalloc when available, fallback to GPA in debug.

const std = @import("std");
const builtin = @import("builtin");

/// The global runtime allocator used by all Bua native code.
/// In ReleaseFast builds this uses mimalloc via the C allocator shim.
/// In Debug builds this uses GeneralPurposeAllocator for leak detection.
pub const runtime_allocator: std.mem.Allocator = blk: {
    if (builtin.mode == .Debug) {
        // GPA catches use-after-free and leaks in debug.
        var gpa = std.heap.GeneralPurposeAllocator(.{
            .safety = true,
            .thread_safe = true,
        }){};
        break :blk gpa.allocator();
    } else {
        // Release: use the C allocator (backed by mimalloc if linked).
        break :blk std.heap.c_allocator;
    }
};

/// Arena allocator for per-request scratch allocations.
/// Not thread-safe; one per agent execution.
pub const ArenaAllocator = struct {
    arena: std.heap.ArenaAllocator,

    pub fn init() ArenaAllocator {
        return .{ .arena = std.heap.ArenaAllocator.init(runtime_allocator) };
    }

    pub fn allocator(self: *ArenaAllocator) std.mem.Allocator {
        return self.arena.allocator();
    }

    /// Free all arena memory in O(1).
    pub fn reset(self: *ArenaAllocator) void {
        _ = self.arena.reset(.retain_capacity);
    }

    pub fn deinit(self: *ArenaAllocator) void {
        self.arena.deinit();
    }
};

test "arena reset" {
    var a = ArenaAllocator.init();
    defer a.deinit();
    const alloc = a.allocator();
    const buf = try alloc.alloc(u8, 1024);
    _ = buf;
    a.reset();
}
