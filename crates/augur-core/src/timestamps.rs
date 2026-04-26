//! Timestamp conversion — common forensic epochs ↔ Unix seconds
//! ↔ ISO-8601 UTC.
//!
//! Sprint 7 P3. Evidence databases store timestamps in many
//! formats. The examiner often needs to verify a single value
//! quickly; this module is the answer to "what date is
//! 1762276748?"
//!
//! # Conversion math
//!
//! Each format reduces to "interval count since some epoch". We
//! convert into i128 seconds-since-Unix-epoch, then format the
//! result as a stable ISO-8601 UTC string. The epoch math:
//!
//! | Format            | Unit                      | Epoch                   |
//! |-------------------|---------------------------|-------------------------|
//! | UnixSeconds       | seconds                   | 1970-01-01              |
//! | UnixMilliseconds  | milliseconds              | 1970-01-01              |
//! | UnixMicroseconds  | microseconds              | 1970-01-01              |
//! | UnixNanoseconds   | nanoseconds               | 1970-01-01              |
//! | AppleCoreData     | seconds                   | 2001-01-01 (Cocoa)      |
//! | AppleNanoseconds  | nanoseconds               | 2001-01-01              |
//! | WindowsFiletime   | 100-ns intervals          | 1601-01-01              |
//! | WebKit            | microseconds              | 1601-01-01              |
//! | HfsPlus           | seconds                   | 1904-01-01              |
//!
//! Apple-Cocoa = Apple Core Data: same number, different name.
//! Sprint 7 spec lists them as separate `TimestampFormat` variants
//! for examiner clarity; here both convert identically.

use crate::error::AugurError;
use serde::Serialize;

/// One supported timestamp format. The variants match AUGUR
/// SPRINT 7 P3 plus the Cocoa alias (Cocoa and AppleCoreData
/// share the 2001-01-01 epoch / seconds unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TimestampFormat {
    UnixSeconds,
    UnixMilliseconds,
    UnixMicroseconds,
    UnixNanoseconds,
    AppleCoreData,
    AppleNanoseconds,
    WindowsFiletime,
    WebKit,
    HfsPlus,
    /// Alias for [`AppleCoreData`] — Cocoa NSDate uses the same
    /// 2001-01-01 / seconds epoch.
    CocoaDate,
}

impl TimestampFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnixSeconds => "unix-seconds",
            Self::UnixMilliseconds => "unix-milliseconds",
            Self::UnixMicroseconds => "unix-microseconds",
            Self::UnixNanoseconds => "unix-nanoseconds",
            Self::AppleCoreData => "apple-coredata",
            Self::AppleNanoseconds => "apple-nanoseconds",
            Self::WindowsFiletime => "windows-filetime",
            Self::WebKit => "webkit",
            Self::HfsPlus => "hfs-plus",
            Self::CocoaDate => "cocoa-date",
        }
    }

    // Named `parse_format` (rather than `from_str`) to avoid
    // colliding with the `FromStr` trait shape clippy warns
    // about — we don't want a trait impl here, just a simple
    // lookup that returns Option (no error type).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "unix-seconds" | "unix" => Self::UnixSeconds,
            "unix-milliseconds" | "unix-ms" => Self::UnixMilliseconds,
            "unix-microseconds" | "unix-us" => Self::UnixMicroseconds,
            "unix-nanoseconds" | "unix-ns" => Self::UnixNanoseconds,
            "apple-coredata" | "core-data" => Self::AppleCoreData,
            "apple-nanoseconds" | "apple-ns" => Self::AppleNanoseconds,
            "windows-filetime" | "filetime" => Self::WindowsFiletime,
            "webkit" => Self::WebKit,
            "hfs-plus" | "hfs+" => Self::HfsPlus,
            "cocoa-date" | "cocoa" | "ns-date" => Self::CocoaDate,
            _ => return None,
        })
    }
}

/// Result of one (value, format) → instant conversion.
#[derive(Debug, Clone, Serialize)]
pub struct TimestampResult {
    pub input: i64,
    pub format: TimestampFormat,
    /// ISO-8601 UTC, e.g. `"2025-11-04T17:19:08Z"`. Empty when
    /// the conversion would overflow the supported range
    /// (years 0001–9999).
    pub utc: String,
    /// Canonical Unix seconds. May be negative (pre-1970) or
    /// zero when the conversion is out of range — paired with
    /// `confidence` so callers can filter.
    pub unix_seconds: i64,
    /// "High" / "Medium" / "Low" — used by the auto-detector
    /// to rank multiple plausible interpretations of one value.
    pub confidence: String,
}

// Seconds between Unix epoch (1970-01-01) and other reference
// epochs, computed once.
const SECS_1970_TO_2001: i64 = 978_307_200; // Apple
const SECS_1601_TO_1970: i64 = 11_644_473_600; // Windows / WebKit
const SECS_1904_TO_1970: i64 = 2_082_844_800; // HFS+

/// Convert a raw integer in a known format to a
/// [`TimestampResult`]. Out-of-range values return a result with
/// `unix_seconds = 0` and `utc = ""` plus `confidence = "Low"`
/// — never a panic.
pub fn convert(value: i64, format: TimestampFormat) -> TimestampResult {
    let secs_opt: Option<i64> = match format {
        TimestampFormat::UnixSeconds => Some(value),
        TimestampFormat::UnixMilliseconds => value.checked_div(1_000),
        TimestampFormat::UnixMicroseconds => value.checked_div(1_000_000),
        TimestampFormat::UnixNanoseconds => value.checked_div(1_000_000_000),
        TimestampFormat::AppleCoreData | TimestampFormat::CocoaDate => {
            value.checked_add(SECS_1970_TO_2001)
        }
        TimestampFormat::AppleNanoseconds => value
            .checked_div(1_000_000_000)
            .and_then(|s| s.checked_add(SECS_1970_TO_2001)),
        TimestampFormat::WindowsFiletime => {
            // 100-ns intervals → seconds, then shift epoch.
            value
                .checked_div(10_000_000)
                .and_then(|s| s.checked_sub(SECS_1601_TO_1970))
        }
        TimestampFormat::WebKit => value
            .checked_div(1_000_000)
            .and_then(|s| s.checked_sub(SECS_1601_TO_1970)),
        TimestampFormat::HfsPlus => value.checked_sub(SECS_1904_TO_1970),
    };
    let (unix_seconds, utc) = match secs_opt.and_then(format_iso8601_utc) {
        Some((s, iso)) => (s, iso),
        None => (0, String::new()),
    };
    let confidence = if utc.is_empty() {
        "Low"
    } else {
        "High"
    }
    .to_string();
    TimestampResult {
        input: value,
        format,
        utc,
        unix_seconds,
        confidence,
    }
}

/// Auto-detect plausible formats by value range and return all
/// of them, ranked by descending confidence. The detector errs
/// on the side of including more interpretations — examiners
/// can scan the table and pick.
pub fn detect_and_convert(value: i64) -> Vec<TimestampResult> {
    let mut out: Vec<TimestampResult> = Vec::new();
    let mut try_format = |fmt: TimestampFormat, conf: &str| {
        let mut r = convert(value, fmt);
        if !r.utc.is_empty() {
            r.confidence = conf.to_string();
            out.push(r);
        }
    };

    // Magnitude-based bands. The bands overlap on purpose — the
    // CLI prints them all so the examiner can pick by context.
    let abs = value.unsigned_abs();
    if abs >= 1_000_000_000_000_000_000 {
        try_format(TimestampFormat::UnixNanoseconds, "High");
        try_format(TimestampFormat::AppleNanoseconds, "Low");
    } else if abs >= 100_000_000_000_000_000 {
        // Windows FILETIME for years near 2025: 100-ns intervals
        // since 1601 sit at ≈ 1.3e17.
        try_format(TimestampFormat::WindowsFiletime, "High");
        try_format(TimestampFormat::UnixNanoseconds, "Low");
    } else if abs >= 1_000_000_000_000_000 {
        try_format(TimestampFormat::UnixMicroseconds, "High");
        try_format(TimestampFormat::WebKit, "Medium");
    } else if abs >= 1_000_000_000_000 {
        try_format(TimestampFormat::UnixMilliseconds, "High");
        try_format(TimestampFormat::WebKit, "Low");
    } else if abs >= 1_000_000_000 {
        // Modern Unix seconds (post-2001) — about 1.7e9 today.
        try_format(TimestampFormat::UnixSeconds, "High");
        // Hfs+ epoch is 1904 → 2025 ≈ 3.8e9; also plausible.
        try_format(TimestampFormat::HfsPlus, "Medium");
        try_format(TimestampFormat::AppleCoreData, "Low");
    } else if abs >= 100_000_000 {
        try_format(TimestampFormat::UnixSeconds, "High");
        try_format(TimestampFormat::AppleCoreData, "Medium");
    } else {
        try_format(TimestampFormat::AppleCoreData, "Medium");
        try_format(TimestampFormat::UnixSeconds, "Low");
    }
    out
}

/// Parse "<value> [label]" lines from a file, one timestamp per
/// line. `#`-prefixed lines and blank lines are ignored.
pub fn parse_input_file(body: &str) -> Result<Vec<(i64, Option<String>)>, AugurError> {
    let mut out = Vec::new();
    for (lineno, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let raw_val = parts.next().unwrap_or("");
        let label = parts.next().map(|s| s.trim().to_string());
        let val: i64 = raw_val.parse().map_err(|e| {
            AugurError::InvalidInput(format!(
                "line {}: cannot parse {raw_val:?} as i64: {e}",
                lineno + 1
            ))
        })?;
        out.push((val, label));
    }
    Ok(out)
}

// ── ISO-8601 formatting (no chrono dep) ──────────────────────────

fn format_iso8601_utc(unix_secs: i64) -> Option<(i64, String)> {
    // Year range we support: 0001..=9999. Outside that we return
    // None and the caller flags the conversion as "out of range."
    // 0001-01-01T00:00:00Z is unix_secs = -62_135_596_800.
    // 9999-12-31T23:59:59Z is unix_secs =  253_402_300_799.
    if !(-62_135_596_800..=253_402_300_799).contains(&unix_secs) {
        return None;
    }
    let (y, mo, d, h, mi, s) = epoch_to_ymdhms(unix_secs);
    Some((
        unix_secs,
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"),
    ))
}

/// Public Civil-date helper — Howard Hinnant's algorithm (PD).
/// Used by both this module and `apps/augur-cli/src/main.rs`'s
/// batch progress-file timestamps.
pub fn epoch_to_ymdhms(unix_secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let s_total = unix_secs.rem_euclid(86_400) as u64;
    let s = (s_total % 60) as u32;
    let mins = s_total / 60;
    let mi = (mins % 60) as u32;
    let h = (mins / 60) as u32;
    let mut days: i64 = unix_secs.div_euclid(86_400);
    days += 719_468;
    let era = days.div_euclid(146_097);
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_seconds_converts_correctly() {
        let r = convert(0, TimestampFormat::UnixSeconds);
        assert_eq!(r.utc, "1970-01-01T00:00:00Z");
        let r = convert(1_762_276_748, TimestampFormat::UnixSeconds);
        // 2025-11-04T17:19:08Z
        assert_eq!(r.utc, "2025-11-04T17:19:08Z");
        assert_eq!(r.unix_seconds, 1_762_276_748);
    }

    #[test]
    fn unix_milliseconds_converts_correctly() {
        let r = convert(1_762_276_748_000, TimestampFormat::UnixMilliseconds);
        assert_eq!(r.utc, "2025-11-04T17:19:08Z");
    }

    #[test]
    fn apple_coredata_converts_correctly() {
        // 0 → 2001-01-01 reference epoch.
        let r = convert(0, TimestampFormat::AppleCoreData);
        assert_eq!(r.utc, "2001-01-01T00:00:00Z");
        // Cocoa is the same epoch.
        let r2 = convert(0, TimestampFormat::CocoaDate);
        assert_eq!(r2.utc, "2001-01-01T00:00:00Z");
    }

    #[test]
    fn windows_filetime_converts_correctly() {
        // 116_444_736_000_000_000 = 1970-01-01T00:00:00Z exactly.
        let r = convert(116_444_736_000_000_000, TimestampFormat::WindowsFiletime);
        assert_eq!(r.utc, "1970-01-01T00:00:00Z");
        assert_eq!(r.unix_seconds, 0);
    }

    #[test]
    fn webkit_converts_correctly() {
        // WebKit microseconds since 1601-01-01.
        // 11_644_473_600_000_000 = 1970-01-01T00:00:00Z.
        let r = convert(11_644_473_600_000_000, TimestampFormat::WebKit);
        assert_eq!(r.utc, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn hfs_plus_converts_correctly() {
        // HFS+ seconds since 1904-01-01.
        // 2_082_844_800 = 1970-01-01T00:00:00Z.
        let r = convert(2_082_844_800, TimestampFormat::HfsPlus);
        assert_eq!(r.utc, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn auto_detection_returns_multiple_interpretations() {
        let interpretations = detect_and_convert(1_762_276_748);
        assert!(
            interpretations.len() >= 2,
            "expected ≥2 plausible formats; got {interpretations:?}"
        );
        // Unix seconds should always be present and ranked High
        // for a value in the 10-digit band.
        let unix = interpretations
            .iter()
            .find(|r| matches!(r.format, TimestampFormat::UnixSeconds))
            .expect("Unix seconds interpretation");
        assert_eq!(unix.confidence, "High");
        assert_eq!(unix.utc, "2025-11-04T17:19:08Z");
    }

    #[test]
    fn parse_input_file_handles_labels_and_comments() {
        let body = "
            # forensic timeline notes — Sprint 7 P3 example
            1762276748 message_sent
            1762276751 message_read

            978307200  apple_epoch_reference
        ";
        let parsed = parse_input_file(body).expect("parse");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], (1_762_276_748, Some("message_sent".into())));
        assert_eq!(parsed[1], (1_762_276_751, Some("message_read".into())));
        assert_eq!(parsed[2], (978_307_200, Some("apple_epoch_reference".into())));
    }

    #[test]
    fn out_of_range_values_return_low_confidence_no_utc() {
        // i64::MAX seconds = year 292_277_026_596 — well out of
        // our 0001..9999 range. Conversion must NOT panic.
        let r = convert(i64::MAX, TimestampFormat::UnixSeconds);
        assert_eq!(r.utc, "");
        assert_eq!(r.confidence, "Low");
    }

    #[test]
    fn format_string_round_trip() {
        for fmt in [
            TimestampFormat::UnixSeconds,
            TimestampFormat::WindowsFiletime,
            TimestampFormat::WebKit,
            TimestampFormat::HfsPlus,
            TimestampFormat::AppleCoreData,
        ] {
            assert_eq!(TimestampFormat::from_str(fmt.as_str()), Some(fmt));
        }
        assert!(TimestampFormat::from_str("not-a-format").is_none());
    }
}
