use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../scripts/generate-icons.mjs");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let icon = manifest_dir.join("icons/icon.ico");
    if !icon.exists() {
        let script = manifest_dir.join("../scripts/generate-icons.mjs");
        let node = if cfg!(windows) { "node.exe" } else { "node" };
        let status = Command::new(node)
            .arg(&script)
            .status()
            .unwrap_or_else(|error| panic!("failed to run {}: {error}", script.display()));
        assert!(status.success(), "icon generation failed with {status}");
    }

    tauri_build::build()
}
