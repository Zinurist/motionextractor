//! Makes sure the embedded frontend folder exists so the crate builds before the web UI is built.

use std::path::Path;

fn main() {
    // Read at run time: `env!` would bake in the path of the checkout the script was compiled in,
    // which breaks when a target directory is reused from another location.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let dist = Path::new(&manifest_dir).join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if !dist.join("index.html").exists() {
        std::fs::create_dir_all(&dist).expect("create web/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>Motion Extractor</title>\
             <p>The web UI has not been built. Run <code>npm --prefix web run build</code>.</p>",
        )
        .expect("write placeholder index.html");
    }
}
