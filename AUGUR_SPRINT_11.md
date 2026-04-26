# AUGUR Sprint 11 — Tauri Desktop GUI: Installer Wizard
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

This sprint builds the AUGUR installer wizard as a standalone
Tauri application. It is separate from the main AUGUR GUI (Sprint 12).
The installer is what an examiner runs once — it sets up all
dependencies and downloads all models, then hands off to the
main AUGUR application.

The examiner's experience:
1. Download AUGUR-Installer.dmg (small, ~30MB)
2. Open it, run the installer
3. Pick a profile (Minimal / Standard / Full)
4. Watch everything install with a live progress view
5. Click "Launch AUGUR" when done

After this sprint, an examiner with a brand new Mac can go from
zero to fully operational AUGUR in a single session with no
Terminal, no Homebrew, no manual steps.

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production — use `log::` macros
- All errors handled explicitly with user-facing messages
- `cargo clippy --workspace -- -D warnings` clean
- `cargo test --workspace` passes
- All UI strings sanitized before display
- No telemetry, no analytics, no network calls except model downloads

---

## Repository structure

Create a new Tauri app at `~/Wolfmark/augur/apps/augur-installer/`:

```
apps/augur-installer/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs
│       ├── installer.rs      ← core install logic
│       ├── download.rs       ← model download with progress
│       ├── bundled.rs        ← bundled deps (ffmpeg, tesseract)
│       ├── python.rs         ← embedded Python setup
│       └── profiles.rs       ← profile definitions
├── src/
│   ├── App.tsx
│   ├── main.tsx
│   ├── components/
│   │   ├── StepNav.tsx
│   │   ├── ProfileSelect.tsx
│   │   ├── InstallProgress.tsx
│   │   └── Complete.tsx
│   ├── types/
│   │   └── index.ts
│   └── ipc/
│       └── index.ts
├── package.json
├── tsconfig.json
└── vite.config.ts
```

---

## PRIORITY 1 — Tauri App Scaffold + Profile Definitions

### Step 1 — Create the Tauri app

```bash
cd ~/Wolfmark/augur/apps
npm create tauri-app@latest augur-installer -- \
    --template react-ts \
    --manager npm
cd augur-installer
```

### Step 2 — tauri.conf.json

```json
{
  "productName": "AUGUR Installer",
  "version": "1.0.0",
  "identifier": "systems.wolfmark.augur.installer",
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
    "resources": [
      "resources/ffmpeg",
      "resources/tesseract/"
    ],
    "macOS": {
      "minimumSystemVersion": "12.0"
    }
  },
  "app": {
    "windows": [
      {
        "title": "AUGUR Installer",
        "width": 680,
        "height": 580,
        "resizable": false,
        "center": true,
        "decorations": true
      }
    ]
  }
}
```

### Step 3 — Profile definitions in Rust

`src-tauri/src/profiles.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Minimal,
    Standard,
    Full,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComponentType {
    Runtime,     // Python embedded runtime
    BundledBin,  // ffmpeg, tesseract
    SttModel,    // Whisper variants
    TransModel,  // NLLB, SeamlessM4T
    Classifier,  // fastText, CAMeL
    Diarization, // pyannote
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
        download_url: Some("https://huggingface.co/openai/whisper-tiny/resolve/main/model.safetensors"),
        is_bundled: false,
    },
    InstallComponent {
        id: "whisper-large-v3",
        name: "Whisper Large-v3",
        description: "High-quality STT — accented and noisy audio",
        size_display: "2.9 GB",
        size_bytes: 2_900_000_000,
        component_type: ComponentType::SttModel,
        download_url: Some("https://huggingface.co/openai/whisper-large-v3/resolve/main/model.safetensors"),
        is_bundled: false,
    },
    InstallComponent {
        id: "whisper-pashto",
        name: "Whisper Pashto",
        description: "Fine-tuned for Pashto speech",
        size_display: "150 MB",
        size_bytes: 150_000_000,
        component_type: ComponentType::SttModel,
        download_url: Some("https://huggingface.co/openai/whisper-small/resolve/main/model.safetensors"),
        is_bundled: false,
    },
    InstallComponent {
        id: "whisper-dari",
        name: "Whisper Dari",
        description: "Fine-tuned for Dari / Afghan Persian",
        size_display: "150 MB",
        size_bytes: 150_000_000,
        component_type: ComponentType::SttModel,
        download_url: Some("https://huggingface.co/openai/whisper-small/resolve/main/model.safetensors"),
        is_bundled: false,
    },
    InstallComponent {
        id: "nllb-600m",
        name: "NLLB-200 600M",
        description: "Translation — 200 languages (fast)",
        size_display: "2.4 GB",
        size_bytes: 2_400_000_000,
        component_type: ComponentType::TransModel,
        download_url: Some("https://huggingface.co/facebook/nllb-200-distilled-600M/resolve/main/pytorch_model.bin"),
        is_bundled: false,
    },
    InstallComponent {
        id: "nllb-1b3",
        name: "NLLB-200 1.3B",
        description: "Translation — higher quality",
        size_display: "5.2 GB",
        size_bytes: 5_200_000_000,
        component_type: ComponentType::TransModel,
        download_url: Some("https://huggingface.co/facebook/nllb-200-1.3B/resolve/main/pytorch_model.bin"),
        is_bundled: false,
    },
    InstallComponent {
        id: "seamless-m4t",
        name: "SeamlessM4T Medium",
        description: "Unified model — handles code-switching",
        size_display: "2.4 GB",
        size_bytes: 2_400_000_000,
        component_type: ComponentType::TransModel,
        download_url: Some("https://huggingface.co/facebook/seamless-m4t-medium/resolve/main/pytorch_model.bin"),
        is_bundled: false,
    },
    InstallComponent {
        id: "camel-arabic",
        name: "CAMeL Arabic Models",
        description: "Arabic dialect identification — Carnegie Mellon",
        size_display: "450 MB",
        size_bytes: 450_000_000,
        component_type: ComponentType::Classifier,
        download_url: Some("https://huggingface.co/CAMeL-Lab/bert-base-arabic-camelbert-mix-did/resolve/main/pytorch_model.bin"),
        is_bundled: false,
    },
    InstallComponent {
        id: "pyannote",
        name: "Speaker Diarization",
        description: "pyannote — who spoke when",
        size_display: "1.0 GB",
        size_bytes: 1_000_000_000,
        component_type: ComponentType::Diarization,
        download_url: None, // HF token required — handled separately
        is_bundled: false,
    },
    InstallComponent {
        id: "fasttext-lid",
        name: "fastText Language ID",
        description: "Language identification — 176 languages",
        size_display: "900 KB",
        size_bytes: 900_000,
        component_type: ComponentType::Classifier,
        download_url: Some("https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz"),
        is_bundled: false,
    },
];

pub fn components_for_profile(profile: &Profile) -> Vec<&'static InstallComponent> {
    let ids: &[&str] = match profile {
        Profile::Minimal => &[
            "python-runtime", "ffmpeg", "tesseract",
            "whisper-tiny", "nllb-600m", "fasttext-lid",
        ],
        Profile::Standard => &[
            "python-runtime", "ffmpeg", "tesseract",
            "whisper-tiny", "whisper-large-v3",
            "nllb-600m", "nllb-1b3",
            "camel-arabic", "fasttext-lid",
        ],
        Profile::Full => &[
            "python-runtime", "ffmpeg", "tesseract",
            "whisper-tiny", "whisper-large-v3",
            "whisper-pashto", "whisper-dari",
            "nllb-600m", "nllb-1b3",
            "seamless-m4t", "camel-arabic",
            "pyannote", "fasttext-lid",
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
```

### Tests

```rust
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
        .iter().map(|c| c.id).collect();
    let standard: Vec<_> = components_for_profile(&Profile::Standard)
        .iter().map(|c| c.id).collect();
    for id in &minimal {
        assert!(standard.contains(id), "Standard missing: {}", id);
    }
}

#[test]
fn total_size_minimal_under_3gb() {
    assert!(total_size_for_profile(&Profile::Minimal) < 3_000_000_000);
}

#[test]
fn no_duplicate_component_ids() {
    let ids: Vec<_> = ALL_COMPONENTS.iter().map(|c| c.id).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len());
}
```

### Acceptance criteria — P1

- [ ] Tauri app scaffold created at `apps/augur-installer/`
- [ ] `tauri.conf.json` configured (680x580, not resizable)
- [ ] `InstallComponent` and `Profile` types defined
- [ ] All 13 components in `ALL_COMPONENTS`
- [ ] `components_for_profile()` returns correct sets
- [ ] 4 tests pass
- [ ] `cargo build` succeeds

---

## PRIORITY 2 — Download Engine with Real Progress

### Step 1 — Download manager

`src-tauri/src/download.rs`:

```rust
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use reqwest::Client;
use tokio::io::AsyncWriteExt;

#[derive(serde::Serialize, Clone)]
pub struct DownloadProgress {
    pub component_id: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub percent: f32,
    pub speed_mbps: f32,
    pub eta_seconds: u64,
}

pub async fn download_component(
    app: &AppHandle,
    component_id: &str,
    url: &str,
    dest_path: &Path,
    expected_bytes: u64,
) -> Result<(), String> {
    let client = Client::new();
    let response = client.get(url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), url));
    }

    let total = response.content_length()
        .unwrap_or(expected_bytes);

    tokio::fs::create_dir_all(dest_path.parent().unwrap_or(Path::new(".")))
        .await
        .map_err(|e| e.to_string())?;

    let mut file = tokio::fs::File::create(dest_path)
        .await
        .map_err(|e| e.to_string())?;

    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();
    let start = std::time::Instant::now();

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        let elapsed = start.elapsed().as_secs_f32();
        let speed_mbps = if elapsed > 0.0 {
            (downloaded as f32 / elapsed) / 1_000_000.0
        } else { 0.0 };

        let eta = if speed_mbps > 0.0 && downloaded < total {
            ((total - downloaded) as f32 / (speed_mbps * 1_000_000.0)) as u64
        } else { 0 };

        let progress = DownloadProgress {
            component_id: component_id.to_string(),
            bytes_downloaded: downloaded,
            total_bytes: total,
            percent: (downloaded as f32 / total as f32) * 100.0,
            speed_mbps,
            eta_seconds: eta,
        };

        app.emit("download-progress", progress).ok();
    }

    file.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}
```

### Step 2 — SHA-256 verification after download

```rust
pub async fn verify_sha256(
    path: &Path,
    expected: &str,
) -> Result<bool, String> {
    if expected.is_empty() {
        return Ok(true); // skip if no hash configured
    }
    use sha2::{Sha256, Digest};
    let mut file = tokio::fs::File::open(path)
        .await.map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        use tokio::io::AsyncReadExt;
        let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    let computed = format!("{:x}", hasher.finalize());
    Ok(computed == expected)
}
```

### Step 3 — Resume support

Check if file already exists and has the correct size before downloading:

```rust
pub async fn should_download(
    dest_path: &Path,
    expected_bytes: u64,
) -> bool {
    match tokio::fs::metadata(dest_path).await {
        Ok(meta) => meta.len() != expected_bytes,
        Err(_) => true,
    }
}
```

If the installer was interrupted mid-download and restarted,
already-complete components are skipped automatically.

### Acceptance criteria — P2

- [ ] Real HTTP download with progress events emitted to frontend
- [ ] `DownloadProgress` event carries percent, speed, ETA
- [ ] SHA-256 verification after each download
- [ ] Resume support — skip already-downloaded components
- [ ] Errors surface to UI with clear message, not crash

---

## PRIORITY 3 — Bundled Dependencies Setup

### Step 1 — Python embedded runtime

`src-tauri/src/python.rs`:

```rust
use std::path::{Path, PathBuf};
use tauri::AppHandle;

pub fn augur_support_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app data dir")
        .join("AUGUR")
}

pub fn models_dir(app: &AppHandle) -> PathBuf {
    augur_support_dir(app).join("models")
}

pub fn python_dir(app: &AppHandle) -> PathBuf {
    augur_support_dir(app).join("python")
}

/// Install pip packages into the bundled Python environment
pub async fn install_pip_packages(
    app: &AppHandle,
    on_progress: impl Fn(String),
) -> Result<(), String> {
    let python = python_dir(app).join("bin").join("python3");
    let packages = &[
        "transformers>=4.35.0",
        "ctranslate2>=3.20.0",
        "torch>=2.1.0",
        "torchaudio>=2.1.0",
        "sentencepiece>=0.1.99",
    ];

    for package in packages {
        on_progress(format!("Installing {}...", package));
        let status = tokio::process::Command::new(&python)
            .args(["-m", "pip", "install", "--quiet", package])
            .status()
            .await
            .map_err(|e| format!("pip failed: {}", e))?;

        if !status.success() {
            return Err(format!("Failed to install {}", package));
        }
    }
    Ok(())
}

pub fn is_python_ready(app: &AppHandle) -> bool {
    python_dir(app).join("bin").join("python3").exists()
}
```

### Step 2 — bundled ffmpeg and Tesseract

```rust
// src-tauri/src/bundled.rs
use std::path::PathBuf;
use tauri::AppHandle;

pub fn ffmpeg_path(app: &AppHandle) -> PathBuf {
    app.path()
        .resource_dir()
        .expect("resource dir")
        .join("resources")
        .join("ffmpeg")
}

pub fn tesseract_path(app: &AppHandle) -> PathBuf {
    app.path()
        .resource_dir()
        .expect("resource dir")
        .join("resources")
        .join("tesseract")
        .join("tesseract")
}

pub fn setup_bundled_bins(app: &AppHandle) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    for path in [ffmpeg_path(app), tesseract_path(app)] {
        if path.exists() {
            std::fs::set_permissions(
                &path,
                std::fs::Permissions::from_mode(0o755),
            ).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
```

### Step 3 — Model cache directory

```rust
pub fn model_path_for(app: &AppHandle, component_id: &str) -> PathBuf {
    models_dir(app).join(component_id)
}
```

All models go to `~/Library/Application Support/AUGUR/models/`.
This is the same path `augur` (the CLI) looks for models, so
if someone has already used the CLI, the installer won't
re-download anything.

### Acceptance criteria — P3

- [ ] `augur_support_dir()` returns correct macOS path
- [ ] `models_dir()` returns correct models path
- [ ] `ffmpeg_path()` and `tesseract_path()` resolve bundled binaries
- [ ] `setup_bundled_bins()` sets executable permissions
- [ ] `install_pip_packages()` installs into bundled Python
- [ ] Existing models detected and skipped

---

## PRIORITY 4 — Tauri Commands + IPC

### Step 1 — Core commands

`src-tauri/src/main.rs`:

```rust
#[tauri::command]
async fn get_profile_components(
    profile: profiles::Profile,
) -> Result<Vec<serde_json::Value>, String> {
    let comps = profiles::components_for_profile(&profile);
    Ok(comps.iter().map(|c| serde_json::json!({
        "id": c.id,
        "name": c.name,
        "description": c.description,
        "sizeDisplay": c.size_display,
        "sizeBytes": c.size_bytes,
        "isBundled": c.is_bundled,
        "componentType": format!("{:?}", c.component_type),
    })).collect())
}

#[tauri::command]
async fn get_total_size(
    profile: profiles::Profile,
) -> u64 {
    profiles::total_size_for_profile(&profile)
}

#[tauri::command]
async fn start_installation(
    app: tauri::AppHandle,
    profile: profiles::Profile,
) -> Result<(), String> {
    let components = profiles::components_for_profile(&profile);
    let total = components.len();

    for (idx, component) in components.iter().enumerate() {
        app.emit("install-component-start", serde_json::json!({
            "id": component.id,
            "index": idx,
            "total": total,
        })).ok();

        if component.is_bundled {
            // bundled bins — just set permissions
            installer::setup_bundled_component(&app, component.id).await?;
        } else if let Some(url) = component.download_url {
            let dest = installer::model_path_for(&app, component.id);
            if download::should_download(&dest, component.size_bytes).await {
                download::download_component(
                    &app, component.id, url, &dest,
                    component.size_bytes,
                ).await?;
            }
        }

        app.emit("install-component-done", serde_json::json!({
            "id": component.id,
            "index": idx,
        })).ok();
    }

    // Write installation manifest
    installer::write_manifest(&app, &profile).await?;
    app.emit("install-complete", serde_json::json!({})).ok();
    Ok(())
}

#[tauri::command]
async fn check_existing_installation(
    app: tauri::AppHandle,
) -> serde_json::Value {
    installer::check_existing(&app).await
}

#[tauri::command]
async fn launch_augur(app: tauri::AppHandle) -> Result<(), String> {
    // Launch main AUGUR.app
    let augur_path = "/Applications/AUGUR.app";
    tokio::process::Command::new("open")
        .arg(augur_path)
        .spawn()
        .map_err(|e| format!("Could not launch AUGUR: {}", e))?;
    app.exit(0);
    Ok(())
}
```

### Acceptance criteria — P4

- [ ] `get_profile_components` returns component list for profile
- [ ] `start_installation` runs components in order
- [ ] `install-component-start` event emitted per component
- [ ] `download-progress` event with percent/speed/ETA
- [ ] `install-component-done` event on completion
- [ ] `install-complete` event when all done
- [ ] `launch_augur` opens main app and exits installer

---

## PRIORITY 5 — React Frontend

### Design reference

The frontend must match the mockup exactly:

```
┌─────────────────────────────────────────────┐
│  [A]  AUGUR Setup Wizard          v1.0.0    │
├─────────────────────────────────────────────┤
│  ✓ Welcome  ›  ● Profile  ›  3  ›  4       │
├─────────────────────────────────────────────┤
│                                             │
│  [screen content]                           │
│                                             │
├─────────────────────────────────────────────┤
│  [progress bar + label]    [Back] [Next →]  │
└─────────────────────────────────────────────┘
```

### Step 1 — App.tsx (step router)

```tsx
import { useState } from 'react'
import StepNav from './components/StepNav'
import ProfileSelect from './components/ProfileSelect'
import InstallProgress from './components/InstallProgress'
import Complete from './components/Complete'

type Step = 1 | 2 | 3 | 4
type Profile = 'minimal' | 'standard' | 'full'

export default function App() {
  const [step, setStep] = useState<Step>(2)
  const [profile, setProfile] = useState<Profile>('standard')
  const [installResult, setInstallResult] = useState<any>(null)

  return (
    <div className="wizard">
      <Header />
      <StepNav currentStep={step} />
      <div className="wiz-body">
        {step === 2 && (
          <ProfileSelect
            selected={profile}
            onSelect={setProfile}
          />
        )}
        {step === 3 && (
          <InstallProgress
            profile={profile}
            onComplete={(result) => {
              setInstallResult(result)
              setStep(4)
            }}
          />
        )}
        {step === 4 && (
          <Complete result={installResult} />
        )}
      </div>
      <Footer
        step={step}
        onBack={() => setStep(s => Math.max(2, s - 1) as Step)}
        onNext={() => setStep(s => Math.min(4, s + 1) as Step)}
        profile={profile}
      />
    </div>
  )
}
```

### Step 2 — ProfileSelect.tsx

Three cards: Minimal / Standard (pre-selected) / Full.
Each card shows: name, size, description, feature list.
Standard card has "Recommended" badge.
On select, bottom preview panel updates with component list.

```tsx
const PROFILES = {
  minimal: {
    name: 'Minimal',
    size: '2.5 GB',
    description: 'Basic documents and clear audio.',
    components: ['Whisper Tiny', 'NLLB-200 600M', 'fastText LID'],
  },
  standard: {
    name: 'Standard',
    size: '11 GB',
    description: 'Recommended for most LE/IC casework.',
    recommended: true,
    components: ['Whisper Large-v3', 'NLLB-200 1.3B',
                 'CAMeL Arabic', 'fastText LID'],
  },
  full: {
    name: 'Full',
    size: '15 GB',
    description: 'All models including Pashto, Dari, SeamlessM4T.',
    components: ['All Standard models', 'Whisper Pashto + Dari',
                 'SeamlessM4T', 'Speaker Diarization'],
  },
}
```

### Step 3 — InstallProgress.tsx

Listen to Tauri events and update state:

```tsx
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'

type ComponentStatus = 'waiting' | 'active' | 'done' | 'error'

interface ComponentState {
  id: string
  name: string
  description: string
  sizeDisplay: string
  status: ComponentStatus
  percent: number
  speedMbps: number
  etaSeconds: number
}

export default function InstallProgress({ profile, onComplete }) {
  const [components, setComponents] = useState<ComponentState[]>([])
  const [overallPct, setOverallPct] = useState(0)
  const [currentLabel, setCurrentLabel] = useState('Starting...')

  useEffect(() => {
    // Load components then start install
    invoke('get_profile_components', { profile })
      .then((comps: any[]) => {
        setComponents(comps.map(c => ({
          ...c, status: 'waiting', percent: 0,
          speedMbps: 0, etaSeconds: 0,
        })))
        return invoke('start_installation', { profile })
      })
      .catch(err => console.error(err))

    const unlisten: Array<() => void> = []

    listen('install-component-start', (e: any) => {
      const { id, index, total } = e.payload
      setComponents(prev => prev.map(c =>
        c.id === id ? { ...c, status: 'active' } : c
      ))
      setCurrentLabel(`Installing: ${id}`)
    }).then(u => unlisten.push(u))

    listen('download-progress', (e: any) => {
      const { component_id, percent, speed_mbps, eta_seconds } = e.payload
      setComponents(prev => prev.map(c =>
        c.id === component_id
          ? { ...c, percent, speedMbps: speed_mbps, etaSeconds: eta_seconds }
          : c
      ))
    }).then(u => unlisten.push(u))

    listen('install-component-done', (e: any) => {
      const { id, index } = e.payload
      setComponents(prev => prev.map(c =>
        c.id === id ? { ...c, status: 'done', percent: 100 } : c
      ))
      setOverallPct(Math.round(((index + 1) / components.length) * 100))
    }).then(u => unlisten.push(u))

    listen('install-complete', () => {
      setOverallPct(100)
      setCurrentLabel('Installation complete')
      setTimeout(() => onComplete({}), 800)
    }).then(u => unlisten.push(u))

    return () => unlisten.forEach(u => u())
  }, [profile])

  return (
    <div className="install-list">
      {components.map(c => (
        <ComponentRow key={c.id} component={c} />
      ))}
    </div>
  )
}
```

### Step 4 — ComponentRow

Each row shows:
- Icon (based on component type)
- Name + description
- Size (right-aligned)
- Status icon: gray dot (waiting) / spinner (active) / green checkmark (done)
- Per-item progress bar (visible only when active)
- Speed and ETA when downloading

```tsx
function ComponentRow({ component }: { component: ComponentState }) {
  const formatEta = (s: number) => {
    if (s <= 0) return ''
    if (s < 60) return `${s}s remaining`
    return `${Math.round(s / 60)}m remaining`
  }

  return (
    <div className={`install-item ${component.status}`}>
      <div className="ii-icon">{iconFor(component.id)}</div>
      <div className="ii-info">
        <div className="ii-name">{component.name}</div>
        <div className="ii-sub">
          {component.status === 'active' && component.speedMbps > 0
            ? `${component.speedMbps.toFixed(1)} MB/s · ${formatEta(component.etaSeconds)}`
            : component.description}
        </div>
        {component.status === 'active' && (
          <div className="item-progress">
            <div
              className="item-progress-fill"
              style={{ width: `${component.percent}%` }}
            />
          </div>
        )}
      </div>
      <div className="ii-size">{component.sizeDisplay}</div>
      <div className="ii-status">
        {component.status === 'waiting' && <div className="waiting-dot" />}
        {component.status === 'active' && <div className="spinner" />}
        {component.status === 'done' && <div className="check">✓</div>}
        {component.status === 'error' && <div className="error-dot" />}
      </div>
    </div>
  )
}
```

### Step 5 — Complete.tsx

Three stat cards: Models installed / Languages supported / Total size.
MT advisory printed at the bottom.
"Launch AUGUR" button calls `launch_augur` Tauri command.

### Step 6 — CSS

Match the mockup exactly:
- App background: `var(--color-background-primary)`
- Wizard border: `0.5px solid var(--color-border-primary)`
- Teal accent: `#085041` (primary), `#1D9E75` (progress fill), `#E1F5EE` (backgrounds)
- Done items: teal background, green checkmark
- Active items: teal border
- Waiting items: 50% opacity
- Progress bar: 5px height, teal fill, rounded
- Spinner: 18px, teal border-top

### Acceptance criteria — P5

- [ ] Four-step wizard renders correctly
- [ ] Profile selection with all three cards
- [ ] Standard pre-selected with recommended badge
- [ ] Component preview updates on profile change
- [ ] Install progress shows all components
- [ ] Each component transitions: waiting → active → done
- [ ] Per-item progress bar visible during download
- [ ] Speed and ETA displayed during active download
- [ ] Overall footer progress bar tracks completion
- [ ] Complete screen with stat cards
- [ ] MT advisory on complete screen
- [ ] "Launch AUGUR" button works
- [ ] Dark mode compatible (CSS variables throughout)

---

## After all priorities complete

```bash
cd ~/Wolfmark/augur/apps/augur-installer
cargo build 2>&1 | tail -10
cargo clippy -- -D warnings 2>&1 | grep "^error" | head -5
npm run build 2>&1 | tail -5
```

Commit:
```bash
git add apps/augur-installer/
git commit -m "feat: augur-sprint-11 installer wizard (Tauri + React)"
```

Report:
- Which priorities passed
- Whether `cargo build` succeeds
- Any deviations from spec
- Screenshots description of each screen

---

_AUGUR Sprint 11 — Installer Wizard_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_After this sprint, AUGUR is a one-click install._
