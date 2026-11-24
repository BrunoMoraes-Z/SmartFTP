fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");
    // Icon resource #1: used by Explorer for the .exe and by gpui for the window / taskbar icon.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
