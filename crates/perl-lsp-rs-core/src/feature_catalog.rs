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
pub const DEFAULT_DAP_FEATURES: &[&str] =
    &["dap.breakpoints.basic", "dap.core", "dap.inline_values"];

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

/// Claim-strength vocabulary for catalog rows (#7029).
///
/// `proven` requires qualifying classified evidence per the catalog `[policy]`
/// section. Advertisement (`advertised = true`) describes the binary surface
/// and can never promote a row on its own.
#[derive(
    Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum Maturity {
    /// Recorded evidence satisfies the class policy for this feature.
    Proven,
    /// Working implementation exercised by cited suites; promotion awaits
    /// validated evidence receipts.
    Preview,
    /// Acknowledged work item without an implementation claim.
    Planned,
    /// Explicitly not implemented and not planned (for example, impossible on
    /// the host platform); never advertised.
    Unsupported,
    /// A claim exists but present recorded evidence does not support even
    /// preview strength.
    NotProven,
}

impl Maturity {
    /// Returns `true` when the row participates in trackable denominators.
    ///
    /// `planned` and `unsupported` rows carry no implementation claim, so they
    /// stay out of both numerator and denominator.
    pub const fn is_trackable(self) -> bool {
        !matches!(self, Self::Planned | Self::Unsupported)
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

/// Classified evidence receipt backing a feature claim (#7029).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct EvidenceReceipt {
    /// Evidence class from the catalog `[policy].evidence_classes` vocabulary.
    pub class: String,
    /// Repository-relative receipt path (test file or recorded manual proof).
    pub path: String,
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
    /// Maturity state.
    pub maturity: Maturity,
    /// Whether this feature is advertised/visible to clients. This describes
    /// the binary surface only; it never promotes [`Maturity::Proven`].
    #[serde(default)]
    pub advertised: bool,
    /// Request direction (`client_to_server`, `server_to_client`, `both`, or
    /// `missing` when unrecorded).
    #[serde(default)]
    pub direction: String,
    /// Client-capability gate key, `none` when ungated, or `missing`.
    #[serde(default)]
    pub capability_gate: String,
    /// Registration route (`static`, `dynamic`, `none`, or `missing`).
    #[serde(default)]
    pub registration: String,
    /// Implementation owner (module path) or `missing`.
    #[serde(default)]
    pub impl_owner: String,
    /// Retained-state owner or `missing`/`none`.
    #[serde(default)]
    pub state_owner: String,
    /// Known limitations or `missing`.
    #[serde(default)]
    pub limitations: String,
    /// Claim boundary relative to the upstream spec or `missing`.
    #[serde(default)]
    pub claim_boundary: String,
    /// Test cases validating the feature (BDD receipts).
    #[serde(default)]
    pub tests: Vec<String>,
    /// Classified evidence receipts; required for `proven`.
    #[serde(default)]
    pub evidence: Vec<EvidenceReceipt>,
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
}

impl Catalog {
    /// All features in declaration order.
    pub fn features(&self) -> &[Feature] {
        &self.feature
    }

    /// IDs for advertised features.
    ///
    /// Advertisement is keyed on the explicit `advertised` flag alone (#7029):
    /// maturity records claim strength, not the binary surface, so a row may
    /// be advertised while its evidence state is still `not_proven`.
    pub fn advertised_feature_ids(&self) -> Vec<&str> {
        let mut ids = self
            .feature
            .iter()
            .filter(|feature| feature.advertised)
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

    /// Proven feature count for BDD/compliance grids.
    /// Excludes entries explicitly marked `counts_in_coverage = false`.
    pub fn proven_trackable_count_for_grid(&self) -> usize {
        self.feature
            .iter()
            .filter(|feature| feature.maturity == Maturity::Proven && feature.counts_in_coverage)
            .count()
    }

    /// Evidence-backed status percentage for BDD/compliance grids: the proven
    /// share of trackable rows (#7029).
    pub fn compliance_percent_for_grid(&self) -> f32 {
        let trackable = self.trackable_feature_count_for_grid();
        if trackable == 0 {
            return 0.0;
        }
        let proven = self.proven_trackable_count_for_grid();
        (proven as f64 / trackable as f64 * 100.0).round() as f32
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
                Maturity::Planned => entry.planned += 1,
                Maturity::Unsupported => entry.unsupported += 1,
                Maturity::NotProven => entry.not_proven += 1,
            }
        }

        stats
    }

    /// Validate constraints not captured by serde parsing alone.
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

        for feature in &self.feature {
            if feature.id.trim().is_empty() {
                issues.push("feature id must not be empty".to_string());
                continue;
            }
            if !seen.insert(&feature.id) {
                issues.push(format!("duplicate feature id: {}", feature.id));
            }
            // #7029 negative controls: rows without an implementation claim
            // must never be advertised, and `proven` requires classified
            // evidence — advertisement alone cannot promote a row.
            if feature.advertised
                && matches!(feature.maturity, Maturity::Planned | Maturity::Unsupported)
            {
                issues.push(format!(
                    "feature {} is advertised but maturity '{}' cannot be advertised",
                    feature.id,
                    feature.maturity.label()
                ));
            }
            if feature.maturity == Maturity::Proven && feature.evidence.is_empty() {
                issues.push(format!(
                    "feature {} claims proven without classified evidence receipts (#7029)",
                    feature.id
                ));
            }
        }

        if issues.is_empty() { Ok(()) } else { Err(CatalogError::Validation(issues.join(", "))) }
    }
}

/// Aggregate area-level information.
#[derive(Debug, Default, Clone, Copy)]
pub struct AreaStats {
    /// Total number of rows in the area.
    pub total: usize,
    /// Advertised row count in the area.
    pub advertised: usize,
    /// Proven count.
    pub proven: usize,
    /// Preview count.
    pub preview: usize,
    /// Planned count.
    pub planned: usize,
    /// Unsupported count.
    pub unsupported: usize,
    /// Not-proven count.
    pub not_proven: usize,
}

impl AreaStats {
    /// Number of rows eligible for trackability.
    ///
    /// Rows without an implementation claim (`planned`, `unsupported`) stay
    /// out of the denominator (#7029).
    pub const fn trackable(&self) -> usize {
        self.total - self.planned - self.unsupported
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
        "    /// Claim strength (`proven`, `preview`, `planned`, `unsupported`, `not_proven`)\n",
    );
    code.push_str("    pub maturity: &'static str,\n");
    code.push_str("    /// Advertised feature flag (binary surface; not an evidence claim)\n");
    code.push_str("    pub advertised: bool,\n");
    code.push_str("    /// Request direction (`client_to_server`, `server_to_client`, `both`)\n");
    code.push_str("    pub direction: &'static str,\n");
    code.push_str("    /// Client-capability gate key or `none`/`missing`\n");
    code.push_str("    pub capability_gate: &'static str,\n");
    code.push_str("    /// Registration route (`static`, `dynamic`, `none`)\n");
    code.push_str("    pub registration: &'static str,\n");
    code.push_str("    /// Implementation owner module path or `missing`\n");
    code.push_str("    pub impl_owner: &'static str,\n");
    code.push_str("    /// Retained-state owner or `missing`/`none`\n");
    code.push_str("    pub state_owner: &'static str,\n");
    code.push_str("    /// Known limitations or `missing`\n");
    code.push_str("    pub limitations: &'static str,\n");
    code.push_str("    /// Claim boundary relative to the upstream spec or `missing`\n");
    code.push_str("    pub claim_boundary: &'static str,\n");
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
        code.push_str(&format!("        direction: {:?},\n", feature.direction));
        code.push_str(&format!("        capability_gate: {:?},\n", feature.capability_gate));
        code.push_str(&format!("        registration: {:?},\n", feature.registration));
        code.push_str(&format!("        impl_owner: {:?},\n", feature.impl_owner));
        code.push_str(&format!("        state_owner: {:?},\n", feature.state_owner));
        code.push_str(&format!("        limitations: {:?},\n", feature.limitations));
        code.push_str(&format!("        claim_boundary: {:?},\n", feature.claim_boundary));
        code.push_str(&format!("        description: {:?},\n", feature.description));
        code.push_str(&format!("        counts_in_coverage: {},\n", feature.counts_in_coverage));
        code.push_str(&format!("        tests: &{:?},\n", feature.tests));
        code.push_str("    },\n");
    }
    code.push_str("];\n\n");

    code.push_str("/// Advertised feature IDs (`advertised = true`; not an evidence claim).\n");
    code.push_str("pub const ADVERTISED_LSP_FEATURES: &[&str] = &[\n");
    for id in &advertised {
        code.push_str(&format!("    {:?},\n", id));
    }
    code.push_str("];\n\n");

    code.push_str("/// Returns advertised feature IDs (`advertised = true`).\n");
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
    use perl_tdd_support::{must, must_some};
    use tempfile::TempDir;

    fn sample_feature(id: &str, maturity: Maturity) -> Feature {
        Feature {
            id: id.to_string(),
            spec: "LSP 3.18".to_string(),
            area: "text_document".to_string(),
            maturity,
            advertised: true,
            direction: "client_to_server".to_string(),
            capability_gate: "none".to_string(),
            registration: "static".to_string(),
            impl_owner: "missing".to_string(),
            state_owner: "missing".to_string(),
            limitations: "missing".to_string(),
            claim_boundary: "method-scoped".to_string(),
            tests: vec![format!("crates/perl-lsp-rs/tests/{id}.rs")],
            evidence: vec![],
            counts_in_coverage: true,
            description: format!("{id} support"),
        }
    }

    fn sample_catalog() -> Catalog {
        let mut completion = sample_feature("lsp.completion", Maturity::NotProven);
        completion.area = "text_document".to_string();
        let mut semantic = sample_feature("lsp.semanticTokens", Maturity::Preview);
        semantic.area = "text_document".to_string();
        let mut code_action = sample_feature("lsp.codeAction", Maturity::Planned);
        code_action.advertised = false;
        code_action.tests = vec![];
        code_action.counts_in_coverage = false;
        code_action.area = "workspace".to_string();
        let mut references = sample_feature("lsp.references", Maturity::Proven);
        references.area = "workspace".to_string();
        references.evidence = vec![EvidenceReceipt {
            class: "integration".to_string(),
            path: "crates/perl-lsp-rs/tests/references.rs".to_string(),
        }];

        Catalog {
            meta: Meta {
                version: "0.42.0".to_string(),
                lsp_version: "3.18".to_string(),
                compliance_percent: None,
            },
            feature: vec![completion, semantic, code_action, references],
        }
    }

    #[test]
    fn advertised_ids_key_on_the_flag_alone_not_maturity() {
        // #7029: advertisement describes the binary surface. A row whose
        // evidence is still not_proven stays advertised when the flag says so.
        let catalog = sample_catalog();
        assert_eq!(
            catalog.advertised_feature_ids(),
            vec!["lsp.completion", "lsp.references", "lsp.semanticTokens",]
        );
    }

    #[test]
    fn compliance_grid_counts_the_proven_share_of_trackable_rows() {
        let catalog = sample_catalog();
        assert_eq!(catalog.trackable_feature_count_for_grid(), 3);
        assert_eq!(catalog.proven_trackable_count_for_grid(), 1);
        assert_eq!(catalog.compliance_percent_for_grid(), 33.0);
    }

    #[test]
    fn downgrading_all_rows_drives_generated_status_to_zero() {
        // #7029 negative control: generated status can no longer report 100%
        // where behavior evidence is absent.
        let mut catalog = sample_catalog();
        for feature in &mut catalog.feature {
            if feature.maturity == Maturity::Proven {
                feature.maturity = Maturity::NotProven;
                feature.evidence.clear();
            }
        }
        assert_eq!(catalog.compliance_percent_for_grid(), 0.0);
    }

    #[test]
    fn area_stats_include_maturity_breakdown() {
        let catalog = sample_catalog();
        let stats = catalog.area_statistics();

        let text_doc = must_some(stats.get("text_document"));
        assert_eq!(text_doc.total, 2);
        assert_eq!(text_doc.advertised, 2);
        assert_eq!(text_doc.not_proven, 1);
        assert_eq!(text_doc.preview, 1);
        assert_eq!(text_doc.trackable(), 2);

        let workspace = must_some(stats.get("workspace"));
        assert_eq!(workspace.total, 2);
        assert_eq!(workspace.proven, 1);
        assert_eq!(workspace.planned, 1);
        assert_eq!(workspace.trackable(), 1);
    }

    #[test]
    fn validation_rejects_advertised_rows_without_an_implementation_claim()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut catalog = sample_catalog();
        catalog.feature.push(sample_feature("dap.restart_frame", Maturity::Unsupported));

        let err =
            catalog.validate().err().ok_or("advertised unsupported row must fail validation")?;
        assert!(err.to_string().contains("cannot be advertised"), "unexpected message: {err}");
        Ok(())
    }

    #[test]
    fn validation_rejects_promotion_from_advertisement_alone()
    -> Result<(), Box<dyn std::error::Error>> {
        // #7029 negative control: flipping a row to proven without recorded
        // evidence must fail closed even while it is advertised and cited.
        let mut catalog = sample_catalog();
        let promoted = sample_feature("lsp.moniker", Maturity::Proven);
        catalog.feature.push(promoted);

        let err = catalog.validate().err().ok_or("unproven promotion must fail validation")?;
        assert!(
            err.to_string().contains("claims proven without classified evidence"),
            "unexpected message: {err}"
        );
        Ok(())
    }

    #[test]
    fn validation_accepts_proven_with_classified_evidence() -> Result<(), Box<dyn std::error::Error>>
    {
        let catalog = sample_catalog();
        catalog
            .validate()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;
        Ok(())
    }

    #[test]
    fn validation_rejects_duplicate_feature_ids() -> Result<(), Box<dyn std::error::Error>> {
        let mut catalog = sample_catalog();
        catalog.feature.push(sample_feature("lsp.completion", Maturity::NotProven));

        let err = catalog.validate().err().ok_or("duplicate id must fail validation")?;
        let message = err.to_string();
        assert!(message.contains("duplicate feature id: lsp.completion"));
        Ok(())
    }

    #[test]
    fn validation_refuses_declared_aggregate_compliance_percent()
    -> Result<(), Box<dyn std::error::Error>> {
        // #6731 recurrence control: a declared aggregate percentage in the
        // catalog must fail validation instead of silently re-entering
        // authoritative status as a compliance claim.
        let mut catalog = sample_catalog();
        catalog.meta.compliance_percent = Some(98);

        let err = catalog.validate().err().ok_or("declared aggregate must fail validation")?;
        let message = err.to_string();
        assert!(
            message.contains("meta.compliance_percent is refused"),
            "unexpected refusal message: {message}"
        );
        assert!(message.contains("#6731"), "refusal must cite its claim: {message}");
        Ok(())
    }

    #[test]
    fn shipped_vendored_catalogs_pass_validation_without_declared_percent()
    -> Result<(), Box<dyn std::error::Error>> {
        // #6731 recurrence control: every crate-local features_sot.toml that a
        // standalone/packaged build can resolve to must still parse and pass
        // validation. A reintroduced meta.compliance_percent fails here instead
        // of poisoning vendored builds into silent zero-feature advertisement.
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
            let catalog = read_catalog(&path).map_err(|e| format!("parsing {relative}: {e}"))?;
            catalog.validate().map_err(|e| format!("validating {relative}: {e}"))?;
        }
        Ok(())
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
        assert!(rendered.contains("maturity: \"not_proven\""));
        assert!(rendered.contains("direction: \"client_to_server\""));
        assert!(rendered.contains("claim_boundary: \"method-scoped\""));

        let code_action_idx = must_some(rendered.find("id: \"lsp.codeAction\""));
        let completion_idx = must_some(rendered.find("id: \"lsp.completion\""));
        let references_idx = must_some(rendered.find("id: \"lsp.references\""));
        let semantic_idx = must_some(rendered.find("id: \"lsp.semanticTokens\""));
        assert!(completion_idx < semantic_idx);
        assert!(semantic_idx < code_action_idx);
        assert!(code_action_idx < references_idx);
    }
}
