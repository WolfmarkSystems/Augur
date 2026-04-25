//! NLLB-200 translation — stub in Sprint 1.
//!
//! Sprint 2 will wire Meta's NLLB-200 model for 200-language
//! translation, fully offline. For Sprint 1 the CLI's
//! `translate` subcommand prints a `TRANSLATION_STUB` marker so the
//! end-to-end pipeline can be exercised without a real translator.

use verify_core::VerifyError;

/// Sentinel string the CLI emits in Sprint 1 where translated text
/// will later live. Keeping it as a named const makes the
/// Sprint 2 replacement grep-able.
pub const TRANSLATION_STUB: &str = "STUB — NLLB-200 integration coming in Sprint 2";

/// Sprint 1 placeholder. Returns the stub sentinel so a caller
/// wiring up the pipeline today sees a clear, intentional marker
/// instead of an error.
pub fn translate_stub(
    _source_text: &str,
    _source_language: &str,
    _target_language: &str,
) -> Result<String, VerifyError> {
    Ok(TRANSLATION_STUB.to_string())
}
