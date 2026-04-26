# AUGUR Sprint 12 — Tauri Desktop GUI: Main Application
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

This sprint builds the main AUGUR desktop application — the tool
examiners use daily for forensic translation work. The installer
wizard (Sprint 11) handles setup. This is the application itself.

Design reference: the mockup is locked and approved. Match it exactly.

Core examiner workflow:
1. Open AUGUR
2. File → Load evidence (audio, video, image, PDF, subtitle, text)
3. Source language auto-detected (or manually selected)
4. Select target language
5. Live translation appears in the right panel as AUGUR processes
6. Export report (JSON, HTML, zip package)

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- All errors surfaced to UI — never silent failures
- MT advisory always visible — cannot be dismissed
- `cargo clippy -- -D warnings` clean
- Dark mode compatible throughout

---

## Repository structure

Create at `~/Wolfmark/augur/apps/augur-desktop/`:

```
apps/augur-desktop/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs
│       ├── pipeline.rs      ← calls augur-core pipeline
│       ├── models.rs        ← model selection + status
│       ├── file_load.rs     ← file open + type detection
│       ├── export.rs        ← report export
│       └── state.rs         ← app state management
├── src/
│   ├── App.tsx
│   ├── main.tsx
│   ├── components/
│   │   ├── TitleBar.tsx
│   │   ├── MenuBar.tsx
│   │   ├── Toolbar.tsx
│   │   ├── LangPicker.tsx       ← the full language dropdown
│   │   ├── WorkspaceDoc.tsx     ← document split view
│   │   ├── WorkspaceAudio.tsx   ← audio/video transcript view
│   │   ├── SourcePanel.tsx
│   │   ├── TranslationPanel.tsx
│   │   ├── DialectCard.tsx
│   │   ├── CodeSwitchBand.tsx
│   │   ├── StatusBar.tsx
│   │   └── ModelManager.tsx    ← Models menu panel
│   ├── types/
│   │   └── index.ts
│   ├── store/
│   │   └── appStore.ts         ← Zustand state
│   └── ipc/
│       └── index.ts
├── package.json
└── vite.config.ts
```

---

## PRIORITY 1 — App Shell + Window Configuration

### tauri.conf.json

```json
{
  "productName": "AUGUR",
  "version": "1.0.0",
  "identifier": "systems.wolfmark.augur",
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../dist"
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/icon.png"],
    "macOS": {
      "minimumSystemVersion": "12.0"
    }
  },
  "app": {
    "windows": [
      {
        "title": "AUGUR",
        "width": 1280,
        "height": 800,
        "minWidth": 900,
        "minHeight": 600,
        "resizable": true,
        "center": true,
        "decorations": true,
        "titleBarStyle": "Default"
      }
    ],
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'"
    }
  }
}
```

### App state (Zustand)

`src/store/appStore.ts`:

```typescript
import { create } from 'zustand'

export interface Language {
  code: string
  name: string
  flag: string
  quality: 'hi' | 'med' | 'low'
  tier: 'High quality' | 'Forensic priority' | 'Limited quality'
  sub?: string
}

export interface TranslationSegment {
  index: number
  startMs?: number
  endMs?: number
  originalText: string
  translatedText: string
  isComplete: boolean
  isCodeSwitch?: boolean
  switchFrom?: string
  switchTo?: string
}

export interface DialectInfo {
  dialect: string
  confidence: number
  source: 'camel' | 'lexical'
  indicators?: string[]
}

export interface AppState {
  // File
  loadedFile: string | null
  fileType: 'document' | 'audio' | 'video' | 'subtitle' | null
  fileName: string | null

  // Languages
  sourceLang: Language
  targetLang: Language

  // Models
  sttModel: string
  translationEngine: string

  // Translation state
  isTranslating: boolean
  segments: TranslationSegment[]
  dialect: DialectInfo | null
  hasCodeSwitching: boolean
  overallProgress: number

  // Case info
  caseNumber: string

  // Actions
  setSourceLang: (lang: Language) => void
  setTargetLang: (lang: Language) => void
  setSttModel: (model: string) => void
  setTranslationEngine: (engine: string) => void
  loadFile: (path: string) => void
  addSegment: (segment: TranslationSegment) => void
  setDialect: (dialect: DialectInfo) => void
  setIsTranslating: (v: boolean) => void
  setCaseNumber: (n: string) => void
}
```

### Acceptance criteria — P1

- [ ] Tauri app scaffold at `apps/augur-desktop/`
- [ ] Window: 1280x800, min 900x600, resizable
- [ ] Zustand store with all state types
- [ ] `cargo build` succeeds
- [ ] `npm run dev` starts successfully

---

## PRIORITY 2 — Language Picker Component

This is the most important UI component. It must match the
approved mockup exactly.

### LangPicker.tsx

The full language list — all 47 languages across three tiers.
Both source and target pickers use the same component.

```typescript
export const ALL_LANGUAGES: Language[] = [
  // High quality
  { code:'ar', name:'Arabic', flag:'🇸🇦', quality:'hi',
    tier:'High quality', sub:'200M+ speakers' },
  { code:'zh', name:'Chinese (Simplified)', flag:'🇨🇳',
    quality:'hi', tier:'High quality', sub:'Mandarin' },
  { code:'ru', name:'Russian', flag:'🇷🇺',
    quality:'hi', tier:'High quality' },
  { code:'fr', name:'French', flag:'🇫🇷',
    quality:'hi', tier:'High quality' },
  { code:'de', name:'German', flag:'🇩🇪',
    quality:'hi', tier:'High quality' },
  { code:'es', name:'Spanish', flag:'🇪🇸',
    quality:'hi', tier:'High quality' },
  { code:'fa', name:'Farsi / Persian', flag:'🇮🇷',
    quality:'hi', tier:'High quality' },
  { code:'hi', name:'Hindi', flag:'🇮🇳',
    quality:'hi', tier:'High quality' },
  { code:'ur', name:'Urdu', flag:'🇵🇰',
    quality:'hi', tier:'High quality' },
  { code:'ko', name:'Korean', flag:'🇰🇵',
    quality:'hi', tier:'High quality' },
  { code:'ja', name:'Japanese', flag:'🇯🇵',
    quality:'hi', tier:'High quality' },
  { code:'tr', name:'Turkish', flag:'🇹🇷',
    quality:'hi', tier:'High quality' },
  { code:'vi', name:'Vietnamese', flag:'🇻🇳',
    quality:'hi', tier:'High quality' },
  { code:'id', name:'Indonesian', flag:'🇮🇩',
    quality:'hi', tier:'High quality' },
  { code:'pt', name:'Portuguese', flag:'🇵🇹',
    quality:'hi', tier:'High quality' },
  { code:'it', name:'Italian', flag:'🇮🇹',
    quality:'hi', tier:'High quality' },
  { code:'nl', name:'Dutch', flag:'🇳🇱',
    quality:'hi', tier:'High quality' },
  { code:'pl', name:'Polish', flag:'🇵🇱',
    quality:'hi', tier:'High quality' },
  { code:'uk', name:'Ukrainian', flag:'🇺🇦',
    quality:'hi', tier:'High quality' },
  { code:'so', name:'Somali', flag:'🇸🇴',
    quality:'hi', tier:'High quality' },
  { code:'bn', name:'Bengali', flag:'🇧🇩',
    quality:'hi', tier:'High quality' },
  { code:'en', name:'English', flag:'🇺🇸',
    quality:'hi', tier:'High quality' },

  // Forensic priority
  { code:'ps', name:'Pashto', flag:'🇦🇫',
    quality:'med', tier:'Forensic priority',
    sub:'fine-tune available' },
  { code:'prs', name:'Dari', flag:'🇦🇫',
    quality:'med', tier:'Forensic priority',
    sub:'Afghan Persian' },
  { code:'am', name:'Amharic', flag:'🇪🇹',
    quality:'med', tier:'Forensic priority' },
  { code:'ti', name:'Tigrinya', flag:'🇪🇷',
    quality:'med', tier:'Forensic priority',
    sub:'Eritrea / Ethiopia' },
  { code:'sw', name:'Swahili', flag:'🇹🇿',
    quality:'med', tier:'Forensic priority' },
  { code:'th', name:'Thai', flag:'🇹🇭',
    quality:'med', tier:'Forensic priority' },
  { code:'pa', name:'Panjabi', flag:'🇮🇳',
    quality:'med', tier:'Forensic priority' },
  { code:'ms', name:'Malay', flag:'🇲🇾',
    quality:'med', tier:'Forensic priority' },

  // Limited quality
  { code:'ha', name:'Hausa', flag:'🇳🇬',
    quality:'low', tier:'Limited quality',
    sub:'West Africa' },
  { code:'yo', name:'Yoruba', flag:'🇳🇬',
    quality:'low', tier:'Limited quality' },
  { code:'ig', name:'Igbo', flag:'🇳🇬',
    quality:'low', tier:'Limited quality' },
  { code:'my', name:'Burmese', flag:'🇲🇲',
    quality:'low', tier:'Limited quality' },
  { code:'km', name:'Khmer', flag:'🇰🇭',
    quality:'low', tier:'Limited quality' },
  { code:'lo', name:'Lao', flag:'🇱🇦',
    quality:'low', tier:'Limited quality' },
  { code:'sd', name:'Sindhi', flag:'🇵🇰',
    quality:'low', tier:'Limited quality' },
]
```

**Picker behavior:**
- Clicking source or target button opens the full dropdown
- Search input filters by name, code, or sub-label as you type
- Three section headers: High quality / Forensic priority / Limited quality
- Active language shows green checkmark
- Quality badge on every row
- Clicking a language closes the dropdown and updates the toolbar
- Clicking outside the dropdown closes it
- ESC key closes the dropdown
- Source and target dropdowns position correctly (source from left edge, target from ~152px right)

**Toolbar display after selection:**
```
[🇸🇦 Arabic / ar · Egyptian dialect ▾] → [🇺🇸 English / en · target ▾]
```

Name, code, and dialect note (if detected) all visible in the button.

### Acceptance criteria — P2

- [ ] Both source and target pickers functional
- [ ] All 35+ languages in correct tiers
- [ ] Search filters correctly
- [ ] Active selection shows checkmark
- [ ] Quality badges on all rows
- [ ] Dropdown positions correctly for both pickers
- [ ] ESC and outside-click close dropdown
- [ ] Toolbar updates immediately on selection
- [ ] Status bar updates to show new language pair

---

## PRIORITY 3 — Document Workspace (Split View)

The core view for text, PDF, image, and subtitle inputs.

### WorkspaceDoc.tsx

Two panels separated by a draggable center divider.

**Left panel (SourcePanel):**
- Panel header: "Original" + language badge + word count
- "code-switching" badge appears when detected
- Document rendered as a white page on gray background
- RTL rendering for Arabic, Hebrew, Persian, Urdu, Pashto
- Code-switch bands appear as amber horizontal rules at switch points
- Dialect card at bottom of page

**Right panel (TranslationPanel):**
- Panel header: "Translation" + "live" badge
- Same page layout as source
- Translated text appears in teal color (`#0F5038`)
- Highlighted terms mirrored from source
- Live cursor blinks at the end of in-progress segment
- Code-switch bands mirrored from source

**Center divider:**
- 1px line with a drag handle (3 dots)
- Draggable to resize panels
- Min width per panel: 280px

**RTL detection:**
```typescript
const RTL_CODES = ['ar', 'he', 'fa', 'ur', 'ps', 'prs', 'yi', 'sd']
const isRtl = (code: string) => RTL_CODES.includes(code)
```

**Code-switch band:**
```tsx
function CodeSwitchBand({ from, to, offset }: {
  from: string, to: string, offset: number
}) {
  return (
    <div className="switch-band">
      <div className="switch-dot" />
      language switch: {from} → {to} (offset {offset})
    </div>
  )
}
```

**Dialect card (bottom of source panel):**
```tsx
function DialectCard({ dialect }: { dialect: DialectInfo }) {
  return (
    <div className="dialect-card">
      <div className="dc-row">
        <span className="dc-label">Dialect</span>
        <span className="dc-val">{dialect.dialect}</span>
      </div>
      <div className="dc-row">
        <span className="dc-label">Confidence</span>
        <span className="dc-val">{dialect.confidence.toFixed(2)}</span>
      </div>
      <div className="dc-bar">
        <div className="dc-fill"
          style={{ width: `${dialect.confidence * 100}%` }} />
      </div>
      <div className="dc-src">
        {dialect.source === 'camel'
          ? 'CAMeL Tools · Carnegie Mellon'
          : 'Script analysis · lexical markers'}
      </div>
    </div>
  )
}
```

### Acceptance criteria — P3

- [ ] Split view renders correctly
- [ ] RTL direction applied for Arabic/Pashto/etc
- [ ] Code-switch bands appear in both panels at matching positions
- [ ] Dialect card shows in source panel when dialect detected
- [ ] Live cursor blinks at end of in-progress segment
- [ ] Translated text is teal colored
- [ ] Divider is draggable with min-width enforcement
- [ ] Panels scroll independently

---

## PRIORITY 4 — Audio/Video Workspace

When the loaded file is audio or video, the workspace switches
from document view to transcript view.

### WorkspaceAudio.tsx

```
Left panel:
  ┌─────────────────────────────────┐
  │  ORIGINAL  Arabic · transcript  │
  ├─────────────────────────────────┤
  │  [Waveform visualization]       │
  ├─────────────────────────────────┤
  │  00:00  مرحبا بالعالم           │
  │  00:03  كيف حالك               │
  │  00:06  [SPEAKER_01]            │
  │  00:08  بخير شكرا              │
  └─────────────────────────────────┘

Right panel:
  ┌─────────────────────────────────┐
  │  TRANSLATION  English · live    │
  ├─────────────────────────────────┤
  │  [Waveform mirror]              │
  ├─────────────────────────────────┤
  │  00:00  Hello world             │
  │  00:03  How are you             │
  │  00:06  [SPEAKER_01]            │
  │  00:08  Fine, thank you▌        │
  └─────────────────────────────────┘
```

**Waveform:**
Use the Web Audio API to render a simple waveform visualization.
It does not need to be interactive — just a visual representation
of the audio amplitude over time. Gray bars, teal overlay for
the played portion.

**Transcript rows:**
Each segment is a row with:
- Timestamp (MM:SS format)
- Speaker label (SPEAKER_00, SPEAKER_01) if diarization active
- Text (original left, translated right)
- Clicking a row highlights both panels at that timestamp

**Speaker labels:**
When diarization is active, speaker changes get a header row:
```
─── SPEAKER_00 ──────────────────────
00:00  مرحبا بالعالم
00:03  كيف حالك
─── SPEAKER_01 ──────────────────────
00:06  ...
```

### Acceptance criteria — P4

- [ ] Audio/video workspace renders when file type is audio/video
- [ ] Waveform visualization (simple amplitude bars)
- [ ] Timestamp rows in both panels
- [ ] Speaker labels when diarization active
- [ ] Clicking a row scrolls both panels to sync
- [ ] Live cursor on the in-progress segment
- [ ] Panels scroll in sync

---

## PRIORITY 5 — Toolbar + Menu Bar + Status Bar

### Toolbar.tsx

Exact layout from the approved mockup:

```
[🇸🇦 Arabic / ar · Egyptian dialect ▾] → [🇺🇸 English / en · target ▾]
 | Whisper Large-v3 ▾ | Auto ▾ | [SeamlessM4T badge] | [● Live]
```

Elements:
- Language group (unified border, source + arrow + target)
- Vertical divider
- STT model selector: `Whisper Tiny / Whisper Large-v3 / Pashto / Dari / Auto`
- Engine selector: `NLLB-600M / NLLB-1.3B / SeamlessM4T / Auto`
- Engine active badge (teal, shows which engine is actually running)
- Live button (teal, pulsing dot when active)

### MenuBar.tsx

```
File  |  View  |  Models  |  Help
```

**File menu items:**
- Open Evidence... (Cmd+O)
- Recent Files (submenu)
- Export Report → HTML / JSON / ZIP package
- Case Number... (dialog to set case number)
- Quit (Cmd+Q)

**View menu items:**
- Document View
- Transcript View
- Toggle Dialect Card
- Toggle Code-Switch Bands

**Models menu:**
Opens a slide-in panel showing:
- Installed models with green checkmarks
- Not-installed models with download buttons
- Profile status (Minimal / Standard / Full)
- "Open Model Manager" button

**Help menu items:**
- About AUGUR
- Documentation (opens USER_MANUAL.md)
- MT Advisory Notice (shows full advisory text)
- Wolfmark Systems Website

### StatusBar.tsx

```
● Translating · SeamlessM4T  |  Whisper Large-v3 · offline  |  
Egyptian Arabic 0.89 (CAMeL)  |  ⚠ Machine translation — verify with human linguist
```

The MT advisory is always the rightmost element. Always visible.
Cannot be hidden. Color: amber (`#BA7517`).

When idle:
```
Ready  |  Standard profile  |  200 languages available  |  
⚠ Machine translation — verify with human linguist
```

### Acceptance criteria — P5

- [ ] Toolbar matches mockup exactly
- [ ] STT model selector with all model options
- [ ] Engine selector with auto-selection display
- [ ] Live button with pulsing animation when active
- [ ] Menu bar with all four menus
- [ ] File → Open Evidence launches file picker
- [ ] File → Export Report works (HTML + JSON + ZIP)
- [ ] Models menu shows install status
- [ ] Status bar always shows MT advisory
- [ ] Status bar updates during translation

---

## PRIORITY 6 — File Loading + Pipeline Integration

### file_load.rs

```rust
#[tauri::command]
async fn open_evidence_dialog(app: tauri::AppHandle)
    -> Result<Option<String>, String>
{
    use tauri_plugin_dialog::DialogExt;
    let path = app.dialog()
        .file()
        .add_filter("Evidence files", &[
            "mp3", "mp4", "wav", "m4a", "aac", "ogg", "flac",
            "mov", "avi", "mkv", "webm",
            "pdf", "txt", "md", "doc", "docx",
            "png", "jpg", "jpeg", "tiff",
            "srt", "vtt",
        ])
        .blocking_pick_file();

    Ok(path.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
async fn detect_file_type(path: String) -> String {
    // Use augur-core's detect_input_kind_robust
    // Return: "document" | "audio" | "video" | "subtitle" | "image"
    let p = std::path::Path::new(&path);
    match p.extension().and_then(|e| e.to_str()) {
        Some("mp3"|"wav"|"m4a"|"aac"|"ogg"|"flac") => "audio",
        Some("mp4"|"mov"|"avi"|"mkv"|"webm") => "video",
        Some("srt"|"vtt") => "subtitle",
        Some("png"|"jpg"|"jpeg"|"tiff") => "image",
        _ => "document",
    }.to_string()
}
```

### pipeline.rs

```rust
#[tauri::command]
async fn start_translation(
    app: tauri::AppHandle,
    file_path: String,
    source_lang: String,
    target_lang: String,
    stt_model: String,
    engine: String,
) -> Result<(), String> {
    // Spawn async task
    // Call augur-core pipeline
    // Emit segment events as they complete:
    //   "segment-ready" { index, originalText, translatedText,
    //                     startMs, endMs, isComplete }
    //   "dialect-detected" { dialect, confidence, source }
    //   "code-switch-detected" { offset, from, to }
    //   "translation-complete" { totalSegments }
    
    tokio::spawn(async move {
        // run pipeline
        // emit events
    });
    Ok(())
}
```

**Event-driven UI update:**

Frontend listens to these events and updates state:
- `segment-ready` → appends to segments array, updates panels
- `dialect-detected` → sets dialect info, shows dialect card
- `code-switch-detected` → shows amber band at offset
- `translation-complete` → sets isTranslating = false

### File picker filter

Include all supported formats in one filter:
```
Audio: mp3, wav, m4a, aac, ogg, flac
Video: mp4, mov, avi, mkv, webm
Documents: pdf, txt, md, doc, docx
Images: png, jpg, jpeg, tiff
Subtitles: srt, vtt
```

### Acceptance criteria — P6

- [ ] File picker opens with correct filter
- [ ] File type detected correctly for all formats
- [ ] Pipeline starts on file load
- [ ] Segments appear live as they complete
- [ ] Dialect card appears when dialect detected
- [ ] Code-switch bands appear when switching detected
- [ ] Translation complete state handled correctly
- [ ] Error state shown clearly if pipeline fails

---

## PRIORITY 7 — Export System

### export.rs

```rust
#[tauri::command]
async fn export_report(
    app: tauri::AppHandle,
    format: String,  // "html" | "json" | "zip"
    output_path: String,
    case_number: String,
    segments: Vec<serde_json::Value>,
) -> Result<String, String> {
    match format.as_str() {
        "html" => export_html(&output_path, &case_number, &segments).await,
        "json" => export_json(&output_path, &case_number, &segments).await,
        "zip"  => export_zip(&app, &output_path, &case_number, &segments).await,
        other  => Err(format!("Unknown format: {}", other)),
    }
}
```

**HTML report must include:**
- AUGUR header with Wolfmark Systems branding
- Case number and date
- Source → target language pair
- Dialect info if detected
- Code-switching notation
- All segments with timestamps
- MT advisory at top AND bottom (cannot be removed)

**ZIP package must include:**
- `REPORT.html`
- `REPORT.json`
- `MANIFEST.json` (with file SHA-256, model used, timestamp)
- `CHAIN_OF_CUSTODY.txt`
- `translations/` (one .txt per segment)

**Save dialog:**
```rust
#[tauri::command]
async fn save_report_dialog(
    app: tauri::AppHandle,
    format: String,
    case_number: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let ext = match format.as_str() {
        "html" => "html",
        "json" => "json",
        "zip"  => "zip",
        _ => "html",
    };
    let default_name = format!(
        "AUGUR_Report_{}_{}",
        case_number.replace(' ', "_"),
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let path = app.dialog()
        .file()
        .set_file_name(&default_name)
        .add_filter(&format.to_uppercase(), &[ext])
        .blocking_save_file();
    Ok(path.map(|p| p.to_string_lossy().to_string()))
}
```

### Acceptance criteria — P7

- [ ] HTML export with all sections and MT advisory
- [ ] JSON export with all segments
- [ ] ZIP package with manifest and chain of custody
- [ ] Save dialog with auto-generated filename
- [ ] MT advisory in all export formats (mandatory)
- [ ] Export errors surface to UI clearly

---

## After all priorities complete

```bash
cd ~/Wolfmark/augur/apps/augur-desktop
cargo build 2>&1 | tail -10
cargo clippy -- -D warnings 2>&1 | grep "^error" | head -5
npm run build 2>&1 | tail -5
```

Commit:
```bash
git add apps/augur-desktop/
git commit -m "feat: augur-sprint-12 main desktop GUI (Tauri + React)"
```

Report:
- Which priorities passed
- Whether `cargo build` and `npm run build` succeed
- Description of each screen/view as rendered
- Any deviations from spec

---

## MT Advisory — non-negotiable

The machine translation advisory must appear in:
- Status bar (always, cannot be dismissed)
- Help → MT Advisory Notice menu item
- Every HTML export (top and bottom)
- Every ZIP package manifest
- Every JSON export (top-level field)

No UI element may hide or dismiss it.
No user setting may disable it.
This is a forensic tool used in legal proceedings.

---

_AUGUR Sprint 12 — Main Desktop GUI_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_Match the approved mockup exactly._
_The MT advisory is non-negotiable._
