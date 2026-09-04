fn main() {
    // Embed the application manifest (common-controls v6, DPI awareness, asInvoker).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("nutils.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("embed manifest");
    }
    delay_load_prism_vendor_dlls();
}

/// Delay-load the screen-reader DLLs Prism imports (`speech` feature, MSVC).
///
/// Prism attaches `/delayload:` link options to its own `prism` target, which is
/// how a machine without Boy PC Reader / ZDSR / PC-Talker installed can still run
/// a Prism binary: the import is resolved on first call, and Prism's failure hook
/// (`source/delayimp.cpp`) turns a missing DLL into "backend unavailable".
///
/// We link Prism as a static library (PRISM_STATIC=1) and rustc performs the
/// final link, so CMake's link options never reach it: the imports come out
/// *static* and Windows refuses to start the process with "byctrl-x64.dll was not
/// found". Re-apply the flags here. The list mirrors `prism_add_import_library`
/// calls in Prism's `cmake/PrismPlatformWindows.cmake`.
fn delay_load_prism_vendor_dlls() {
    if std::env::var_os("CARGO_FEATURE_SPEECH").is_none()
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    // Prism's Orca / speech-dispatcher bridges are Unix-only, so they are left out
    // here: naming a DLL nothing imports only earns an LNK4199 warning.
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let vendor: &[&str] = match arch.as_str() {
        "x86_64" => &["ZDSRAPI_x64.dll", "byctrl-x64.dll", "PCTKUSR.dll"],
        "x86" => &["ZDSRAPI.dll", "byctrl.dll", "PCTKUSR.dll"],
        _ => &["PCTKUSR.dll"],
    };
    for dll in vendor {
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
    }
    // Prism calls __FUnloadDelayLoadedDLL2 when a backend is torn down.
    println!("cargo:rustc-link-arg-bins=/DELAY:unload");
}
