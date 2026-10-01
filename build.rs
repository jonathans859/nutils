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
/// Prism's backends import vendor DLLs (Boy PC Reader, ZDSR, PC-Talker, …) that
/// most machines don't have. Delay-loaded, an import is resolved on first call
/// and Prism turns a missing DLL into "backend unavailable"; imported normally,
/// Windows refuses to start the process ("byctrl-x64.dll was not found").
///
/// Prism is linked statically and rustc performs the final link, and Cargo only
/// applies link args from the crate that emits them, so prismer can't set these
/// flags itself. It publishes the list instead, matching the backends that were
/// actually built.
fn delay_load_prism_vendor_dlls() {
    println!("cargo:rerun-if-env-changed=DEP_PRISMER_DELAY_LOAD_DLLS");
    if std::env::var_os("CARGO_FEATURE_SPEECH").is_none()
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    // Prism's Orca / speech-dispatcher bridges are Unix-only, so nothing here
    // imports them: naming them only earns an LNK4199 warning.
    const UNIX_ONLY: &[&str] = &["prism_orca_bridge.dll", "prism_speech_dispatcher_bridge.dll"];
    let dlls = std::env::var("DEP_PRISMER_DELAY_LOAD_DLLS").unwrap_or_default();
    for dll in dlls.split(';').filter(|dll| !dll.is_empty() && !UNIX_ONLY.contains(dll)) {
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
    }
    // Prism calls __FUnloadDelayLoadedDLL2 when a backend is torn down; without
    // this the unload is a no-op.
    println!("cargo:rustc-link-arg-bins=/DELAY:unload");
}
