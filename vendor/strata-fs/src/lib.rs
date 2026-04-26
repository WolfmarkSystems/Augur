//! Minimal `strata-fs` stub.
//!
//! Vendored alongside `strata-plugin-sdk` so VERIFY's
//! `--features verify-plugin-sdk/strata` build does NOT pull in
//! the upstream NTFS / APFS / ext4 / EWF / sysinfo parser tree.
//! VERIFY's Strata plugin does not need any VFS-backed evidence
//! containers — it walks `PluginContext::root_path` directly via
//! `std::fs::read_dir` — but the upstream SDK re-exports the
//! `VirtualFilesystem` trait at its public boundary (and uses it
//! internally inside `PluginContext::list_dir` /
//! `PluginContext::find_by_name`), so we have to provide a
//! compile-compatible shape here.
//!
//! Sprint 8 P1 — see CLAUDE.md.

pub mod vfs {
    use serde::{Deserialize, Serialize};
    use std::fmt;

    /// Result type used at the VFS boundary. Stubbed at
    /// `Result<T, VfsError>` — same shape as upstream.
    pub type VfsResult<T> = Result<T, VfsError>;

    /// Minimal error enum. The stub never produces these in
    /// practice (VERIFY's plugin never instantiates a VFS); a
    /// hypothetical caller that *does* mount one is responsible
    /// for plugging in a real `strata-fs` build.
    #[derive(Debug)]
    pub enum VfsError {
        NotFound,
        Io(String),
        Unsupported,
    }

    impl fmt::Display for VfsError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::NotFound => f.write_str("vfs: not found"),
                Self::Io(s) => write!(f, "vfs io: {s}"),
                Self::Unsupported => f.write_str("vfs: unsupported"),
            }
        }
    }

    impl std::error::Error for VfsError {}

    /// Walk decision returned from a `VirtualFilesystem::walk`
    /// filter callback. Variants match upstream byte-for-byte.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum WalkDecision {
        Descend,
        Skip,
        Stop,
    }

    /// Trimmed `VfsEntry`. Upstream carries timestamps + filesystem-
    /// specific metadata (NTFS MFT records, APFS object IDs, …);
    /// the SDK only reads `path / name / is_directory`, so those
    /// are the only fields we model.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct VfsEntry {
        pub path: String,
        pub name: String,
        pub is_directory: bool,
    }

    /// VFS trait — same method shape as upstream so the SDK's
    /// `Option<Arc<dyn VirtualFilesystem>>` field still type-checks.
    pub trait VirtualFilesystem: Send + Sync {
        fn fs_type(&self) -> &'static str;
        fn list_dir(&self, path: &str) -> VfsResult<Vec<VfsEntry>>;
        fn read_file(&self, path: &str) -> VfsResult<Vec<u8>>;
        fn exists(&self, path: &str) -> bool;
        fn walk(&self, filter: &mut dyn FnMut(&VfsEntry) -> WalkDecision)
            -> VfsResult<()>;
    }
}
