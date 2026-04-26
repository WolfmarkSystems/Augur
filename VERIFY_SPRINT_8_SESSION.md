# VERIFY Sprint 8 — 2026-04-26 — Session log

## Pre-flight
- Pre-start tests: 105 passing.
- Pre-start clippy: clean.

## P1 Strata plugin SDK vendored: PASSED
- Copied `~/Wolfmark/strata/crates/strata-plugin-sdk/` to
  `vendor/strata-plugin-sdk/` and dropped the dev-deps
  (`tempfile = { workspace = true }`) since cargo otherwise
  can't resolve them outside the upstream Strata workspace.
- Built a minimal `vendor/strata-fs/` stub crate exposing just
  `vfs::{WalkDecision, VfsEntry, VfsResult, VfsError,
  VirtualFilesystem}` — the surface the upstream SDK actually
  re-exports + invokes. VERIFY's plugin walks `root_path`
  directly (no VFS), so the stub trait body is never called;
  it only needs to type-check.
- Workspace `Cargo.toml` adds `[workspace.exclude]` for both
  vendored crates so `cargo build --workspace` does NOT compile
  them. The strata feature pulls them in via path deps from
  `crates/verify-plugin-sdk/Cargo.toml`.
- `cargo build --features verify-plugin-sdk/strata` ✅ succeeds
  on this machine without the sibling Strata workspace.
- 2 new feature-gated tests pin Sprint 8 acceptance:
  `strata_plugin_skips_non_foreign_files` (empty / .txt-only
  tempdir → no artifacts), and
  `strata_plugin_execute_returns_advisory_artifacts`
  (every emitted artifact upholds `assert_advisory_invariant`).
  The existing 7 strata-feature tests from Sprint 5 still pass.

## P2 Multi-language batch: PASSED
- `BatchResult` gains `language_groups: Vec<LanguageGroup>` and
  `dominant_language: Option<String>`. New
  `BatchResult::build_language_groups` clusters per-file rows
  by detected ISO 639-1 code, computes per-group word counts,
  sorts by file count descending, and picks the most-frequent
  *foreign* language (excluding `target_language`). Idempotent;
  CLI calls it once after `build_summary`.
- Helper `language_name_for(iso)` covers the major +
  forensic-priority languages (Arabic / Persian / Pashto /
  Urdu / Chinese / Russian / etc.) with `(unknown)` as a
  graceful fallback.
- HTML renderer extracted a `push_results_table` helper, then
  added a `Language summary` block + dominant-language banner +
  per-language sections (each with its own MT advisory) in
  `render_batch_html`. A multi-language report prints the MT
  advisory at minimum 4 places (top + per-section + bottom),
  pinned by a new test.
- New CLI flag `verify batch --all-foreign` plumbed through
  `cmd_batch`. Behavior is the same as the post-Sprint-3
  default (every non-target file translated); the flag prints
  an explicit log line so the run banner reflects the operator
  intent, and the language-group block populates regardless.
- 5 new tests pin: language grouping with sort + word-count
  rollup, dominant-foreign-language selection with target
  exclusion, no-foreign → `dominant_language = None`, the
  `language_name_for` lookup, and HTML per-language sections
  emitting per-section advisories.

## P3 Video diarization + speaker advisory: PASSED
- New `SPEAKER_DIARIZATION_ADVISORY` const in
  `verify-stt::diarize`. Non-suppressible at the same level as
  the MT advisory; warns that speaker labels are produced by
  automated voice segmentation and must NOT be relied upon as
  biometric identification.
- `ResolvedSource` gains `audio_path: Option<PathBuf>` +
  `audio_path_is_scratch: bool`. The video resolver now keeps
  the ffmpeg-extracted WAV alive (instead of deleting it
  immediately after STT) so pyannote can read it for
  diarization; the CLI cleans the scratch up after diarization
  runs (or unconditionally on the non-diarize path). Audio
  inputs use the original input path directly; image / PDF /
  text inputs leave `audio_path = None`.
- `cmd_translate` prefers `resolved.audio_path` over
  `resolved_path` when invoking `run_diarization` — pyannote
  reads audio containers, not video.
- After a diarized translation prints, the CLI fires
  `print_speaker_advisory()` immediately after `print_advisory`
  (MT advisory). Both fire; neither replaces the other.
- 3 new tests pin Sprint 8 P3 acceptance:
  - `video_diarization_pipeline_produces_enriched_segments`
    (3 STT segments × 2 speakers; verify the right speaker
    lands on each segment by max temporal overlap).
  - `speaker_advisory_always_present_when_diarization_used`
    (const non-empty + carries the "automated" + "identification"
    keywords).
  - `video_without_diarization_still_produces_transcript`
    (empty diar list → all segments labeled `UNKNOWN`, no text
    is lost).

## Final results
- **Default-build test count: 113 passing** (up from Sprint 7's
  105). 4 integration tests `#[ignore]`-gated on
  `VERIFY_RUN_INTEGRATION_TESTS=1`.
- **`--features verify-plugin-sdk/strata` build**: 119 unit
  tests (113 + 6 strata-only).
- **Clippy: CLEAN** under both
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo clippy --workspace --all-targets --features
   verify-plugin-sdk/strata -- -D warnings`.
- **Offline invariant: MAINTAINED.** No new permitted egress
  URLs in this sprint. Pyannote weight cache + GeoIP DB
  handling all unchanged.
- **MT advisory + speaker advisory: ALWAYS PRESENT.** Diarized
  transcripts now fire BOTH advisories — not one or the other.

## Deviations from spec
- The vendored SDK's dev-dep `tempfile = { workspace = true }`
  was dropped because cargo can't resolve it outside the
  upstream Strata workspace. The SDK's own test suite never
  runs from VERIFY, so this is safe.
- Spec sketched `strata-plugin-sdk` as "only depends on serde
  and common crates" — the actual dep tree includes
  `strata-fs` for the VFS trait surface. Resolution: minimal
  stub (per spec's "stub those out" branch).
- `--all-foreign` is plumbed but is a no-op on existing
  behavior (Sprint 3 already translated every non-target
  file). It exists for examiner intent clarity. The new
  `language_groups` block always populates regardless of the
  flag.