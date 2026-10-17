fn main() {
    #[cfg(feature = "desktop")]
    {
        // tauri-build checks that the frontend folder exists; the server crate creates a placeholder
        // too, but its build script may not have run yet.
        let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
        if !dist.join("index.html").exists() {
            std::fs::create_dir_all(&dist).expect("create web/dist");
            std::fs::write(
                dist.join("index.html"),
                "<!doctype html><title>Motion Extractor</title>",
            )
            .expect("write placeholder");
        }
        tauri_build::build();
    }
}
