fn main() {
    // Embeds the manifest and app.ico (icon resource 1, used by Explorer and the tray).
    println!("cargo::rerun-if-changed=app.rc");
    println!("cargo::rerun-if-changed=app.ico");
    println!("cargo::rerun-if-changed=app.manifest");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("compiling app.rc failed");
}
