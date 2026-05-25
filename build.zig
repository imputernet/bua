// build.zig — Bua platform layer build (Zig 0.12)
//
// Builds:
//   - libbua_platform.a  (Zig allocator, OS primitives)
//   - libbua_jsc.a       (C++ JSC bridge, if JSC is available)
//
// Usage:
//   zig build
//   zig build test
//   zig build -Doptimize=ReleaseFast

const std = @import("std");

pub fn build(b: *std.Build) void {
    const target   = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // -----------------------------------------------------------------------
    // Platform library (Zig)
    // -----------------------------------------------------------------------
    const platform = b.addStaticLibrary(.{
        .name    = "bua_platform",
        .root_source_file = b.path("src/platform/root.zig"),
        .target  = target,
        .optimize = optimize,
    });

    platform.addIncludePath(b.path("jsc/include"));
    b.installArtifact(platform);

    // -----------------------------------------------------------------------
    // JSC bridge (C++)
    // -----------------------------------------------------------------------
    const jsc_bridge = b.addStaticLibrary(.{
        .name    = "bua_jsc",
        .target  = target,
        .optimize = optimize,
    });

    jsc_bridge.addCSourceFiles(.{
        .files = &.{"jsc/src/bua_jsc.cpp"},
        .flags = &.{ "-std=c++17", "-fno-exceptions", "-fno-rtti" },
    });
    jsc_bridge.addIncludePath(b.path("jsc/include"));
    jsc_bridge.linkLibCpp();

    // Link JavaScriptCore (macOS ships it in the framework)
    if (target.result.os.tag == .macos) {
        jsc_bridge.linkFramework("JavaScriptCore");
    } else {
        // Linux: expect JSC built from WebKitGTK or custom build
        jsc_bridge.linkSystemLibrary("javascriptcoregtk-4.1");
    }

    b.installArtifact(jsc_bridge);

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------
    const tests = b.addTest(.{
        .root_source_file = b.path("src/platform/root.zig"),
        .target  = target,
        .optimize = optimize,
    });

    const run_tests = b.addRunArtifact(tests);
    const test_step = b.step("test", "Run platform tests");
    test_step.dependOn(&run_tests.step);

    // -----------------------------------------------------------------------
    // Benchmarks
    // -----------------------------------------------------------------------
    const bench = b.addExecutable(.{
        .name    = "bua_bench",
        .root_source_file = b.path("src/bench/main.zig"),
        .target  = target,
        .optimize = .ReleaseFast,
    });
    bench.addIncludePath(b.path("jsc/include"));
    b.installArtifact(bench);

    const run_bench = b.addRunArtifact(bench);
    const bench_step = b.step("bench", "Run benchmarks");
    bench_step.dependOn(&run_bench.step);
}
