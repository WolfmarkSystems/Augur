# AUGUR Sprint 16 — Evidence Package GUI + Multi-File Workflow
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

The `augur package` CLI command exists and produces a ZIP with
MANIFEST.json, CHAIN_OF_CUSTODY.txt, translations, and reports.
The desktop GUI has no way to trigger it. This sprint adds a
complete evidence packaging workflow to the GUI — the thing an
examiner hands to a prosecutor.

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- MT advisory in every output format — mandatory
- Chain of custody in every package — mandatory
- `cargo clippy -- -D warnings` clean

---

## PRIORITY 1 — Evidence Package Command in GUI

### Step 1 — Tauri command

In `apps/augur-desktop/src-tauri/src/export.rs`, add:

```rust
#[tauri::command]
pub async fn create_evidence_package(
    app: AppHandle,
    input_path: String,        // file or directory
    target_lang: String,
    case_number: String,
    examiner_name: String,
    agency: String,
    output_path: String,       // where to save the .zip
) -> Result<String, String> {
    let augur = find_augur_binary()
        .ok_or("AUGUR CLI not found.")?;

    let mut cmd = Command::new(&augur);
    cmd.arg("package")
       .arg("--input").arg(&input_path)
       .arg("--target").arg(&target_lang)
       .arg("--output").arg(&output_path)
       .arg("--case-number").arg(&case_number)
       .arg("--examiner").arg(&examiner_name)
       .arg("--agency").arg(&agency)
       .arg("--format-progress").arg("ndjson")
       .stdout(std::process::Stdio::piped());

    // Parse progress events:
    // {"type":"package_file_start","file":"recording.mp3","index":1,"total":12}
    // {"type":"package_file_done","file":"recording.mp3"}
    // {"type":"package_complete","output_path":"/path/to/case.zip",
    //  "total_files":12,"translated_files":7,"size_bytes":45000000}

    // Emit each as Tauri event
    // Return the output path on success

    Ok(output_path)
}
```

### Step 2 — Package config in augur CLI

Add `--case-number`, `--examiner`, `--agency` flags to
`augur package` command. These flow into:
- The MANIFEST.json `case_number`, `examiner`, `agency` fields
- The CHAIN_OF_CUSTODY.txt header
- The HTML report header

All three are optional — if not provided, fields are blank in
the output but the package still generates.

### Step 3 — Package workflow in GUI

Add "Create Evidence Package" to the File menu.

When clicked, open a multi-step dialog:

```
Step 1 — Select evidence
  [Select File]  or  [Select Folder]
  Shows: /evidence/phone_dump/ (47 files)

Step 2 — Case information
  Case Number: [2026-042        ]
  Examiner:    [D. Examiner     ]
  Agency:      [Wolfmark Systems]

Step 3 — Output
  Save package as: [AUGUR_Package_2026-042_20260426.zip]
  Format: [ZIP ▾]  (ZIP only for now)

Step 4 — Packaging (live progress)
  [1/12] recording_001.mp3   ████████ 100% ✓
  [2/12] document_002.pdf    ████░░░░  41% ↓
  ...
  Overall: 4.2 GB · 2m remaining

Step 5 — Done
  Package saved: /Users/.../AUGUR_Package_2026-042.zip
  [Open in Finder]  [Close]
```

Create `PackageWizard.tsx` component — modal overlay, same
visual style as the installer wizard.

### Step 4 — augur package NDJSON progress

Add `--format-progress ndjson` to `augur package` CLI that
emits per-file events during packaging so the GUI progress
wizard works.

### Step 5 — Chain of custody config flags

When `--examiner` and `--agency` are provided, the
CHAIN_OF_CUSTODY.txt becomes:

```
AUGUR Evidence Package — Chain of Custody
==========================================
Package created: 2026-04-26 16:00:00 UTC
Case number:     2026-042
Examiner:        D. Examiner
Agency:          Wolfmark Systems
System:          MacBook Pro M1 Max (darwin arm64)
AUGUR version:   1.0.0

[rest of chain of custody...]
```

### Tests

```rust
#[test]
fn package_manifest_includes_case_number() {
    let manifest = build_manifest(
        "2026-042", "D. Examiner", "Wolfmark Systems", &[]
    );
    assert_eq!(manifest.case_number, "2026-042");
}

#[test]
fn chain_of_custody_includes_examiner() {
    let coc = render_chain_of_custody(
        "2026-042", "D. Examiner", "Wolfmark Systems"
    );
    assert!(coc.contains("D. Examiner"));
    assert!(coc.contains("2026-042"));
}

#[test]
fn package_ndjson_complete_event_has_output_path() {
    let json = serde_json::json!({
        "type": "package_complete",
        "output_path": "/tmp/test.zip",
        "total_files": 5,
        "translated_files": 3,
    });
    assert!(json["output_path"].is_string());
}
```

### Acceptance criteria — P1

- [ ] `create_evidence_package` Tauri command
- [ ] `augur package` accepts case/examiner/agency flags
- [ ] CHAIN_OF_CUSTODY.txt includes examiner info
- [ ] MANIFEST.json includes case number
- [ ] `PackageWizard.tsx` 5-step modal
- [ ] Live per-file progress during packaging
- [ ] MT advisory in all package outputs
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 2 — Recent Files + Case Management

### Context

Examiners work multiple cases simultaneously. AUGUR needs to
remember recently opened files and allow setting a case number
that persists across sessions.

### Step 1 — Persist case state

Store case state in `~/Library/Application Support/AUGUR/case_state.json`:

```json
{
  "case_number": "2026-042",
  "examiner_name": "D. Examiner",
  "agency": "Wolfmark Systems",
  "recent_files": [
    {
      "path": "/evidence/recording_001.mp3",
      "opened_at": "2026-04-26T16:00:00Z",
      "source_lang": "ar",
      "target_lang": "en",
      "file_type": "audio"
    }
  ],
  "last_output_dir": "/Users/examiner/Cases/2026-042/"
}
```

### Step 2 — Tauri commands for case state

```rust
#[tauri::command]
pub async fn get_case_state(app: AppHandle)
    -> Result<serde_json::Value, String>

#[tauri::command]
pub async fn set_case_info(
    app: AppHandle,
    case_number: String,
    examiner_name: String,
    agency: String,
) -> Result<(), String>

#[tauri::command]
pub async fn add_recent_file(
    app: AppHandle,
    path: String,
    source_lang: String,
    target_lang: String,
    file_type: String,
) -> Result<(), String>
```

### Step 3 — Recent Files in File menu

File menu "Recent Files" submenu shows last 10 opened files.
Clicking reopens the file and restores the source/target language.

### Step 4 — Case Number in title bar

Title bar shows current case number:
```
AUGUR — Case 2026-042 — recording_001.mp3
```

When no case number is set:
```
AUGUR — No case set — [set case]
```

Clicking "[set case]" opens a simple input dialog.

### Tests

```rust
#[test]
fn case_state_persists_to_disk() {
    // Write case state, read it back, verify fields match
}

#[test]
fn recent_files_capped_at_ten() {
    // Add 15 files, verify only 10 remain
}
```

### Acceptance criteria — P2

- [ ] Case state persisted to disk
- [ ] `get_case_state` / `set_case_info` / `add_recent_file` commands
- [ ] Recent Files in File menu (last 10)
- [ ] Title bar shows case number
- [ ] Case info flows into package wizard automatically
- [ ] 2 new tests pass
- [ ] Clippy clean

---

## After both priorities complete

```bash
cd apps/augur-desktop/src-tauri && cargo build 2>&1 | tail -5
cargo clippy -- -D warnings 2>&1 | tail -3
cd ../../.. && cargo test --workspace 2>&1 | tail -5
```

Commit:
```bash
git add apps/augur-desktop/ apps/augur-cli/
git commit -m "feat: augur-sprint-16 evidence package GUI + case management"
```

Report:
- Whether PackageWizard renders all 5 steps
- Whether case state persists correctly
- Test count before and after
- Any deviations from spec

---

_AUGUR Sprint 16 — Evidence Package GUI + Case Management_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
