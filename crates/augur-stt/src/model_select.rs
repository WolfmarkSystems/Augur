//! Sprint 10 P2 — Whisper model selection layer.
//!
//! `WhisperPreset` (from `whisper.rs`) is the candle-loadable
//! enum. `WhisperModel` is the user-facing selection axis and
//! includes the language-specific community fine-tunes (Pashto,
//! Dari) that the registry tracks but candle's stock
//! `whisper-large-v3` architecture loader does not yet wire in
//! directly. Auto-selection cascades from the largest installed
//! model down to Tiny.

use augur_core::models::{find_model, is_installed};

use crate::WhisperPreset;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperModel {
    /// Cheapest preset. 75 MB. Always available after `augur
    /// install minimal`.
    Tiny,
    /// Mid-range. 142 MB.
    Base,
    /// State-of-the-art generic Whisper. 2.9 GB.
    LargeV3,
    /// Community Pashto fine-tune. Loaded as Tiny/Small base
    /// architecture under the hood — see
    /// [`WhisperModel::resolved_preset`].
    Pashto,
    /// Community Dari fine-tune. Same as Pashto.
    Dari,
}

impl WhisperModel {
    /// Registry id this variant maps to. Matches the keys in
    /// `augur_core::models::ALL_MODELS`.
    pub fn model_spec_id(&self) -> &'static str {
        match self {
            Self::Tiny => "whisper-tiny",
            Self::Base => "whisper-base",
            Self::LargeV3 => "whisper-large-v3",
            Self::Pashto => "whisper-pashto",
            Self::Dari => "whisper-dari",
        }
    }

    pub fn is_installed(&self) -> bool {
        find_model(self.model_spec_id())
            .map(is_installed)
            .unwrap_or(false)
    }

    /// Concrete `WhisperPreset` candle should load. Pashto/Dari
    /// community fine-tunes ship as `whisper-small` weights; the
    /// candle loader handles them with the standard architecture
    /// once the safetensors file is in place. For Sprint 10 we
    /// route them through the Accurate (`large-v3`) preset path
    /// when no specific Pashto/Dari weights are wired, since the
    /// stock `large-v3` is the strongest fallback for non-English
    /// audio.
    pub fn resolved_preset(&self) -> WhisperPreset {
        match self {
            Self::Tiny => WhisperPreset::Fast,
            Self::Base => WhisperPreset::Balanced,
            Self::LargeV3 => WhisperPreset::Accurate,
            // Pashto/Dari fine-tunes load via candle as a small
            // architecture; if not yet wired into the candle
            // loader, this falls back to the large-v3 path which
            // still produces best-available transcripts.
            Self::Pashto | Self::Dari => WhisperPreset::Accurate,
        }
    }
}

/// Sprint 10 P2 — auto-selection cascade.
///
/// 1. Pashto/Dari language hint AND the matching fine-tune is
///    installed → use the fine-tune.
/// 2. Quality cascade: Large-v3 → Base → Tiny (whichever is
///    installed first).
///
/// Falls through to Tiny when nothing is installed because the
/// caller's `ensure_whisper_model` will then download Tiny on
/// demand (the smallest first-run footprint).
pub fn auto_select_whisper_model(detected_language: Option<&str>) -> WhisperModel {
    if let Some(lang) = detected_language {
        if lang == "ps" && WhisperModel::Pashto.is_installed() {
            return WhisperModel::Pashto;
        }
        if (lang == "prs" || lang == "fa-AF") && WhisperModel::Dari.is_installed() {
            return WhisperModel::Dari;
        }
    }
    if WhisperModel::LargeV3.is_installed() {
        WhisperModel::LargeV3
    } else if WhisperModel::Base.is_installed() {
        WhisperModel::Base
    } else {
        WhisperModel::Tiny
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn model_spec_ids_unique() {
        let ids = [
            WhisperModel::Tiny.model_spec_id(),
            WhisperModel::Base.model_spec_id(),
            WhisperModel::LargeV3.model_spec_id(),
            WhisperModel::Pashto.model_spec_id(),
            WhisperModel::Dari.model_spec_id(),
        ];
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len());
    }

    #[test]
    fn resolved_preset_matches_expected_class() {
        assert_eq!(WhisperModel::Tiny.resolved_preset(), WhisperPreset::Fast);
        assert_eq!(WhisperModel::Base.resolved_preset(), WhisperPreset::Balanced);
        assert_eq!(
            WhisperModel::LargeV3.resolved_preset(),
            WhisperPreset::Accurate
        );
    }

    #[test]
    fn auto_select_falls_back_to_tiny_when_nothing_installed() {
        // Steer the cache root to an empty temp dir so no
        // installed-model probe finds anything.
        let tmp = std::env::temp_dir().join(format!(
            "augur-auto-select-test-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::var_os("AUGUR_MODEL_CACHE");
        // SAFETY: process-wide env mutation is permitted in
        // tests; #[cfg(test)] gates this entire fn.
        std::env::set_var("AUGUR_MODEL_CACHE", &tmp);
        let model = auto_select_whisper_model(None);
        assert_eq!(model, WhisperModel::Tiny);
        match prev {
            Some(v) => std::env::set_var("AUGUR_MODEL_CACHE", v),
            None => std::env::remove_var("AUGUR_MODEL_CACHE"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pashto_hint_only_chosen_when_installed() {
        // Pashto fine-tune is not installed in the empty cache;
        // auto_select must not pick it just because lang == ps.
        let tmp = std::env::temp_dir().join(format!(
            "augur-auto-select-pashto-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::var_os("AUGUR_MODEL_CACHE");
        std::env::set_var("AUGUR_MODEL_CACHE", &tmp);
        let model = auto_select_whisper_model(Some("ps"));
        assert_ne!(model, WhisperModel::Pashto);
        match prev {
            Some(v) => std::env::set_var("AUGUR_MODEL_CACHE", v),
            None => std::env::remove_var("AUGUR_MODEL_CACHE"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn rand_suffix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }
}
