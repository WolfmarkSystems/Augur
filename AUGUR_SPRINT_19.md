# AUGUR Sprint 19 — Real-time Audio Mode + Live Interview Support
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

Examiners conducting interviews with foreign-language subjects
need real-time translation — microphone → Whisper STT → NLLB
translation → live subtitles on screen. This sprint adds a
"Live" mode to AUGUR Desktop using the streaming pipeline
from Sprint 14.

This is the capability that makes AUGUR useful in the field,
not just in the lab.

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- MT advisory permanently visible during live mode
- Live mode advisory: "LIVE MACHINE TRANSLATION — unverified"
- `cargo clippy -- -D warnings` clean
- Offline invariant maintained — no content leaves machine

---

## PRIORITY 1 — Microphone Input Pipeline

### Step 1 — Audio capture Tauri command

```rust
// apps/augur-desktop/src-tauri/src/live_audio.rs

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

static RECORDING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn start_live_translation(
    app: AppHandle,
    target_lang: String,
    chunk_duration_ms: u64,  // default 3000 — 3 second chunks
) -> Result<(), String> {
    if RECORDING.swap(true, Ordering::SeqCst) {
        return Err("Already recording".into());
    }

    let app_clone = app.clone();
    let target = target_lang.clone();

    tokio::spawn(async move {
        // Use augur CLI in live mode
        let augur = find_augur_binary()
            .expect("augur binary required");

        let mut cmd = Command::new(&augur);
        cmd.arg("live")
           .arg("--target").arg(&target)
           .arg("--chunk-ms").arg(chunk_duration_ms.to_string())
           .arg("--format").arg("ndjson")
           .stdout(std::process::Stdio::piped());

        let mut child = cmd.spawn().expect("spawn failed");
        let stdout = child.stdout.take().unwrap();
        let mut lines = BufReader::new(stdout).lines();

        while RECORDING.load(Ordering::SeqCst) {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        app_clone.emit("live-segment", &json).ok();
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        child.kill().await.ok();
        RECORDING.store(false, Ordering::SeqCst);
        app_clone.emit("live-stopped", serde_json::json!({})).ok();
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_live_translation() -> Result<(), String> {
    RECORDING.store(false, Ordering::SeqCst);
    Ok(())
}
```

### Step 2 — `augur live` CLI command

Add a `live` subcommand to the AUGUR CLI:

```rust
// apps/augur-cli/src/live.rs

pub fn cmd_live(target_lang: &str, chunk_ms: u64) -> Result<(), AugurError> {
    // 1. Open default microphone via cpal or portaudio
    // 2. Capture audio in chunk_ms chunks
    // 3. For each chunk:
    //    a. Save to temp WAV file
    //    b. Run Whisper STT on it
    //    c. Detect language
    //    d. If foreign: translate with NLLB
    //    e. Emit NDJSON segment event
    //    f. Flush stdout
    // 4. Loop until SIGINT or stdin closes

    // MT advisory in first line of output:
    println!("{}", serde_json::json!({
        "type": "live_started",
        "machine_translation_notice": MT_ADVISORY,
        "live_advisory": "LIVE MACHINE TRANSLATION — unverified. \
                         Do not use for legal decisions in real time.",
    }));
    std::io::stdout().flush().ok();

    // Audio capture loop...
    Ok(())
}
```

**Audio capture dependency:**
Add `cpal` for cross-platform audio capture:
```toml
# In apps/augur-cli/Cargo.toml
cpal = "0.15"
```

`cpal` is pure Rust, works on macOS/Linux/Windows, no system
library required on macOS (uses CoreAudio directly).

**Chunk processing:**
Each 3-second audio chunk:
1. Saved to `/tmp/augur_live_chunk_N.wav`
2. Passed to `augur-stt` Whisper for transcription
3. Language classified
4. If confidence > 0.6 and is_foreign → translate
5. NDJSON segment emitted
6. Temp file deleted

```json
{"type":"live_segment",
 "chunk_index": 1,
 "original": "مرحبا كيف حالك",
 "translated": "Hello how are you",
 "source_lang": "ar",
 "confidence": 0.94,
 "chunk_start_ms": 0,
 "chunk_end_ms": 3000}
```

### Step 3 — Silence detection

Don't process silent chunks (waste of compute):

```rust
pub fn is_silence(samples: &[f32], threshold: f32) -> bool {
    let rms = (samples.iter().map(|s| s * s).sum::<f32>()
               / samples.len() as f32).sqrt();
    rms < threshold
}
```

If RMS < 0.01 → skip transcription, emit nothing.

### Acceptance criteria — P1

- [ ] `augur live` CLI command captures microphone
- [ ] Chunks processed every 3 seconds (configurable)
- [ ] NDJSON output flushed per segment
- [ ] Silence detection skips silent chunks
- [ ] `start_live_translation` Tauri command spawns augur live
- [ ] `stop_live_translation` terminates the process
- [ ] Live advisory emitted on start
- [ ] MT advisory in first NDJSON line
- [ ] Clippy clean

---

## PRIORITY 2 — Live Mode UI

### Step 1 — Live workspace view

Add `WorkspaceLive.tsx` — activated by View → Live Mode or
clicking the microphone button in the toolbar:

```
┌─────────────────────────────────────────────────────────┐
│  ⚠ LIVE MACHINE TRANSLATION — NOT VERIFIED              │
│  Do not use for legal decisions in real time.           │
├─────────────────────────────────────────────────────────┤
│  ● RECORDING  Arabic → English  |  Stop                │
├─────────────────────────────────────────────────────────┤
│  LIVE  [Waveform animation]                             │
├────────────────────────┬────────────────────────────────┤
│  ORIGINAL              │  TRANSLATION                   │
│                        │                                │
│  00:03 مرحبا كيف حالك  │  00:03 Hello how are you      │
│  00:06 بخير شكرا       │  00:06 Fine thanks             │
│  00:09 ▌               │  00:09 ▌                       │
│                        │                                │
└────────────────────────┴────────────────────────────────┘
│  Segments: 3  |  Duration: 00:09  |  Language: Arabic  │
│  ⚠ LIVE MT — UNVERIFIED                               │
└─────────────────────────────────────────────────────────┘
```

**Live advisory banner:** Amber at the top. Always visible.
Cannot be dismissed in live mode.

**Recording indicator:** Red pulsing dot + "RECORDING" text.

**Stop button:** Ends the session. Offers to save the session
as a standard evidence package.

**Save session:**
When "Stop" is clicked, dialog:
```
Save live session?

The following was captured:
  Duration: 4m 32s
  Language: Arabic (Egyptian)
  Segments: 87
  Translation: NLLB-200 (arz_Arab)

[Save as Evidence Package]  [Save Transcript Only]  [Discard]
```

### Step 2 — Toolbar microphone button

Add a microphone button to the toolbar:
```
[🇸🇦 Arabic ▾] → [🇺🇸 English ▾] | Whisper ▾ | Auto ▾ | 🎙 Live | ● Live
```

The 🎙 button toggles Live mode.
When active: red pulsing dot appears, workspace switches to LiveWorkspace.
When stopped: offers to save session.

### Step 3 — Language auto-detection in live mode

In live mode, the source language can be "auto" — AUGUR detects
the language per chunk. The toolbar source picker changes to:

```
[Auto-detect ▾] → [English ▾]
```

When Arabic is detected, the dialect card appears automatically
with real-time dialect confidence updates.

### Step 4 — Live session export

When the examiner saves a live session, it produces the same
package format as a file-based translation:

- Transcript of all chunks with timestamps
- Translation of each chunk
- Chain of custody: "Live session captured [date] via microphone"
- Full MT advisory + live advisory

CHAIN_OF_CUSTODY.txt for live sessions:
```
AUGUR Evidence Package — Live Session Chain of Custody
======================================================
Session type:   LIVE MICROPHONE CAPTURE
Session start:  2026-04-26 16:00:00 UTC
Session end:    2026-04-26 16:04:32 UTC
Duration:       4m 32s
Language:       Arabic (Egyptian, CAMeL confidence: 0.87)
Model (STT):    Whisper Large-v3
Model (Trans):  NLLB-200 (arz_Arab — Egyptian Arabic token)
Examiner:       D. Examiner
Case:           2026-042

MACHINE TRANSLATION NOTICE:
This transcript was produced by real-time machine translation.
Content has NOT been reviewed. Verify ALL content with a
certified human linguist before use in legal proceedings.

LIVE SESSION ADVISORY:
Real-time translation is inherently less accurate than
offline processing. Background noise, overlapping speech,
and fast speech all reduce accuracy significantly.
```

### Tests

```rust
#[test]
fn silence_detection_returns_true_for_zero_samples() {
    let samples = vec![0.0f32; 4800]; // 100ms of silence at 48kHz
    assert!(is_silence(&samples, 0.01));
}

#[test]
fn silence_detection_returns_false_for_loud_speech() {
    let samples: Vec<f32> = (0..4800)
        .map(|i| (i as f32 * 0.01).sin() * 0.5)
        .collect();
    assert!(!is_silence(&samples, 0.01));
}

#[test]
fn live_chain_of_custody_includes_live_advisory() {
    let coc = render_live_chain_of_custody(&session_info);
    assert!(coc.contains("LIVE MICROPHONE CAPTURE"));
    assert!(coc.contains("real-time machine translation"));
    assert!(coc.contains("certified human linguist"));
}
```

### Acceptance criteria — P2

- [ ] `WorkspaceLive.tsx` renders live session view
- [ ] Live advisory banner always visible (amber, not dismissible)
- [ ] Recording indicator (red pulsing dot)
- [ ] Segments appear in real-time as chunks complete
- [ ] Stop button offers to save session
- [ ] Save session produces standard evidence package
- [ ] Live chain of custody includes live-specific advisory
- [ ] Toolbar microphone button toggles live mode
- [ ] Language auto-detection works in live mode
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## After both priorities complete

```bash
cargo test --workspace 2>&1 | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

Commit:
```bash
git add apps/augur-desktop/ apps/augur-cli/ crates/
git commit -m "feat: augur-sprint-19 real-time audio mode + live interview support"
```

Report:
- Whether `augur live` captures microphone audio
- Whether the live advisory banner is non-dismissible
- Test count before and after
- Any deviations from spec (cpal build issues etc)

---

_AUGUR Sprint 19 — Real-time Audio Mode_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_The capability that makes AUGUR useful in the field._
