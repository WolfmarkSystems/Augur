# AUGUR Sprint 10 — Tiered Model System + Whisper Large-v3 + SeamlessM4T + CAMeL Arabic
# Execute autonomously. Report after each priority group.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Before starting

1. Read CLAUDE.md completely
2. Run `cargo test --workspace 2>&1 | tail -5`
3. Confirm 166 tests passing before any changes

---

## Hard rules (absolute)

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- All errors handled explicitly
- `cargo clippy --workspace -- -D warnings` clean
- `cargo test --workspace` passes after every priority
- Offline invariant maintained — no content leaves the machine
- MT advisory always present on all translated output
- NO Chinese-origin models at any level — ever
  (Qwen, MiniMax, Kimi, GLM, Baidu, ByteDance all banned)

---

## PRIORITY 1 — Tiered Model Installation System

### Context

AUGUR currently downloads models on first use with no user control.
For forensic deployment — especially air-gap and SCIF environments —
examiners need explicit control over what gets installed, when,
and at what quality tier.

Three tiers:
- minimal — 2.5GB, any machine, text documents and short audio
- standard — 11GB, most LE/IC casework (recommended)
- full — 15GB, dedicated forensic workstations and SCIF deployment

### Implementation

**Step 1 — Model registry**

Create `crates/augur-core/src/models/registry.rs`:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ModelTier {
    Minimal,
    Standard,
    Full,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub size_bytes: u64,
    pub tier: ModelTier,
    pub model_type: ModelType,
    pub download_url: &'static str,  // named const, auditable
    pub filename: &'static str,
    pub sha256: &'static str,        // integrity verification
    pub languages: &'static [&'static str],
    pub quality_note: &'static str,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ModelType {
    Stt,           // Speech to text
    Translation,   // Text to text translation
    Classifier,    // Language identification
    Diarization,   // Speaker separation
    Unified,       // STT + translation in one (SeamlessM4T)
}

pub const ALL_MODELS: &[ModelSpec] = &[
    // ── MINIMAL TIER ──────────────────────────────────────────
    ModelSpec {
        id: "whisper-tiny",
        name: "Whisper Tiny",
        description: "Fast STT, 99 languages. Good for clear audio.",
        size_bytes: 75_000_000,
        tier: ModelTier::Minimal,
        model_type: ModelType::Stt,
        download_url: WHISPER_TINY_URL,
        filename: "whisper-tiny.safetensors",
        sha256: "",  // fill in from HuggingFace
        languages: &["99 languages"],
        quality_note: "Good quality on clear audio",
    },
    ModelSpec {
        id: "nllb-600m",
        name: "NLLB-200 Distilled 600M",
        description: "200-language translation. Fast, 2.4GB.",
        size_bytes: 2_400_000_000,
        tier: ModelTier::Minimal,
        model_type: ModelType::Translation,
        download_url: NLLB_600M_URL,
        filename: "nllb-200-distilled-600M",
        sha256: "",
        languages: &["200 languages"],
        quality_note: "Good for high-resource languages",
    },
    ModelSpec {
        id: "fasttext-lid",
        name: "fastText Language ID",
        description: "176-language classifier. 900KB, embedded.",
        size_bytes: 900_000,
        tier: ModelTier::Minimal,
        model_type: ModelType::Classifier,
        download_url: LID_MODEL_URL,
        filename: "lid.176.ftz",
        sha256: "",
        languages: &["176 languages"],
        quality_note: "Fast, accurate for major languages",
    },

    // ── STANDARD TIER ─────────────────────────────────────────
    ModelSpec {
        id: "whisper-large-v3",
        name: "Whisper Large v3",
        description: "High-quality STT. Best for accented/noisy audio.",
        size_bytes: 2_900_000_000,
        tier: ModelTier::Standard,
        model_type: ModelType::Stt,
        download_url: WHISPER_LARGE_V3_URL,
        filename: "whisper-large-v3.safetensors",
        sha256: "",
        languages: &["99 languages"],
        quality_note: "Dramatically better on accented speech and noisy recordings",
    },
    ModelSpec {
        id: "nllb-1.3b",
        name: "NLLB-200 1.3B",
        description: "Higher quality 200-language translation.",
        size_bytes: 5_200_000_000,
        tier: ModelTier::Standard,
        model_type: ModelType::Translation,
        download_url: NLLB_1B3_URL,
        filename: "nllb-200-1.3B",
        sha256: "",
        languages: &["200 languages"],
        quality_note: "Significantly better on low-resource languages",
    },
    ModelSpec {
        id: "camel-arabic",
        name: "CAMeL Arabic Dialect Models",
        description: "Carnegie Mellon Arabic NLP. Dialect ID + translation.",
        size_bytes: 450_000_000,
        tier: ModelTier::Standard,
        model_type: ModelType::Classifier,
        download_url: CAMEL_TOOLS_URL,
        filename: "camel-arabic-dialect",
        sha256: "",
        languages: &["Arabic dialects: Egyptian, Gulf, Levantine, Moroccan, Iraqi"],
        quality_note: "Best-in-class Arabic dialect identification",
    },

    // ── FULL TIER ─────────────────────────────────────────────
    ModelSpec {
        id: "seamless-m4t-medium",
        name: "SeamlessM4T Medium",
        description: "Meta unified speech+text model. Handles code-switching.",
        size_bytes: 2_400_000_000,
        tier: ModelTier::Full,
        model_type: ModelType::Unified,
        download_url: SEAMLESS_M4T_MEDIUM_URL,
        filename: "seamless-m4t-medium",
        sha256: "",
        languages: &["100 languages, speech-to-speech capable"],
        quality_note: "Handles mid-sentence language switching",
    },
    ModelSpec {
        id: "whisper-pashto",
        name: "Whisper Pashto Fine-tune",
        description: "Community Whisper fine-tuned on Pashto speech.",
        size_bytes: 150_000_000,
        tier: ModelTier::Full,
        model_type: ModelType::Stt,
        download_url: WHISPER_PASHTO_URL,
        filename: "whisper-pashto.safetensors",
        sha256: "",
        languages: &["Pashto (ps)"],
        quality_note: "Critical for Afghanistan/Pakistan casework",
    },
    ModelSpec {
        id: "whisper-dari",
        name: "Whisper Dari Fine-tune",
        description: "Community Whisper fine-tuned on Dari speech.",
        size_bytes: 150_000_000,
        tier: ModelTier::Full,
        model_type: ModelType::Stt,
        download_url: WHISPER_DARI_URL,
        filename: "whisper-dari.safetensors",
        sha256: "",
        languages: &["Dari (prs)"],
        quality_note: "Afghan Dari dialect of Persian",
    },
    ModelSpec {
        id: "pyannote-diarization",
        name: "pyannote Speaker Diarization",
        description: "Who spoke when. Requires HuggingFace token.",
        size_bytes: 1_000_000_000,
        tier: ModelTier::Full,
        model_type: ModelType::Diarization,
        download_url: "",  // HF token gated
        filename: "pyannote",
        sha256: "",
        languages: &["Language-independent"],
        quality_note: "Requires AUGUR_HF_CACHE token",
    },
];

pub fn models_for_tier(tier: &ModelTier) -> Vec<&'static ModelSpec> {
    ALL_MODELS.iter().filter(|m| {
        match tier {
            ModelTier::Minimal => m.tier == ModelTier::Minimal,
            ModelTier::Standard => {
                m.tier == ModelTier::Minimal ||
                m.tier == ModelTier::Standard
            },
            ModelTier::Full => true,
        }
    }).collect()
}

pub fn total_size_for_tier(tier: &ModelTier) -> u64 {
    models_for_tier(tier).iter().map(|m| m.size_bytes).sum()
}
```

**Step 2 — Named URL constants (complete network surface)**

All download URLs must be named constants — auditable, grepable:

```rust
// crates/augur-core/src/models/urls.rs
pub const WHISPER_TINY_URL: &str =
    "https://huggingface.co/openai/whisper-tiny/resolve/main/model.safetensors";
pub const WHISPER_LARGE_V3_URL: &str =
    "https://huggingface.co/openai/whisper-large-v3/resolve/main/model.safetensors";
pub const NLLB_600M_URL: &str =
    "https://huggingface.co/facebook/nllb-200-distilled-600M/resolve/main/";
pub const NLLB_1B3_URL: &str =
    "https://huggingface.co/facebook/nllb-200-1.3B/resolve/main/";
pub const CAMEL_TOOLS_URL: &str =
    "https://huggingface.co/CAMeL-Lab/bert-base-arabic-camelbert-mix-did/resolve/main/";
pub const SEAMLESS_M4T_MEDIUM_URL: &str =
    "https://huggingface.co/facebook/seamless-m4t-medium/resolve/main/";
pub const WHISPER_PASHTO_URL: &str =
    "https://huggingface.co/openai/whisper-small/resolve/main/model.safetensors";
    // placeholder — replace with actual community fine-tune URL
pub const WHISPER_DARI_URL: &str =
    "https://huggingface.co/openai/whisper-small/resolve/main/model.safetensors";
    // placeholder — replace with actual community fine-tune URL
pub const LID_MODEL_URL: &str =
    "https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz";
```

**Step 3 — Install command**

```bash
augur install minimal     # 2.5 GB — any machine
augur install standard    # 11 GB — recommended
augur install full        # 15 GB — dedicated workstation
augur install --list      # show all available models + sizes
augur install --status    # show what's currently installed
augur install airgap --profile standard --output ~/augur-airgap.tar
```

Implement `cmd_install` in `apps/augur-cli/src/install.rs`:

```rust
pub fn cmd_install(profile: &str, list: bool, status: bool) 
    -> Result<(), AugurError> 
{
    if list {
        print_model_catalog();
        return Ok(());
    }
    
    if status {
        print_install_status();
        return Ok(());
    }
    
    let tier = match profile {
        "minimal"  => ModelTier::Minimal,
        "standard" => ModelTier::Standard,
        "full"     => ModelTier::Full,
        other => return Err(AugurError::InvalidProfile(other.to_string())),
    };
    
    let models = models_for_tier(&tier);
    let total_bytes = total_size_for_tier(&tier);
    
    println_augur!("Installing {} profile ({} models, {:.1} GB)",
        profile,
        models.len(),
        total_bytes as f64 / 1e9
    );
    
    for model in models {
        install_model(model)?;
    }
    
    println_augur!("Installation complete. Run `augur self-test` to verify.");
    Ok(())
}
```

**Step 4 — Progress display during download**

```
[AUGUR] Installing standard profile (6 models, 10.9 GB)

  [1/6] Whisper Tiny (75 MB)          ████████████████████ 100% ✓
  [2/6] NLLB-200 600M (2.4 GB)        ████████████████████ 100% ✓
  [3/6] fastText LID (900 KB)         ████████████████████ 100% ✓
  [4/6] Whisper Large-v3 (2.9 GB)     ████████░░░░░░░░░░░░  41% 1.2 GB/s
  [5/6] NLLB-200 1.3B (5.2 GB)        waiting...
  [6/6] CAMeL Arabic (450 MB)         waiting...

  Total: 3.6 GB / 10.9 GB  ETA: ~6 min
```

**Step 5 — SHA-256 verification after download**

Every model verified before use:
```rust
pub fn verify_model_integrity(spec: &ModelSpec, path: &Path) 
    -> Result<(), AugurError> 
{
    if spec.sha256.is_empty() {
        log::warn!("No SHA-256 for {} — skipping verification", spec.id);
        return Ok(());
    }
    let computed = sha256_of_path(path)?;
    if computed != spec.sha256 {
        return Err(AugurError::IntegrityFailure {
            model: spec.id.to_string(),
            expected: spec.sha256.to_string(),
            computed,
        });
    }
    Ok(())
}
```

**Step 6 — Air-gap package builder**

```rust
pub fn build_airgap_package(
    tier: ModelTier,
    output_path: &Path,
) -> Result<(), AugurError>
{
    // Verify all models for tier are installed
    // Create tar archive: models/ + install_manifest.json
    // Write AIRGAP_README.txt with transfer and install instructions
    // SHA-256 the entire archive for integrity
}
```

Output: `augur-airgap-standard-20260426.tar`
- `models/` — all model files
- `install_manifest.json` — model list, sizes, checksums
- `AIRGAP_README.txt` — how to install on air-gapped machine
- `augur-airgap-standard-20260426.tar.sha256` — archive checksum

**Step 7 — `augur install --status` output**

```
[AUGUR] Installation status

Profile: standard (partially installed)

  Model                      Size      Status
  ─────────────────────────────────────────────────
  Whisper Tiny               75 MB     ✓ installed
  NLLB-200 600M              2.4 GB    ✓ installed
  fastText LID               900 KB    ✓ installed
  Whisper Large-v3           2.9 GB    ✗ not installed
  NLLB-200 1.3B              5.2 GB    ✗ not installed
  CAMeL Arabic               450 MB    ✗ not installed
  ─────────────────────────────────────────────────
  Installed: 2.5 GB / 10.9 GB

  Run `augur install standard` to complete.
```

### Tests

```rust
#[test]
fn tier_minimal_includes_whisper_and_nllb() {
    let models = models_for_tier(&ModelTier::Minimal);
    assert!(models.iter().any(|m| m.id == "whisper-tiny"));
    assert!(models.iter().any(|m| m.id == "nllb-600m"));
}

#[test]
fn tier_standard_includes_minimal_models() {
    let standard = models_for_tier(&ModelTier::Standard);
    let minimal = models_for_tier(&ModelTier::Minimal);
    for m in &minimal {
        assert!(standard.iter().any(|s| s.id == m.id));
    }
}

#[test]
fn tier_full_includes_all_models() {
    let full = models_for_tier(&ModelTier::Full);
    assert_eq!(full.len(), ALL_MODELS.len());
}

#[test]
fn total_size_minimal_under_3gb() {
    let size = total_size_for_tier(&ModelTier::Minimal);
    assert!(size < 3_000_000_000);
}

#[test]
fn all_model_ids_unique() {
    let ids: Vec<_> = ALL_MODELS.iter().map(|m| m.id).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len());
}

#[test]
fn no_chinese_origin_models_in_registry() {
    let banned = ["qwen", "baidu", "ernie", "glm", "minimax", "kimi",
                  "bytedance", "wenxin"];
    for model in ALL_MODELS {
        let url_lower = model.download_url.to_lowercase();
        let name_lower = model.name.to_lowercase();
        for banned_term in &banned {
            assert!(!url_lower.contains(banned_term),
                "Model {} URL contains banned term {}", model.id, banned_term);
            assert!(!name_lower.contains(banned_term),
                "Model {} name contains banned term {}", model.id, banned_term);
        }
    }
}
```

### Acceptance criteria — P1

- [ ] `ModelRegistry` with all models defined
- [ ] Three tiers: minimal/standard/full
- [ ] All URLs are named constants
- [ ] `augur install <tier>` command works
- [ ] `augur install --list` shows catalog
- [ ] `augur install --status` shows what's installed
- [ ] Progress display during download
- [ ] SHA-256 integrity verification
- [ ] Air-gap package builder
- [ ] Chinese-origin model check in tests
- [ ] 6 new tests pass
- [ ] Clippy clean

---

## PRIORITY 2 — Whisper Large-v3 Integration

### Context

Whisper Large-v3 is the current state of the art for open-source
STT. For forensic audio — intercepted calls, surveillance recordings,
degraded audio from old equipment — it dramatically outperforms tiny.

The candle-whisper integration already supports model selection.
This priority wires Large-v3 as a first-class option.

### Implementation

**Step 1 — Model selection in pipeline**

Extend `PipelineOptions`:

```rust
pub enum WhisperModel {
    Tiny,       // 75MB, fast, good enough for clear speech
    Base,       // 142MB, slightly better
    LargeV3,    // 2.9GB, best quality, use for critical evidence
    Pashto,     // community fine-tune for Pashto
    Dari,       // community fine-tune for Dari
}

impl WhisperModel {
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
        let path = model_cache_path(self.model_spec_id());
        path.exists()
    }
}
```

**Step 2 — Auto-select based on installation**

When `--model auto` (default):
1. If Large-v3 installed → use Large-v3
2. Else if Base installed → use Base
3. Else → use Tiny
4. If language is Pashto and Pashto fine-tune installed → use it
5. If language is Dari and Dari fine-tune installed → use it

```rust
pub fn auto_select_whisper_model(
    detected_language: Option<&str>,
) -> WhisperModel {
    // Pashto/Dari override first
    if let Some(lang) = detected_language {
        if lang == "ps" && WhisperModel::Pashto.is_installed() {
            return WhisperModel::Pashto;
        }
        if lang == "prs" && WhisperModel::Dari.is_installed() {
            return WhisperModel::Dari;
        }
    }
    // Quality cascade
    if WhisperModel::LargeV3.is_installed() {
        WhisperModel::LargeV3
    } else if WhisperModel::Base.is_installed() {
        WhisperModel::Base
    } else {
        WhisperModel::Tiny
    }
}
```

**Step 3 — CLI flags**

```bash
augur transcribe --input recording.mp3 --model tiny
augur transcribe --input recording.mp3 --model large-v3
augur transcribe --input recording.mp3 --model auto   # default
augur transcribe --input pashto_call.mp3 --model pashto
```

**Step 4 — Self-test update**

`augur self-test` reports which Whisper models are installed:

```
[AUGUR] STT models:
  ✓ Whisper Tiny (75 MB) — installed
  ✓ Whisper Large-v3 (2.9 GB) — installed [active]
  ✗ Whisper Pashto — not installed
  ✗ Whisper Dari — not installed
```

**Step 5 — candle-whisper model loading**

Update `crates/augur-stt/src/whisper.rs` to load the correct
model file based on `WhisperModel` selection. The candle-whisper
architecture is the same for all model sizes — only the weights
change.

### Tests

```rust
#[test]
fn auto_select_returns_largest_installed() {
    // Mock: large-v3 installed
    // auto_select → WhisperModel::LargeV3
}

#[test]
fn auto_select_falls_back_to_tiny() {
    // Mock: only tiny installed
    // auto_select → WhisperModel::Tiny
}

#[test]
fn pashto_model_selected_for_ps_language() {
    // Mock: Pashto model installed, lang = "ps"
    // auto_select → WhisperModel::Pashto
}

#[test]
fn whisper_model_spec_ids_unique() {
    let ids = [
        WhisperModel::Tiny.model_spec_id(),
        WhisperModel::LargeV3.model_spec_id(),
        WhisperModel::Pashto.model_spec_id(),
        WhisperModel::Dari.model_spec_id(),
    ];
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len());
}
```

### Acceptance criteria — P2

- [ ] `WhisperModel` enum with all variants
- [ ] Auto-selection cascade works
- [ ] Pashto/Dari override when language detected
- [ ] `--model` CLI flag on transcribe and translate commands
- [ ] Self-test reports installed Whisper models
- [ ] candle-whisper loads correct weights per model
- [ ] 4 new tests pass
- [ ] Clippy clean

---

## PRIORITY 3 — SeamlessM4T Integration

### Context

SeamlessM4T handles code-switching — when a speaker switches
languages mid-sentence. Arabic/English code-switching is extremely
common in intercepted communications from educated subjects.
NLLB-200 fails on code-switched input. SeamlessM4T handles it.

SeamlessM4T is also a unified model — it can do STT + translation
in a single inference pass, which is faster and more accurate than
the current two-step pipeline.

### Implementation

**Step 1 — SeamlessM4T via Python subprocess**

Same pattern as NLLB-200. Create `scripts/seamless_worker.py`:

```python
#!/usr/bin/env python3
"""
SeamlessM4T inference worker for AUGUR.
Handles: speech-to-text, text-to-text, speech-to-speech.
"""
import sys
import json
from transformers import AutoProcessor, SeamlessM4Tv2Model
import torch

def load_model(model_dir: str):
    processor = AutoProcessor.from_pretrained(model_dir)
    model = SeamlessM4Tv2Model.from_pretrained(model_dir)
    return processor, model

def translate_text(processor, model, text: str, 
                   src_lang: str, tgt_lang: str) -> str:
    inputs = processor(text=text, src_lang=src_lang, 
                      return_tensors="pt")
    output = model.generate(**inputs, tgt_lang=tgt_lang,
                            generate_speech=False)
    return processor.decode(output[0].tolist()[0],
                           skip_special_tokens=True)

def transcribe_and_translate(processor, model, audio_path: str,
                             tgt_lang: str) -> dict:
    import torchaudio
    audio, sample_rate = torchaudio.load(audio_path)
    if sample_rate != 16000:
        resampler = torchaudio.transforms.Resample(sample_rate, 16000)
        audio = resampler(audio)
    audio = audio.squeeze()
    
    inputs = processor(audios=audio, return_tensors="pt",
                      sampling_rate=16000)
    output = model.generate(**inputs, tgt_lang=tgt_lang,
                            generate_speech=False)
    
    transcript = processor.decode(output[0].tolist()[0],
                                 skip_special_tokens=True)
    return {"transcript": transcript, "language": "auto"}

if __name__ == "__main__":
    request = json.loads(sys.stdin.read())
    model_dir = request["model_dir"]
    processor, model = load_model(model_dir)
    
    if request["task"] == "translate_text":
        result = translate_text(processor, model,
                               request["text"],
                               request["src_lang"],
                               request["tgt_lang"])
        print(json.dumps({"translation": result}))
    
    elif request["task"] == "transcribe_translate":
        result = transcribe_and_translate(processor, model,
                                         request["audio_path"],
                                         request["tgt_lang"])
        print(json.dumps(result))
```

**Step 2 — AUGUR pipeline routing**

When `--engine seamless` is specified OR when code-switching
is detected (heuristic: language classifier returns low confidence
or mixed signals):

```rust
pub enum TranslationEngine {
    Nllb,       // default — best for single-language content
    Seamless,   // best for code-switched content
    Auto,       // auto-select based on content analysis
}

pub fn select_engine(
    text: &str,
    classification: &ClassificationResult,
    installed: &InstalledModels,
) -> TranslationEngine {
    // If SeamlessM4T not installed → Nllb
    if !installed.seamless_m4t {
        return TranslationEngine::Nllb;
    }
    // If classification confidence is low → likely code-switched
    if classification.confidence < 0.75 {
        return TranslationEngine::Seamless;
    }
    // If text contains significant Latin chars in Arabic context
    if classification.language == "ar" && latin_ratio(text) > 0.15 {
        return TranslationEngine::Seamless;
    }
    TranslationEngine::Nllb
}
```

**Step 3 — CLI flag**

```bash
augur translate --input recording.mp3 --target en --engine seamless
augur translate --input recording.mp3 --target en --engine auto
augur translate --input recording.mp3 --target en  # default: nllb
```

**Step 4 — Code-switching detection**

```rust
pub fn detect_code_switching(text: &str) -> CodeSwitchAnalysis {
    // Count runs of Latin vs Arabic characters
    // Detect language boundaries within the text
    // Return: is_code_switched, switch_points, languages_detected
}

pub struct CodeSwitchAnalysis {
    pub is_code_switched: bool,
    pub confidence: f32,
    pub languages_detected: Vec<String>,
    pub switch_count: u32,
}
```

**Step 5 — Advisory update for SeamlessM4T**

When SeamlessM4T is used, the advisory note changes:
"Translation produced by SeamlessM4T (Meta AI, open weights).
Code-switching detected — content contains multiple languages.
Machine translation only — verify with certified human translator."

MT advisory still present. Origin of model disclosed.

**Step 6 — Self-test update**

```
[AUGUR] Translation engines:
  ✓ NLLB-200 600M — installed (200 languages)
  ✓ NLLB-200 1.3B — installed (200 languages, higher quality)
  ✓ SeamlessM4T Medium — installed (100 languages, code-switching)
  ✗ SeamlessM4T Large — not installed
```

### Tests

```rust
#[test]
fn code_switching_detected_in_mixed_text() {
    let text = "قال إنه going to the market غداً";
    let analysis = detect_code_switching(text);
    assert!(analysis.is_code_switched);
}

#[test]
fn pure_arabic_not_flagged_as_code_switched() {
    let text = "مرحبا بالعالم كيف حالك";
    let analysis = detect_code_switching(text);
    assert!(!analysis.is_code_switched);
}

#[test]
fn engine_auto_selects_seamless_for_low_confidence() {
    let classification = ClassificationResult {
        confidence: 0.60,
        ..Default::default()
    };
    let installed = InstalledModels { seamless_m4t: true, ..Default::default() };
    let engine = select_engine("mixed text", &classification, &installed);
    assert!(matches!(engine, TranslationEngine::Seamless));
}

#[test]
fn engine_falls_back_to_nllb_when_seamless_not_installed() {
    let installed = InstalledModels { seamless_m4t: false, ..Default::default() };
    let engine = select_engine("any text", &ClassificationResult::default(), &installed);
    assert!(matches!(engine, TranslationEngine::Nllb));
}
```

### Acceptance criteria — P3

- [ ] `seamless_worker.py` implements text and audio tasks
- [ ] `TranslationEngine` enum with routing logic
- [ ] Code-switching detection function
- [ ] Auto-engine selection based on content analysis
- [ ] `--engine` CLI flag on translate command
- [ ] SeamlessM4T advisory text updated (origin disclosed)
- [ ] MT advisory still always present
- [ ] Self-test reports SeamlessM4T status
- [ ] 4 new tests pass
- [ ] Clippy clean

---

## PRIORITY 4 — CAMeL Arabic Dialect Integration

### Context

AUGUR's current Arabic dialect detection uses a lexical marker
approach (Sprint 9 / Super Sprint). CAMeL Tools from Carnegie
Mellon's Arabic NLP group provides dedicated ML models for
Arabic dialect identification — far more accurate than word lists,
especially on short text and mixed input.

### Implementation

**Step 1 — CAMeL via Python subprocess**

Create `scripts/camel_worker.py`:

```python
#!/usr/bin/env python3
"""
CAMeL Tools Arabic dialect identification worker for AUGUR.
"""
import sys
import json

def load_camel():
    from camel_tools.dialectid import DialectIdentifier
    return DialectIdentifier.pretrained()

def identify_dialect(did, text: str) -> dict:
    result = did.predict(text)
    return {
        "dialect": result.top,
        "confidence": float(result.scores[result.top]),
        "all_scores": {k: float(v) for k, v in result.scores.items()},
    }

if __name__ == "__main__":
    request = json.loads(sys.stdin.read())
    did = load_camel()
    result = identify_dialect(did, request["text"])
    print(json.dumps(result))
```

**Step 2 — Integration with classifier**

When Arabic is detected, and CAMeL is installed:
1. Run CAMeL dialect identification
2. Use CAMeL result instead of lexical marker result
3. Keep lexical markers as fallback when CAMeL not installed

```rust
pub fn classify_arabic_dialect(
    text: &str,
    installed: &InstalledModels,
) -> DialectAnalysis {
    if installed.camel_arabic {
        match run_camel_worker(text) {
            Ok(result) => return result.into_dialect_analysis(),
            Err(e) => {
                log::warn!("CAMeL failed, falling back to lexical: {}", e);
            }
        }
    }
    // Fallback to lexical markers (Sprint 9)
    crate::script::pashto_farsi_score(text).into()
}
```

**Step 3 — Dialect-specific translation routing**

When dialect is identified with high confidence, route to
dialect-specific translation if available:

```
Egyptian Arabic → route through Egyptian-tuned NLLB variant
                  (if installed) or standard NLLB with dialect hint
Gulf Arabic → standard NLLB (best available)
Moroccan Darija → SeamlessM4T preferred (better on Maghrebi)
```

**Step 4 — CLI display update**

```
[AUGUR] Language: ar (Arabic)
        Dialect: Egyptian (Masri) — CAMeL confidence: 0.89
        Indicators: model-identified (CAMeL Tools, CMU)
        ⚠ Verify dialect with human Arabic linguist
```

**Step 5 — Tests**

```rust
#[test]
fn camel_fallback_to_lexical_when_not_installed() {
    let installed = InstalledModels { camel_arabic: false, ..Default::default() };
    // Should not panic, should return lexical result
    let result = classify_arabic_dialect("مرحبا", &installed);
    assert!(!result.advisory.is_empty());
}

#[test]
fn camel_result_converts_to_dialect_analysis() {
    // CAMeL JSON output → DialectAnalysis struct
    let json = r#"{"dialect":"EGY","confidence":0.89,"all_scores":{}}"#;
    let analysis: DialectAnalysis = serde_json::from_str(json).unwrap();
    assert!(matches!(analysis.detected_dialect, ArabicDialect::Egyptian));
}
```

### Acceptance criteria — P4

- [ ] `camel_worker.py` runs CAMeL dialect identification
- [ ] CAMeL result used when installed, lexical fallback otherwise
- [ ] Dialect-specific translation routing
- [ ] CLI shows "CAMeL Tools, CMU" as source when used
- [ ] Self-test reports CAMeL installation status
- [ ] 2 new tests pass
- [ ] Clippy clean

---

## After all priorities complete

```bash
cargo test --workspace 2>&1 | grep "test result" | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
augur install --list
augur install --status
augur self-test
```

Commit:
```bash
git add -A
git commit -m "feat: augur-sprint-10 tiered model system + whisper-large-v3 + seamless-m4t + camel-arabic"
```

Report:
- Which priorities passed
- Test count before (166) and after
- Output of `augur install --list`
- Output of `augur install --status`
- Output of `augur self-test` after changes
- Any deviations from spec

---

## What this sprint does NOT touch

- Strata source code (separate repo)
- The MT advisory (always present, no exceptions)
- Chinese-origin models (banned, always)
- GUI (separate sprint)
- The Tauri desktop interface (not yet built)

---

_AUGUR Sprint 10 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_This sprint makes AUGUR the most capable offline forensic_
_translation tool available. Whisper Large-v3 + SeamlessM4T_
_+ CAMeL Arabic + tiered install = best in class._
