fn main() {
    // Embeds the manifest, app.ico (icon resource 1, used by Explorer and the tray) and the
    // version shown in the file properties.
    println!("cargo::rerun-if-changed=app.rc");
    println!("cargo::rerun-if-changed=app.ico");
    println!("cargo::rerun-if-changed=app.manifest");
    // Taken one by one, so that a suffix such as "-beta" does not end up among the numbers.
    let numbers = [
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH"),
    ];
    let macros = [
        format!("VERSION_NUMBERS={},0", numbers.join(",")),
        format!("VERSION_TEXT=\"{}\"", env!("CARGO_PKG_VERSION")),
    ];
    embed_resource::compile("app.rc", &macros)
        // Without a resource compiler the windows would lack their manifest: fail instead.
        .manifest_required()
        .expect("compiling app.rc failed");
}
