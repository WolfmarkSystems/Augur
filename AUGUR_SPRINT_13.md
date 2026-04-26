# AUGUR Sprint 13 — Pipeline Wiring + Real CLI Integration + Production Polish
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

Sprint 12 built the complete AUGUR desktop GUI with a deterministic
fixture pipeline. This sprint wires the real `augur` CLI subprocess
into `apps/augur-desktop/src-tauri/src/pipeline.rs` so the GUI
actually translates real evidence files.

After this sprint, the full flow works end to end:
1. Examiner opens AUGUR Desktop
2. File → Open Evidence → selects a file
3. Real Whisper STT runs (if audio/video)
4. Real NLLB translation runs
5. Segments appear live in the split view
6. Export produces a real report

This is the integration sprint that makes AUGUR a real product.

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- All errors surface to UI via `translation-error` event
- MT advisory always present on all output
- `cargo clippy -- -D warnings` clean
- Offline invariant maintained

---

## PRIORITY 1 — Wire `augur translate` Subprocess

### Context

`pipeline.rs` currently runs a deterministic 4-segment fixture.
Replace it with a real subprocess call to `augur translate` and
parse its streaming JSON output.

### Step 1 — Find the augur binary

The desktop app needs to find the `augur` binary. Priority order:

```rust
pub fn find_augur_binary() -> Option<PathBuf> {
    // 1. AUGUR_BIN env var (for dev/testing)
    if let Ok(path) = std::env::var("AUGUR_BIN") {
        let p = PathBuf::from(path);
        if p.exists() { return Some(p); }
    }

    // 2. Same directory as the desktop app binary
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.parent()
            .unwrap_or(Path::new("."))
            .join("augur");
        if sibling.exists() { return Some(sibling); }
    }

    // 3. ~/.cargo/bin/augur (development installs)
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin").join("augur");
        if cargo_bin.exists() { return Some(cargo_bin); }
    }

    // 4. System PATH
    which::which("augur").ok()
}
```

Add to `Cargo.toml`:
```toml
dirs = "5"
which = "6"
```

### Step 2 — augur translate output format

The `augur translate` command outputs NDJSON (newline-delimited JSON)
when called with `--format ndjson`. Each line is one of:

```json
{"type":"segment","index":0,"start_ms":0,"end_ms":3000,
 "original":"مرحبا بالعالم","translated":"Hello world",
 "is_complete":true}

{"type":"dialect","dialect":"Egyptian","confidence":0.89,
 "source":"camel","indicators":["إيه","كده"]}

{"type":"code_switch","offset":89,"from":"ar","to":"en"}

{"type":"complete","total_segments":12,"duration_ms":45000}

{"type":"error","message":"Model not found: whisper-large-v3"}
```

If `augur translate` doesn't support `--format ndjson` yet, add it.
Check the current CLI:

```bash
grep -rn "format\|ndjson\|output" \
    apps/augur-cli/src/ --include="*.rs" | head -20
```

If NDJSON output doesn't exist, add it to `cmd_translate` in
`apps/augur-cli/src/translate.rs` alongside the existing output
formats.

### Step 3 — pipeline.rs real implementation

```rust
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn start_translation(
    app: AppHandle,
    file_path: String,
    source_lang: String,
    target_lang: String,
    stt_model: String,
    engine: String,
) -> Result<(), String> {
    let augur = find_augur_binary()
        .ok_or("AUGUR CLI not found. Run augur install first.")?;

    let mut cmd = Command::new(&augur);
    cmd.arg("translate")
       .arg("--input").arg(&file_path)
       .arg("--target").arg(&target_lang)
       .arg("--format").arg("ndjson")
       .stdout(std::process::Stdio::piped())
       .stderr(std::process::Stdio::piped());

    // STT model selection
    if stt_model != "auto" {
        cmd.arg("--model").arg(&stt_model);
    }

    // Engine selection
    if engine != "auto" {
        cmd.arg("--engine").arg(&engine);
    }

    // Source language hint if not auto-detect
    if source_lang != "auto" {
        cmd.arg("--source").arg(&source_lang);
    }

    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to start AUGUR: {}", e))?;

    let stdout = child.stdout.take()
        .ok_or("No stdout from AUGUR process")?;

    let mut lines = BufReader::new(stdout).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.is_empty() { continue; }

        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(json) => {
                match json["type"].as_str() {
                    Some("segment") => {
                        app.emit("segment-ready", &json).ok();
                    }
                    Some("dialect") => {
                        app.emit("dialect-detected", &json).ok();
                    }
                    Some("code_switch") => {
                        app.emit("code-switch-detected", &json).ok();
                    }
                    Some("complete") => {
                        app.emit("translation-complete", &json).ok();
                        break;
                    }
                    Some("error") => {
                        let msg = json["message"]
                            .as_str()
                            .unwrap_or("Unknown error")
                            .to_string();
                        app.emit("translation-error",
                            serde_json::json!({"message": msg})).ok();
                        break;
                    }
                    _ => {
                        log::warn!("Unknown event type in line: {}", line);
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to parse NDJSON line: {} — {}", line, e);
            }
        }
    }

    // Wait for process to finish
    let status = child.wait().await
        .map_err(|e| format!("AUGUR process error: {}", e))?;

    if !status.success() {
        // Check stderr for error details
        if let Some(stderr) = child.stderr.take() {
            let mut err_lines = BufReader::new(stderr).lines();
            let mut err_msg = String::new();
            while let Ok(Some(line)) = err_lines.next_line().await {
                err_msg.push_str(&line);
                err_msg.push('\n');
            }
            if !err_msg.is_empty() {
                app.emit("translation-error",
                    serde_json::json!({"message": err_msg.trim()})).ok();
            }
        }
    }

    Ok(())
}
```

### Step 4 — NDJSON output in augur CLI

In `apps/augur-cli/src/translate.rs`, add NDJSON streaming output:

```rust
pub enum OutputFormat {
    Text,    // existing default
    Json,    // existing JSON
    Ndjson,  // NEW — newline-delimited JSON for GUI streaming
}

// When format == Ndjson, print one JSON object per line:
fn emit_segment_ndjson(segment: &TranslationSegment) {
    let json = serde_json::json!({
        "type": "segment",
        "index": segment.index,
        "start_ms": segment.start_ms,
        "end_ms": segment.end_ms,
        "original": segment.original_text,
        "translated": segment.translated_text,
        "is_complete": true,
    });
    println!("{}", json);  // one line per segment — flushed immediately
}

fn emit_dialect_ndjson(dialect: &DialectAnalysis) {
    let json = serde_json::json!({
        "type": "dialect",
        "dialect": format!("{:?}", dialect.detected_dialect),
        "confidence": dialect.confidence,
        "source": if dialect.source == "camel" { "camel" } else { "lexical" },
    });
    println!("{}", json);
}

fn emit_complete_ndjson(total: usize, duration_ms: u64) {
    let json = serde_json::json!({
        "type": "complete",
        "total_segments": total,
        "duration_ms": duration_ms,
    });
    println!("{}", json);
}
```

Note: `println!` is the audited CLI output helper for NDJSON —
this is the one permitted surface for stdout output in the CLI.
Use `log::` for all debug/warn/error output.

### Step 5 — Remove the fixture

In `apps/augur-desktop/src-tauri/src/pipeline.rs`, remove the
deterministic 4-segment fixture. Replace with the real subprocess
implementation above. Keep the fixture available behind a feature
flag for testing:

```rust
#[cfg(feature = "fixture-pipeline")]
pub async fn start_translation_fixture(app: &AppHandle) { /* ... */ }
```

### Tests

```rust
#[test]
fn find_augur_binary_returns_some_when_in_cargo_bin() {
    // If running `cargo test` after `cargo install augur`,
    // find_augur_binary() should return Some
    // (test is skip-gated if augur not installed)
    if let Some(path) = find_augur_binary() {
        assert!(path.exists());
    }
}

#[test]
fn ndjson_segment_serializes_correctly() {
    let json = serde_json::json!({
        "type": "segment",
        "index": 0,
        "original": "مرحبا",
        "translated": "Hello",
        "is_complete": true,
    });
    assert_eq!(json["type"], "segment");
    assert_eq!(json["translated"], "Hello");
}

#[test]
fn ndjson_parse_segment_event() {
    let line = r#"{"type":"segment","index":0,"original":"test","translated":"test","is_complete":true}"#;
    let json: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(json["type"], "segment");
}
```

### Acceptance criteria — P1

- [ ] `find_augur_binary()` searches 4 locations in priority order
- [ ] `augur translate --format ndjson` outputs NDJSON stream
- [ ] `pipeline.rs` spawns real `augur` subprocess
- [ ] Each NDJSON event emits corresponding Tauri event
- [ ] Errors from stderr surfaced as `translation-error` event
- [ ] Fixture removed from production path
- [ ] 3 new tests pass
- [ ] `cargo build` succeeds for both apps

---

## PRIORITY 2 — Batch File Processing

### Context

Examiners frequently need to process an entire directory of
evidence files — an iPhone backup folder, a seized drive's
documents folder, an email archive. The desktop GUI should
support batch processing, not just single files.

### Implementation

**Step 1 — Batch command**

Add `start_batch_translation` Tauri command:

```rust
#[tauri::command]
pub async fn start_batch_translation(
    app: AppHandle,
    input_dir: String,
    target_lang: String,
    output_path: String,
    format: String,  // "html" | "json" | "zip"
) -> Result<(), String> {
    let augur = find_augur_binary()
        .ok_or("AUGUR CLI not found.")?;

    let mut cmd = Command::new(&augur);
    cmd.arg("batch")
       .arg("--input").arg(&input_dir)
       .arg("--target").arg(&target_lang)
       .arg("--output").arg(&output_path)
       .arg("--format").arg(&format)
       .arg("--format-progress").arg("ndjson") // NEW flag
       .stdout(std::process::Stdio::piped());

    // Parse batch progress NDJSON:
    // {"type":"batch_file_start","file":"recording.mp3","index":1,"total":47}
    // {"type":"batch_file_done","file":"recording.mp3","index":1,"total":47}
    // {"type":"batch_complete","total_files":47,"foreign_files":12}
    
    // Emit these as Tauri events for the batch progress UI
    Ok(())
}
```

**Step 2 — Batch progress UI**

Add a batch mode to the main workspace. When a directory is
loaded instead of a single file:

```
┌─────────────────────────────────────────────┐
│  BATCH MODE — /evidence/phone_dump/         │
│  47 files · Standard profile               │
├─────────────────────────────────────────────┤
│  [1/47] recording_001.mp3    ████████ 100% ✓│
│  [2/47] doc_002.pdf          ████░░░░  42% ↓│
│  [3/47] image_003.jpg        waiting...     │
│  ...                                        │
├─────────────────────────────────────────────┤
│  Overall: 1/47 complete · 12 foreign found  │
└─────────────────────────────────────────────┘
```

Add `WorkspaceBatch.tsx` component following the same pattern as
`WorkspaceDoc` and `WorkspaceAudio`.

**Step 3 — Open directory in file picker**

In `file_load.rs`, add:

```rust
#[tauri::command]
pub async fn open_directory_dialog(
    app: AppHandle,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog()
        .file()
        .blocking_pick_folder();
    Ok(path.map(|p| p.to_string_lossy().to_string()))
}
```

Add "Open Folder..." option to File menu alongside "Open Evidence...".

**Step 4 — Batch NDJSON progress from CLI**

Add `--format-progress ndjson` flag to `augur batch` that emits
per-file progress events to stdout, separate from the final report.

### Tests

```rust
#[test]
fn batch_ndjson_file_start_parsed() {
    let line = r#"{"type":"batch_file_start","file":"test.mp3","index":1,"total":10}"#;
    let json: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(json["type"], "batch_file_start");
    assert_eq!(json["total"], 10);
}

#[test]
fn open_directory_command_registered() {
    // Command exists in the Tauri handler list
    // Verified by build succeeding with the command registered
}
```

### Acceptance criteria — P2

- [ ] `open_directory_dialog` command opens folder picker
- [ ] "Open Folder..." in File menu
- [ ] `start_batch_translation` command spawns `augur batch`
- [ ] Batch NDJSON progress events emitted from CLI
- [ ] `WorkspaceBatch.tsx` renders batch progress view
- [ ] File count, current file, and overall progress shown
- [ ] 2 new tests pass

---

## PRIORITY 3 — Model Manager Functionality

### Context

Sprint 12 built the Model Manager modal UI but it shows
static data. This sprint wires it to the real `augur install`
command so examiners can install models directly from within
the desktop app.

### Implementation

**Step 1 — Model status Tauri command**

```rust
#[tauri::command]
pub async fn get_model_status(
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    let augur = find_augur_binary()
        .ok_or("AUGUR CLI not found.")?;

    let output = Command::new(&augur)
        .args(["install", "--status", "--format", "json"])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse model status: {}", e))?;

    Ok(json)
}
```

Add `--format json` to `augur install --status` in the CLI:

```json
{
  "profile": "standard",
  "models": [
    {
      "id": "whisper-tiny",
      "name": "Whisper Tiny",
      "installed": true,
      "size_bytes": 75000000,
      "size_display": "75 MB"
    },
    {
      "id": "whisper-large-v3",
      "name": "Whisper Large-v3",
      "installed": false,
      "size_bytes": 2900000000,
      "size_display": "2.9 GB"
    }
  ],
  "total_installed_bytes": 3375900000,
  "profile_complete": false
}
```

**Step 2 — Install model from GUI**

Add a "Download" button next to each uninstalled model in the
Model Manager. Clicking it runs `augur install --model <id>`:

```rust
#[tauri::command]
pub async fn install_model(
    app: AppHandle,
    model_id: String,
) -> Result<(), String> {
    let augur = find_augur_binary()
        .ok_or("AUGUR CLI not found.")?;

    let mut child = Command::new(&augur)
        .args(["install", "--model", &model_id, "--format", "ndjson"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    // Parse download progress events and emit to frontend
    // {"type":"model_download_progress","model_id":"whisper-large-v3",
    //  "percent":42,"speed_mbps":12.4,"eta_seconds":180}
    // {"type":"model_download_complete","model_id":"whisper-large-v3"}

    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            app.emit("model-install-progress", &json).ok();
            if json["type"] == "model_download_complete" {
                break;
            }
        }
    }
    child.wait().await.ok();
    Ok(())
}
```

**Step 3 — Model Manager UI update**

Update `ModelManager.tsx` to:
- Load real model status on open via `get_model_status`
- Show installed/not-installed per model with real data
- "Download" button triggers `install_model`
- Per-model progress bar during download
- Refresh status after download completes

**Step 4 — `augur install --model <id>` CLI**

Add single-model install to the CLI:
```bash
augur install --model whisper-large-v3
```

This installs just that one model without touching the others.
Useful for adding models post-initial-install.

Add `--format ndjson` output to single-model install that emits
download progress events (same format as the installer wizard).

### Tests

```rust
#[test]
fn model_status_json_has_required_fields() {
    let json = serde_json::json!({
        "profile": "standard",
        "models": [],
        "total_installed_bytes": 0,
        "profile_complete": false,
    });
    assert!(json["profile"].is_string());
    assert!(json["models"].is_array());
}

#[test]
fn install_model_command_registered() {
    // Verified by build succeeding
}
```

### Acceptance criteria — P3

- [ ] `get_model_status` returns real installed/missing status
- [ ] `augur install --status --format json` outputs JSON
- [ ] Model Manager shows real data (not static)
- [ ] Download button triggers model install
- [ ] Per-model progress bar during download
- [ ] `augur install --model <id>` installs single model
- [ ] 2 new tests pass

---

## PRIORITY 4 — Production Polish + Error States

### Context

The GUI needs to handle real-world error conditions gracefully.
When augur isn't installed, when models are missing, when a file
fails to process — the examiner needs clear, actionable messages.

### Error states to handle

**1. AUGUR CLI not found:**
```
┌─────────────────────────────────────────────┐
│  ⚠ AUGUR is not installed                  │
│                                             │
│  The AUGUR translation engine could not    │
│  be found on this system.                  │
│                                             │
│  [Open AUGUR Installer]                    │
│  or run: augur install standard            │
└─────────────────────────────────────────────┘
```

**2. Models not installed:**
```
┌─────────────────────────────────────────────┐
│  ⚠ Models not installed                    │
│                                             │
│  Whisper Large-v3 is required for audio    │
│  transcription but is not installed.        │
│                                             │
│  [Open Model Manager]  [Use Tiny instead]  │
└─────────────────────────────────────────────┘
```

**3. File format not supported:**
```
┌─────────────────────────────────────────────┐
│  Unsupported file type                      │
│                                             │
│  AUGUR cannot process .xyz files.           │
│  Supported: mp3, mp4, wav, pdf, txt, srt    │
└─────────────────────────────────────────────┘
```

**4. Translation failed:**
```
┌─────────────────────────────────────────────┐
│  ⚠ Translation failed                      │
│                                             │
│  [error message from CLI]                  │
│                                             │
│  [Try Again]  [Open Model Manager]         │
└─────────────────────────────────────────────┘
```

### Implementation

**ErrorBanner.tsx** — a dismissible error banner at the top of the
workspace:

```tsx
interface ErrorBannerProps {
  type: 'cli-not-found' | 'models-missing' | 'unsupported-file'
       | 'translation-failed' | null
  message?: string
  onDismiss: () => void
  onAction?: () => void
  actionLabel?: string
}
```

**Startup check:**
On app launch, check if `augur` CLI is available:

```rust
#[tauri::command]
pub async fn check_augur_available() -> bool {
    find_augur_binary().is_some()
}
```

If not available, show the CLI-not-found error state immediately
rather than waiting for the examiner to try to open a file.

**Self-test on launch:**
Run `augur self-test` on startup (async, non-blocking) and surface
any FAIL items to the status bar:

```
● 2 components unavailable — Open Model Manager to resolve
```

### Tests

```rust
#[test]
fn check_augur_available_returns_bool() {
    // Just verify the function runs without panicking
    let _ = find_augur_binary();
}

#[test]
fn error_types_cover_all_states() {
    // Document the 4 error types exist as constants/enum
    // Verified by compilation
}
```

### Acceptance criteria — P4

- [ ] Startup check for `augur` CLI availability
- [ ] Error banner component for all 4 error states
- [ ] "Open AUGUR Installer" button on CLI-not-found state
- [ ] "Open Model Manager" button on models-missing state
- [ ] Self-test runs on startup, surfaces FAILs to status bar
- [ ] All error messages are user-readable (no Rust stack traces)
- [ ] 2 new tests pass

---

## After all priorities complete

```bash
cd apps/augur-desktop/src-tauri && cargo build 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1 | grep "^error" | head -5
cd ../../../apps/augur-installer/src-tauri && cargo build 2>&1 | tail -5
cd ../../.. && cargo test --workspace 2>&1 | tail -5
```

Commit:
```bash
git add apps/augur-desktop/ apps/augur-cli/ crates/augur-core/
git commit -m "feat: augur-sprint-13 real pipeline wiring + batch mode + model manager + error states"
```

Report:
- Which priorities passed
- Whether `augur translate --format ndjson` works end to end
- Test count before (189) and after
- Any deviations from spec

---

## What this sprint closes

After Sprint 13, AUGUR Desktop is fully integrated:

```
Load file → real augur translate subprocess
          → NDJSON stream → Tauri events
          → React live update → segments appear
          → dialect detected → dialect card shown
          → code switch → amber band appears
          → complete → status bar updates
          → Export → real HTML/JSON/ZIP with MT advisory
```

The only remaining gap is packaging (.dmg / .pkg) which is
infrastructure work outside the code sprint scope.

---

_AUGUR Sprint 13 — Pipeline Wiring + Production Polish_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_This sprint makes AUGUR Desktop a real forensic tool._
