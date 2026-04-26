# AUGUR Sprint 14+15 — True Streaming Pipeline + Dialect-Aware Translation Routing
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

Sprint 13 wired the real augur CLI subprocess into the desktop GUI.
One known deviation: segments arrive as a batch after full translation
completes rather than streaming live during inference. This sprint
fixes that and adds dialect-aware translation routing so detected
Arabic dialects route to the best available model.

After this sprint:
- Long audio files show live segment-by-segment translation as Whisper
  processes each chunk — the cursor actually moves in real time
- Egyptian Arabic routes differently than Gulf Arabic
- Moroccan Darija prefers SeamlessM4T when available
- Every dialect decision is explained to the examiner

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production — NDJSON output via audited path only
- All errors surface to UI
- MT advisory always present on all output
- Dialect advisory always accompanies dialect routing decisions
- `cargo clippy --workspace -- -D warnings` clean
- Offline invariant maintained

---

## PRIORITY 1 — True Streaming Segments During Inference

### Context

The current pipeline in `crates/augur-core/src/pipeline.rs` calls
`translate_segments()` which collects all segments into a Vec
before returning. This means for a 30-minute audio file, nothing
appears in the UI for 10+ minutes, then everything arrives at once.

Real forensic use requires live feedback — especially for audio
interviews where the examiner needs to know immediately if the
content is relevant.

### Investigation first

```bash
# Find the current translate_segments implementation
grep -rn "translate_segments\|TranslationEngine\|translate_batch" \
    crates/augur-core/src/ --include="*.rs" | head -20

# Find where STT segments are produced
grep -rn "SttSegment\|transcribe\|segments" \
    crates/augur-stt/src/ --include="*.rs" | head -20

# Find the pipeline entry point
grep -rn "fn run\|fn translate\|fn process" \
    crates/augur-core/src/pipeline.rs | head -20
```

### Step 1 — Add a segment callback to the pipeline

Modify `Pipeline::run()` to accept an optional progress callback:

```rust
pub type SegmentCallback = Box<dyn Fn(PipelineSegmentEvent) + Send + Sync>;

#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineSegmentEvent {
    pub event_type: SegmentEventType,
    pub segment: Option<TranslationSegment>,
    pub dialect: Option<DialectInfo>,
    pub code_switch: Option<CodeSwitchEvent>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum SegmentEventType {
    SegmentReady,
    DialectDetected,
    CodeSwitchDetected,
    Complete,
    Error,
}

impl Pipeline {
    pub fn run_with_callback(
        &self,
        input: PipelineInput,
        target_language: &str,
        options: &PipelineOptions,
        on_event: impl Fn(PipelineSegmentEvent) + Send + Sync,
    ) -> Result<PipelineResult, AugurError>
```

**Key change:** After each segment is translated, call `on_event`
immediately rather than collecting into a Vec. The Vec is still
built for the final `PipelineResult` but events fire per-segment.

```rust
// In the STT + translate loop:
for (idx, stt_segment) in stt_segments.iter().enumerate() {
    // Translate this one segment
    let translated = self.translation_engine
        .translate_single(&stt_segment.text, source_lang, target_language)?;

    let segment = TranslationSegment {
        index: idx,
        start_ms: stt_segment.start_ms,
        end_ms: stt_segment.end_ms,
        original_text: stt_segment.text.clone(),
        translated_text: translated.clone(),
        is_complete: true,
    };

    // Fire callback IMMEDIATELY — don't wait for all segments
    on_event(PipelineSegmentEvent {
        event_type: SegmentEventType::SegmentReady,
        segment: Some(segment.clone()),
        dialect: None,
        code_switch: None,
    });

    segments.push(segment);
}
```

### Step 2 — Add `translate_single` to TranslationEngine

Currently `TranslationEngine` may only expose batch translation.
Add a single-segment method:

```rust
impl TranslationEngine {
    /// Translate a single text segment.
    /// More expensive per-call than batch but enables streaming output.
    pub fn translate_single(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
    ) -> Result<String, AugurError> {
        // For NLLB: call the Python worker with a single-item batch
        // For SeamlessM4T: same pattern
        // Reuses existing subprocess infrastructure
        self.translate_batch(
            &[text.to_string()],
            source_lang,
            target_lang,
        ).map(|results| {
            results.into_iter().next()
                .unwrap_or_default()
        })
        // Note: the unwrap_or_default() here is safe —
        // translate_batch of 1 item always returns 1 item on success
    }
}
```

### Step 3 — NDJSON CLI streaming

The `--format ndjson` path in `cmd_translate` currently collects
all segments then emits. Change it to emit per-segment:

```rust
// In apps/augur-cli/src/translate.rs cmd_translate_ndjson:

pipeline.run_with_callback(
    input,
    &target_lang,
    &options,
    |event| {
        match event.event_type {
            SegmentEventType::SegmentReady => {
                if let Some(seg) = event.segment {
                    let json = serde_json::json!({
                        "type": "segment",
                        "index": seg.index,
                        "start_ms": seg.start_ms,
                        "end_ms": seg.end_ms,
                        "original": seg.original_text,
                        "translated": seg.translated_text,
                        "is_complete": true,
                    });
                    println!("{}", json);
                    // Flush immediately so the GUI receives it
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            }
            SegmentEventType::DialectDetected => {
                if let Some(dialect) = event.dialect {
                    let json = serde_json::json!({
                        "type": "dialect",
                        "dialect": dialect.dialect,
                        "confidence": dialect.confidence,
                        "source": dialect.source,
                    });
                    println!("{}", json);
                    std::io::stdout().flush().ok();
                }
            }
            _ => {}
        }
    }
)?;
```

**Critical:** `std::io::stdout().flush()` after each `println!` is
what makes the streaming work. Without explicit flush, the OS
buffers output and the GUI sees nothing until the process exits.

### Step 4 — Backward compatibility

The existing `Pipeline::run()` without callback still works —
wrap it to use the callback version internally:

```rust
pub fn run(
    &self,
    input: PipelineInput,
    target_language: &str,
    options: &PipelineOptions,
) -> Result<PipelineResult, AugurError> {
    self.run_with_callback(
        input,
        target_language,
        options,
        |_event| {}, // no-op callback — collect only, no streaming
    )
}
```

All existing tests continue to work unchanged.

### Step 5 — Verify end-to-end streaming

After implementation, test:

```bash
# This should print segments one by one as they're translated
# not all at once at the end
augur translate --text "مرحبا. كيف حالك. بخير شكرا." \
    --target en \
    --format ndjson
```

Each sentence should print as a separate segment with a short
gap between them (the time to translate each one), NOT all three
arriving simultaneously.

### Tests

```rust
#[test]
fn pipeline_callback_fires_per_segment() {
    let mut segment_count = 0;
    let callback_count = Arc::new(Mutex::new(0));
    let cc = Arc::clone(&callback_count);

    pipeline.run_with_callback(
        PipelineInput::Text("Hello. How are you. Fine thanks.".into()),
        "en",
        &PipelineOptions::default(),
        move |event| {
            if matches!(event.event_type, SegmentEventType::SegmentReady) {
                *cc.lock().unwrap() += 1;
            }
        },
    ).unwrap();

    // Should have fired once per sentence
    assert!(*callback_count.lock().unwrap() > 0);
}

#[test]
fn translate_single_returns_non_empty_string() {
    // Unit test with a mock translation engine
    // Verify translate_single returns a non-empty translated string
}

#[test]
fn ndjson_segment_flushed_immediately() {
    // This is tested implicitly by the end-to-end streaming test
    // Document it here as a known behavior invariant
}
```

### Acceptance criteria — P1

- [ ] `Pipeline::run_with_callback` added
- [ ] `translate_single` added to TranslationEngine
- [ ] Callback fires per segment during inference
- [ ] stdout flushed after each segment in NDJSON mode
- [ ] `Pipeline::run()` still works (backward compatible)
- [ ] All existing 189 tests still pass
- [ ] 2 new tests pass
- [ ] Clippy clean

---

## PRIORITY 2 — Dialect-Aware Translation Routing

### Context

AUGUR detects Arabic dialects (Egyptian, Gulf, Levantine, Moroccan,
Iraqi) but sends all Arabic through the same NLLB-200 path.

The problem: NLLB-200 was trained primarily on Modern Standard
Arabic (MSA). It handles Egyptian and Levantine reasonably well
because they're closer to MSA, but Moroccan Darija is heavily
French-influenced and NLLB struggles with it. SeamlessM4T handles
Darija significantly better.

This sprint adds routing logic that selects the best available
model based on detected dialect.

### Step 1 — Dialect routing table

Create `crates/augur-core/src/dialect_routing.rs`:

```rust
use crate::classifier::arabic_dialect::{ArabicDialect, DialectAnalysis};

#[derive(Debug, Clone, PartialEq)]
pub enum TranslationRoute {
    /// Use NLLB-200 with standard Arabic source token
    NllbMsa,
    /// Use NLLB-200 with Egyptian Arabic source token (arz_Arab)
    NllbEgyptian,
    /// Use SeamlessM4T — better on Maghrebi dialects
    SeamlessM4T,
    /// Use NLLB-200 with Levantine source token (apc_Arab)  
    NllbLevantine,
    /// Default — use NLLB with best available Arabic token
    NllbDefault,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RoutingDecision {
    pub route: TranslationRoute,
    pub reason: String,
    pub model_used: String,
    pub dialect_advisory: String,  // always non-empty
    pub confidence: f32,
}

pub fn route_arabic_translation(
    dialect: &DialectAnalysis,
    installed_models: &InstalledModels,
) -> RoutingDecision {
    match &dialect.detected_dialect {
        ArabicDialect::Egyptian if dialect.confidence >= 0.70 => {
            RoutingDecision {
                route: TranslationRoute::NllbEgyptian,
                reason: "Egyptian (Masri) detected with high confidence. \
                         Routing to Egyptian Arabic NLLB token (arz_Arab).".into(),
                model_used: "NLLB-200 (Egyptian Arabic token)".into(),
                dialect_advisory: dialect_advisory_text(&dialect.detected_dialect),
                confidence: dialect.confidence,
            }
        }

        ArabicDialect::Moroccan if dialect.confidence >= 0.65 => {
            if installed_models.seamless_m4t {
                RoutingDecision {
                    route: TranslationRoute::SeamlessM4T,
                    reason: "Moroccan Darija detected. SeamlessM4T selected — \
                             significantly better than NLLB-200 on Darija due to \
                             French-Arabic code-switching in Maghrebi dialects.".into(),
                    model_used: "SeamlessM4T Medium".into(),
                    dialect_advisory: dialect_advisory_text(&dialect.detected_dialect),
                    confidence: dialect.confidence,
                }
            } else {
                RoutingDecision {
                    route: TranslationRoute::NllbDefault,
                    reason: "Moroccan Darija detected but SeamlessM4T not installed. \
                             Falling back to NLLB-200. Quality may be reduced. \
                             Install SeamlessM4T for better Darija translation: \
                             augur install --model seamless-m4t-medium".into(),
                    model_used: "NLLB-200 (fallback — SeamlessM4T preferred)".into(),
                    dialect_advisory: dialect_advisory_text(&dialect.detected_dialect),
                    confidence: dialect.confidence,
                }
            }
        }

        ArabicDialect::Levantine if dialect.confidence >= 0.65 => {
            RoutingDecision {
                route: TranslationRoute::NllbLevantine,
                reason: "Levantine Arabic detected (Syrian/Lebanese/Palestinian/Jordanian). \
                         Routing to Levantine NLLB token (apc_Arab).".into(),
                model_used: "NLLB-200 (Levantine Arabic token)".into(),
                dialect_advisory: dialect_advisory_text(&dialect.detected_dialect),
                confidence: dialect.confidence,
            }
        }

        ArabicDialect::Gulf if dialect.confidence >= 0.65 => {
            RoutingDecision {
                route: TranslationRoute::NllbDefault,
                reason: "Gulf Arabic detected (Saudi/Emirati/Kuwaiti/Qatari). \
                         Using standard Arabic NLLB token — Gulf Arabic \
                         is relatively close to MSA for NLLB-200.".into(),
                model_used: "NLLB-200 (standard Arabic token)".into(),
                dialect_advisory: dialect_advisory_text(&dialect.detected_dialect),
                confidence: dialect.confidence,
            }
        }

        // Low confidence or Unknown/MSA — use standard path
        _ => {
            RoutingDecision {
                route: TranslationRoute::NllbDefault,
                reason: format!(
                    "Arabic dialect: {} (confidence: {:.2}). \
                     Using standard Modern Standard Arabic translation path.",
                    format!("{:?}", dialect.detected_dialect),
                    dialect.confidence
                ),
                model_used: "NLLB-200 (standard Arabic token)".into(),
                dialect_advisory: dialect_advisory_text(&dialect.detected_dialect),
                confidence: dialect.confidence,
            }
        }
    }
}

/// Always non-empty. Dialect-specific advisory text.
fn dialect_advisory_text(dialect: &ArabicDialect) -> String {
    let dialect_name = match dialect {
        ArabicDialect::Egyptian => "Egyptian (Masri) Arabic",
        ArabicDialect::Gulf => "Gulf Arabic",
        ArabicDialect::Levantine => "Levantine Arabic",
        ArabicDialect::Moroccan => "Moroccan Darija",
        ArabicDialect::Iraqi => "Iraqi Arabic",
        ArabicDialect::ModernStandard => "Modern Standard Arabic (MSA)",
        ArabicDialect::Unknown => "Arabic (dialect unresolved)",
        _ => "Arabic",
    };

    format!(
        "Detected dialect: {}. Machine translation quality varies by dialect. \
         NLLB-200 was trained primarily on Modern Standard Arabic — \
         dialectal content may have reduced translation accuracy. \
         Verify all translations with a certified Arabic linguist \
         before use in legal proceedings.",
        dialect_name
    )
}
```

### Step 2 — NLLB language tokens for Arabic dialects

NLLB-200 supports specific Arabic dialect tokens:
```
ara_Arab  — Modern Standard Arabic (default)
arz_Arab  — Egyptian Arabic
apc_Arab  — North Levantine Arabic
acm_Arab  — Mesopotamian Arabic (Iraqi)
ary_Arab  — Moroccan Arabic (limited quality)
```

Expose these in the translation engine:

```rust
pub fn arabic_nllb_token(dialect: &ArabicDialect) -> &'static str {
    match dialect {
        ArabicDialect::Egyptian  => "arz_Arab",
        ArabicDialect::Levantine => "apc_Arab",
        ArabicDialect::Iraqi     => "acm_Arab",
        ArabicDialect::Moroccan  => "ary_Arab",
        _                        => "ara_Arab",  // MSA default
    }
}
```

Use the correct token when calling the NLLB Python worker:
```python
# In scripts/worker_script.py or script_ct2.py:
# The source_lang now comes from the routing decision
# e.g., "arz_Arab" instead of "ara_Arab" for Egyptian
tokens = [f"__{source_nllb_token}__"] + sp.encode(text, out_type=str)
```

### Step 3 — Wire routing into the pipeline

In `crates/augur-core/src/pipeline.rs`, after dialect detection:

```rust
// After classify_arabic_dialect() runs:
let routing = if source_lang == "ar" {
    let decision = route_arabic_translation(&dialect_analysis, &self.installed_models);

    // Fire the routing decision as a callback event so the GUI shows it
    on_event(PipelineSegmentEvent {
        event_type: SegmentEventType::DialectDetected,
        dialect: Some(DialectInfo {
            dialect: format!("{:?}", dialect_analysis.detected_dialect),
            confidence: dialect_analysis.confidence,
            source: dialect_analysis.source.clone(),
            routing_decision: Some(decision.clone()),
        }),
        ..Default::default()
    });

    Some(decision)
} else {
    None
};

// Use routing.route to select the translation path:
let translated = match routing.as_ref().map(|r| &r.route) {
    Some(TranslationRoute::SeamlessM4T) => {
        self.seamless_engine.translate(&text, "ar", target_lang)?
    }
    Some(TranslationRoute::NllbEgyptian) => {
        self.nllb_engine.translate_with_token(&text, "arz_Arab", target_lang)?
    }
    Some(TranslationRoute::NllbLevantine) => {
        self.nllb_engine.translate_with_token(&text, "apc_Arab", target_lang)?
    }
    _ => {
        self.nllb_engine.translate(&text, "ar", target_lang)?
    }
};
```

### Step 4 — Routing decision in NDJSON output

When routing occurs, emit a new event type in the NDJSON stream:

```json
{"type":"dialect_routing",
 "dialect":"Egyptian",
 "confidence":0.89,
 "route":"nllb_egyptian",
 "model":"NLLB-200 (Egyptian Arabic token: arz_Arab)",
 "reason":"Egyptian (Masri) detected with high confidence...",
 "dialect_advisory":"Detected dialect: Egyptian (Masri) Arabic..."}
```

This event appears in the NDJSON stream before the first segment.
The GUI receives it and:
- Updates the dialect card with routing info
- Shows which model was selected and why
- Displays the dialect advisory

### Step 5 — CLI display for routing decisions

When not in NDJSON mode, print the routing decision clearly:

```
[AUGUR] Arabic dialect: Egyptian (Masri) — confidence: 0.89 (CAMeL Tools)
[AUGUR] Routing: NLLB-200 with Egyptian Arabic token (arz_Arab)
[AUGUR] Reason: Egyptian detected with high confidence. Better accuracy
        than standard MSA token for Masri dialect content.
⚠ Detected dialect: Egyptian (Masri) Arabic. Machine translation quality
  varies by dialect. Verify with a certified Arabic linguist.
```

### Step 6 — GUI dialect card update

Update `DialectCard.tsx` to show the routing decision:

```tsx
interface DialectCardProps {
  dialect: string
  confidence: number
  source: 'camel' | 'lexical'
  routingDecision?: {
    route: string
    model: string
    reason: string
    dialectAdvisory: string
  }
}
```

When `routingDecision` is present, expand the card to show:
```
Dialect:    Egyptian (Masri)
Confidence: 0.89  ████████░░
Source:     CAMeL Tools · Carnegie Mellon
Model:      NLLB-200 (arz_Arab)
Reason:     Egyptian detected with high confidence...
⚠ Verify with a certified Arabic linguist
```

### Tests

```rust
#[test]
fn egyptian_high_confidence_routes_to_nllb_egyptian() {
    let dialect = DialectAnalysis {
        detected_dialect: ArabicDialect::Egyptian,
        confidence: 0.89,
        ..Default::default()
    };
    let installed = InstalledModels { seamless_m4t: false, ..Default::default() };
    let decision = route_arabic_translation(&dialect, &installed);
    assert!(matches!(decision.route, TranslationRoute::NllbEgyptian));
    assert_eq!(decision.model_used.contains("arz_Arab"), true);
}

#[test]
fn moroccan_routes_to_seamless_when_installed() {
    let dialect = DialectAnalysis {
        detected_dialect: ArabicDialect::Moroccan,
        confidence: 0.75,
        ..Default::default()
    };
    let installed = InstalledModels { seamless_m4t: true, ..Default::default() };
    let decision = route_arabic_translation(&dialect, &installed);
    assert!(matches!(decision.route, TranslationRoute::SeamlessM4T));
}

#[test]
fn moroccan_falls_back_to_nllb_when_seamless_missing() {
    let dialect = DialectAnalysis {
        detected_dialect: ArabicDialect::Moroccan,
        confidence: 0.75,
        ..Default::default()
    };
    let installed = InstalledModels { seamless_m4t: false, ..Default::default() };
    let decision = route_arabic_translation(&dialect, &installed);
    assert!(matches!(decision.route, TranslationRoute::NllbDefault));
    // Reason should mention SeamlessM4T and how to install it
    assert!(decision.reason.contains("SeamlessM4T"));
    assert!(decision.reason.contains("augur install"));
}

#[test]
fn dialect_advisory_always_non_empty() {
    for dialect in [
        ArabicDialect::Egyptian, ArabicDialect::Gulf,
        ArabicDialect::Levantine, ArabicDialect::Moroccan,
        ArabicDialect::Iraqi, ArabicDialect::ModernStandard,
        ArabicDialect::Unknown,
    ] {
        let advisory = dialect_advisory_text(&dialect);
        assert!(!advisory.is_empty(),
            "Advisory empty for {:?}", dialect);
    }
}

#[test]
fn arabic_nllb_token_correct_per_dialect() {
    assert_eq!(arabic_nllb_token(&ArabicDialect::Egyptian),  "arz_Arab");
    assert_eq!(arabic_nllb_token(&ArabicDialect::Levantine), "apc_Arab");
    assert_eq!(arabic_nllb_token(&ArabicDialect::Iraqi),     "acm_Arab");
    assert_eq!(arabic_nllb_token(&ArabicDialect::Gulf),      "ara_Arab");
    assert_eq!(arabic_nllb_token(&ArabicDialect::Moroccan),  "ary_Arab");
}

#[test]
fn low_confidence_dialect_uses_default_path() {
    let dialect = DialectAnalysis {
        detected_dialect: ArabicDialect::Egyptian,
        confidence: 0.45, // below 0.70 threshold
        ..Default::default()
    };
    let installed = InstalledModels::default();
    let decision = route_arabic_translation(&dialect, &installed);
    assert!(matches!(decision.route, TranslationRoute::NllbDefault));
}
```

### Acceptance criteria — P2

- [ ] `dialect_routing.rs` with full routing table
- [ ] `route_arabic_translation()` covers all 7 dialect variants
- [ ] NLLB dialect-specific tokens (arz/apc/acm/ary/ara)
- [ ] SeamlessM4T selected for Moroccan when installed
- [ ] Fallback with install instructions when model missing
- [ ] `dialect_advisory_text()` always non-empty
- [ ] Routing decision emitted as NDJSON event
- [ ] CLI displays routing decision in human-readable mode
- [ ] `DialectCard.tsx` shows routing info when present
- [ ] MT advisory still present on all output
- [ ] Dialect advisory present alongside MT advisory (not replacing it)
- [ ] 6 new tests pass
- [ ] Clippy clean

---

## After both priorities complete

```bash
cargo test --workspace 2>&1 | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3

# Verify streaming works end-to-end
cd apps/augur-cli
cargo run -- translate \
    --text "مرحبا. كيف حالك. بخير شكرا." \
    --target en \
    --format ndjson 2>/dev/null
# Each sentence should print separately with a gap between them
```

Commit:
```bash
git add -A
git commit -m "feat: augur-sprint-14-15 streaming pipeline + Arabic dialect routing"
```

Report:
- Whether segments now stream one-by-one vs batch
- Which dialect routes to which model
- Test count before (189) and after
- Output of the end-to-end streaming test
- Any deviations from spec

---

## What this sprint closes

**P1 closes:** The Sprint 13 deviation where segments arrived as
a batch. After this sprint, a 30-minute audio file shows live
progress — segment by segment — as Whisper and NLLB process it.

**P2 closes:** The disconnect between dialect detection and
translation quality. AUGUR now uses the knowledge it has about
the dialect to actually improve the translation — not just report
the dialect and do nothing with it.

**Together:** AUGUR now does what no other offline forensic
translation tool does — detects the specific Arabic dialect,
routes to the best available model for that dialect, explains
the routing decision to the examiner, and streams the translation
live.

---

## The two advisories

Both must always be present and neither replaces the other:

**MT advisory (always):**
"Machine translation — verify with a certified human translator
for legal proceedings."

**Dialect advisory (when dialect detected):**
"Detected dialect: [Egyptian/Gulf/Levantine/Moroccan/Iraqi] Arabic.
Machine translation quality varies by dialect. Verify with a
certified Arabic linguist."

The dialect advisory is IN ADDITION TO the MT advisory.
The MT advisory cannot be suppressed.
The dialect advisory cannot be suppressed when dialect is detected.

---

_AUGUR Sprint 14+15 — Streaming Pipeline + Dialect Routing_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_P1 makes the live cursor actually live._
_P2 makes dialect detection actually matter._
