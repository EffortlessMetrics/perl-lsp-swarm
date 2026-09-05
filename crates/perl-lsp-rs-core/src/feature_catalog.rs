//! Shared feature catalog parsing and code-generation helpers.
//!
//! Absorbed from `perl-feature-catalog` crate into `perl-lsp-rs-core`
//! as part of Wave Final PR B (#4541). This module centralizes `features.toml`
//! parsing so LSP, DAP, and xtask all consume the same metadata, validation,
//! and rendering behavior.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Default DAP feature identifiers emitted when catalog processing fails.
///
/// `dap.inline_values` is deliberately absent (#9089): the custom inlineValues
/// extension is fail-closed, so a fallback that re-advertised it on catalog
/// failure would contradict the single negotiation authority.
pub const DEFAULT_DAP_FEATURES: &[&str] = &["dap.breakpoints.basic", "dap.core"];

/// Source metadata for the catalog file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Meta {
    /// Canonical release or feature-set version.
    pub version: String,
    /// LSP version this catalog was built against.
    pub lsp_version: String,
    /// Declared aggregate compliance percentage. Refused by [`Catalog::validate`]
    /// since #6731: a declaration-count percentage is not behavior evidence and
    /// must not re-enter the authoritative catalog.
    #[serde(default)]
    pub compliance_percent: Option<u32>,
}

/// Feature maturity state used to record earned claims (#7029).
///
/// Maturity is an evidence-backed claim about a row, independent of the
/// `advertised` runtime fact on the same row. Only [`Maturity::Proven`] may be
/// claimed from qualifying behavior evidence; every other value records an
/// explicit weaker state so later PRs can earn promotion without silently
/// inheriting green.
#[derive(
    Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub enum Maturity {
    /// Claimed and backed by qualifying behavior evidence plus complete
    /// ownership metadata (validated by [`Catalog::validate`]).
    #[serde(rename = "proven", alias = "ga", alias = "production")]
    Proven,
    /// Working surface with partial evidence or bounded claims.
    #[serde(rename = "preview")]
    Preview,
    /// Acknowledged work item that is not implemented.
    #[serde(rename = "planned")]
    Planned,
    /// Explicitly not implemented; must never advertise.
    #[serde(rename = "unsupported")]
    Unsupported,
    /// Fail-closed default: available evidence does not qualify the row for
    /// any stronger claim (#7029).
    #[serde(rename = "not_proven", alias = "experimental")]
    NotProven,
}

impl Maturity {
    /// Pre-#7029 spelling of [`Maturity::Proven`].
    #[deprecated(note = "#7029 renamed catalog maturity to `proven`; use Maturity::Proven")]
    #[allow(non_upper_case_globals)]
    pub const Ga: Self = Self::Proven;
    /// Pre-#7029 spelling of [`Maturity::Proven`].
    #[deprecated(note = "#7029 merged `production` into `proven`; use Maturity::Proven")]
    #[allow(non_upper_case_globals)]
    pub const Production: Self = Self::Proven;
    /// Pre-#7029 spelling now recorded as [`Maturity::NotProven`].
    #[deprecated(note = "#7029 records unevidenced rows as `not_proven`; use Maturity::NotProven")]
    #[allow(non_upper_case_globals)]
    pub const Experimental: Self = Self::NotProven;

    /// Returns `true` when this maturity may back a runtime advertisement.
    ///
    /// Advertisement is primarily the per-row `advertised` fact; planned and
    /// unsupported rows can never advertise even if mis-flagged (#7029).
    pub const fn may_advertise(self) -> bool {
        !matches!(self, Self::Planned | Self::Unsupported)
    }

    /// Returns `true` when the feature participates in the compatibility grid.
    pub const fn is_trackable(self) -> bool {
        !matches!(self, Self::Planned)
    }

    /// Human-readable lowercase label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Preview => "preview",
            Self::Planned => "planned",
            Self::Unsupported => "unsupported",
            Self::NotProven => "not_proven",
        }
    }
}

/// One recorded evidence citation for a feature row (#7029).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct EvidenceEntry {
    /// Evidence class (for example `integration_test`). Classes are classified
    /// qualifying or non-qualifying by the catalog's `[evidence_classes]`.
    pub class: String,
    /// Citation identifying the evidence (usually a repo-relative test path).
    pub id: String,
}

/// Per-feature catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Feature {
    /// Canonical feature identifier (for example: `lsp.completion`).
    pub id: String,
    /// Spec reference string (for example: `LSP 3.18`).
    #[serde(default)]
    pub spec: String,
    /// Area bucket (`text_document`, `workspace`, `debug`, etc.).
    #[serde(default)]
    pub area: String,
    /// Minimum-evidence policy class used for promotion decisions (#7029).
    #[serde(default)]
    pub policy_class: String,
    /// Earned-claim maturity state; independent of [`Feature::advertised`].
    pub maturity: Maturity,
    /// Whether this feature is advertised/visible to clients (runtime fact).
    #[serde(default)]
    pub advertised: bool,
    /// Wire direction (`client_to_server`, `server_to_client`,
    /// `bidirectional`); recorded as `missing` when unverified.
    #[serde(default)]
    pub direction: String,
    /// Capability field gating the feature, or `none`; `missing` when
    /// unverified.
    #[serde(default)]
    pub capability_gate: String,
    /// Advertisement/registration route (`static_capabilities`,
    /// `dynamic_registration`, `client_capability_gated`); `missing` when
    /// unverified.
    #[serde(default)]
    pub registration: String,
    /// Source path owning the implementation; literal `missing` when
    /// unverified (#7029 records missing rather than guessing).
    #[serde(default)]
    pub implementation_owner: String,
    /// Owner of retained cross-request state; literal `missing` when
    /// unverified.
    #[serde(default)]
    pub state_owner: String,
    /// Test cases historically associated with the feature. Presence is not
    /// behavior evidence on its own (#7029).
    #[serde(default)]
    pub tests: Vec<String>,
    /// Classified evidence citations. Only qualifying classes may promote a
    /// row to [`Maturity::Proven`].
    #[serde(default)]
    pub evidence: Vec<EvidenceEntry>,
    /// Known limitations of the claim or implementation.
    #[serde(default)]
    pub limitations: Vec<String>,
    /// Explicit non-claims boundary; required for `proven` rows.
    #[serde(default)]
    pub claim_boundary: String,
    /// Include this feature in coverage/compliance accounting.
    #[serde(default = "default_counts_in_coverage")]
    pub counts_in_coverage: bool,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
}

const fn default_counts_in_coverage() -> bool {
    true
}

/// Full catalog loaded from `features.toml`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Catalog {
    /// Shared metadata section.
    pub meta: Meta,
    /// Ordered feature rows.
    pub feature: Vec<Feature>,
    /// Minimum-evidence policy per feature class (#7029). Maps a
    /// `policy_class` to the evidence classes that qualify a row of that
    /// class for `proven`.
    #[serde(default)]
    pub evidence_policy: BTreeMap<String, Vec<String>>,
    /// Evidence-class taxonomy (#7029). Maps a class name to its qualifier:
    /// values starting with `non_qualifying` can never promote a row.
    #[serde(default)]
    pub evidence_classes: BTreeMap<String, String>,
}

impl Catalog {
    /// All features in declaration order.
    pub fn features(&self) -> &[Feature] {
        &self.feature
    }

    /// IDs for advertised trackable features (#7029).
    ///
    /// Advertisement is the per-row `advertised` runtime fact, gated only by
    /// maturities that can never advertise (planned/unsupported). Earned-claim
    /// maturity deliberately does NOT gate this list: downgrading a claim must
    /// not silently remove a working capability from clients.
    pub fn advertised_feature_ids(&self) -> Vec<&str> {
        let mut ids = self
            .feature
            .iter()
            .filter(|feature| feature.advertised && feature.maturity.may_advertise())
            .map(|feature| feature.id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    /// IDs for all features in a specific area.
    pub fn area_feature_ids(&self, area: &str) -> Vec<&str> {
        let mut ids: Vec<&str> = self
            .feature
            .iter()
            .filter(|feature| feature.area == area)
            .map(|feature| feature.id.as_str())
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Trackable feature count for BDD/compliance grids.
    /// Excludes entries explicitly marked `counts_in_coverage = false`.
    pub fn trackable_feature_count_for_grid(&self) -> usize {
        self.feature
            .iter()
            .filter(|feature| feature.maturity.is_trackable() && feature.counts_in_coverage)
            .count()
    }

    /// Advertised trackable count for BDD/compliance grids.
    /// Excludes entries explicitly marked `counts_in_coverage = false`.
    pub fn advertised_trackable_count_for_grid(&self) -> usize {
        self.feature
            .iter()
            .filter(|feature| {
                feature.advertised && feature.maturity.may_advertise() && feature.counts_in_coverage
            })
            .count()
    }

    /// Compliance percentage for BDD/compliance grids.
    pub fn compliance_percent_for_grid(&self) -> f32 {
        let trackable = self.trackable_feature_count_for_grid();
        if trackable == 0 {
            return 0.0;
        }
        let advertised = self.advertised_trackable_count_for_grid();
        (advertised as f64 / trackable as f64 * 100.0).round() as f32
    }

    /// Compatibility-only alias for [`Self::trackable_feature_count_for_grid`].
    ///
    /// This name is retained for existing catalog consumers. It is not a
    /// compliance, status, or reporting authority.
    #[deprecated(note = "compatibility-only; use trackable_feature_count_for_grid")]
    pub fn trackable_feature_count(&self) -> usize {
        self.feature.iter().filter(|feature| feature.maturity.is_trackable()).count()
    }

    /// Compatibility-only alias for [`Self::advertised_trackable_count_for_grid`].
    ///
    /// This name is retained for existing catalog consumers. It is not a
    /// compliance, status, or reporting authority.
    #[deprecated(note = "compatibility-only; use advertised_trackable_count_for_grid")]
    pub fn advertised_trackable_count(&self) -> usize {
        self.feature
            .iter()
            .filter(|feature| feature.advertised && feature.maturity.may_advertise())
            .count()
    }

    /// Pre-#6731 compatibility percentage using the compatibility count aliases.
    ///
    /// This name is retained for existing catalog consumers. It is not a
    /// compliance, status, or reporting authority.
    #[deprecated(note = "compatibility-only; use compliance_percent_for_grid")]
    #[allow(deprecated)]
    pub fn compliance_percent(&self) -> f32 {
        let trackable = self.trackable_feature_count();
        if trackable == 0 {
            return 0.0;
        }
        let advertised = self.advertised_trackable_count();
        (advertised as f64 / trackable as f64 * 100.0).round() as f32
    }

    /// Per-area statistics useful for documentation and reporting.
    pub fn area_statistics(&self) -> BTreeMap<String, AreaStats> {
        let mut stats: BTreeMap<String, AreaStats> = BTreeMap::new();

        for feature in &self.feature {
            let entry = stats.entry(feature.area.clone()).or_default();
            entry.total += 1;
            if feature.advertised {
                entry.advertised += 1;
            }

            match feature.maturity {
                Maturity::Proven => entry.proven += 1,
                Maturity::Preview => entry.preview += 1,
                Maturity::NotProven => entry.not_proven += 1,
                Maturity::Planned => entry.planned += 1,
                Maturity::Unsupported => entry.unsupported += 1,
            }
        }

        stats
    }

    /// Validate constraints not captured by serde parsing alone.
    ///
    /// #7029 fail-closed rules: a row may hold `maturity = "proven"` only when
    /// it carries at least one qualifying evidence citation for its policy
    /// class AND complete ownership metadata. Advertisement, helper presence,
    /// method counts, file names, and non-qualifying citations can never
    /// promote a row.
    pub fn validate(&self) -> Result<(), CatalogError> {
        let mut seen = BTreeSet::new();
        let mut issues = Vec::new();

        if self.meta.compliance_percent.is_some() {
            issues.push(
                "meta.compliance_percent is refused (#6731): a declaration-count aggregate \
                 is not behavior evidence; generated status renders evidence state instead"
                    .to_string(),
            );
        }

        let known_classes: BTreeSet<&str> =
            self.evidence_classes.keys().map(String::as_str).collect();

        for feature in &self.feature {
            if feature.id.trim().is_empty() {
                issues.push("feature id must not be empty".to_string());
                continue;
            }
            if !seen.insert(&feature.id) {
                issues.push(format!("duplicate feature id: {}", feature.id));
            }

            if feature.advertised && !feature.maturity.may_advertise() {
                issues.push(format!(
                    "feature {} is advertised but maturity {} can never advertise (#7029)",
                    feature.id,
                    feature.maturity.label()
                ));
            }

            if matches!(feature.maturity, Maturity::Proven) {
                if feature.policy_class.is_empty() {
                    issues.push(format!(
                        "proven feature {} must declare its policy_class (#7029)",
                        feature.id
                    ));
                } else {
                    let required = self.evidence_policy.get(feature.policy_class.as_str());
                    match required {
                        None => issues.push(format!(
                            "proven feature {} names unknown policy_class {:?} (#7029)",
                            feature.id, feature.policy_class
                        )),
                        Some(required) => {
                            let has_qualifying = feature.evidence.iter().any(|entry| {
                                required.contains(&entry.class)
                                    && !self.evidence_non_qualifying(&entry.class)
                            });
                            if !has_qualifying {
                                issues.push(format!(
                                    "proven feature {} lacks a qualifying {:?} evidence entry \
                                     for policy_class {:?} (#7029)",
                                    feature.id, required, feature.policy_class
                                ));
                            }
                        }
                    }
                }

                for (field, value) in [
                    ("direction", &feature.direction),
                    ("capability_gate", &feature.capability_gate),
                    ("registration", &feature.registration),
                    ("implementation_owner", &feature.implementation_owner),
                    ("state_owner", &feature.state_owner),
                ] {
                    if value.is_empty() || value == "missing" {
                        issues.push(format!(
                            "proven feature {} must record {field}; found {value:?} (#7029)",
                            feature.id
                        ));
                    }
                }

                if feature.claim_boundary.trim().is_empty() {
                    issues.push(format!(
                        "proven feature {} must state its claim_boundary (#7029)",
                        feature.id
                    ));
                }

                for entry in &feature.evidence {
                    if !known_classes.contains(entry.class.as_str()) {
                        issues.push(format!(
                            "proven feature {} cites unknown evidence class {:?} (#7029)",
                            feature.id, entry.class
                        ));
                    }
                }
            }
        }

        if issues.is_empty() { Ok(()) } else { Err(CatalogError::Validation(issues.join(", "))) }
    }

    fn evidence_non_qualifying(&self, class: &str) -> bool {
        self.evidence_classes
            .get(class)
            .is_some_and(|qualifier| qualifier.starts_with("non_qualifying"))
    }
}

/// Aggregate area-level information.
#[derive(Debug, Default, Clone, Copy)]
pub struct AreaStats {
    /// Total number of rows in the area.
    pub total: usize,
    /// Advertised row count in the area.
    pub advertised: usize,
    /// Proven count (earned claims).
    pub proven: usize,
    /// Preview count.
    pub preview: usize,
    /// Not-proven count (fail-closed baseline).
    pub not_proven: usize,
    /// Planned count.
    pub planned: usize,
    /// Unsupported count.
    pub unsupported: usize,
}

impl AreaStats {
    /// Number of rows eligible for trackability.
    pub const fn trackable(&self) -> usize {
        self.total - self.planned
    }

    /// Advertised ratio in percent for this area.
    pub fn coverage_percent(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.advertised as f64 / self.total as f64) * 100.0).round() as u32
    }

    /// Advertised ratio for trackable features.
    pub fn trackable_coverage_percent(&self) -> u32 {
        let trackable = self.trackable();
        if trackable == 0 {
            return 0;
        }
        ((self.advertised as f64 / trackable as f64) * 100.0).round() as u32
    }
}

/// Error type used by catalog operations.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// Missing catalog source file on the expected paths.
    #[error("features catalog not found for manifest dir: {0}")]
    MissingSource(PathBuf),

    /// An explicitly configured override path does not exist.
    #[error("FEATURES_TOML_OVERRIDE path does not exist: {0}")]
    MissingOverride(PathBuf),

    /// I/O failure while reading the catalog source.
    #[error("failed to read features catalog: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parser error.
    #[error("failed to parse features catalog: {0}")]
    Parse(#[from] toml::de::Error),

    /// Validation failure after deserialization.
    #[error("invalid features catalog: {0}")]
    Validation(String),
}

impl perl_parser_core::ErrorClass for CatalogError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        match self {
            // File system / infrastructure issues.
            Self::MissingSource(_) | Self::MissingOverride(_) | Self::Io(_) => {
                perl_parser_core::ErrorCategory::Infra
            }
            // The catalog is our own build artifact — a parse or validation
            // failure means we shipped a broken catalog, which is our bug.
            Self::Parse(_) | Self::Validation(_) => perl_parser_core::ErrorCategory::Bug,
        }
    }
}

/// Source selection detail for generated outputs and traceability.
#[derive(Debug, Clone)]
pub struct CatalogSource {
    /// Resolved source path.
    pub path: PathBuf,
    /// Selected source type.
    pub kind: CatalogSourceKind,
}

impl CatalogSource {
    /// Source tag emitted into generated modules.
    pub const fn comment(&self) -> &'static str {
        match self.kind {
            CatalogSourceKind::Override => "// source: FEATURES_TOML_OVERRIDE\n",
            CatalogSourceKind::Workspace => "// source: features.toml\n",
            CatalogSourceKind::Vendored => "// source: features_sot.toml\n",
        }
    }
}

/// Which catalog source path was selected.
#[derive(Debug, Clone, Copy)]
pub enum CatalogSourceKind {
    /// Path came from `FEATURES_TOML_OVERRIDE`.
    Override,
    /// Path came from workspace `features.toml`.
    Workspace,
    /// Path came from crate-local `features_sot.toml`.
    Vendored,
}

/// Resolve catalog path using workspace-first lookup and override support.
pub fn resolve_catalog_source(manifest_dir: &Path) -> Result<CatalogSource, CatalogError> {
    resolve_catalog_source_with_override(
        manifest_dir,
        env::var_os("FEATURES_TOML_OVERRIDE").map(PathBuf::from),
    )
}

fn resolve_catalog_source_with_override(
    manifest_dir: &Path,
    override_path: Option<PathBuf>,
) -> Result<CatalogSource, CatalogError> {
    if let Some(override_path) = override_path {
        if !override_path.exists() {
            return Err(CatalogError::MissingOverride(override_path));
        }
        return Ok(CatalogSource { path: override_path, kind: CatalogSourceKind::Override });
    }

    let local_workspace_candidate = manifest_dir.join("features.toml");
    if local_workspace_candidate.exists() {
        return Ok(CatalogSource {
            path: local_workspace_candidate,
            kind: CatalogSourceKind::Workspace,
        });
    }

    let parent_workspace = manifest_dir.parent().and_then(Path::parent).and_then(|p| {
        let path = p.join("features.toml");
        path.exists().then_some(path)
    });
    if let Some(path) = parent_workspace {
        return Ok(CatalogSource { path, kind: CatalogSourceKind::Workspace });
    }

    let vendored = manifest_dir.join("features_sot.toml");
    if vendored.exists() {
        return Ok(CatalogSource { path: vendored, kind: CatalogSourceKind::Vendored });
    }

    Err(CatalogError::MissingSource(manifest_dir.to_path_buf()))
}

/// Load and validate catalog from an explicit path.
pub fn read_catalog(path: &Path) -> Result<Catalog, CatalogError> {
    let content = fs::read_to_string(path)?;
    let catalog: Catalog = toml::from_str(&content)?;
    catalog.validate()?;
    Ok(catalog)
}

/// Load and validate catalog using workspace-style resolution.
pub fn load_catalog_for_build(
    manifest_dir: &Path,
) -> Result<(Catalog, CatalogSource), CatalogError> {
    let source = resolve_catalog_source(manifest_dir)?;
    let catalog = read_catalog(&source.path)?;
    Ok((catalog, source))
}

/// Render `features.rs`-compatible LSP runtime module source.
pub fn render_lsp_feature_catalog_module(catalog: &Catalog, source_comment: &str) -> String {
    let mut sorted = catalog.feature.clone();
    sorted.sort_by(|a, b| a.area.cmp(&b.area).then_with(|| a.id.cmp(&b.id)));

    let advertised = catalog.advertised_feature_ids();

    let mut code = String::new();
    code.push_str("// @generated by build.rs; DO NOT EDIT.\n");
    code.push_str(source_comment);
    code.push('\n');

    code.push_str("/// Current parser version extracted from features.toml metadata\n");
    code.push_str(&format!("pub const VERSION: &str = {:?};\n", catalog.meta.version));
    code.push_str("/// LSP protocol version supported by this parser implementation\n");
    code.push_str(&format!("pub const LSP_VERSION: &str = {:?};\n", catalog.meta.lsp_version));
    code.push_str(
        "/// Represents a single LSP feature with its metadata and implementation status\n",
    );
    code.push_str("#[derive(Debug, Clone)]\n");
    code.push_str("pub struct Feature {\n");
    code.push_str("    /// Unique identifier for this feature\n");
    code.push_str("    pub id: &'static str,\n");
    code.push_str("    /// LSP specification reference\n");
    code.push_str("    pub spec: &'static str,\n");
    code.push_str("    /// Functional area for this feature\n");
    code.push_str("    pub area: &'static str,\n");
    code.push_str(
        "    /// Maturity level (`proven`, `preview`, `planned`, `unsupported`, `not_proven`)\n",
    );
    code.push_str("    pub maturity: &'static str,\n");
    code.push_str("    /// Advertised feature flag\n");
    code.push_str("    pub advertised: bool,\n");
    code.push_str("    /// Human-readable description\n");
    code.push_str("    pub description: &'static str,\n");
    code.push_str("    /// Include this feature in coverage / compliance accounting\n");
    code.push_str("    pub counts_in_coverage: bool,\n");
    code.push_str("    /// Test cases validating the feature\n");
    code.push_str("    pub tests: &'static [&'static str],\n");
    code.push_str("}\n\n");

    code.push_str(
        "/// Comprehensive catalog of all LSP features with their implementation status\n",
    );
    code.push_str("pub const ALL_FEATURES: &[Feature] = &[\n");
    for feature in sorted {
        code.push_str("    Feature {\n");
        code.push_str(&format!("        id: {:?},\n", feature.id));
        code.push_str(&format!("        spec: {:?},\n", feature.spec));
        code.push_str(&format!("        area: {:?},\n", feature.area));
        code.push_str(&format!("        maturity: {:?},\n", feature.maturity.label()));
        code.push_str(&format!("        advertised: {},\n", feature.advertised));
        code.push_str(&format!("        description: {:?},\n", feature.description));
        code.push_str(&format!("        counts_in_coverage: {},\n", feature.counts_in_coverage));
        code.push_str(&format!("        tests: &{:?},\n", feature.tests));
        code.push_str("    },\n");
    }
    code.push_str("];\n\n");

    code.push_str("/// Advertised feature IDs (GA/production and `advertised = true`).\n");
    code.push_str("pub const ADVERTISED_LSP_FEATURES: &[&str] = &[\n");
    for id in &advertised {
        code.push_str(&format!("    {:?},\n", id));
    }
    code.push_str("];\n\n");

    code.push_str("/// Returns advertised feature IDs (GA/production and `advertised = true`).\n");
    code.push_str("pub fn advertised_features() -> &'static [&'static str] {\n");
    code.push_str("    ADVERTISED_LSP_FEATURES\n");
    code.push_str("}\n\n");

    code.push_str("/// Checks whether a feature is currently advertised.\n");
    code.push_str("pub fn has_feature(id: &str) -> bool {\n");
    code.push_str("    ADVERTISED_LSP_FEATURES.contains(&id)\n");
    code.push_str("}\n\n");

    code
}

/// Render the catalog's declaration-only navigation table.
pub fn render_navigation_table(catalog: &Catalog) -> String {
    let mut by_area: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for feature in &catalog.feature {
        let entry = by_area.entry(feature.area.as_str()).or_default();
        entry.1 += 1;
        if matches!(feature.maturity, Maturity::Proven | Maturity::Preview) {
            entry.0 += 1;
        }
    }

    let mut lines = vec![
        "| Area | Declared proven/preview rows | Total rows |".to_string(),
        "|------|---------------------------|------------|".to_string(),
    ];
    let mut declared = 0;
    let mut total = 0;
    for (area, (area_declared, area_total)) in by_area {
        lines.push(format!("| {area} | {area_declared} | {area_total} |"));
        declared += area_declared;
        total += area_total;
    }
    lines.push(format!("| **Overall** | **{declared}** | **{total}** |"));
    lines.push(String::new());
    lines.push(
        "Counts are navigation only (#6731): maturity labels are declarations without per-row \
         behavior-evidence ownership."
            .to_string(),
    );
    lines.join("\n")
}

/// Render DAP runtime module source.
pub fn render_dap_feature_catalog_module(ids: &[&str]) -> String {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut code = String::new();
    code.push_str("// @generated by build.rs; DO NOT EDIT.\n\n");
    code.push_str("pub const ADVERTISED_DAP_FEATURES: &[&str] = &[\n");
    for id in &sorted {
        code.push_str(&format!("    {:?},\n", id));
    }
    code.push_str("];\n\n");
    code.push_str("pub fn advertised_features() -> &'static [&'static str] {\n");
    code.push_str("    ADVERTISED_DAP_FEATURES\n");
    code.push_str("}\n\n");
    code.push_str("pub fn has_feature(id: &str) -> bool {\n");
    code.push_str("    ADVERTISED_DAP_FEATURES.contains(&id)\n");
    code.push_str("}\n");
    code
}

/// Render fallback DAP catalog for offline or error cases.
pub fn render_dap_fallback_module(default_features: &[&str]) -> String {
    render_dap_feature_catalog_module(default_features)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must, must_err, must_some};
    use tempfile::TempDir;

    /// Fully-defaulted row so tests only name the fields under assertion.
    fn base_feature(id: &str) -> Feature {
        Feature {
            id: id.to_string(),
            spec: "LSP 3.18".to_string(),
            area: "text_document".to_string(),
            policy_class: String::new(),
            maturity: Maturity::NotProven,
            advertised: true,
            direction: String::new(),
            capability_gate: String::new(),
            registration: String::new(),
            implementation_owner: String::new(),
            state_owner: String::new(),
            tests: vec![],
            evidence: vec![],
            limitations: vec![],
            claim_boundary: String::new(),
            counts_in_coverage: true,
            description: String::new(),
        }
    }

    fn proven_feature(id: &str) -> Feature {
        let mut feature = base_feature(id);
        feature.maturity = Maturity::Proven;
        feature.policy_class = "request_response".to_string();
        feature.direction = "client_to_server".to_string();
        feature.capability_gate = "hoverProvider".to_string();
        feature.registration = "static_capabilities".to_string();
        feature.implementation_owner = "crates/example.rs".to_string();
        feature.state_owner = "document_store".to_string();
        feature.claim_boundary = "No cross-workspace claims.".to_string();
        feature.evidence =
            vec![EvidenceEntry { class: "integration_test".to_string(), id: format!("{id}.rs") }];
        feature
    }

    fn policy_maps() -> (BTreeMap<String, Vec<String>>, BTreeMap<String, String>) {
        let mut policy = BTreeMap::new();
        policy.insert("request_response".to_string(), vec!["integration_test".to_string()]);
        let mut classes = BTreeMap::new();
        classes.insert("integration_test".to_string(), "qualifying".to_string());
        classes.insert("declaration_only".to_string(), "non_qualifying".to_string());
        (policy, classes)
    }

    fn sample_catalog() -> Catalog {
        let (evidence_policy, evidence_classes) = policy_maps();
        let mut completion = proven_feature("lsp.completion");
        completion.tests = vec!["crates/perl-lsp-rs/tests/completion.rs".to_string()];
        completion.description = "Completion support".to_string();

        let mut references = base_feature("lsp.references");
        references.area = "workspace".to_string();
        references.description = "References downgraded to not_proven (#7029)".to_string();

        Catalog {
            meta: Meta {
                version: "0.42.0".to_string(),
                lsp_version: "3.18".to_string(),
                compliance_percent: None,
            },
            feature: vec![
                completion,
                {
                    let mut f = base_feature("lsp.semanticTokens");
                    f.maturity = Maturity::Preview;
                    f.description = "Semantic token support".to_string();
                    f
                },
                {
                    let mut f = base_feature("lsp.codeAction");
                    f.area = "workspace".to_string();
                    f.advertised = false;
                    f.counts_in_coverage = false;
                    f.maturity = Maturity::Planned;
                    f.description = "Code actions planned".to_string();
                    f
                },
                references,
            ],
            evidence_policy,
            evidence_classes,
        }
    }

    #[test]
    fn advertisement_survives_claim_downgrade() {
        // #7029 core invariant: downgrading an earned claim must not silently
        // remove a working capability from clients. A preview row that is
        // advertised also stays in the runtime set.
        let catalog = sample_catalog();
        assert_eq!(
            catalog.advertised_feature_ids(),
            vec!["lsp.completion", "lsp.references", "lsp.semanticTokens"]
        );
    }

    #[test]
    fn planned_and_unsupported_rows_can_never_advertise() {
        let mut catalog = sample_catalog();
        catalog.feature[2].advertised = true; // planned row mis-flagged
        catalog.feature.push({
            let mut f = base_feature("lsp.never");
            f.maturity = Maturity::Unsupported;
            f.advertised = true;
            f
        });
        let err = must_err(catalog.validate());
        assert!(err.to_string().contains("can never advertise"), "unexpected refusal: {err}");
        assert!(
            !catalog.advertised_feature_ids().contains(&"lsp.codeAction"),
            "planned rows must stay out of the runtime advertisement set"
        );
        assert!(
            !catalog.advertised_feature_ids().contains(&"lsp.never"),
            "unsupported rows must stay out of the runtime advertisement set"
        );
    }

    #[test]
    #[allow(deprecated)]
    fn compliance_math_uses_trackable_features_only() {
        let catalog = sample_catalog();
        assert_eq!(catalog.trackable_feature_count_for_grid(), 3);
        assert_eq!(catalog.advertised_trackable_count_for_grid(), 3);
        assert_eq!(catalog.trackable_feature_count(), 3);
        assert_eq!(catalog.advertised_trackable_count(), 3);
    }

    #[test]
    fn area_stats_include_maturity_breakdown() {
        let catalog = sample_catalog();
        let stats = catalog.area_statistics();

        let text_doc = must_some(stats.get("text_document"));
        assert_eq!(text_doc.total, 2);
        assert_eq!(text_doc.proven, 1);
        assert_eq!(text_doc.preview, 1);

        let workspace = must_some(stats.get("workspace"));
        assert_eq!(workspace.not_proven, 1);
        assert_eq!(workspace.planned, 1);
    }

    #[test]
    fn validation_rejects_duplicate_feature_ids() {
        let mut catalog = sample_catalog();
        catalog.feature.push(proven_feature("lsp.completion"));

        let err = must_err(catalog.validate());
        assert!(err.to_string().contains("duplicate feature id: lsp.completion"));
    }

    #[test]
    fn validation_refuses_declared_aggregate_compliance_percent() {
        // #6731 recurrence control: a declared aggregate percentage in the
        // catalog must fail validation instead of silently re-entering
        // authoritative status as a compliance claim.
        let mut catalog = sample_catalog();
        catalog.meta.compliance_percent = Some(98);

        let err = must_err(catalog.validate());
        let message = err.to_string();
        assert!(
            message.contains("meta.compliance_percent is refused"),
            "unexpected refusal message: {message}"
        );
        assert!(message.contains("#6731"), "refusal must cite its claim: {message}");
    }

    #[test]
    fn proven_row_requires_qualifying_evidence() {
        // #7029 negative control: stripping the citation demotes nothing — it
        // makes the catalog invalid instead of inheriting a green label.
        let mut catalog = sample_catalog();
        catalog.feature[0].evidence.clear();
        let err = must_err(catalog.validate());
        assert!(err.to_string().contains("lacks a qualifying"), "unexpected refusal: {err}");
    }

    #[test]
    fn proven_row_rejects_non_qualifying_evidence_class() {
        let mut catalog = sample_catalog();
        catalog.feature[0].evidence = vec![EvidenceEntry {
            class: "declaration_only".to_string(),
            id: "somewhere.toml".to_string(),
        }];
        let err = must_err(catalog.validate());
        assert!(
            err.to_string().contains("lacks a qualifying"),
            "non-qualifying classes cannot promote (#7029): {err}"
        );
    }

    #[test]
    fn proven_row_requires_complete_ownership_metadata() {
        for field in
            ["direction", "capability_gate", "registration", "implementation_owner", "state_owner"]
        {
            let mut catalog = sample_catalog();
            match field {
                "direction" => catalog.feature[0].direction = "missing".to_string(),
                "capability_gate" => catalog.feature[0].capability_gate = String::new(),
                "registration" => catalog.feature[0].registration = "missing".to_string(),
                "implementation_owner" => {
                    catalog.feature[0].implementation_owner = "missing".to_string()
                }
                _ => catalog.feature[0].state_owner = "missing".to_string(),
            }
            let err = must_err(catalog.validate());
            assert!(
                err.to_string().contains(&format!("must record {field}")),
                "empty {field} must block proven (#7029): {err}"
            );
        }
    }

    #[test]
    fn proven_row_requires_claim_boundary() {
        let mut catalog = sample_catalog();
        catalog.feature[0].claim_boundary = " ".to_string();
        let err = must_err(catalog.validate());
        assert!(err.to_string().contains("claim_boundary"), "{err}");
    }

    #[test]
    fn not_proven_rows_validate_without_evidence_or_ownership() {
        // Fail-closed baseline: recording less than proven is valid as long as
        // the row does not claim more than its evidence supports.
        let mut catalog = sample_catalog();
        catalog.feature[0].maturity = Maturity::NotProven;
        catalog.feature[0].direction = "missing".to_string();
        catalog.validate().expect("not_proven row must validate");
    }

    #[test]
    fn shipped_vendored_catalogs_pass_validation_without_declared_percent()
    -> Result<(), Box<dyn std::error::Error>> {
        // #6731/#7029 recurrence control: every crate-local features_sot.toml
        // that a standalone/packaged build can resolve to must parse and pass
        // validation, including the fail-closed #7029 rules.
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .ok_or("cannot resolve workspace root from CARGO_MANIFEST_DIR")?;
        let catalog_files = [
            "crates/perl-lsp-rs/features_sot.toml",
            "crates/perl-lsp-rs-core/features_sot.toml",
            "crates/perl-parser/features_sot.toml",
            "crates/perl-dap/features_sot.toml",
        ];
        for relative in catalog_files {
            let path = workspace_root.join(relative);
            let raw = fs::read_to_string(&path).map_err(|e| format!("reading {relative}: {e}"))?;
            assert!(
                !raw.contains("compliance_percent"),
                "{relative} must not declare meta.compliance_percent (#6731)"
            );
            assert!(
                raw.contains("GENERATED PROJECTIONS"),
                "{relative} is a generated projection and must say so (#7029)"
            );
            let catalog = read_catalog(&path).map_err(|e| format!("parsing {relative}: {e}"))?;
            catalog.validate().map_err(|e| format!("validating {relative}: {e}"))?;
        }
        Ok(())
    }

    #[test]
    fn legacy_maturity_spellings_parse_as_new_vocabulary() {
        let text = "\
[meta]
version = 't'
lsp_version = '3.18'
[[feature]]
id = 'a'
maturity = 'ga'
[[feature]]
id = 'b'
maturity = 'production'
[[feature]]
id = 'c'
maturity = 'experimental'
";
        let catalog: Catalog = toml::from_str(text).expect("legacy spellings parse");
        assert_eq!(catalog.feature[0].maturity, Maturity::Proven);
        assert_eq!(catalog.feature[1].maturity, Maturity::Proven);
        assert_eq!(catalog.feature[2].maturity, Maturity::NotProven);
    }

    #[test]
    fn resolve_catalog_source_prefers_workspace_then_vendored() {
        let temp = must(TempDir::new());
        let manifest_dir = temp.path().join("crates/perl-lsp-rs-core");
        must(std::fs::create_dir_all(&manifest_dir));

        let parent_workspace = temp.path().join("features.toml");
        must(std::fs::write(&parent_workspace, "[meta]\nversion='0.1.0'\nlsp_version='3.18'\n"));
        let source = must(resolve_catalog_source(&manifest_dir));
        assert!(matches!(source.kind, CatalogSourceKind::Workspace));
        assert_eq!(source.path, parent_workspace);

        must(std::fs::remove_file(temp.path().join("features.toml")));
        let vendored = manifest_dir.join("features_sot.toml");
        must(std::fs::write(&vendored, "[meta]\nversion='0.1.0'\nlsp_version='3.18'\n"));
        let source = must(resolve_catalog_source(&manifest_dir));
        assert!(matches!(source.kind, CatalogSourceKind::Vendored));
        assert_eq!(source.path, vendored);
    }

    #[test]
    fn resolve_catalog_source_rejects_missing_explicit_override() {
        let temp = must(TempDir::new());
        let manifest_dir = temp.path().join("crates/perl-lsp-rs-core");
        must(std::fs::create_dir_all(&manifest_dir));
        let workspace = temp.path().join("features.toml");
        must(std::fs::write(&workspace, "[meta]\nversion='0.1.0'\nlsp_version='3.18'\n"));
        let missing_override = temp.path().join("missing-features.toml");

        let error =
            resolve_catalog_source_with_override(&manifest_dir, Some(missing_override.clone()))
                .expect_err("missing explicit override must be terminal");
        assert!(matches!(error, CatalogError::MissingOverride(path) if path == missing_override));
    }

    #[test]
    fn render_lsp_module_sorts_features_and_emits_expected_constants() {
        let catalog = sample_catalog();
        let rendered = render_lsp_feature_catalog_module(&catalog, "// source: test\n");

        assert!(rendered.contains("pub const VERSION: &str = \"0.42.0\";"));
        assert!(rendered.contains("pub const LSP_VERSION: &str = \"3.18\";"));
        assert!(!rendered.contains("COMPLIANCE_PERCENT"));
        assert!(!rendered.contains("compliance_percent()"));
        assert!(
            rendered.contains("pub const ADVERTISED_LSP_FEATURES: &[&str] = &[\n    \"lsp.completion\",\n    \"lsp.references\",\n    \"lsp.semanticTokens\",\n];")
        );
        assert!(
            rendered.contains("maturity: \"proven\""),
            "generated module must carry #7029 vocabulary"
        );

        let code_action_idx = must_some(rendered.find("id: \"lsp.codeAction\""));
        let completion_idx = must_some(rendered.find("id: \"lsp.completion\""));
        let references_idx = must_some(rendered.find("id: \"lsp.references\""));
        let semantic_idx = must_some(rendered.find("id: \"lsp.semanticTokens\""));
        assert!(completion_idx < semantic_idx);
        assert!(semantic_idx < code_action_idx);
        assert!(code_action_idx < references_idx);
    }

    #[test]
    fn checked_in_override_fixtures_parse_and_validate() {
        // crates/perl-parser/tests/data/*.toml are consumed through this
        // catalog loader via FEATURES_TOML_OVERRIDE, not by any perl-parser
        // test target (#2006 review finding): they must parse and validate
        // here, where the real consumer lives. Enumerate every TOML the
        // allowlist glob admits so a future fixture cannot escape coverage
        // (#12721 review).
        let parser_data = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|crates| crates.join("perl-parser/tests/data"))
            .unwrap_or_else(|| PathBuf::from("../perl-parser/tests/data"));
        let mut fixture_names: Vec<String> = must(fs::read_dir(&parser_data))
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".toml"))
            .collect();
        fixture_names.sort();
        assert!(!fixture_names.is_empty(), "override fixture directory must not be empty");
        for name in &fixture_names {
            must(read_catalog(&parser_data.join(name)));
        }
        // The gating scenarios these fixtures exist for:
        let minimal = must(read_catalog(&parser_data.join("features_minimal.toml")));
        assert!(
            minimal.feature.iter().any(|feature| feature.id == "lsp.hover" && !feature.advertised),
            "features_minimal.toml must disable lsp.hover for the gating test"
        );
        let disabled = must(read_catalog(&parser_data.join("features_disabled_test.toml")));
        assert!(
            disabled.feature.iter().any(|feature| !feature.advertised),
            "features_disabled_test.toml must disable at least one feature"
        );
    }
}
