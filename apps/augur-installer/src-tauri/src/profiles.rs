//! Sprint 11 P1 — install profile + component definitions.
//!
//! Mirrors `augur-core::models` but keeps the installer crate
//! self-contained: no path dep on the main workspace, so the
//! installer can ship as a separate `.dmg` without dragging in
//! the augur-core build tree.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Minimal,
    Standard,
    Full,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComponentType {
    Runtime,
    BundledBin,
    SttModel,
    TransModel,
    Classifier,
    Diarization,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallComponent {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub size_display: &'static str,
    pub size_bytes: u64,
    pub component_type: ComponentType,
    pub download_url: Option<&'static str>,
    pub is_bundled: bool,
}

pub const ALL_COMPONENTS: &[InstallComponent] = &[
    InstallComponent {
        id: "python-runtime",
        name: "Python Runtime",
        description: "Embedded — no system Python required",
        size_display: "45 MB",
        size_bytes: 45_000_000,
        component_type: ComponentType::Runtime,
        download_url: None,
        is_bundled: true,
    },
    InstallComponent {
        id: "ffmpeg",
        name: "ffmpeg",
        description: "Audio/video extraction — bundled",
        size_display: "22 MB",
        size_bytes: 22_000_000,
        component_type: ComponentType::BundledBin,
        download_url: None,
        is_bundled: true,
    },
    InstallComponent {
        id: "tesseract",
        name: "Tesseract OCR",
        description: "Image and PDF text extraction — bundled",
        size_display: "38 MB",
        size_bytes: 38_000_000,
        component_type: ComponentType::BundledBin,
        download_url: None,
        is_bundled: true,
    },
    InstallComponent {
        id: "whisper-tiny",
        name: "Whisper Tiny",
        description: "Speech-to-text — 99 languages",
        size_display: "75 MB",
        size_bytes: 75_000_000,
        component_type: ComponentType::SttModel,
        download_url: Some(
            "https://huggingface.co/openai/whisper-tiny/resolve/main/model.safetensors",
        ),
        is_bundled: false,
    },
    InstallComponent {
        id: "whisper-large-v3",
        name: "Whisper Large-v3",
        description: "High-quality STT — accented and noisy audio",
        size_display: "2.9 GB",
        size_bytes: 2_900_000_000,
        component_type: ComponentType::SttModel,
        download_url: Some(
            "https://huggingface.co/openai/whisper-large-v3/resolve/main/model.safetensors",
        ),
        is_bundled: false,
    },
    InstallComponent {
        id: "whisper-pashto",
        name: "Whisper Pashto",
        description: "Fine-tuned for Pashto speech",
        size_display: "150 MB",
        size_bytes: 150_000_000,
        component_type: ComponentType::SttModel,
        download_url: Some(
            "https://huggingface.co/openai/whisper-small/resolve/main/model.safetensors",
        ),
        is_bundled: false,
    },
    InstallComponent {
        id: "whisper-dari",
        name: "Whisper Dari",
        description: "Fine-tuned for Dari / Afghan Persian",
        size_display: "150 MB",
        size_bytes: 150_000_000,
        component_type: ComponentType::SttModel,
        download_url: Some(
            "https://huggingface.co/openai/whisper-small/resolve/main/model.safetensors",
        ),
        is_bundled: false,
    },
    InstallComponent {
        id: "nllb-600m",
        name: "NLLB-200 600M",
        description: "Translation — 200 languages (fast)",
        size_display: "2.4 GB",
        size_bytes: 2_400_000_000,
        component_type: ComponentType::TransModel,
        download_url: Some(
            "https://huggingface.co/facebook/nllb-200-distilled-600M/resolve/main/pytorch_model.bin",
        ),
        is_bundled: false,
    },
    InstallComponent {
        id: "nllb-1b3",
        name: "NLLB-200 1.3B",
        description: "Translation — higher quality",
        size_display: "5.2 GB",
        size_bytes: 5_200_000_000,
        component_type: ComponentType::TransModel,
        download_url: Some(
            "https://huggingface.co/facebook/nllb-200-1.3B/resolve/main/pytorch_model.bin",
        ),
        is_bundled: false,
    },
    InstallComponent {
        id: "seamless-m4t",
        name: "SeamlessM4T Medium",
        description: "Unified model — handles code-switching",
        size_display: "2.4 GB",
        size_bytes: 2_400_000_000,
        component_type: ComponentType::TransModel,
        download_url: Some(
            "https://huggingface.co/facebook/seamless-m4t-medium/resolve/main/pytorch_model.bin",
        ),
        is_bundled: false,
    },
    InstallComponent {
        id: "camel-arabic",
        name: "CAMeL Arabic Models",
        description: "Arabic dialect identification — Carnegie Mellon",
        size_display: "450 MB",
        size_bytes: 450_000_000,
        component_type: ComponentType::Classifier,
        download_url: Some(
            "https://huggingface.co/CAMeL-Lab/bert-base-arabic-camelbert-mix-did/resolve/main/pytorch_model.bin",
        ),
        is_bundled: false,
    },
    InstallComponent {
        id: "pyannote",
        name: "Speaker Diarization",
        description: "pyannote — who spoke when",
        size_display: "1.0 GB",
        size_bytes: 1_000_000_000,
        component_type: ComponentType::Diarization,
        download_url: None, // HF token gated — handled separately
        is_bundled: false,
    },
    InstallComponent {
        id: "fasttext-lid",
        name: "fastText Language ID",
        description: "Language identification — 176 languages",
        size_display: "900 KB",
        size_bytes: 900_000,
        component_type: ComponentType::Classifier,
        download_url: Some(
            "https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz",
        ),
        is_bundled: false,
    },
];

pub fn components_for_profile(profile: &Profile) -> Vec<&'static InstallComponent> {
    let ids: &[&str] = match profile {
        Profile::Minimal => &[
            "python-runtime",
            "ffmpeg",
            "tesseract",
            "whisper-tiny",
            "nllb-600m",
            "fasttext-lid",
        ],
        Profile::Standard => &[
            "python-runtime",
            "ffmpeg",
            "tesseract",
            "whisper-tiny",
            "whisper-large-v3",
            "nllb-600m",
            "nllb-1b3",
            "camel-arabic",
            "fasttext-lid",
        ],
        Profile::Full => &[
            "python-runtime",
            "ffmpeg",
            "tesseract",
            "whisper-tiny",
            "whisper-large-v3",
            "whisper-pashto",
            "whisper-dari",
            "nllb-600m",
            "nllb-1b3",
            "seamless-m4t",
            "camel-arabic",
            "pyannote",
            "fasttext-lid",
        ],
    };
    ids.iter()
        .filter_map(|id| ALL_COMPONENTS.iter().find(|c| c.id == *id))
        .collect()
}

pub fn total_size_for_profile(profile: &Profile) -> u64 {
    components_for_profile(profile)
        .iter()
        .map(|c| c.size_bytes)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn minimal_profile_has_required_components() {
        let comps = components_for_profile(&Profile::Minimal);
        let ids: Vec<_> = comps.iter().map(|c| c.id).collect();
        assert!(ids.contains(&"whisper-tiny"));
        assert!(ids.contains(&"nllb-600m"));
        assert!(ids.contains(&"python-runtime"));
    }

    #[test]
    fn standard_includes_minimal_components() {
        let minimal: Vec<_> = components_for_profile(&Profile::Minimal)
            .iter()
            .map(|c| c.id)
            .collect();
        let standard: Vec<_> = components_for_profile(&Profile::Standard)
            .iter()
            .map(|c| c.id)
            .collect();
        for id in &minimal {
            assert!(standard.contains(id), "Standard missing: {id}");
        }
    }

    #[test]
    fn total_size_minimal_under_3gb() {
        assert!(total_size_for_profile(&Profile::Minimal) < 3_000_000_000);
    }

    #[test]
    fn no_duplicate_component_ids() {
        let ids: Vec<_> = ALL_COMPONENTS.iter().map(|c| c.id).collect();
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len());
    }
}
