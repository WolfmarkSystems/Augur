# AUGUR Sprint 9 — 2026-04-26 — Session log

## Pre-flight
- Pre-start tests: 113 passing.
- Pre-start clippy: clean.

## P1 Pashto/Farsi script disambiguation: PASSED
- New `augur_classifier::script` module:
  - `pashto_farsi_score(text)` — pure function, no I/O. Counts
    Pashto-specific glyphs (ټ ډ ړ ږ ښ ګ ڼ ۍ ې) and
    Farsi-specific glyphs (پ چ ژ گ); produces a
    `PashtoFarsiAnalysis { pashto_char_count, farsi_char_count,
    pashto_specific_chars (deduped), farsi_specific_chars,
    recommendation, confidence }`.
  - `ScriptRecommendation::{LikelyPashto, LikelyFarsi,
    Ambiguous}`.
  - Confidence curve: a single Pashto-specific glyph with no
    Farsi-specific clears the 0.7 reclassification bar; ≥5
    Pashto-specific occurrences with zero Farsi saturate at 0.95.
- `ClassificationResult` gains
  `disambiguation_note: Option<String>`. When the LID layer
  reports `fa`, `classify()` runs the script analyzer; on
  `LikelyPashto + conf >= 0.7` it reclassifies to `ps` and
  writes a note that names the specific glyphs found and
  cites the confidence. Ambiguous-but-mixed cases stay `fa`
  with an "inconclusive" note.
- CLI: `print_classification` now prints the note as a second
  `⚠` line under the existing advisory. `augur self-test`
  gains `run_pashto_disambiguation_check` — drives the
  analyzer on a Pashto-glyph-heavy probe, expects
  `LikelyPashto` with confidence ≥ 0.7, fully offline.
- 4 new public-API tests pin Sprint 9 P1 acceptance:
  reclassification fires, no false-positive on pure Farsi,
  ambiguous text stays `fa` with no note, the
  `disambiguation_note` round-trips through the
  `ClassificationResult` shape. Plus 6 new tests at the
  script-module level (deduplication, mixed dominance, empty
  input, etc).

## P2 Parallel batch processing: PASSED
- `rayon = "1.12"` added as a direct dep of `augur-cli`. Pool
  built per `cmd_batch` invocation via
  `rayon::ThreadPoolBuilder` with a stable thread name
  (`augur-batch-N`).
- Default thread count: `0` resolves to
  `min(num_cpus, 8)` via `std::thread::available_parallelism`.
  Cap at 8 keeps STT model loads (each ~150 MB) from blowing
  memory under parallelism. New CLI flag
  `augur batch --threads <N>` (also wired into
  `augur package`); `--threads 1` forces sequential behaviour
  for parity with the pre-Sprint-9 path.
- Live counters use `AtomicU32`; the progress JSON's
  `recent_files` is protected by a small `Mutex<Vec<String>>`.
  `write_progress_snapshot` takes a pre-cloned `&[String]`
  so the lock is held only across the vec push, not across
  the JSON serialise.
- Sequential vs parallel benchmark (this host, 20 .txt files
  routed through the STT fail-fast path so the pure
  parallelism speedup shows up):
  - **Sequential (`--threads 1`): 9.06 s, 78% CPU**
  - **Parallel auto (`--threads 0`): 1.62 s, 557% CPU**
  - **Parallel 4 (`--threads 4`): 2.10 s, 379% CPU**
  - **Speedup (auto vs sequential): 5.59×**.
- 3 new tests pin: `resolve_thread_count(0)` returns a value
  in `1..=8`, explicit values pass through, and the threaded
  progress writer produces well-formed JSON with the MT
  notice and the recent-files array.

## P3 Evidence package export: PASSED
- New `apps/augur-cli/src/package.rs` module — `Manifest`,
  `ManifestFile`, `build_manifest`, `render_chain_of_custody`,
  `sha256_of_path` (64 KiB chunked reads — bounded memory),
  `write_package`. ZIP layout matches the spec:
  `MANIFEST.json`, `CHAIN_OF_CUSTODY.txt`, `REPORT.html`,
  `REPORT.json`, `translations/<file>.<target>.txt`,
  `original/...` (only with `--include-originals`).
- New CLI subcommand `augur package --input <dir>
  [--output <zip>] [--target en] [--config <toml>]
  [--preset balanced] [--ocr-lang en] [--include-originals]`.
  Default output: `augur-package-YYYYMMDD.zip` in the cwd.
  Internally runs the parallel batch pipeline first to build
  the report, then assembles the ZIP from the result.
- Forensic invariants:
  - `Manifest::assert_advisory()` refuses to write a manifest
    where `translated_count > 0 && machine_translation_notice
    .is_empty()`. Same shape as the pipeline-level
    `BatchResult::assert_advisory`.
  - `CHAIN_OF_CUSTODY.txt` always renders the MT notice in
    prose (a long-form heading + the canonical const).
  - SHA-256 hashes use `sha2 = "0.10"`; chunked reads keep
    memory bounded for large evidence files.
- 5 new tests pin: manifest carries MT notice, manifest
  SHA-256 matches a hand-computed digest, chain-of-custody
  contains examiner / case / agency / timestamp, the manifest
  advisory invariant rejects an empty notice, and a full
  ZIP write round-trip re-opens with the four required entries
  + per-translation `.txt` for every foreign-language file.

## Final results
- **Default-build test count: 131 passing** (up from Sprint 8's
  113 — +18). 4 integration tests `#[ignore]`-gated on
  `AUGUR_RUN_INTEGRATION_TESTS=1`.
- **`--features augur-plugin-sdk/strata` build**: 137 unit
  tests (131 + 6 strata-only).
- **Clippy: CLEAN** under both
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo clippy --workspace --all-targets --features
   augur-plugin-sdk/strata -- -D warnings`.
- **Offline invariant: MAINTAINED.** No new permitted egress
  URLs in this sprint. The package writer reads only files
  the user pointed at; rayon parallelism is process-local.
- **MT advisory: ALWAYS PRESENT.** New layer added —
  `Manifest::assert_advisory()` enforces at the package
  layer. Existing layers from Sprints 2-8 still hold.
- **Parallel batch benchmark documented in CLAUDE.md** under
  Sprint 9 decisions.

## Deviations from spec
- The script-analysis confidence formula uses a hand-tuned
  ramp rather than the spec's "± character count" sketch.
  One Pashto-specific glyph with zero Farsi-specific reaches
  the 0.7 reclassification bar (per spec); 5+ saturate at
  0.95 — a Pashto vs Farsi tie produces 0.3 (Ambiguous).
- Spec listed ژ in both Pashto-specific AND Farsi-specific
  lists. Decision: left in only the Farsi-specific list since
  it's far more common in Farsi (Pashto uses ږ for the same
  sound). The Pashto-specific list still has 9 distinguishing
  glyphs without it.
- `augur package` runs its own internal parallel batch pass
  rather than re-using a previously-written report (the spec
  was ambiguous on whether the input was a directory or an
  already-generated `BatchResult`). Future sprint can add a
  `--from-report <p>` flag for the second case.
- The package writer always emits the JSON report; the spec
  diagram showed both `REPORT.json` AND `REPORT.html`, and
  both are written unconditionally so the package is useful
  to both human and machine consumers.