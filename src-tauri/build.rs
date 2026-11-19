fn main() {
    tauri_build::build();

    // The lib test binary needs the Common Controls v6 manifest too.
    //
    // tauri-build embeds the manifest only in the app binary
    // (tauri-winres `compile()` links the resource into binaries, not
    // test harnesses), so without it the test binary loads comctl32 v5,
    // which does not export TaskDialogIndirect, and the Windows loader
    // refuses the whole binary with 0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND)
    // before any test runs. CI proved it: of the 334 imports in the
    // test binary, TaskDialogIndirect was the only one that did not
    // resolve on the runner.
    //
    // MSVC only: the GNU linker does not understand /MANIFESTINPUT.
    // The app binary keeps tauri-build's copy; the linker merges the
    // two identical manifests into one.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("app.manifest");
        println!("cargo:rerun-if-changed=app.manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
