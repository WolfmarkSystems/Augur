//! VERIFY pipeline orchestrator.
//!
//! `verify-core` owns the end-to-end flow: given an evidence input
//! (text / audio / image / video path), it routes through the
//! classifier, decides whether translation is needed, dispatches to
//! the STT / OCR / NLLB sub-engines, and collects a unified result
//! for the CLI or the Strata plugin adapter to present.
//!
//! Sprint 1 scaffold: only `error` + `pipeline` module shells exist.
//! Real orchestration logic lands in Sprint 2+.

pub mod error;
pub mod geoip;
pub mod pipeline;
pub mod report;
pub mod timestamps;

pub use error::VerifyError;
