// runtime/build.rs
//
// Platform-aware build script for bua-runtime.
//
// Responsibilities:
//   1. Detect JSC availability (macOS framework, Linux GTK, custom path)
//   2. Compile the C++ JSC bridge (jsc/src/bua_jsc.cpp)
//   3. Configure linker flags per platform
//   4. Gate the `jsc` feature so stub mode works without JSC installed
//   5. Emit cargo metadata for downstream crates

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(jsc_available)");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let jsc_dir = manifest_dir.parent().unwrap().join("jsc");

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rerun-if-changed={}",
        jsc_dir.join("src/bua_jsc.cpp").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        jsc_dir.join("include/bua_jsc.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        jsc_dir.join("bindings/bua_jsc_sys.rs").display()
    );

    let jsc_available = detect_jsc(&target_os, &jsc_dir);

    if jsc_available {
        compile_jsc_bridge(&jsc_dir, &target_os, &out_dir);
        link_jsc(&target_os);
        println!("cargo:rustc-cfg=feature=\"jsc\"");
        println!("cargo:rustc-cfg=jsc_available");
    } else {
        eprintln!(
            "cargo:warning=JSC not found — building with stub engine. \
             Set BUA_JSC_PATH or install WebKit/JavaScriptCore to enable real JS execution."
        );
    }
}

fn detect_jsc(os: &str, _jsc_dir: &Path) -> bool {
    // 1. Explicit override via env var
    if let Ok(path) = env::var("BUA_JSC_PATH") {
        let lib_path = Path::new(&path);
        if lib_path.exists() {
            println!("cargo:rustc-link-search=native={}", lib_path.display());
            return true;
        }
    }

    // 2. macOS system framework
    if os == "macos" || os == "ios" {
        // JavaScriptCore.framework ships with Xcode and macOS SDK
        let sdk_path = get_macos_sdk_path();
        if let Some(sdk) = sdk_path {
            let framework_path = sdk.join("System/Library/Frameworks");
            if framework_path.join("JavaScriptCore.framework").exists() {
                return true;
            }
        }
        // Fallback: assume macOS has it (true for macOS 10.9+)
        if os == "macos" {
            return true;
        }
    }

    // 3. Linux: pkg-config for javascriptcoregtk
    if os == "linux" {
        if pkg_config_check("javascriptcoregtk-4.1") || pkg_config_check("javascriptcoregtk-4.0") {
            return true;
        }
        // Check common paths
        for path in &[
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib/aarch64-linux-gnu",
            "/usr/local/lib",
        ] {
            if Path::new(path).join("libjavascriptcoregtk-4.1.so").exists() {
                println!("cargo:rustc-link-search=native={path}");
                return true;
            }
        }
    }

    false
}

fn compile_jsc_bridge(jsc_dir: &Path, os: &str, _out_dir: &Path) {
    let mut build = cc::Build::new();

    build
        .cpp(true)
        .file(jsc_dir.join("src/bua_jsc.cpp"))
        .include(jsc_dir.join("include"))
        .flag("-std=c++17")
        .flag("-fno-exceptions")
        .flag("-fno-rtti")
        .flag("-O2")
        .flag("-fvisibility=hidden");

    // Platform-specific headers
    match os {
        "macos" | "ios" => {
            // JavaScriptCore.framework header path
            if let Some(sdk) = get_macos_sdk_path() {
                let jsc_headers =
                    sdk.join("System/Library/Frameworks/JavaScriptCore.framework/Headers");
                if jsc_headers.exists() {
                    build.include(&jsc_headers);
                }
            }
        }
        "linux" => {
            // Try common GTK JSC header paths
            for inc in &[
                "/usr/include/webkitgtk-4.1",
                "/usr/include/webkitgtk-4.0",
                "/usr/local/include/webkitgtk-4.1",
            ] {
                if Path::new(inc).exists() {
                    build.include(inc);
                    break;
                }
            }
        }
        _ => {}
    }

    build.compile("bua_jsc");
    println!("cargo:rustc-link-lib=static=bua_jsc");
}

fn link_jsc(os: &str) {
    match os {
        "macos" | "ios" => {
            println!("cargo:rustc-link-lib=framework=JavaScriptCore");
            println!("cargo:rustc-link-lib=framework=CoreFoundation");
        }
        "linux" => {
            // Try 4.1 first, fall back to 4.0
            if pkg_config_check("javascriptcoregtk-4.1") {
                println!("cargo:rustc-link-lib=javascriptcoregtk-4.1");
            } else {
                println!("cargo:rustc-link-lib=javascriptcoregtk-4.0");
            }
            println!("cargo:rustc-link-lib=stdc++");
        }
        _ => {
            println!("cargo:warning=Unsupported platform for JSC linking: {os}");
        }
    }
}

fn pkg_config_check(lib: &str) -> bool {
    std::process::Command::new("pkg-config")
        .args(["--exists", lib])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn get_macos_sdk_path() -> Option<PathBuf> {
    let output = std::process::Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?.trim().to_string();
        Some(PathBuf::from(path))
    } else {
        None
    }
}
