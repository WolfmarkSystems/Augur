//! AUGUR pipeline orchestrator.
//!
//! `augur-core` owns the end-to-end flow: given an evidence input
//! (text / audio / image / video path), it routes through the
//! classifier, decides whether translation is needed, dispatches to
//! the STT / OCR / NLLB sub-engines, and collects a unified result
//! for the CLI or the Strata plugin adapter to present.
//!
//! Sprint 1 scaffold: only `error` + `pipeline` module shells exist.
//! Real orchestration logic lands in Sprint 2+.

pub mod dialect_routing;
pub mod error;
pub mod geoip;
pub mod models;
pub mod pipeline;
pub mod report;
pub mod resilience;
pub mod subtitle;
pub mod timestamps;
pub mod yara_scan;

pub use error::AugurError;

/// Sprint 20 — canonical MT advisory text for the workspace.
/// Source of truth re-exported through `augur-core` so every
/// crate / app can grab it via `augur_core::MT_ADVISORY` without
/// pulling the full `augur-translate` dependency just for the
/// constant. Mirrors `augur_translate::MACHINE_TRANSLATION_NOTICE`
/// — kept in sync with the workspace-wide quality gate test in
/// `crates/augur-core/tests/quality_gate.rs`.
pub const MT_ADVISORY: &str =
    "Machine translation — verify with a certified human translator for legal proceedings.";
