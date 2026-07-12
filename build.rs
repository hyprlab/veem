//! Compile the bundled icon set (resources/veem.gresource.xml) into a
//! `.gresource` blob that is embedded in the binary and registered at startup
//! (see `main.rs`). This lets every symbolic icon Veem draws render identically
//! on any distribution, regardless of the host icon theme — the icons are
//! prefixed with the app ID so no system theme can override them.

fn main() {
    println!("cargo:rerun-if-changed=resources/veem.gresource.xml");
    println!("cargo:rerun-if-changed=resources/icons");
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/veem.gresource.xml",
        "veem.gresource",
    );
}
