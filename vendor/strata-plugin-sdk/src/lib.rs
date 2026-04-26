use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

// Re-export the VirtualFilesystem trait from strata-fs so plugins
// can pull it from the SDK.
pub use strata_fs::vfs::{VirtualFilesystem, WalkDecision};

/// Plugin API version
pub const PLUGIN_API_VERSION: &str = "0.3.0";

/// The standard structure for a forensic artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub category: String,
    pub timestamp: Option<u64>,
    pub source: String,
    pub data: HashMap<String, String>,
}

impl Artifact {
    pub fn new(category: &str, source: &str) -> Self {
        Self {
            category: category.to_string(),
            timestamp: None,
            source: source.to_string(),
            data: HashMap::new(),
        }
    }

    pub fn add_field(&mut self, key: &str, value: &str) {
        self.data.insert(key.to_string(), value.to_string());
    }
}

/// The context provided to each plugin during execution.
#[derive(Clone)]
pub struct PluginContext {
    /// Legacy root path (host filesystem for directory sources).
    pub root_path: String,
    /// Optional mounted VFS when evidence is an E01/Raw/VMDK/VHD.
    /// When present, the helper methods route through it; when
    /// `None`, they fall back to `root_path` on the host filesystem.
    pub vfs: Option<Arc<dyn VirtualFilesystem>>,
    pub config: HashMap<String, String>,
    /// Results from previously-run plugins (used by Sigma for correlation).
    pub prior_results: Vec<PluginOutput>,
}

impl PluginContext {
    /// Resolve a logical path against `root_path`. VFS-backed
    /// contexts bypass `resolve` — callers use `read_file`/`list_dir`
    /// directly with the logical path.
    pub fn resolve(&self, path: &str) -> std::path::PathBuf {
        let rel = path.trim_start_matches('/');
        if rel.is_empty() {
            std::path::PathBuf::from(&self.root_path)
        } else {
            std::path::Path::new(&self.root_path).join(rel)
        }
    }

    /// Read a file. Returns None on missing / unreadable targets so
    /// plugins can chain Option methods instead of unwrapping errors.
    /// Routes through the VFS when one is mounted.
    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        if let Some(vfs) = &self.vfs {
            return vfs.read_file(path).ok();
        }
        std::fs::read(self.resolve(path)).ok()
    }

    /// File-or-dir existence check. Routes through the VFS when
    /// mounted, else checks the host filesystem rooted at `root_path`.
    pub fn file_exists(&self, path: &str) -> bool {
        if let Some(vfs) = &self.vfs {
            return vfs.exists(path);
        }
        self.resolve(path).exists()
    }

    /// List the children of a directory. Empty vec on missing /
    /// non-directory paths. Routes through the VFS when mounted.
    pub fn list_dir(&self, path: &str) -> Vec<String> {
        if let Some(vfs) = &self.vfs {
            return vfs
                .list_dir(path)
                .map(|v| v.into_iter().map(|e| e.name).collect())
                .unwrap_or_default();
        }
        let target = self.resolve(path);
        let Ok(entries) = std::fs::read_dir(target) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect()
    }

    /// Depth-bounded case-insensitive recursive search for files
    /// whose leaf name equals `name`. Returns **logical path
    /// strings** (suitable for `read_file`) when VFS-backed, or
    /// **host-filesystem PathBufs** when not.
    ///
    /// Cap of 8 levels keeps plugins from accidentally walking
    /// enormous user-data trees.
    pub fn find_by_name(&self, name: &str) -> Vec<std::path::PathBuf> {
        let needle = name.to_ascii_lowercase();
        if let Some(vfs) = &self.vfs {
            let mut out = Vec::new();
            let mut filter = |e: &strata_fs::vfs::VfsEntry| {
                if !e.is_directory && e.name.to_ascii_lowercase() == needle {
                    out.push(std::path::PathBuf::from(&e.path));
                }
                WalkDecision::Descend
            };
            let _ = vfs.walk(&mut filter);
            return out;
        }
        let mut out = Vec::new();
        let mut stack: Vec<(std::path::PathBuf, u32)> =
            vec![(std::path::PathBuf::from(&self.root_path), 0)];
        while let Some((dir, depth)) = stack.pop() {
            if depth > 8 {
                continue;
            }
            let Ok(iter) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in iter.flatten() {
                let p = entry.path();
                let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                if fname.to_ascii_lowercase() == needle {
                    out.push(p.clone());
                }
                if p.is_dir() {
                    stack.push((p, depth + 1));
                }
            }
        }
        out
    }
}

/// The result returned by a plugin.
pub type PluginResult = Result<Vec<Artifact>, PluginError>;

/// Plugin type classification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginType {
    Carver,
    Analyzer,
    Cipher,
}

/// Minimum license tier required to run a plugin.
///
/// This is **independent** of `strata_license::LicenseTier` to keep
/// the plugin SDK from depending on the license crate (which would
/// pull license logic into every plugin). The strata-tree app maps
/// `LicenseTier` to `PluginTier` at the gating call site in
/// `AppState::run_plugin`.
///
/// `PartialOrd`/`Ord` ordering goes Free < Trial < Professional <
/// Enterprise, so a tier check is `user_tier >= plugin.required_tier()`.
///
/// **Free** is reserved for plugins that must always be available
/// regardless of license — currently only the CSAM Sentinel plugin.
/// Per the v1.4.0 spec: free on every license tier, no gating, ever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PluginTier {
    Free = 0,
    Trial = 1,
    Professional = 2,
    Enterprise = 3,
}

impl PluginTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginTier::Free => "Free",
            PluginTier::Trial => "Trial",
            PluginTier::Professional => "Professional",
            PluginTier::Enterprise => "Enterprise",
        }
    }
}

/// What a plugin can do
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginCapability {
    FileCarving,
    TimelineEnrichment,
    ArtifactExtraction,
    EncryptionAnalysis,
    ExecutionTracking,
    CredentialExtraction,
    NetworkArtifacts,
    DeletedFileRecovery,
}

/// Forensic significance level
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ForensicValue {
    Critical,
    High,
    Medium,
    Low,
    Informational,
}

/// Artifact category for the Artifacts panel
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ArtifactCategory {
    Communications,
    SocialMedia,
    WebActivity,
    UserActivity,
    SystemActivity,
    CloudSync,
    AccountsCredentials,
    Media,
    DeletedRecovered,
    ExecutionHistory,
    NetworkArtifacts,
    EncryptionKeyMaterial,
}

impl ArtifactCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Communications => "Communications",
            Self::SocialMedia => "Social Media",
            Self::WebActivity => "Web Activity",
            Self::UserActivity => "User Activity",
            Self::SystemActivity => "System Activity",
            Self::CloudSync => "Cloud & Sync",
            Self::AccountsCredentials => "Accounts & Credentials",
            Self::Media => "Media",
            Self::DeletedRecovered => "Deleted & Recovered",
            Self::ExecutionHistory => "Execution History",
            Self::NetworkArtifacts => "Network Artifacts",
            Self::EncryptionKeyMaterial => "Encryption Key Material",
        }
    }
}

/// A single parsed artifact record for the Artifacts panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub category: ArtifactCategory,
    pub subcategory: String,
    pub timestamp: Option<i64>,
    pub title: String,
    pub detail: String,
    pub source_path: String,
    pub forensic_value: ForensicValue,
    pub mitre_technique: Option<String>,
    pub is_suspicious: bool,
    pub raw_data: Option<serde_json::Value>,
    /// Confidence score (0-100). Defaults to 0 (unscored) for backward
    /// compatibility. Populated by `compute_confidence` after plugin
    /// execution. High (80+), Medium (50-79), Low (<50).
    #[serde(default)]
    pub confidence: u8,
}

/// Compute confidence score for an artifact record.
///
/// Scoring factors:
/// - Parser certainty: known schema columns present = high base
/// - Data completeness: populated optional fields raise the score
/// - Suspicious flag: suspicious artifacts get a small boost (examiner attention)
pub fn compute_confidence(record: &ArtifactRecord) -> u8 {
    let mut score: u32 = 0;

    // Base: parser produced a titled, categorized record from a known schema
    if !record.title.is_empty() && !record.subcategory.is_empty() {
        score += 40;
    }

    // Source path present (proves provenance)
    if !record.source_path.is_empty() {
        score += 10;
    }

    // Timestamp present (temporal anchor)
    if record.timestamp.is_some() {
        score += 15;
    }

    // Detail field populated with substance (>20 chars)
    if record.detail.len() > 20 {
        score += 10;
    }

    // MITRE technique mapped (structured threat intel)
    if record.mitre_technique.is_some() {
        score += 10;
    }

    // Raw data present (machine-readable backup of the finding)
    if record.raw_data.is_some() {
        score += 10;
    }

    // Forensic value assessed above Low
    match record.forensic_value {
        ForensicValue::Critical => score += 5,
        ForensicValue::High => score += 5,
        _ => {}
    }

    score.min(100) as u8
}

/// Compute and set confidence scores for all artifacts in a plugin output.
pub fn score_plugin_output(output: &mut PluginOutput) {
    for record in &mut output.artifacts {
        record.confidence = compute_confidence(record);
    }
}

/// Compute confidence across multiple plugin outputs with corroboration bonus.
/// Records that appear in multiple plugins (same source_path + same subcategory)
/// get a corroboration boost.
pub fn score_with_corroboration(outputs: &mut [PluginOutput]) {
    use std::collections::HashMap;

    // First pass: score each record individually
    for output in outputs.iter_mut() {
        score_plugin_output(output);
    }

    // Second pass: find corroborated records (same source_path + subcategory across plugins)
    let mut evidence_map: HashMap<(String, String), usize> = HashMap::new();
    for output in outputs.iter() {
        for record in &output.artifacts {
            if record.source_path.is_empty() {
                continue;
            }
            let key = (record.source_path.clone(), record.subcategory.clone());
            *evidence_map.entry(key).or_insert(0) += 1;
        }
    }

    // Third pass: apply corroboration bonus
    for output in outputs.iter_mut() {
        for record in &mut output.artifacts {
            if record.source_path.is_empty() {
                continue;
            }
            let key = (record.source_path.clone(), record.subcategory.clone());
            if let Some(&count) = evidence_map.get(&key) {
                if count >= 2 {
                    record.confidence = (record.confidence as u32 + 10).min(100) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod context_helper_tests {
    use super::*;

    fn ctx_for(root: &std::path::Path) -> PluginContext {
        PluginContext {
            root_path: root.to_string_lossy().into_owned(),
            vfs: None,
            config: Default::default(),
            prior_results: Default::default(),
        }
    }

    #[test]
    fn read_file_returns_none_on_missing() {
        let tmp = tempfile::tempdir().expect("tmp");
        let ctx = ctx_for(tmp.path());
        assert!(ctx.read_file("/does/not/exist").is_none());
    }

    #[test]
    fn read_file_reads_bytes() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(tmp.path().join("a.txt"), b"hello").expect("w");
        let ctx = ctx_for(tmp.path());
        assert_eq!(
            ctx.read_file("/a.txt").as_deref(),
            Some(b"hello".as_slice())
        );
    }

    #[test]
    fn file_exists_positive_and_negative() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(tmp.path().join("x.txt"), b"").expect("w");
        let ctx = ctx_for(tmp.path());
        assert!(ctx.file_exists("/x.txt"));
        assert!(!ctx.file_exists("/nope.txt"));
    }

    #[test]
    fn list_dir_returns_entries() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(tmp.path().join("a.txt"), b"").expect("w");
        std::fs::write(tmp.path().join("b.txt"), b"").expect("w");
        let ctx = ctx_for(tmp.path());
        let mut entries = ctx.list_dir("/");
        entries.sort();
        assert_eq!(entries, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn find_by_name_walks_recursively_case_insensitive() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(tmp.path().join("dir1/dir2")).expect("mk");
        std::fs::write(tmp.path().join("dir1/dir2/SYSTEM"), b"").expect("w");
        let ctx = ctx_for(tmp.path());
        let hits = ctx.find_by_name("system");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].ends_with("SYSTEM"));
    }
}

#[cfg(test)]
mod confidence_tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn make_record(
        title: &str,
        subcategory: &str,
        timestamp: Option<i64>,
        detail: &str,
        source_path: &str,
        mitre: Option<&str>,
        raw_data: bool,
        forensic_value: ForensicValue,
    ) -> ArtifactRecord {
        ArtifactRecord {
            category: ArtifactCategory::SystemActivity,
            subcategory: subcategory.into(),
            timestamp,
            title: title.into(),
            detail: detail.into(),
            source_path: source_path.into(),
            forensic_value,
            mitre_technique: mitre.map(String::from),
            is_suspicious: false,
            raw_data: if raw_data {
                Some(serde_json::json!({"key": "value"}))
            } else {
                None
            },
            confidence: 0,
        }
    }

    #[test]
    fn high_confidence_for_complete_record() {
        let record = make_record(
            "Prefetch Execution",
            "Prefetch",
            Some(1712435400),
            "CCLEANER64.EXE — 3 executions detected in prefetch",
            "/Windows/Prefetch/CCLEANER64.EXE-ABCD1234.pf",
            Some("T1070.004"),
            true,
            ForensicValue::High,
        );
        let score = compute_confidence(&record);
        assert!(score >= 80, "expected high confidence, got {}", score);
    }

    #[test]
    fn low_confidence_for_sparse_record() {
        let record = make_record(
            "",
            "",
            None,
            "some data",
            "",
            None,
            false,
            ForensicValue::Low,
        );
        let score = compute_confidence(&record);
        assert!(score < 50, "expected low confidence, got {}", score);
    }

    #[test]
    fn confidence_capped_at_100() {
        let record = make_record(
            "Full Record",
            "Test Category",
            Some(1712435400),
            "A very detailed description that is longer than 20 chars",
            "/some/path/file.db",
            Some("T1078"),
            true,
            ForensicValue::Critical,
        );
        let score = compute_confidence(&record);
        assert!(score <= 100);
    }

    #[test]
    fn corroboration_boosts_score() {
        let record = make_record(
            "ShimCache Entry",
            "Execution",
            Some(1712435400),
            "cmd.exe found in ShimCache with timestamp",
            "/Windows/System32/cmd.exe",
            None,
            false,
            ForensicValue::Medium,
        );

        let base_score = compute_confidence(&record);

        let mut outputs = vec![
            PluginOutput {
                plugin_name: "Phantom".into(),
                plugin_version: "1.0".into(),
                executed_at: String::new(),
                duration_ms: 0,
                artifacts: vec![record.clone()],
                summary: PluginSummary {
                    total_artifacts: 1,
                    suspicious_count: 0,
                    categories_populated: vec![],
                    headline: String::new(),
                },
                warnings: vec![],
            },
            PluginOutput {
                plugin_name: "Trace".into(),
                plugin_version: "1.0".into(),
                executed_at: String::new(),
                duration_ms: 0,
                artifacts: vec![record],
                summary: PluginSummary {
                    total_artifacts: 1,
                    suspicious_count: 0,
                    categories_populated: vec![],
                    headline: String::new(),
                },
                warnings: vec![],
            },
        ];

        score_with_corroboration(&mut outputs);
        let boosted = outputs[0].artifacts[0].confidence;
        assert!(
            boosted > base_score,
            "corroboration should boost: base={} boosted={}",
            base_score,
            boosted
        );
    }
}

/// Plugin execution summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSummary {
    pub total_artifacts: usize,
    pub suspicious_count: usize,
    pub categories_populated: Vec<String>,
    pub headline: String,
}

/// Rich plugin result with artifacts, timeline events, and summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginOutput {
    pub plugin_name: String,
    pub plugin_version: String,
    pub executed_at: String,
    pub duration_ms: u64,
    pub artifacts: Vec<ArtifactRecord>,
    pub summary: PluginSummary,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum PluginError {
    Internal(String),
    UnsupportedInput(String),
    ExecutionFailed(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::Internal(e) => write!(f, "Internal Error: {}", e),
            PluginError::UnsupportedInput(e) => write!(f, "Unsupported Input: {}", e),
            PluginError::ExecutionFailed(e) => write!(f, "Execution Failed: {}", e),
        }
    }
}

impl std::error::Error for PluginError {}

/// The core trait that all Strata plugins must implement.
pub trait StrataPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn supported_inputs(&self) -> Vec<String>;
    fn plugin_type(&self) -> PluginType;
    fn capabilities(&self) -> Vec<PluginCapability>;
    fn description(&self) -> &str;

    /// Minimum license tier required to run this plugin. The default
    /// is `Professional` — most analyzer/carver plugins are gated.
    /// **Override to `PluginTier::Free`** for plugins that must
    /// always be available regardless of license (currently only the
    /// CSAM Sentinel plugin per the v1.4.0 spec).
    ///
    /// Backward-compatible: existing plugins that don't override
    /// inherit `Professional`, matching their current de facto status.
    fn required_tier(&self) -> PluginTier {
        PluginTier::Professional
    }

    /// Run the plugin with the given context.
    fn run(&self, context: PluginContext) -> PluginResult;

    /// Run the plugin and return rich output with artifact records.
    /// Default implementation wraps run() for backwards compatibility.
    fn execute(&self, context: PluginContext) -> Result<PluginOutput, PluginError> {
        let start = std::time::Instant::now();
        let artifacts = self.run(context)?;

        let mut records = Vec::new();
        for artifact in &artifacts {
            // Sprint-11 P1 — propagate the legacy `data` map into
            // `raw_data` so downstream consumers (engine adapter
            // grouping, conversation view) can read message
            // metadata (handle, thread_originator_guid, was_downgraded,
            // is_from_me, service, …) without every plugin needing to
            // be ported off the legacy Artifact shape. Backwards-
            // compatible: any plugin that doesn't add fields keeps
            // `raw_data: None` because the map is empty.
            let raw_data = if artifact.data.is_empty() {
                None
            } else {
                let json: serde_json::Map<String, serde_json::Value> = artifact
                    .data
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();
                Some(serde_json::Value::Object(json))
            };
            records.push(ArtifactRecord {
                category: ArtifactCategory::UserActivity,
                subcategory: artifact.category.clone(),
                timestamp: artifact.timestamp.map(|t| t as i64),
                title: artifact
                    .data
                    .get("title")
                    .cloned()
                    .unwrap_or_else(|| artifact.source.clone()),
                detail: artifact.data.get("detail").cloned().unwrap_or_default(),
                source_path: artifact.source.clone(),
                forensic_value: ForensicValue::Medium,
                mitre_technique: artifact.data.get("mitre").cloned(),
                is_suspicious: artifact
                    .data
                    .get("suspicious")
                    .map(|v| v == "true")
                    .unwrap_or(false),
                raw_data,
                confidence: 0,
            });
        }

        let suspicious_count = records.iter().filter(|r| r.is_suspicious).count();

        Ok(PluginOutput {
            plugin_name: self.name().to_string(),
            plugin_version: self.version().to_string(),
            executed_at: String::new(),
            duration_ms: start.elapsed().as_millis() as u64,
            artifacts: records,
            summary: PluginSummary {
                total_artifacts: artifacts.len(),
                suspicious_count,
                categories_populated: vec![],
                headline: format!("{}: {} artifacts", self.name(), artifacts.len()),
            },
            warnings: vec![],
        })
    }
}
