//! Embeds the window icon and the application manifest (per-monitor DPI, UTF-8 code
//! page, common controls 6) into `freejazz.exe`.

fn main() {
    println!("cargo:rerun-if-changed=assets/freejazz.rc");
    println!("cargo:rerun-if-changed=assets/freejazz.manifest");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/freejazz.rc", embed_resource::NONE)
            .manifest_required()
            .expect("compile Windows resources");
    }
}
