# AUGUR Sprint 17 — Human Review Workflow + Segment Flagging
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

The MT advisory tells examiners to verify translations with a human
linguist. But there's no workflow in AUGUR to actually do that.
This sprint adds segment-level human review — examiners flag
specific segments that need review, and the export clearly marks
them. Closes the loop between the advisory and examiner workflow.

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- MT advisory always present
- Flagged segments always marked in all export formats
- `cargo clippy -- -D warnings` clean

---

## PRIORITY 1 — Segment Flagging in GUI

### Step 1 — Flag state in Zustand store

Add to `appStore.ts`:

```typescript
interface SegmentFlag {
  segmentIndex: number
  flaggedAt: string    // ISO timestamp
  examinerNote: string
  reviewStatus: 'needs_review' | 'reviewed' | 'disputed'
}

// In AppState:
flaggedSegments: Map<number, SegmentFlag>
flagSegment: (index: number, note: string) => void
unflagSegment: (index: number) => void
setReviewStatus: (index: number, status: SegmentFlag['reviewStatus']) => void
```

### Step 2 — Flag UI on each segment

In `TranslationPanel.tsx`, add a flag button to each segment row.
When hovered, a small flag icon appears at the right edge of the
segment. Clicking it:

1. Opens a small popover with a text input: "Note for reviewer..."
2. Examiner types a note (optional)
3. Clicks "Flag for Review"
4. Segment gets a red left-border accent
5. Flag icon turns red and stays visible (not hover-only)

```tsx
function SegmentRow({ segment, flag, onFlag, onUnflag }) {
  return (
    <div className={`segment-row ${flag ? 'flagged' : ''}`}>
      <div className="segment-timestamp">{formatMs(segment.startMs)}</div>
      <div className="segment-text">{segment.translatedText}</div>
      <button
        className={`flag-btn ${flag ? 'active' : ''}`}
        onClick={() => flag ? onUnflag() : onFlag()}
        title={flag ? 'Flagged for review' : 'Flag for review'}
      >
        ⚑
      </button>
      {flag && (
        <div className="flag-note">{flag.examinerNote}</div>
      )}
    </div>
  )
}
```

Flagged segments:
- Red `border-left: 3px solid var(--color-danger)`
- Background: `var(--color-background-danger)` (very light red)
- Flag icon: red, always visible
- Note text shown below the segment in muted small text

### Step 3 — Needs Review panel

Add a "Review" tab in the right sidebar (alongside the dialect
card area). When any segments are flagged, this tab shows a badge:

```
┌─────────────────────┐
│  Needs Review  [3]  │
├─────────────────────┤
│ 00:45  [needs_review]│
│  "The package..."   │
│  Note: check "سلاح" │
│  [Mark Reviewed]    │
│                     │
│ 01:23  [needs_review]│
│  "Tomorrow at..."   │
│  [Mark Reviewed]    │
└─────────────────────┘
```

### Step 4 — Persist flags to disk

Save flagged segments alongside case state in
`~/Library/Application Support/AUGUR/case_state.json`:

```json
{
  "flagged_segments": {
    "/evidence/recording_001.mp3": [
      {
        "segment_index": 3,
        "flagged_at": "2026-04-26T16:30:00Z",
        "examiner_note": "Check translation of سلاح",
        "review_status": "needs_review"
      }
    ]
  }
}
```

```rust
#[tauri::command]
pub async fn save_segment_flags(
    app: AppHandle,
    file_path: String,
    flags: Vec<serde_json::Value>,
) -> Result<(), String>

#[tauri::command]
pub async fn get_segment_flags(
    app: AppHandle,
    file_path: String,
) -> Result<Vec<serde_json::Value>, String>
```

### Acceptance criteria — P1

- [ ] Flag button on every translation segment
- [ ] Flag popover with examiner note input
- [ ] Flagged segments have red left-border accent
- [ ] "Needs Review" panel shows all flagged segments
- [ ] "Mark Reviewed" changes status
- [ ] Flags persisted to disk via Tauri command
- [ ] Flags restored when file is reopened
- [ ] Clippy clean

---

## PRIORITY 2 — Flagged Segments in Export

### Step 1 — HTML report flagged section

When flagged segments exist, add a "Segments Requiring Human Review"
section at the top of the HTML report, before the main translation:

```html
<div class="review-required-banner">
  ⚠ 3 segments flagged for human review — see Section 2
</div>

<section id="segments-requiring-review">
  <h2>2. Segments Requiring Human Review</h2>
  <p>The following segments were flagged by the examiner as
     requiring verification by a certified human linguist.</p>

  <div class="flagged-segment">
    <div class="timestamp">00:45 — 00:52</div>
    <div class="original">الحزمة ستصل غداً مع السلاح</div>
    <div class="translation">[PENDING HUMAN REVIEW] The package
      will arrive tomorrow with the weapon</div>
    <div class="examiner-note">Examiner note: Check translation
      of سلاح — could be weapon, arm, or rifle</div>
    <div class="review-status">Status: NEEDS REVIEW</div>
  </div>
</section>
```

### Step 2 — JSON export flagged segments

```json
{
  "flagged_segments_count": 3,
  "flagged_segments": [
    {
      "segment_index": 3,
      "start_ms": 45000,
      "end_ms": 52000,
      "original": "الحزمة ستصل غداً مع السلاح",
      "translation": "The package will arrive tomorrow with the weapon",
      "review_status": "needs_review",
      "examiner_note": "Check translation of سلاح",
      "flagged_at": "2026-04-26T16:30:00Z",
      "machine_translation_notice": "..."
    }
  ]
}
```

### Step 3 — ZIP package review folder

Add `review/` directory to the ZIP package:

```
case-package.zip/
  MANIFEST.json
  CHAIN_OF_CUSTODY.txt
  REPORT.html
  REPORT.json
  translations/
  review/                          ← NEW
    REVIEW_REQUIRED.txt            ← human-readable summary
    flagged_segments.json          ← structured data
```

`REVIEW_REQUIRED.txt`:
```
AUGUR Evidence Package — Human Review Required
===============================================
3 segments have been flagged by the examiner for human review.

This package contains machine translations (NLLB-200, Meta AI).
The flagged segments below require verification by a certified
human linguist before use in legal proceedings.

Segment 3 (00:45 — 00:52):
  Original: الحزمة ستصل غداً مع السلاح
  MT Translation: The package will arrive tomorrow with the weapon
  Examiner note: Check translation of سلاح
  Status: NEEDS REVIEW

[...]

MACHINE TRANSLATION NOTICE:
All translations in this package are machine-generated and have
not been reviewed by a certified human translator. Flagged segments
are of particular concern. Verify ALL translations before use in
legal proceedings.
```

### Step 4 — Export command accepts flag data

Update `create_evidence_package` Tauri command to accept flags:

```rust
#[tauri::command]
pub async fn create_evidence_package(
    app: AppHandle,
    input_path: String,
    target_lang: String,
    case_number: String,
    examiner_name: String,
    agency: String,
    output_path: String,
    flagged_segments: Vec<serde_json::Value>,  // NEW
) -> Result<String, String>
```

### Tests

```rust
#[test]
fn html_report_includes_review_banner_when_flags_present() {
    let flags = vec![/* one flag */];
    let html = render_html_report(&segments, &flags, &config);
    assert!(html.contains("Segments Requiring Human Review"));
    assert!(html.contains("PENDING HUMAN REVIEW"));
}

#[test]
fn html_report_no_review_banner_when_no_flags() {
    let html = render_html_report(&segments, &[], &config);
    assert!(!html.contains("Segments Requiring Human Review"));
}

#[test]
fn zip_package_has_review_folder_when_flags_present() {
    // Build package with flags, verify review/ dir in ZIP
}

#[test]
fn review_required_txt_contains_mt_advisory() {
    let txt = render_review_required_txt(&flags);
    assert!(txt.contains("machine-generated"));
    assert!(txt.contains("certified human linguist"));
}
```

### Acceptance criteria — P2

- [ ] HTML report shows review banner when flags present
- [ ] Flagged segments rendered with [PENDING HUMAN REVIEW] label
- [ ] JSON export includes flagged_segments array
- [ ] ZIP package includes review/ directory
- [ ] REVIEW_REQUIRED.txt has MT advisory
- [ ] No review section when no segments flagged
- [ ] 4 new tests pass
- [ ] Clippy clean

---

## After both priorities complete

Commit:
```bash
git add apps/augur-desktop/ apps/augur-cli/ crates/augur-core/
git commit -m "feat: augur-sprint-17 human review workflow + segment flagging + export integration"
```

Report:
- Which priorities passed
- Test count before and after
- Description of flagging UX
- Any deviations from spec

---

_AUGUR Sprint 17 — Human Review Workflow_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_Makes the MT advisory actionable instead of decorative._
