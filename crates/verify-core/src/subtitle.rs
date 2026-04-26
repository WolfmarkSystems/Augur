//! SRT / WebVTT subtitle parsing and rendering.
//!
//! Super Sprint Group B P2. Subtitle files are common in video
//! evidence (screen recordings, downloaded content, court
//! transcripts). They carry timestamped text that VERIFY can
//! translate without running Whisper STT — a substantial
//! shortcut on long videos.
//!
//! Both SRT and WebVTT share the same conceptual shape — an
//! ordered list of `(start, end, text)` cues — so this module
//! parses them into a single [`SubtitleEntry`] list and renders
//! back to either format.

use crate::error::VerifyError;

/// One timestamped cue. `start_ms` and `end_ms` are absolute
/// offsets from the start of the video; `text` is the raw cue
/// text (multi-line strings preserved as `\n`-joined).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleEntry {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Parse `HH:MM:SS,mmm` (SRT) into milliseconds.
pub fn parse_srt_timestamp(s: &str) -> Result<u64, VerifyError> {
    parse_timestamp(s, ',')
}

/// Parse `HH:MM:SS.mmm` (WebVTT) into milliseconds.
pub fn parse_vtt_timestamp(s: &str) -> Result<u64, VerifyError> {
    parse_timestamp(s, '.')
}

fn parse_timestamp(s: &str, frac_sep: char) -> Result<u64, VerifyError> {
    let s = s.trim();
    let (hms, frac) = s
        .split_once(frac_sep)
        .ok_or_else(|| VerifyError::InvalidInput(format!("missing '{frac_sep}' in {s:?}")))?;
    let parts: Vec<&str> = hms.split(':').collect();
    if parts.len() != 3 {
        return Err(VerifyError::InvalidInput(format!(
            "expected HH:MM:SS in {hms:?}"
        )));
    }
    let h: u64 = parts[0]
        .parse()
        .map_err(|e| VerifyError::InvalidInput(format!("hours parse {parts:?}: {e}")))?;
    let m: u64 = parts[1]
        .parse()
        .map_err(|e| VerifyError::InvalidInput(format!("minutes parse {parts:?}: {e}")))?;
    let s_int: u64 = parts[2]
        .parse()
        .map_err(|e| VerifyError::InvalidInput(format!("seconds parse {parts:?}: {e}")))?;
    let ms: u64 = frac
        .parse()
        .map_err(|e| VerifyError::InvalidInput(format!("ms parse {frac:?}: {e}")))?;
    Ok(h * 3_600_000 + m * 60_000 + s_int * 1_000 + ms)
}

/// Format milliseconds as `HH:MM:SS,mmm` (SRT) or
/// `HH:MM:SS.mmm` (WebVTT).
pub fn format_timestamp(ms: u64, frac_sep: char) -> String {
    let h = ms / 3_600_000;
    let rem = ms % 3_600_000;
    let m = rem / 60_000;
    let rem = rem % 60_000;
    let s = rem / 1_000;
    let f = rem % 1_000;
    format!("{h:02}:{m:02}:{s:02}{frac_sep}{f:03}")
}

/// Parse SRT content. Tolerant: BOM-stripped, `\r\n` accepted,
/// blank-line cue separators required (the standard).
pub fn parse_srt(content: &str) -> Result<Vec<SubtitleEntry>, VerifyError> {
    let body = strip_bom(content);
    let normalized = body.replace("\r\n", "\n");
    let mut out: Vec<SubtitleEntry> = Vec::new();
    for block in normalized.split("\n\n") {
        let block = block.trim_matches('\n');
        if block.is_empty() {
            continue;
        }
        let lines: Vec<&str> = block.lines().collect();
        // Some SRT files omit the index line. Try both shapes.
        let (idx, ts_line, text_lines) = if lines[0].contains("-->") {
            (0u32, lines[0], &lines[1..])
        } else {
            if lines.len() < 2 {
                return Err(VerifyError::InvalidInput(format!(
                    "SRT block missing timestamp: {block:?}"
                )));
            }
            let parsed_idx: u32 = lines[0].trim().parse().unwrap_or(0);
            (parsed_idx, lines[1], &lines[2..])
        };
        if !ts_line.contains("-->") {
            return Err(VerifyError::InvalidInput(format!(
                "SRT timestamp line missing '-->': {ts_line:?}"
            )));
        }
        let (start, end) = ts_line
            .split_once("-->")
            .ok_or_else(|| VerifyError::InvalidInput("split '-->'".to_string()))?;
        let start_ms = parse_srt_timestamp(start.trim())?;
        let end_ms = parse_srt_timestamp(end.trim())?;
        let text = text_lines.join("\n");
        out.push(SubtitleEntry {
            index: if idx == 0 {
                (out.len() as u32) + 1
            } else {
                idx
            },
            start_ms,
            end_ms,
            text,
        });
    }
    Ok(out)
}

/// Parse WebVTT content. Strips the `WEBVTT` header line plus
/// any optional `NOTE`/`STYLE` blocks; cue identifiers
/// (optional one-line labels before the timestamp) are dropped
/// since VERIFY only cares about the timestamped text.
pub fn parse_vtt(content: &str) -> Result<Vec<SubtitleEntry>, VerifyError> {
    let body = strip_bom(content);
    let normalized = body.replace("\r\n", "\n");
    let mut out: Vec<SubtitleEntry> = Vec::new();
    for block in normalized.split("\n\n") {
        let block = block.trim_matches('\n');
        if block.is_empty() {
            continue;
        }
        // Skip the header / NOTE / STYLE blocks — they don't
        // contain `-->`.
        if !block.contains("-->") {
            continue;
        }
        let mut lines: Vec<&str> = block.lines().collect();
        // Drop an optional cue identifier (line before the
        // timestamp that doesn't itself contain `-->`).
        if !lines.is_empty() && !lines[0].contains("-->") {
            lines.remove(0);
        }
        if lines.is_empty() {
            continue;
        }
        let ts_line = lines[0];
        let (start, end_with_settings) = ts_line
            .split_once("-->")
            .ok_or_else(|| VerifyError::InvalidInput("vtt split '-->'".to_string()))?;
        // WebVTT timestamps may carry trailing settings
        // ("00:00:01.000 --> 00:00:04.000 align:start").
        // Take only the first whitespace-bounded token after
        // the arrow.
        let end = end_with_settings.split_whitespace().next().unwrap_or("");
        let start_ms = parse_vtt_timestamp(start.trim())?;
        let end_ms = parse_vtt_timestamp(end)?;
        let text = lines[1..].join("\n");
        out.push(SubtitleEntry {
            index: (out.len() as u32) + 1,
            start_ms,
            end_ms,
            text,
        });
    }
    Ok(out)
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{FEFF}').unwrap_or(s)
}

/// Render entries back to SRT format. Useful for the
/// `--output-srt` flag — produce a translated subtitle file
/// that drops into any media player.
pub fn render_srt(entries: &[SubtitleEntry]) -> String {
    let mut out = String::with_capacity(entries.len() * 80);
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&format!("{}\n", i + 1));
        out.push_str(&format!(
            "{} --> {}\n",
            format_timestamp(e.start_ms, ','),
            format_timestamp(e.end_ms, ',')
        ));
        out.push_str(&e.text);
        out.push_str("\n\n");
    }
    out
}

/// Render entries back to WebVTT format.
pub fn render_vtt(entries: &[SubtitleEntry]) -> String {
    let mut out = String::with_capacity(entries.len() * 80 + 10);
    out.push_str("WEBVTT\n\n");
    for e in entries {
        out.push_str(&format!(
            "{} --> {}\n",
            format_timestamp(e.start_ms, '.'),
            format_timestamp(e.end_ms, '.')
        ));
        out.push_str(&e.text);
        out.push_str("\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_parser_extracts_entries_correctly() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nHello\n\n2\n00:00:05,500 --> 00:00:08,000\nSecond cue\n";
        let entries = parse_srt(srt).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].start_ms, 1_000);
        assert_eq!(entries[0].end_ms, 4_000);
        assert_eq!(entries[0].text, "Hello");
        assert_eq!(entries[1].start_ms, 5_500);
        assert_eq!(entries[1].text, "Second cue");
    }

    #[test]
    fn srt_parser_handles_multiline_cue_text() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nFirst line\nSecond line\n";
        let entries = parse_srt(srt).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "First line\nSecond line");
    }

    #[test]
    fn vtt_parser_handles_webvtt_header() {
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nHello\n\n00:00:05.500 --> 00:00:08.000\nSecond\n";
        let entries = parse_vtt(vtt).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].start_ms, 1_000);
        assert_eq!(entries[1].text, "Second");
    }

    #[test]
    fn vtt_parser_strips_optional_cue_identifier() {
        let vtt = "WEBVTT\n\ncue-1\n00:00:01.000 --> 00:00:04.000 align:start\nHello\n";
        let entries = parse_vtt(vtt).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "Hello");
    }

    #[test]
    fn srt_timestamps_parse_to_milliseconds() {
        assert_eq!(parse_srt_timestamp("01:23:45,678").unwrap(), 5_025_678);
        assert_eq!(parse_srt_timestamp("00:00:00,000").unwrap(), 0);
    }

    #[test]
    fn vtt_timestamps_parse_to_milliseconds() {
        assert_eq!(parse_vtt_timestamp("01:23:45.678").unwrap(), 5_025_678);
    }

    #[test]
    fn srt_round_trip_preserves_content() {
        let entries = vec![SubtitleEntry {
            index: 1,
            start_ms: 1_000,
            end_ms: 4_000,
            text: "Hello world".into(),
        }];
        let rendered = render_srt(&entries);
        let parsed = parse_srt(&rendered).unwrap();
        assert_eq!(parsed, entries);
    }

    #[test]
    fn vtt_round_trip_preserves_content() {
        let entries = vec![SubtitleEntry {
            index: 1,
            start_ms: 1_000,
            end_ms: 4_000,
            text: "Hello world".into(),
        }];
        let rendered = render_vtt(&entries);
        let parsed = parse_vtt(&rendered).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "Hello world");
    }

    #[test]
    fn malformed_srt_returns_err_not_panic() {
        let bad = "this is not srt\nat all";
        assert!(parse_srt(bad).is_err());
    }
}
