fn main() {
    // Embed the application manifest (common-controls v6, DPI awareness, asInvoker).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("nutils.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("embed manifest");
    }
}
