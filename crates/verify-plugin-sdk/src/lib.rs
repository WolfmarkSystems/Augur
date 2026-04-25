//! Strata plugin adapter — stub in Sprint 1.
//!
//! Sprint 2 wires this to the `strata-plugin-sdk` `StrataPlugin`
//! trait so VERIFY surfaces as an artifact emitter inside Strata,
//! producing `ArtifactRecord`s for every foreign-language finding
//! (with `mitre_technique = T1005` / data from local system).
//!
//! Kept as a thin shell in Sprint 1 — no Strata-side dependency
//! yet, so VERIFY can stand alone as a binary today.

/// Placeholder marker type. Sprint 2 replaces this with a real
/// plugin struct implementing `strata_plugin_sdk::StrataPlugin`.
#[derive(Debug, Default)]
pub struct VerifyStrataPlugin;

impl VerifyStrataPlugin {
    pub fn new() -> Self {
        Self
    }
}
