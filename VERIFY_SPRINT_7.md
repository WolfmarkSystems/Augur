# VERIFY Sprint 7 — IP Geolocation + Report Customization + SQLite Viewer
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/verify/_

---

## Before starting

1. Read CLAUDE.md completely
2. Run `cargo test --workspace 2>&1 | tail -5`
3. Confirm 81 tests passing before any changes

---

## Hard rules (absolute)

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- All errors handled explicitly
- `cargo clippy --workspace -- -D warnings` clean
- `cargo test --workspace` passes after every change
- Offline invariant maintained
- MT advisory always present

---

## PRIORITY 1 — IP Geolocation via MaxMind GeoLite2

### Context

Network artifacts extracted from evidence often contain IP addresses.
Examiners need to know the geographic origin of those IPs. MaxMind's
GeoLite2 is the standard offline geolocation database used by the
forensic community — free, accurate, offline, and widely deployed.

### Implementation

**Step 1 — MaxMind GeoLite2 integration**

Add to `Cargo.toml`:
```toml
maxminddb = "0.23"  # pure Rust MaxMind DB reader
```

**Step 2 — GeoIP engine**

Create `crates/verify-core/src/geoip.rs`:

```rust
use maxminddb::geoip2;
use std::net::IpAddr;
use std::path::Path;

pub struct GeoIpEngine {
    reader: maxminddb::Reader<Vec<u8>>,
}

pub struct GeoIpResult {
    pub ip: String,
    pub country_code: Option<String>,    // "US", "RU", "CN"
    pub country_name: Option<String>,    // "United States"
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub is_private: bool,               // RFC 1918 / loopback
    pub asn: Option<u32>,               // Autonomous System Number
    pub org: Option<String>,            // "AS15169 Google LLC"
}

impl GeoIpEngine {
    pub fn load(db_path: &Path) -> Result<Self, VerifyError>;
    
    pub fn lookup(&self, ip: &str) -> Result<GeoIpResult, VerifyError>;
    
    /// Check if an IP is private/loopback (RFC 1918)
    pub fn is_private(ip: &str) -> bool;
}
```

**Step 3 — Database download**

GeoLite2 requires a free MaxMind account for download. Add to
`ModelManager` pattern:

```rust
// GEOIP_DB_URL is intentionally empty — requires MaxMind account
// Examiners must download manually or use VERIFY_GEOIP_PATH env var
pub const GEOIP_DB_FILENAME: &str = "GeoLite2-City.mmdb";
pub const GEOIP_DB_INSTRUCTIONS: &str =
    "Download GeoLite2-City.mmdb from https://dev.maxmind.com/geoip/geolite2-free-geolocation-data \
     (free account required) and set VERIFY_GEOIP_PATH=/path/to/GeoLite2-City.mmdb";
```

This is the one VERIFY feature that cannot auto-download due to
MaxMind's license requirement. Handle gracefully:
- If `VERIFY_GEOIP_PATH` set → use that path
- If `~/.cache/verify/GeoLite2-City.mmdb` exists → use it
- Otherwise → return `VerifyError::GeoIpNotConfigured` with instructions

**Step 4 — CLI integration**

```bash
# Geolocate a single IP
verify geoip 8.8.8.8

# Geolocate IPs from a file (one per line)
verify geoip --input ips.txt

# Geolocate IPs extracted from a batch result
verify batch --input /evidence --target en --geoip
```

Output:
```
[VERIFY] GeoIP: 8.8.8.8
  Country: United States (US)
  City: Mountain View, CA
  Coords: 37.3860, -122.0838
  ASN: AS15169 Google LLC
  Private: No
```

**Step 5 — Wire into self-test**

Add a GeoIP check to `verify self-test`:
```
✓ [PASS] GeoIP: database configured at ~/.cache/verify/GeoLite2-City.mmdb
  OR
⚠ [SKIP] GeoIP: VERIFY_GEOIP_PATH not set, database not found
         Run: verify geoip --setup for instructions
```

**Step 6 — Add `verify geoip --setup` subcommand**

Prints the MaxMind download instructions with the exact path to
place the database file.

### Tests

```rust
#[test]
fn private_ip_detected_as_private() {
    // 192.168.1.1, 10.0.0.1, 127.0.0.1
    // is_private() returns true
}

#[test]
fn geoip_not_configured_returns_clear_error() {
    // No database configured
    // Returns VerifyError::GeoIpNotConfigured, not panic
}

#[test]
fn geoip_result_has_all_fields() {
    // Synthetic result struct construction
    // All fields present and typed correctly
}
```

### Acceptance criteria — P1

- [ ] `maxminddb` crate integrated
- [ ] `GeoIpEngine` loads `.mmdb` database
- [ ] Private IP detection working (RFC 1918)
- [ ] Clear error when database not configured
- [ ] `verify geoip` CLI subcommand works
- [ ] `verify geoip --setup` prints instructions
- [ ] GeoIP check in `verify self-test`
- [ ] 3 new tests pass
- [ ] Offline invariant maintained (no automatic DB download)
- [ ] Clippy clean

---

## PRIORITY 2 — Batch Report Customization

### Context

Sprint 3 shipped batch JSON/CSV reports. Sprint 6 added confidence
tiers and progress tracking. Forensic agencies need reports that
include their agency name, case number, examiner signature block,
and classification markings.

### Implementation

**Step 2 — Report header configuration**

```rust
pub struct ReportConfig {
    pub agency_name: Option<String>,
    pub case_number: Option<String>,
    pub examiner_name: Option<String>,
    pub examiner_badge: Option<String>,
    pub classification: Option<String>, // "UNCLASSIFIED", "CUI", etc
    pub report_title: Option<String>,
    pub logo_path: Option<PathBuf>,     // path to agency logo (PNG)
    pub include_mt_advisory: bool,      // always true, non-overridable
    pub include_confidence_tiers: bool, // default true
}
```

**Step 2 — Config file support**

```bash
# Create a config file
verify config init --output ~/.verify_report.toml

# Use config in batch
verify batch --input /evidence --target en \
    --config ~/.verify_report.toml --output report.json
```

Config file format (TOML):
```toml
[report]
agency_name = "Wolfmark Systems"
case_number = "2026-001"
examiner_name = "D. Examiner"
examiner_badge = "12345"
classification = "UNCLASSIFIED // FOR OFFICIAL USE ONLY"
report_title = "VERIFY Foreign Language Analysis Report"

[output]
include_confidence_tiers = true
include_language_limitations = true  # appends LANGUAGE_LIMITATIONS.md content
```

**Step 3 — Apply to JSON report output**

JSON report header becomes:

```json
{
  "report_metadata": {
    "agency": "Wolfmark Systems",
    "case_number": "2026-001",
    "examiner": "D. Examiner",
    "badge": "12345",
    "classification": "UNCLASSIFIED // FOR OFFICIAL USE ONLY",
    "generated_at": "2026-04-26T08:15:32Z",
    "verify_version": "1.0.0"
  },
  "machine_translation_notice": "...",  // always present
  "summary": { ... },
  "results": [ ... ]
}
```

**Step 4 — `verify config` subcommand**

```bash
verify config init           # create default config file
verify config show           # display current config
verify config set key value  # set a single config value
```

**Step 5 — HTML report option**

Add `--format html` to batch command. Generates a self-contained
HTML report with:
- Agency header with name and classification marking
- Summary statistics table
- Per-file results table with confidence tiers
- MT advisory in red at the top and bottom
- Pashto/Persian disambiguation notice if fa detected

HTML is self-contained (no external dependencies) so it can be
emailed or printed.

### Tests

```rust
#[test]
fn report_config_loads_from_toml() {
    // Write a TOML config, load it, verify fields match
}

#[test]
fn json_report_includes_metadata_when_configured() {
    // Configure agency name, verify in JSON output
}

#[test]
fn html_report_contains_mt_advisory() {
    // Generate HTML report, verify MT advisory present
    // Even with all other config options, advisory cannot be suppressed
}
```

### Acceptance criteria — P2

- [ ] `ReportConfig` struct with all header fields
- [ ] TOML config file support
- [ ] `verify config init/show/set` subcommands
- [ ] JSON report includes metadata header when configured
- [ ] HTML report format working with agency branding
- [ ] MT advisory always present in all formats
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 3 — Timestamp Conversion Utility

### Context

Evidence databases contain timestamps in many formats: Unix
(seconds/milliseconds/nanoseconds), Apple Core Data epoch,
Windows FILETIME, WebKit epoch, and others. Examiners frequently
need to convert timestamps manually to verify artifact dates.

This is a utility feature that makes VERIFY genuinely useful for
day-to-day examiner work beyond just translation.

### Implementation

**Step 1 — Timestamp converter**

Create `crates/verify-core/src/timestamps.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TimestampFormat {
    UnixSeconds,           // 1700000000
    UnixMilliseconds,      // 1700000000000
    UnixMicroseconds,      // 1700000000000000
    UnixNanoseconds,       // 1700000000000000000
    AppleCoreData,         // seconds since 2001-01-01
    AppleNanoseconds,      // nanoseconds since 2001-01-01
    WindowsFiletime,       // 100ns intervals since 1601-01-01
    WebKit,                // microseconds since 1601-01-01
    HfsPlus,               // seconds since 1904-01-01
    CochraneCocoaDate,     // same as AppleCoreData
}

pub struct TimestampResult {
    pub input: i64,
    pub format: TimestampFormat,
    pub utc: String,        // "2023-11-14 22:13:20 UTC"
    pub unix_seconds: i64,  // canonical Unix timestamp
    pub confidence: String, // "High" / "Medium" — some formats overlap
}

pub fn detect_and_convert(value: i64) -> Vec<TimestampResult>;
pub fn convert(value: i64, format: TimestampFormat) -> TimestampResult;
```

**Step 2 — Auto-detection**

When given a raw integer, detect likely format by value range:
- > 1_000_000_000_000_000_000 → nanoseconds (Unix or Apple)
- > 1_000_000_000_000_000   → microseconds
- > 1_000_000_000_000       → milliseconds
- > 1_000_000_000           → seconds (Unix — post 2001)
- > 978_307_200             → could be Apple Core Data OR late Unix
- etc.

Return all plausible interpretations with confidence.

**Step 3 — CLI subcommand**

```bash
verify timestamp 1762276748
```

Output:
```
[VERIFY] Timestamp analysis: 1762276748

Format              Interpretation              UTC
──────────────────────────────────────────────────────
Unix seconds        HIGH confidence    2025-11-04 17:19:08 UTC
Apple CoreData      MEDIUM confidence  2057-10-26 12:06:28 UTC  ← unlikely
Windows FILETIME    LOW confidence     (value too small)
```

```bash
verify timestamp 1762276748 --format unix-seconds
```

Output:
```
[VERIFY] 1762276748 (Unix seconds) = 2025-11-04 17:19:08 UTC
```

**Step 4 — Multi-format batch**

```bash
verify timestamp --input timestamps.txt
```

File: one timestamp per line, optional label:
```
1762276748 message_sent
1762276751 message_read
978307200  apple_epoch_reference
```

**Step 5 — Tests**

```rust
#[test]
fn unix_seconds_converts_correctly() {
    // Known timestamp → known UTC string
    // 0 → 1970-01-01 00:00:00 UTC
}

#[test]
fn apple_coredata_converts_correctly() {
    // 0 → 2001-01-01 00:00:00 UTC
    // 978307200 → 2032-01-01... wait, no
    // Document exact expected value in test
}

#[test]
fn windows_filetime_converts_correctly() {
    // 116444736000000000 → 1970-01-01 00:00:00 UTC
    // (FILETIME epoch to Unix epoch reference point)
}

#[test]
fn auto_detection_returns_multiple_interpretations() {
    // Single value → multiple TimestampResult entries
    // At least Unix and Apple interpretations
}
```

### Acceptance criteria — P3

- [ ] All 7 timestamp formats implemented
- [ ] Auto-detection returns plausible interpretations
- [ ] `verify timestamp` CLI subcommand working
- [ ] Multi-format batch from file works
- [ ] 4 new tests pass (including the three reference conversions)
- [ ] Clippy clean

---

## After all priorities complete

```bash
cargo test --workspace 2>&1 | grep "test result" | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

Commit:
```bash
git add -A
git commit -m "feat: verify-sprint-7 GeoIP + report customization + timestamp converter"
```

Report:
- Which priorities passed
- Test count before (81) and after
- Output of `verify timestamp 1762276748`
- Output of `verify geoip --setup`
- Any deviations from spec

---

_VERIFY Sprint 7 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/verify/_
_Three utility features that make VERIFY genuinely useful_
_for day-to-day examiner work beyond translation._
