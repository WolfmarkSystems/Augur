// Sprint 18 P2 — inject the workspace VERSION as `AUGUR_VERSION`
// so the binary can render it in About dialogs and CLI output
// without drifting from the .dmg / Cargo.toml version.
fn main() {
    let version = std::fs::read_to_string("../../../VERSION")
        .ok()
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=AUGUR_VERSION={version}");
    println!("cargo:rerun-if-changed=../../../VERSION");
    tauri_build::build()
}
