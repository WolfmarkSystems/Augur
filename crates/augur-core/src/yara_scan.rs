//! YARA pattern scanning.
//!
//! Super Sprint Group B P3. Subprocess-driven (same pattern as
//! `ffmpeg`, `tesseract`, `pdftoppm`) — invokes the system
//! `yara` binary against a rules file and the input content.
//! No `libyara` linkage at the Rust layer, no FFI; if `yara` is
//! not on PATH, callers see a clean
//! [`AugurError::YaraNotInstalled`] error with the install
//! hint.
//!
//! # Why subprocess
//!
//! The `yara` Rust crate links against `libyara` system
//! headers; that's a heavier system dep than AUGUR needs for
//! pattern matching. The `yara` CLI is universally available
//! on forensic workstations (`brew install yara`,
//! `apt install yara`) and produces structured output AUGUR
//! parses cheaply.
//!
//! # Output format
//!
//! `yara -s -p 1 <rules> <input>` (`-s` = print matched
//! strings, `-p 1` = limit to one thread for deterministic
//! output) emits:
//! ```text
//! rule_name path/to/input
//! 0x4b:$identifier: matched_substring
//! 0xa3:$other: matched_substring
//! ```
//! We parse rule names from the first column and string
//! matches from the indented `0xOFFSET:$id: data` lines.

use crate::error::AugurError;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const YARA_BIN: &str = "yara";

/// One match from a single rule.
#[derive(Debug, Clone, Serialize)]
pub struct YaraStringMatch {
    /// `$identifier` from the YARA rule (without the `$`).
    pub identifier: String,
    /// Byte offset into the scanned content.
    pub offset: u64,
    /// The matched substring as the YARA binary reported it.
    pub data: String,
}

/// One rule firing.
#[derive(Debug, Clone, Serialize)]
pub struct YaraMatch {
    pub rule_name: String,
    pub matched_strings: Vec<YaraStringMatch>,
    /// Where the match was scanned — `"text"` for in-memory
    /// strings (translated content, source text), or the file
    /// path for `scan_file` calls.
    pub scanned_source: String,
}

/// YARA engine — wraps the subprocess invocation. Cheap to
/// construct; each `scan_*` call spawns its own `yara` process.
#[derive(Debug, Clone)]
pub struct YaraEngine {
    pub rules_path: PathBuf,
    pub yara_cmd: String,
}

impl YaraEngine {
    /// Construct an engine pointing at a rules file or directory.
    /// The path is checked for existence; rule compilation
    /// itself is deferred to the first scan call (yara compiles
    /// and matches in one invocation).
    pub fn load(rules_path: &Path) -> Result<Self, AugurError> {
        if !rules_path.exists() {
            return Err(AugurError::Yara(format!(
                "YARA rules path does not exist: {rules_path:?}"
            )));
        }
        Ok(Self {
            rules_path: rules_path.to_path_buf(),
            yara_cmd: YARA_BIN.to_string(),
        })
    }

    /// `true` when the configured `yara` binary is available on
    /// PATH. Pure check — never spawns more than `<bin> --version`.
    pub fn is_available(&self) -> bool {
        Command::new(&self.yara_cmd)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Scan an in-memory text buffer. Writes the buffer to a
    /// temporary file first (yara always reads from disk),
    /// invokes the subprocess, parses output, removes the temp.
    pub fn scan_text(&self, text: &str) -> Result<Vec<YaraMatch>, AugurError> {
        let tmp = std::env::temp_dir().join(format!(
            "augur-yara-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&tmp, text.as_bytes())?;
        let result = self.scan_file_internal(&tmp, "text");
        let _ = std::fs::remove_file(&tmp);
        result
    }

    /// Scan a file directly.
    pub fn scan_file(&self, path: &Path) -> Result<Vec<YaraMatch>, AugurError> {
        if !path.exists() {
            return Err(AugurError::InvalidInput(format!(
                "YARA scan target not found: {path:?}"
            )));
        }
        self.scan_file_internal(path, &path.to_string_lossy())
    }

    fn scan_file_internal(
        &self,
        target: &Path,
        scanned_label: &str,
    ) -> Result<Vec<YaraMatch>, AugurError> {
        if !self.is_available() {
            return Err(AugurError::YaraNotInstalled(format!(
                "`{}` not found on PATH. Install YARA: \
                 `brew install yara` (macOS) or `apt install yara` (Linux). \
                 AUGUR's --yara-rules feature is opt-in.",
                self.yara_cmd
            )));
        }
        let output = Command::new(&self.yara_cmd)
            .arg("-s") // print matched strings
            .arg("-p")
            .arg("1") // single-threaded for deterministic output
            .arg(&self.rules_path)
            .arg(target)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| AugurError::Yara(format!("spawn yara: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(AugurError::Yara(format!(
                "yara exit {:?}: {stderr}",
                output.status.code()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(parse_yara_output(&stdout, scanned_label))
    }
}

/// Parse YARA's `-s` (verbose-strings) output format.
///
/// Format (one rule per match group):
/// ```text
/// rule_name /path/to/scanned
/// 0x4b:$identifier: matched_substring
/// 0xa3:$other: another_match
/// ```
/// Multiple rules can fire on the same input; each rule starts
/// a new "rule_name path" line.
pub fn parse_yara_output(stdout: &str, scanned_label: &str) -> Vec<YaraMatch> {
    let mut out: Vec<YaraMatch> = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("0x") {
            // Match-string line: `0xOFFSET:$identifier: data`
            if let Some((offset_id, data)) = stripped.split_once(": ") {
                if let Some((offset_hex, ident)) = offset_id.split_once(":$") {
                    let offset: u64 = u64::from_str_radix(offset_hex, 16).unwrap_or(0);
                    if let Some(last) = out.last_mut() {
                        last.matched_strings.push(YaraStringMatch {
                            identifier: ident.to_string(),
                            offset,
                            data: data.to_string(),
                        });
                    }
                }
            }
            continue;
        }
        // Rule line: `rule_name path` (path may contain spaces;
        // we only care about the first whitespace-delimited token).
        if let Some(rule) = trimmed.split_whitespace().next() {
            out.push(YaraMatch {
                rule_name: rule.to_string(),
                matched_strings: Vec::new(),
                scanned_source: scanned_label.to_string(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yara_no_rules_path_returns_clear_error() {
        let bogus = Path::new("/nonexistent/strata/verify/rules.yar");
        match YaraEngine::load(bogus) {
            Err(AugurError::Yara(msg)) => {
                assert!(msg.contains("does not exist"), "msg: {msg}");
            }
            other => panic!("expected Yara error, got {other:?}"),
        }
    }

    #[test]
    fn yara_match_includes_offset_and_data() {
        // Pure parser test — synthetic stdout, no yara binary
        // required.
        let stdout = "url_pattern /tmp/x.txt\n\
                      0x4b:$url: https://evil.com/payload\n\
                      bitcoin_wallet_address /tmp/x.txt\n\
                      0xa3:$btc: 1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2\n";
        let parsed = parse_yara_output(stdout, "/tmp/x.txt");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].rule_name, "url_pattern");
        assert_eq!(parsed[0].matched_strings.len(), 1);
        assert_eq!(parsed[0].matched_strings[0].identifier, "url");
        assert_eq!(parsed[0].matched_strings[0].offset, 0x4b);
        assert!(parsed[0].matched_strings[0]
            .data
            .contains("evil.com/payload"));
        assert_eq!(parsed[1].rule_name, "bitcoin_wallet_address");
        assert_eq!(parsed[1].matched_strings[0].offset, 0xa3);
    }

    #[test]
    fn yara_parser_handles_empty_output() {
        let parsed = parse_yara_output("", "text");
        assert!(parsed.is_empty());
    }

    #[test]
    fn yara_engine_load_accepts_rules_file_that_exists() {
        // Use this source file as a stand-in for a rules file —
        // the load step only checks existence; compilation runs
        // at scan time when yara is invoked.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(path.exists());
        let engine = YaraEngine::load(&path).expect("load");
        assert_eq!(engine.rules_path, path);
        assert_eq!(engine.yara_cmd, "yara");
    }

    #[test]
    fn yara_scan_returns_not_installed_when_yara_missing() {
        // Force an engine pointing at a real rules-path stand-in
        // but with a yara command that doesn't exist. We get a
        // structured `YaraNotInstalled` error rather than a panic.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let mut engine = YaraEngine::load(&path).expect("load");
        engine.yara_cmd = "this-yara-binary-does-not-exist-xyz".into();
        match engine.scan_text("anything") {
            Err(AugurError::YaraNotInstalled(msg)) => {
                assert!(msg.contains("yara"));
            }
            other => panic!("expected YaraNotInstalled, got {other:?}"),
        }
    }
}
