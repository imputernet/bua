// src/platform/clock.zig
const std = @import("std");

/// Monotonic nanosecond clock, never goes backward.
pub fn monotonic_ns() u64 {
    return @intCast(std.time.nanoTimestamp());
}

/// Monotonic microsecond clock.
pub fn monotonic_us() u64 {
    return monotonic_ns() / 1000;
}

/// Wall-clock Unix timestamp in seconds.
pub fn unix_timestamp() i64 {
    return std.time.timestamp();
}

test "clock is monotonic" {
    const a = monotonic_ns();
    const b = monotonic_ns();
    try std.testing.expect(b >= a);
}
