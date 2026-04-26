//! Speaker diarization — "who spoke when" on multi-party audio.
//!
//! Sprint 5 P2. Implementation strategy mirrors `augur-translate`:
//! a bundled Python worker script
//! ([`diarize.py`](./diarize.py)) is invoked via `python3 -c`,
//! takes a JSON request on stdin, returns segments on stdout.
//! Same offline-first contract — pyannote model weights are
//! downloaded once into the Hugging Face cache and reused on every
//! subsequent call. Audio data never leaves the examiner's machine.
//!
//! pyannote requires a free Hugging Face account token to download
//! the gated `pyannote/speaker-diarization-3.1` model. AUGUR
//! manages that token via [`HfTokenManager`] — it is read from
//! `~/.cache/augur/hf_token` (chmod 600) and is the first AUGUR
//! feature that needs an HF account.

use log::{debug, info, warn};
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use augur_core::AugurError;

/// Default pyannote model id. Newer revisions can be passed via
/// the [`DiarizationEngine::model`] field.
pub const DEFAULT_PYANNOTE_MODEL: &str = "pyannote/speaker-diarization-3.1";

/// Sprint 8 P3 — speaker-attribution advisory. Non-suppressible
/// at the same level as the machine-translation advisory. Whenever
/// the CLI prints a diarized transcript, this line MUST appear
/// alongside the MT advisory. Speaker labels are produced by an
/// automated voice-segmentation model; they are not biometric
/// identification and must not be relied on as such.
pub const SPEAKER_DIARIZATION_ADVISORY: &str =
    "Speaker labels (SPEAKER_00, SPEAKER_01, ...) are produced by automated \
     voice segmentation. Do NOT use these as definitive identification of \
     individuals without expert verification.";

const DIARIZE_SCRIPT: &str = include_str!("diarize.py");

/// One contiguous span attributed to a single speaker by pyannote.
/// `speaker_id` is the model's anonymous label (`"SPEAKER_00"`,
/// `"SPEAKER_01"`, …); `speaker_label` is reserved for examiner-
/// assigned human labels (Sprint 6+).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiarizationSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker_id: String,
    pub speaker_label: Option<String>,
}

/// One STT segment enriched with the speaker that pyannote
/// attributed to it (by maximum temporal overlap) and an optional
/// translation.
#[derive(Debug, Clone)]
pub struct EnrichedSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub speaker_id: String,
    pub translated_text: Option<String>,
}

/// Manages the on-disk Hugging Face token AUGUR uses to download
/// gated pyannote weights. The token lives at
/// `~/.cache/augur/hf_token` and is treated as a secret — we
/// chmod it to 0600 on write and load via [`HfTokenManager::load`]
/// only at the moment we need it (never put it in env vars on
/// disk, never echo it to logs).
#[derive(Debug, Clone)]
pub struct HfTokenManager {
    pub token_path: PathBuf,
}

impl HfTokenManager {
    /// Default path: `~/.cache/augur/hf_token`.
    pub fn with_xdg_cache() -> Result<Self, AugurError> {
        let home = std::env::var("HOME").map_err(|_| {
            AugurError::Stt(
                "HOME not set; pass an explicit token path".to_string(),
            )
        })?;
        Ok(Self {
            token_path: PathBuf::from(home).join(".cache/augur/hf_token"),
        })
    }

    pub fn new(token_path: PathBuf) -> Self {
        Self { token_path }
    }

    /// `true` when the token file exists. Does NOT read or parse
    /// the token; callers can use this to gate pyannote-dependent
    /// features without ever touching the secret.
    pub fn is_configured(&self) -> bool {
        self.token_path.exists()
    }

    /// Read the token. Returns a structured error pointing at
    /// `augur setup --hf-token <T>` if the file is missing.
    pub fn load(&self) -> Result<String, AugurError> {
        if !self.token_path.exists() {
            return Err(AugurError::Stt(format!(
                "Hugging Face token not configured at {:?}. \
                 Run `augur setup --hf-token <token>` to write it. \
                 Get a free token at https://huggingface.co/settings/tokens \
                 and accept the pyannote model terms at \
                 https://huggingface.co/{DEFAULT_PYANNOTE_MODEL}",
                self.token_path,
            )));
        }
        let raw = std::fs::read_to_string(&self.token_path)?;
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            return Err(AugurError::Stt(format!(
                "HF token file at {:?} is empty",
                self.token_path
            )));
        }
        Ok(trimmed)
    }

    /// Persist the token to disk. Creates the parent directory and
    /// chmods the file to 0600 on Unix so other local users cannot
    /// read it.
    pub fn save(&self, token: &str) -> Result<(), AugurError> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(AugurError::InvalidInput(
                "refusing to save an empty HF token".to_string(),
            ));
        }
        if let Some(parent) = self.token_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.token_path, trimmed)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&self.token_path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&self.token_path, perms)?;
        }
        info!("augur-stt: HF token written to {:?}", self.token_path);
        Ok(())
    }
}

/// Speaker diarization engine. Spawns a short-lived `python3`
/// subprocess against the bundled [`diarize.py`] worker per call.
#[derive(Debug, Clone)]
pub struct DiarizationEngine {
    pub python_cmd: String,
    pub model: String,
    pub token_manager: HfTokenManager,
    pub hf_cache: Option<PathBuf>,
}

impl DiarizationEngine {
    /// Default engine: `python3`, `pyannote/speaker-diarization-3.1`,
    /// HF cache at `~/.cache/augur/models/pyannote/`.
    pub fn with_xdg_cache() -> Result<Self, AugurError> {
        let home = std::env::var("HOME").map_err(|_| {
            AugurError::Stt("HOME not set; pass cache dir explicitly".to_string())
        })?;
        Ok(Self {
            python_cmd: "python3".into(),
            model: DEFAULT_PYANNOTE_MODEL.into(),
            token_manager: HfTokenManager::with_xdg_cache()?,
            hf_cache: Some(PathBuf::from(home).join(".cache/augur/models/pyannote")),
        })
    }

    /// `true` when pyannote can plausibly be invoked: the python
    /// command exists AND the HF token is configured. Does not
    /// import pyannote (would require running python and is too
    /// slow for a probe). The actual subprocess call surfaces a
    /// structured error if pyannote itself is missing.
    pub fn is_available(&self) -> bool {
        let py_ok = Command::new(&self.python_cmd)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        py_ok && self.token_manager.is_configured()
    }

    /// Run pyannote diarization on a 16 kHz mono WAV and return
    /// the speaker turns it produces.
    pub fn diarize(&self, audio_path: &Path) -> Result<Vec<DiarizationSegment>, AugurError> {
        if !audio_path.exists() {
            return Err(AugurError::InvalidInput(format!(
                "audio file not found: {:?}",
                audio_path
            )));
        }
        let token = self.token_manager.load()?;
        if let Some(cache) = &self.hf_cache {
            std::fs::create_dir_all(cache)?;
        }
        warn!(
            "augur-stt: invoking pyannote diarization ({}) on {:?} — \
             one-time HF model download on first run, all inference local.",
            self.model, audio_path
        );

        let request = DiarizeRequest {
            audio_path: audio_path
                .to_str()
                .ok_or_else(|| AugurError::Stt(format!("non-UTF8 path: {audio_path:?}")))?,
            hf_token: &token,
            model: &self.model,
        };
        let req_json = serde_json::to_string(&request).map_err(|e| {
            AugurError::Stt(format!("diarize request serialise: {e}"))
        })?;

        let mut cmd = Command::new(&self.python_cmd);
        cmd.arg("-c").arg(DIARIZE_SCRIPT);
        if let Some(cache) = &self.hf_cache {
            cmd.env("AUGUR_HF_CACHE", cache);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AugurError::Stt(format!(
                "failed to spawn {}: {e}. Install Python 3 and \
                 `pip3 install --user pyannote.audio` to enable diarization.",
                self.python_cmd
            ))
        })?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(req_json.as_bytes())
                .map_err(|e| AugurError::Stt(format!("write stdin: {e}")))?;
        } else {
            return Err(AugurError::Stt(
                "child process stdin not piped".to_string(),
            ));
        }
        let output = child
            .wait_with_output()
            .map_err(|e| AugurError::Stt(format!("child wait: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        debug!(
            "augur-stt: pyannote exit={:?} stderr_bytes={} stdout_bytes={}",
            output.status.code(),
            stderr.len(),
            stdout.len()
        );

        let resp: DiarizeResponse = serde_json::from_str(stdout.trim()).map_err(|e| {
            AugurError::Stt(format!(
                "could not parse pyannote response as JSON: {e}; \
                 stdout={stdout:?}; stderr={stderr:?}"
            ))
        })?;
        if let Some(err) = resp.error {
            return Err(AugurError::Stt(format!(
                "pyannote worker error: {err}; stderr={stderr}"
            )));
        }
        let segments = resp
            .segments
            .ok_or_else(|| AugurError::Stt("pyannote returned no segments".into()))?
            .into_iter()
            .map(|s| DiarizationSegment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                speaker_id: s.speaker,
                speaker_label: None,
            })
            .collect();
        Ok(segments)
    }
}

#[derive(serde::Serialize)]
struct DiarizeRequest<'a> {
    audio_path: &'a str,
    hf_token: &'a str,
    model: &'a str,
}

#[derive(Deserialize)]
struct DiarizeResponse {
    segments: Option<Vec<RawSegment>>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct RawSegment {
    start_ms: u64,
    end_ms: u64,
    speaker: String,
}

/// Merge timestamped STT segments with pyannote diarization
/// segments by maximum temporal overlap — each STT segment gets
/// the `speaker_id` of the diarization segment with which it
/// shares the most milliseconds.
///
/// STT segments that overlap nothing are emitted with
/// `speaker_id = "UNKNOWN"`. The `translated_text` field is left
/// `None`; the CLI fills it in after running NLLB on each segment.
pub fn merge_stt_with_diarization(
    stt: &[crate::SttSegment],
    diarization: &[DiarizationSegment],
) -> Vec<EnrichedSegment> {
    stt.iter()
        .map(|s| {
            let speaker = best_speaker(s.start_ms, s.end_ms, diarization)
                .unwrap_or_else(|| "UNKNOWN".to_string());
            EnrichedSegment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: s.text.clone(),
                speaker_id: speaker,
                translated_text: None,
            }
        })
        .collect()
}

fn best_speaker(
    start_ms: u64,
    end_ms: u64,
    diarization: &[DiarizationSegment],
) -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    for d in diarization {
        let lo = start_ms.max(d.start_ms);
        let hi = end_ms.min(d.end_ms);
        if hi <= lo {
            continue;
        }
        let overlap = hi - lo;
        match &best {
            Some((cur, _)) if *cur >= overlap => {}
            _ => best = Some((overlap, d.speaker_id.clone())),
        }
    }
    best.map(|(_, s)| s)
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SttSegment;

    fn diarization_fixture() -> Vec<DiarizationSegment> {
        vec![
            DiarizationSegment {
                start_ms: 0,
                end_ms: 4_500,
                speaker_id: "SPEAKER_00".into(),
                speaker_label: None,
            },
            DiarizationSegment {
                start_ms: 4_500,
                end_ms: 8_000,
                speaker_id: "SPEAKER_01".into(),
                speaker_label: None,
            },
        ]
    }

    #[test]
    fn enriched_segment_merges_stt_and_diarization_by_overlap() {
        let stt = vec![
            SttSegment {
                start_ms: 0,
                end_ms: 5_000,
                text: "hello".into(),
            },
            SttSegment {
                start_ms: 5_000,
                end_ms: 7_500,
                text: "world".into(),
            },
        ];
        let merged = merge_stt_with_diarization(&stt, &diarization_fixture());
        assert_eq!(merged.len(), 2);
        // [0..5000] overlaps SPEAKER_00 4.5s and SPEAKER_01 0.5s
        // → SPEAKER_00 wins.
        assert_eq!(merged[0].speaker_id, "SPEAKER_00");
        assert_eq!(merged[0].text, "hello");
        // [5000..7500] is fully within SPEAKER_01's window.
        assert_eq!(merged[1].speaker_id, "SPEAKER_01");
        assert_eq!(merged[1].text, "world");
        for m in &merged {
            assert!(m.translated_text.is_none());
        }
    }

    #[test]
    fn merge_assigns_unknown_when_no_diarization_overlap() {
        let stt = vec![SttSegment {
            start_ms: 100_000,
            end_ms: 101_000,
            text: "later".into(),
        }];
        let merged = merge_stt_with_diarization(&stt, &diarization_fixture());
        assert_eq!(merged[0].speaker_id, "UNKNOWN");
    }

    #[test]
    fn hf_token_manager_returns_clear_error_when_missing() {
        let bogus = HfTokenManager::new(PathBuf::from(
            "/nonexistent/strata/verify/hf_token_xxx",
        ));
        assert!(!bogus.is_configured());
        match bogus.load() {
            Err(AugurError::Stt(msg)) => {
                assert!(
                    msg.contains("augur setup --hf-token"),
                    "expected setup hint, got: {msg}"
                );
            }
            other => panic!("expected Stt error with setup hint, got {other:?}"),
        }
    }

    #[test]
    fn hf_token_manager_round_trip() {
        let tmp = std::env::temp_dir().join(format!(
            "augur-hf-token-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let mgr = HfTokenManager::new(tmp.clone());
        mgr.save("hf_test_token_1234").expect("save");
        assert!(mgr.is_configured());
        assert_eq!(mgr.load().expect("load"), "hf_test_token_1234");
        // Cleanup so we don't leave a token lying around.
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn save_rejects_empty_token() {
        let tmp = std::env::temp_dir().join("augur-hf-token-empty-test");
        let mgr = HfTokenManager::new(tmp);
        match mgr.save("   ") {
            Err(AugurError::InvalidInput(_)) => {}
            other => panic!("expected InvalidInput on empty token, got {other:?}"),
        }
    }

    #[test]
    fn video_diarization_pipeline_produces_enriched_segments() {
        // Sprint 8 P3 — exercises the merge path that the video
        // pipeline drives. Three STT segments, two speakers,
        // verify the right speaker_id lands on each segment by
        // maximum temporal overlap.
        let stt = vec![
            crate::SttSegment {
                start_ms: 0,
                end_ms: 5_000,
                text: "مرحبا بالعالم".into(),
            },
            crate::SttSegment {
                start_ms: 5_000,
                end_ms: 12_000,
                text: "كيف حالك".into(),
            },
            crate::SttSegment {
                start_ms: 12_000,
                end_ms: 18_000,
                text: "بخير شكرا".into(),
            },
        ];
        let diar = vec![
            DiarizationSegment {
                start_ms: 0,
                end_ms: 5_500,
                speaker_id: "SPEAKER_00".into(),
                speaker_label: None,
            },
            DiarizationSegment {
                start_ms: 5_500,
                end_ms: 12_000,
                speaker_id: "SPEAKER_01".into(),
                speaker_label: None,
            },
            DiarizationSegment {
                start_ms: 12_000,
                end_ms: 18_000,
                speaker_id: "SPEAKER_00".into(),
                speaker_label: None,
            },
        ];
        let merged = merge_stt_with_diarization(&stt, &diar);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].speaker_id, "SPEAKER_00");
        assert_eq!(merged[1].speaker_id, "SPEAKER_01");
        assert_eq!(merged[2].speaker_id, "SPEAKER_00");
        // The translated_text slot is left None — the CLI fills
        // it after running NLLB on each segment.
        for m in &merged {
            assert!(m.translated_text.is_none());
            assert!(!m.text.is_empty());
        }
    }

    #[test]
    fn speaker_advisory_always_present_when_diarization_used() {
        // Sprint 8 P3 — the speaker-diarization advisory const
        // must be non-empty and carry both sides of the warning
        // (automated nature + don't use as biometric ID).
        assert!(!SPEAKER_DIARIZATION_ADVISORY.is_empty());
        assert!(SPEAKER_DIARIZATION_ADVISORY.contains("automated"));
        assert!(SPEAKER_DIARIZATION_ADVISORY.contains("identification"));
    }

    #[test]
    fn video_without_diarization_still_produces_transcript() {
        // Sprint 8 P3 — when diarization isn't run (empty diar
        // vector), every STT segment is still emitted, just
        // labeled UNKNOWN. The transcript itself is never lost.
        let stt = vec![
            crate::SttSegment {
                start_ms: 0,
                end_ms: 5_000,
                text: "alpha".into(),
            },
            crate::SttSegment {
                start_ms: 5_000,
                end_ms: 10_000,
                text: "beta".into(),
            },
        ];
        let merged = merge_stt_with_diarization(&stt, &[]);
        assert_eq!(merged.len(), 2);
        for m in &merged {
            assert_eq!(m.speaker_id, "UNKNOWN");
            assert!(!m.text.is_empty());
        }
    }

    #[test]
    fn diarization_engine_reports_unavailable_without_token() {
        // Construct an engine pointed at a token path that doesn't
        // exist. Even if python3 IS on PATH, is_available() must
        // be false because the token is missing.
        let engine = DiarizationEngine {
            python_cmd: "python3".into(),
            model: DEFAULT_PYANNOTE_MODEL.into(),
            token_manager: HfTokenManager::new(PathBuf::from(
                "/nonexistent/verify/hf_token_unavailable",
            )),
            hf_cache: None,
        };
        assert!(!engine.is_available());
    }
}
