fn main() {
    // The running build's identity, shown on the SYSTEM page. `git` is absent
    // on some build hosts and inside vendored snapshots, so "dev" is a value,
    // not an error.
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let hash = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "dev".to_string());
    println!("cargo:rustc-env=GPAD_BUILD={hash}");

    // A new commit must reach the binary without a source edit. In a worktree
    // `.git` is a file, so ask git where HEAD and the branch tips really live.
    // Only paths that exist are watched: cargo treats a missing path as
    // always-dirty and would recompile the crate on every build.
    for name in ["HEAD", "refs/heads", "packed-refs"] {
        let Some(path) = git(&["rev-parse", "--git-path", name]) else { continue };
        if std::path::Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    if std::env::var("CARGO_FEATURE_TAKEOVER").is_ok() {
        println!("cargo:rerun-if-env-changed=QUILL_BUILD_DIR");
        println!("cargo:rerun-if-env-changed=QUILL_VENDOR_DIR");
        println!("cargo:rerun-if-env-changed=RIDDLE_SDK_SYSROOT_LIB");

        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let target = std::env::var("TARGET").unwrap();
        let quill_build = std::env::var("QUILL_BUILD_DIR")
            .unwrap_or_else(|_| format!("{manifest}/quill/build/{target}"));
        let quill_vendor = std::env::var("QUILL_VENDOR_DIR")
            .unwrap_or_else(|_| format!("{manifest}/quill/vendor/{target}"));
        println!("cargo:rustc-link-search=native={quill_build}");
        println!("cargo:rustc-link-search=native={quill_vendor}");
        println!("cargo:rustc-link-lib=dylib=quill");
        println!("cargo:rustc-link-lib=dylib=qsgepaper");
        println!(
            "cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN:/home/root/quill:/usr/lib/plugins/scenegraph"
        );
        if let Ok(sysroot_lib) = std::env::var("RIDDLE_SDK_SYSROOT_LIB") {
            println!("cargo:rustc-link-arg=-Wl,-rpath-link,{sysroot_lib}");
        }
    }
}
