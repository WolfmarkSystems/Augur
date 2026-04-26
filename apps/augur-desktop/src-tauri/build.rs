// Sprint 18 P2 — inject the workspace VERSION as `AUGUR_VERSION`.
// See augur-installer/src-tauri/build.rs for the rationale.
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
