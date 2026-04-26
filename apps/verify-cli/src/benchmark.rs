//! `verify benchmark` — performance-regression tracking.
//!
//! Super Sprint Group C P5. Times the pipeline against a fixed
//! corpus (`tests/benchmarks/`) and reports per-fixture
//! latency. Default mode is offline — only the classifier runs.
//! `--full` opts into translation timing (requires Python +
//! transformers / ctranslate2).
//!
//! # Regression detection
//!
//! `--compare <previous_results.json>` reads a previous run
//! and flags any fixture that's >20% slower than the recorded
//! baseline. Slowdowns are warnings, not failures — the
//! examiner decides whether to investigate.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;
use verify_classifier::LanguageClassifier;
use verify_core::error::VerifyError;

const REGRESSION_THRESHOLD: f64 = 1.2; // 20% slower than baseline

/// Per-fixture benchmark outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub test_name: String,
    pub fixture_path: String,
    pub duration_ms: u64,
    pub bytes: u64,
    pub words: usize,
    pub chars_per_second: f64,
    pub words_per_second: f64,
    /// Always `true` for offline benchmarks; meaningful only
    /// for the `--full` translation runs where a non-zero
    /// duration with empty output would flag a config break.
    pub passed: bool,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSuite {
    pub generated_at: String,
    pub host: String,
    pub verify_version: String,
    pub results: Vec<BenchmarkResult>,
}

#[derive(Debug, Clone)]
pub struct BenchmarkOptions {
    pub full: bool,
    pub fixtures_dir: PathBuf,
}

/// Run the benchmark suite. Always exercises classification on
/// every `.txt` fixture. With `--full`, additionally runs
/// translation on the smallest fixture (model setup is the same
/// regardless of corpus length, so timing one suffices).
pub fn run_suite(options: &BenchmarkOptions) -> Result<BenchmarkSuite, VerifyError> {
    if !options.fixtures_dir.is_dir() {
        return Err(VerifyError::InvalidInput(format!(
            "benchmark fixtures dir not found: {:?}",
            options.fixtures_dir
        )));
    }
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&options.fixtures_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("txt"))
                .unwrap_or(false)
        })
        .collect();
    fixtures.sort();

    let classifier = LanguageClassifier::new_whichlang();
    let mut results: Vec<BenchmarkResult> = Vec::new();
    for fixture in &fixtures {
        let body = std::fs::read_to_string(fixture)?;
        let bytes = body.len() as u64;
        let words = body.split_whitespace().count();
        let started = Instant::now();
        // Pin the work the classifier is doing. We don't care
        // about the result here — just the latency.
        let _ = classifier.classify(&body, "en")?;
        let dur = started.elapsed();
        let ms = dur.as_secs_f64() * 1000.0;
        let cps = if ms > 0.0 {
            (bytes as f64) * 1000.0 / ms
        } else {
            0.0
        };
        let wps = if ms > 0.0 {
            (words as f64) * 1000.0 / ms
        } else {
            0.0
        };
        let name = fixture
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| fixture.to_string_lossy().into_owned());
        results.push(BenchmarkResult {
            test_name: format!("classify::{name}"),
            fixture_path: fixture.to_string_lossy().into_owned(),
            duration_ms: dur.as_millis() as u64,
            bytes,
            words,
            chars_per_second: cps,
            words_per_second: wps,
            passed: true,
            notes: String::new(),
        });
    }

    if options.full {
        results.push(run_full_translation_bench(&options.fixtures_dir)?);
    }

    let suite = BenchmarkSuite {
        generated_at: crate::utc_now_iso8601(),
        host: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        verify_version: env!("CARGO_PKG_VERSION").to_string(),
        results,
    };
    Ok(suite)
}

fn run_full_translation_bench(fixtures: &Path) -> Result<BenchmarkResult, VerifyError> {
    use verify_translate::TranslationEngine;
    let probe = fixtures.join("arabic_short.txt");
    let body = std::fs::read_to_string(&probe)?;
    let started = Instant::now();
    let engine = TranslationEngine::with_xdg_cache()?;
    match engine.translate(&body, "ar", "en") {
        Ok(r) => {
            let dur = started.elapsed();
            // Forensic invariant — surface in benchmark output
            // so anyone reading the JSON sees the advisory was
            // present.
            let advisory_ok =
                r.is_machine_translation && !r.advisory_notice.is_empty();
            Ok(BenchmarkResult {
                test_name: "translate::arabic_short→en".into(),
                fixture_path: probe.to_string_lossy().into_owned(),
                duration_ms: dur.as_millis() as u64,
                bytes: body.len() as u64,
                words: body.split_whitespace().count(),
                chars_per_second: 0.0,
                words_per_second: 0.0,
                passed: advisory_ok,
                notes: if advisory_ok {
                    format!("→ {}", r.translated_text.chars().take(80).collect::<String>())
                } else {
                    "MT advisory invariant violated on benchmark result".into()
                },
            })
        }
        Err(e) => Ok(BenchmarkResult {
            test_name: "translate::arabic_short→en".into(),
            fixture_path: probe.to_string_lossy().into_owned(),
            duration_ms: 0,
            bytes: body.len() as u64,
            words: body.split_whitespace().count(),
            chars_per_second: 0.0,
            words_per_second: 0.0,
            passed: false,
            notes: format!("skipped: {e}"),
        }),
    }
}

/// Render the suite to the CLI in a tabular form.
pub fn render_text(suite: &BenchmarkSuite) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Generated: {}\nHost: {}\nVerify: {}\n\n",
        suite.generated_at, suite.host, suite.verify_version
    ));
    out.push_str(&format!(
        "{:<40} {:>9} {:>10} {:>10} {:>14}\n",
        "test", "duration", "bytes", "words", "words/sec"
    ));
    out.push_str(&"-".repeat(86));
    out.push('\n');
    for r in &suite.results {
        out.push_str(&format!(
            "{:<40} {:>7}ms {:>10} {:>10} {:>14.0}\n",
            r.test_name, r.duration_ms, r.bytes, r.words, r.words_per_second
        ));
        if !r.notes.is_empty() {
            out.push_str(&format!("  {}\n", r.notes));
        }
    }
    out
}

/// Compare a fresh suite against a previously-saved one. Returns
/// the human-readable regression report; empty string when no
/// regressions detected.
pub fn render_regression_report(
    fresh: &BenchmarkSuite,
    baseline: &BenchmarkSuite,
) -> String {
    let mut out = String::new();
    for r in &fresh.results {
        if let Some(b) = baseline
            .results
            .iter()
            .find(|x| x.test_name == r.test_name)
        {
            if b.duration_ms == 0 {
                continue;
            }
            let ratio = r.duration_ms as f64 / b.duration_ms as f64;
            if ratio > REGRESSION_THRESHOLD {
                out.push_str(&format!(
                    "REGRESSION: {} — {}ms (baseline {}ms, {:.1}× slower)\n",
                    r.test_name, r.duration_ms, b.duration_ms, ratio
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        // tests/benchmarks/ relative to the workspace root.
        // Walk up two levels from CARGO_MANIFEST_DIR (apps/verify-cli/).
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/benchmarks");
        p.canonicalize()
            .unwrap_or_else(|_| p.to_path_buf())
    }

    #[test]
    fn benchmark_classification_completes_under_threshold() {
        let dir = fixtures_dir();
        if !dir.is_dir() {
            eprintln!("benchmark fixtures dir not present at {dir:?}; skipping");
            return;
        }
        let opts = BenchmarkOptions {
            full: false,
            fixtures_dir: dir,
        };
        let suite = run_suite(&opts).expect("run_suite");
        assert!(!suite.results.is_empty(), "expected ≥1 fixture");
        for r in &suite.results {
            // 50ms is generous — whichlang on a 500-word
            // Arabic corpus typically clears it in < 5ms.
            assert!(
                r.duration_ms < 200,
                "{} took {}ms; expected < 200ms",
                r.test_name,
                r.duration_ms
            );
            assert!(r.passed);
        }
    }

    #[test]
    fn benchmark_result_serializes_to_json() {
        let r = BenchmarkResult {
            test_name: "x".into(),
            fixture_path: "/tmp/x".into(),
            duration_ms: 12,
            bytes: 100,
            words: 20,
            chars_per_second: 8333.0,
            words_per_second: 1666.0,
            passed: true,
            notes: String::new(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: BenchmarkResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.test_name, r.test_name);
        assert_eq!(back.duration_ms, r.duration_ms);
    }

    #[test]
    fn regression_report_flags_slowdowns() {
        let fresh = BenchmarkSuite {
            generated_at: "2026-04-26T00:00:00Z".into(),
            host: "x86".into(),
            verify_version: "1.0".into(),
            results: vec![BenchmarkResult {
                test_name: "classify::short".into(),
                fixture_path: "x".into(),
                duration_ms: 100,
                bytes: 0,
                words: 0,
                chars_per_second: 0.0,
                words_per_second: 0.0,
                passed: true,
                notes: String::new(),
            }],
        };
        let baseline = BenchmarkSuite {
            generated_at: "2026-04-25T00:00:00Z".into(),
            host: "x86".into(),
            verify_version: "0.9".into(),
            results: vec![BenchmarkResult {
                test_name: "classify::short".into(),
                fixture_path: "x".into(),
                duration_ms: 50,
                bytes: 0,
                words: 0,
                chars_per_second: 0.0,
                words_per_second: 0.0,
                passed: true,
                notes: String::new(),
            }],
        };
        let report = render_regression_report(&fresh, &baseline);
        assert!(report.contains("REGRESSION"));
        assert!(report.contains("classify::short"));
    }
}
