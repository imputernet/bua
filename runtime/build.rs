use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(jsc_available)");
    println!("cargo:rerun-if-env-changed=BUA_SKIP_JSC");
    println!("cargo:rerun-if-env-changed=BUA_JSC_PATH");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let jsc_dir = manifest_dir.parent().unwrap().join("jsc");

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

    let skip_jsc = env::var("BUA_SKIP_JSC").is_ok();
    let jsc_available = !skip_jsc && detect_jsc(&target_os, &jsc_dir);

    if jsc_available {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        compile_jsc_bridge(&jsc_dir, &target_os, &out_dir);
        link_jsc(&target_os);
        println!("cargo:rustc-cfg=jsc_available");
    } else {
        if skip_jsc {
            println!("cargo:warning=BUA_SKIP_JSC is set; building in STUB mode.");
        } else {
            println!("cargo:warning=JSC not found; building in STUB mode.");
        }
    }
}

fn detect_jsc(os: &str, _jsc_dir: &Path) -> bool {
    if let Ok(path) = env::var("BUA_JSC_PATH") {
        if Path::new(&path).exists() {
            return true;
        }
    }
    if os == "macos" || os == "ios" {
        return true;
    }
    if os == "linux" {
        return pkg_config_check("javascriptcoregtk-4.1")
            || pkg_config_check("javascriptcoregtk-4.0");
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

    if os == "macos" || os == "ios" {
        if let Some(sdk) = get_macos_sdk_path() {
            let h = sdk.join("System/Library/Frameworks/JavaScriptCore.framework/Headers");
            if h.exists() {
                build.include(&h);
            }
        }
    } else if os == "linux" {
        for inc in &[
            "/usr/include/webkitgtk-4.1",
            "/usr/include/webkitgtk-4.0",
            "/usr/include/javascriptcoregtk-4.1",
            "/usr/include/javascriptcoregtk-4.0",
        ] {
            if Path::new(inc).exists() {
                build.include(inc);
                break;
            }
        }
    }
    build.compile("bua_jsc");
}

fn link_jsc(os: &str) {
    if os == "macos" || os == "ios" {
        println!("cargo:rustc-link-lib=framework=JavaScriptCore");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    } else if os == "linux" {
        if pkg_config_check("javascriptcoregtk-4.1") {
            println!("cargo:rustc-link-lib=javascriptcoregtk-4.1");
        } else {
            println!("cargo:rustc-link-lib=javascriptcoregtk-4.0");
        }
        println!("cargo:rustc-link-lib=stdc++");
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
