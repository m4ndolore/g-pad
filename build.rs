fn main() {
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
