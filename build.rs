fn main() {
    // Embeds app.ico as icon resource 1, used by Explorer and the tray.
    println!("cargo::rerun-if-changed=app.rc");
    println!("cargo::rerun-if-changed=app.ico");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("compiling app.rc failed");
}
