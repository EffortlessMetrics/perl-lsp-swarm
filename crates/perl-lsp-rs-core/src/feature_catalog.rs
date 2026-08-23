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

/// Feature maturity state (#7029 evidence-honest vocabulary).
///
/// Maturity records the *evidence state* of a row, not its wire behavior:
/// `advertised` plus the [`Maturity::is_servable`] predicate decide the
/// advertisement route, so a row can be advertised while its evidence state
/// is [`Maturity::NotProven`]. Advertisement, an implementation path, or a
/// named test can never independently yield [`Maturity::Proven`]; promotion
/// requires the per-class evidence policy in [`Catalog::validate`].
#[derive(
    Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum Maturity {
    /// Behavior evidence verified against the row's class policy and cited.
    Proven,
    /// Implemented behind a documented preview boundary.
    Preview,
    /// Acknowledged protocol surface that is not implemented yet.
    Planned,
    /// Deliberately unsupported surface (negative-gated).
    Unsupported,
    /// Implemented surface without qualifying behavior evidence. Fail-closed
    /// default for rows whose citations are declaration-, tolerance-, or
    /// comment-only (#7029).
    NotProven,
}

impl Maturity {
    /// Returns `true` when the feature may take part in the advertisement
    /// route (`advertised = true` rows with this maturity are advertised).
    ///
    /// Evidence state does not gate advertisement: a [`Maturity::NotProven`]
    /// row stays advertised while its evidence gap is recorded honestly.
    pub const fn is_servable(self) -> bool {
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

/// Message flow direction for a catalog row (#7029 required dimension).
#[derive(
    Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Client-to-server request/response or notification.
    ClientToServer,
    /// Server-to-client request or notification.
    ServerToClient,
    /// Both directions (for example full DAP sessions or handshake surfaces).
    Bidirectional,
}

impl Direction {
    /// Human-readable lowercase label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClientToServer => "client_to_server",
            Self::ServerToClient => "server_to_client",
            Self::Bidirectional => "bidirectional",
        }
    }
}

/// Feature class used to select the promotion policy (#7029).
#[derive(
    Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum FeatureClass {
    /// Client request/response surface.
    RequestResponse,
    /// Server-initiated request or push surface.
    ServerRequest,
    /// Document/workspace notification surface (sync, configuration, file events).
    DocumentWorkspace,
    /// Cancellation and progress plumbing (`$/...`).
    CancellationProgress,
    /// Surface whose proof depends on an editor receipt.
    EditorDependent,
}

impl FeatureClass {
    /// Human-readable lowercase label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::RequestResponse => "request_response",
            Self::ServerRequest => "server_request",
            Self::DocumentWorkspace => "document_workspace",
            Self::CancellationProgress => "cancellation_progress",
            Self::EditorDependent => "editor_dependent",
        }
    }
}

/// How a row's capability/registration route is exposed (#7029).
#[derive(
    Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityRoute {
    /// Advertised through `initialize` server capabilities.
    InitializeCapability,
    /// Registered at runtime via `client/registerCapability`.
    DynamicRegistration,
    /// Emitted only when the client declares the matching capability.
    ClientCapabilityGated,
    /// Always-on push with no capability negotiation.
    Unsolicited,
    /// Route not yet recorded; blocks `proven` promotion.
    Missing,
}

impl CapabilityRoute {
    /// Human-readable lowercase label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::InitializeCapability => "initialize_capability",
            Self::DynamicRegistration => "dynamic_registration",
            Self::ClientCapabilityGated => "client_capability_gated",
            Self::Unsolicited => "unsolicited",
            Self::Missing => "missing",
        }
    }
}

/// Explicit missing-value token for owner and state-owner fields (#7029).
pub const MISSING: &str = "missing";
/// Explicit stateless token for the state-owner field (#7029).
pub const STATELESS: &str = "stateless";

/// Minimum evidence a feature class demands before `proven` is allowed
/// (#7029). Declares the policy without requiring the missing tests to
/// exist yet; executable evidence validation belongs to the follow-up
/// #6731 gate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct EvidencePolicy {
    /// Feature class this policy governs.
    pub class: FeatureClass,
    /// Minimum count of cited behavior-evidence tests for `proven`.
    pub min_behavior_tests: usize,
    /// Human-readable policy statement.
    pub description: String,
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
    /// Maturity (evidence state) of the row.
    pub maturity: Maturity,
    /// Message flow direction (#7029 required dimension).
    pub direction: Direction,
    /// Feature class selecting the promotion policy (#7029 required dimension).
    pub class: FeatureClass,
    /// Capability/registration route (#7029 required dimension).
    pub route: CapabilityRoute,
    /// Implementation owner (crate granularity) or [`MISSING`].
    pub owner: String,
    /// Retained-state owner, [`STATELESS`], or [`MISSING`].
    pub state_owner: String,
    /// Whether this feature is advertised/visible to clients.
    #[serde(default)]
    pub advertised: bool,
    /// Test cases validating the feature.
    #[serde(default)]
    pub tests: Vec<String>,
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
    /// Minimum-evidence policy per feature class (#7029).
    #[serde(default)]
    pub evidence_policy: Vec<EvidencePolicy>,
    /// Ordered feature rows.
    pub feature: Vec<Feature>,
}

impl Catalog {
    /// All features in declaration order.
    pub fn features(&self) -> &[Feature] {
        &self.feature
    }

    /// Policy declared for a feature class, if any.
    pub fn evidence_policy_for(&self, class: FeatureClass) -> Option<&EvidencePolicy> {
        self.evidence_policy.iter().find(|policy| policy.class == class)
    }

    /// IDs for advertised rows (`advertised = true` and a servable maturity).
    ///
    /// Advertisement is a wire-surface declaration; it is intentionally
    /// independent of the evidence state recorded by [`Maturity`] (#7029).
    pub fn advertised_feature_ids(&self) -> Vec<&str> {
        let mut ids = self
            .feature
            .iter()
            .filter(|feature| feature.advertised && feature.maturity.is_servable())
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
                feature.advertised && feature.maturity.is_servable() && feature.counts_in_coverage
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
            .filter(|feature| feature.advertised && feature.maturity.is_servable())
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

        let mut policy_classes = BTreeSet::new();
        for policy in &self.evidence_policy {
            if !policy_classes.insert(policy.class) {
                issues.push(format!("duplicate evidence policy class: {}", policy.class.label()));
            }
        }

        for feature in &self.feature {
            if feature.id.trim().is_empty() {
                issues.push("feature id must not be empty".to_string());
                continue;
            }
            if !seen.insert(&feature.id) {
                issues.push(format!("duplicate feature id: {}", feature.id));
            }

            // #7029: every used class must have an explicit promotion policy.
            if self.evidence_policy_for(feature.class).is_none() {
                issues.push(format!(
                    "feature {}: class {} has no evidence policy",
                    feature.id,
                    feature.class.label()
                ));
            }

            // #7029: advertisement and non-servable maturities are contradictory.
            if feature.advertised && !feature.maturity.is_servable() {
                issues.push(format!(
                    "feature {}: advertised rows cannot be {}",
                    feature.id,
                    feature.maturity.label()
                ));
            }

            // #7029 fail-closed promotion rule: `advertised = true`, an
            // implementation path, or a named test cannot independently yield
            // `proven`. Promotion requires the conjunction of cited behavior
            // evidence (per class policy), a recorded route, and recorded
            // owners.
            if feature.maturity == Maturity::Proven {
                let min = self
                    .evidence_policy_for(feature.class)
                    .map_or(1, |policy| policy.min_behavior_tests.max(1));
                if feature.tests.len() < min {
                    issues.push(format!(
                        "feature {}: proven requires at least {} cited behavior test(s) per {} \
                         policy (#7029); downgrade to not_proven or cite evidence",
                        feature.id,
                        min,
                        feature.class.label()
                    ));
                }
                if feature.route == CapabilityRoute::Missing {
                    issues.push(format!(
                        "feature {}: proven requires a recorded capability route (#7029)",
                        feature.id
                    ));
                }
                if feature.owner == MISSING {
                    issues.push(format!(
                        "feature {}: proven requires a recorded implementation owner (#7029)",
                        feature.id
                    ));
                }
                if feature.state_owner == MISSING {
                    issues.push(format!(
                        "feature {}: proven requires a recorded state owner or stateless (#7029)",
                        feature.id
                    ));
                }
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
    /// Preview count.
    pub preview: usize,
    /// Proven count.
    pub proven: usize,
    /// Planned count.
    pub planned: usize,
    /// Unsupported count.
    pub unsupported: usize,
    /// Not-proven count.
    pub not_proven: usize,
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
    code.push_str("/// Represents a single LSP feature with its metadata and evidence state\n");
    code.push_str("#[derive(Debug, Clone)]\n");
    code.push_str("pub struct Feature {\n");
    code.push_str("    /// Unique identifier for this feature\n");
    code.push_str("    pub id: &'static str,\n");
    code.push_str("    /// LSP specification reference\n");
    code.push_str("    pub spec: &'static str,\n");
    code.push_str("    /// Functional area for this feature\n");
    code.push_str("    pub area: &'static str,\n");
    code.push_str(
        "    /// Evidence state (`proven`, `preview`, `planned`, `unsupported`, `not_proven`)\n",
    );
    code.push_str("    pub maturity: &'static str,\n");
    code.push_str("    /// Message flow direction\n");
    code.push_str("    pub direction: &'static str,\n");
    code.push_str("    /// Feature class selecting the promotion policy\n");
    code.push_str("    pub class: &'static str,\n");
    code.push_str("    /// Capability/registration route\n");
    code.push_str("    pub route: &'static str,\n");
    code.push_str("    /// Implementation owner (crate granularity) or `missing`\n");
    code.push_str("    pub owner: &'static str,\n");
    code.push_str("    /// Retained-state owner, `stateless`, or `missing`\n");
    code.push_str("    pub state_owner: &'static str,\n");
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
        code.push_str(&format!("        direction: {:?},\n", feature.direction.label()));
        code.push_str(&format!("        class: {:?},\n", feature.class.label()));
        code.push_str(&format!("        route: {:?},\n", feature.route.label()));
        code.push_str(&format!("        owner: {:?},\n", feature.owner));
        code.push_str(&format!("        state_owner: {:?},\n", feature.state_owner));
        code.push_str(&format!("        advertised: {},\n", feature.advertised));
        code.push_str(&format!("        description: {:?},\n", feature.description));
        code.push_str(&format!("        counts_in_coverage: {},\n", feature.counts_in_coverage));
        code.push_str(&format!("        tests: &{:?},\n", feature.tests));
        code.push_str("    },\n");
    }
    code.push_str("];\n\n");

    code.push_str("/// Advertised feature IDs (`advertised = true` with a servable maturity).\n");
    code.push_str("pub const ADVERTISED_LSP_FEATURES: &[&str] = &[\n");
    for id in &advertised {
        code.push_str(&format!("    {:?},\n", id));
    }
    code.push_str("];\n\n");

    code.push_str("/// Returns advertised feature IDs (`advertised = true`, servable maturity).\n");
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
        "|------|-------------------|------------|".to_string(),
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

    fn sample_catalog() -> Catalog {
        Catalog {
            meta: Meta {
                version: "0.42.0".to_string(),
                lsp_version: "3.18".to_string(),
                compliance_percent: None,
            },
            evidence_policy: vec![EvidencePolicy {
                class: FeatureClass::RequestResponse,
                min_behavior_tests: 1,
                description: "client request/response needs a behavior test".to_string(),
            }],
            feature: vec![
                Feature {
                    id: "lsp.completion".to_string(),
                    spec: "LSP 3.18".to_string(),
                    area: "text_document".to_string(),
                    maturity: Maturity::Proven,
                    direction: Direction::ClientToServer,
                    class: FeatureClass::RequestResponse,
                    route: CapabilityRoute::InitializeCapability,
                    owner: "perl-lsp-rs".to_string(),
                    state_owner: "perl-lsp-rs::state".to_string(),
                    advertised: true,
                    tests: vec!["crates/perl-lsp-rs/tests/completion.rs".to_string()],
                    counts_in_coverage: true,
                    description: "Completion support".to_string(),
                },
                Feature {
                    id: "lsp.semanticTokens".to_string(),
                    spec: "LSP 3.18".to_string(),
                    area: "text_document".to_string(),
                    maturity: Maturity::Preview,
                    direction: Direction::ClientToServer,
                    class: FeatureClass::RequestResponse,
                    route: CapabilityRoute::InitializeCapability,
                    owner: "perl-lsp-rs".to_string(),
                    state_owner: "perl-lsp-rs::state".to_string(),
                    advertised: true,
                    tests: vec!["crates/perl-lsp-rs/tests/semantic_tokens.rs".to_string()],
                    counts_in_coverage: true,
                    description: "Semantic token support".to_string(),
                },
                Feature {
                    id: "lsp.codeAction".to_string(),
                    spec: "LSP 3.18".to_string(),
                    area: "workspace".to_string(),
                    maturity: Maturity::Planned,
                    direction: Direction::ClientToServer,
                    class: FeatureClass::RequestResponse,
                    route: CapabilityRoute::Missing,
                    owner: MISSING.to_string(),
                    state_owner: MISSING.to_string(),
                    advertised: true,
                    tests: vec![],
                    counts_in_coverage: false,
                    description: "Code actions".to_string(),
                },
                Feature {
                    id: "lsp.references".to_string(),
                    spec: "LSP 3.18".to_string(),
                    area: "workspace".to_string(),
                    maturity: Maturity::NotProven,
                    direction: Direction::ClientToServer,
                    class: FeatureClass::RequestResponse,
                    route: CapabilityRoute::InitializeCapability,
                    owner: "perl-lsp-rs".to_string(),
                    state_owner: "perl-lsp-rs::state".to_string(),
                    advertised: true,
                    tests: vec![],
                    counts_in_coverage: true,
                    description: "References".to_string(),
                },
            ],
        }
    }

    #[test]
    fn advertised_ids_are_sorted_and_exclude_planned_rows() {
        let catalog = sample_catalog();
        // Advertisement is decoupled from evidence state (#7029): the
        // not_proven row stays advertised while its evidence gap is recorded.
        assert_eq!(
            catalog.advertised_feature_ids(),
            vec!["lsp.completion", "lsp.references", "lsp.semanticTokens"]
        );
    }

    #[test]
    fn not_proven_rows_remain_advertised_but_are_not_proven() {
        let catalog = sample_catalog();
        let references = must_some(catalog.feature.iter().find(|f| f.id == "lsp.references"));
        assert!(references.advertised);
        assert_eq!(references.maturity, Maturity::NotProven);
        assert!(!catalog.advertised_feature_ids().is_empty());
    }

    #[test]
    #[allow(deprecated)]
    fn compliance_math_uses_trackable_features_only() {
        let catalog = sample_catalog();
        assert_eq!(catalog.trackable_feature_count_for_grid(), 3);
        assert_eq!(catalog.advertised_trackable_count_for_grid(), 3);
        assert_eq!(catalog.compliance_percent_for_grid(), 100.0);
        assert_eq!(catalog.trackable_feature_count(), 3);
        assert_eq!(catalog.advertised_trackable_count(), 3);
        assert_eq!(catalog.compliance_percent(), 100.0);
    }

    #[test]
    #[allow(deprecated)]
    fn compatibility_compliance_preserves_pre_6731_counts() {
        let mut catalog = sample_catalog();
        catalog.feature.push(Feature {
            id: "lsp.compatibility_only".to_string(),
            spec: "LSP 3.18".to_string(),
            area: "text_document".to_string(),
            maturity: Maturity::Proven,
            direction: Direction::ClientToServer,
            class: FeatureClass::RequestResponse,
            route: CapabilityRoute::InitializeCapability,
            owner: "perl-lsp-rs".to_string(),
            state_owner: STATELESS.to_string(),
            advertised: true,
            tests: vec![],
            counts_in_coverage: false,
            description: "Compatibility-only catalog row".to_string(),
        });

        assert_eq!(catalog.trackable_feature_count_for_grid(), 3);
        assert_eq!(catalog.advertised_trackable_count_for_grid(), 3);
        assert_eq!(catalog.compliance_percent_for_grid(), 100.0);
        assert_eq!(catalog.trackable_feature_count(), 4);
        assert_eq!(catalog.advertised_trackable_count(), 4);
        assert_eq!(catalog.compliance_percent(), 100.0);
    }

    #[test]
    fn area_stats_include_maturity_breakdown() {
        let catalog = sample_catalog();
        let stats = catalog.area_statistics();

        let text_doc = must_some(stats.get("text_document"));
        assert_eq!(text_doc.total, 2);
        assert_eq!(text_doc.advertised, 2);
        assert_eq!(text_doc.proven, 1);
        assert_eq!(text_doc.preview, 1);
        assert_eq!(text_doc.trackable_coverage_percent(), 100);

        let workspace = must_some(stats.get("workspace"));
        assert_eq!(workspace.total, 2);
        assert_eq!(workspace.not_proven, 1);
        assert_eq!(workspace.planned, 1);
        assert_eq!(workspace.trackable(), 1);
        assert_eq!(workspace.trackable_coverage_percent(), 200);
    }

    #[test]
    fn validation_rejects_duplicate_feature_ids() -> Result<(), Box<dyn std::error::Error>> {
        let mut catalog = sample_catalog();
        catalog.feature.push(Feature {
            id: "lsp.completion".to_string(),
            spec: "LSP 3.18".to_string(),
            area: "text_document".to_string(),
            maturity: Maturity::Proven,
            direction: Direction::ClientToServer,
            class: FeatureClass::RequestResponse,
            route: CapabilityRoute::InitializeCapability,
            owner: "perl-lsp-rs".to_string(),
            state_owner: "perl-lsp-rs::state".to_string(),
            advertised: true,
            tests: vec![],
            counts_in_coverage: true,
            description: "duplicate row".to_string(),
        });

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
    fn validation_refuses_proven_without_cited_evidence() {
        // #7029 negative control: advertisement, an implementation path, or a
        // named test cannot independently yield proven. A proven row with no
        // cited tests fails validation.
        let mut catalog = sample_catalog();
        catalog.feature[0].maturity = Maturity::Proven;
        catalog.feature[0].tests.clear();

        let err = must_some(catalog.validate().err());
        assert!(
            err.to_string().contains("proven requires at least 1 cited behavior test"),
            "unexpected message: {}",
            err
        );
    }

    #[test]
    fn validation_refuses_proven_with_missing_route_or_owners() {
        // #7029 negative control: promotion from advertisement alone must
        // fail — proven demands a recorded route, owner, and state owner.
        let mut catalog = sample_catalog();
        catalog.feature[0].route = CapabilityRoute::Missing;
        catalog.feature[0].owner = MISSING.to_string();
        catalog.feature[0].state_owner = MISSING.to_string();

        let message = must_some(catalog.validate().err()).to_string();
        assert!(
            message.contains("proven requires a recorded capability route"),
            "message: {message}"
        );
        assert!(
            message.contains("proven requires a recorded implementation owner"),
            "message: {message}"
        );
        assert!(message.contains("proven requires a recorded state owner"), "message: {message}");
    }

    #[test]
    fn validation_refuses_advertised_planned_or_unsupported_rows() {
        // #7029: a planned or unsupported row cannot carry the advertisement
        // route; the wire surface and the non-servable maturity contradict.
        let mut catalog = sample_catalog();
        catalog.feature[3].maturity = Maturity::Unsupported;

        let message = must_some(catalog.validate().err()).to_string();
        assert!(message.contains("advertised rows cannot be unsupported"), "message: {message}");
    }

    #[test]
    fn validation_refuses_rows_without_a_class_policy() {
        // #7029: every used feature class must have an explicit promotion
        // policy, so a new class cannot silently bypass the evidence rules.
        let mut catalog = sample_catalog();
        catalog.evidence_policy.clear();

        let message = must_some(catalog.validate().err()).to_string();
        assert!(message.contains("has no evidence policy"), "message: {message}");
    }

    #[test]
    fn parsing_rejects_the_pre_7029_vocabulary() {
        // #7029 negative control: the pre-PR vocabulary (`ga`, `production`,
        // `experimental`) must fail to parse, so a stale catalog cannot
        // silently retain unearned maturity claims.
        let raw = "[meta]\n\
                   version = \"0.17.0\"\n\
                   lsp_version = \"3.18\"\n\
                   \n\
                   [[feature]]\n\
                   id = \"lsp.hover\"\n\
                   maturity = \"ga\"\n\
                   direction = \"client_to_server\"\n\
                   class = \"request_response\"\n\
                   route = \"initialize_capability\"\n\
                   owner = \"perl-lsp-rs\"\n\
                   state_owner = \"perl-lsp-rs::state\"\n";
        let parsed: Result<Catalog, _> = toml::from_str(raw);
        assert!(parsed.is_err(), "maturity = \"ga\" must not parse after #7029");
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
        assert!(rendered.contains("maturity: \"proven\""));
        assert!(rendered.contains("maturity: \"not_proven\""));
        assert!(rendered.contains("direction: \"client_to_server\""));
        assert!(rendered.contains("route: \"initialize_capability\""));
        assert!(
            rendered.contains("pub const ADVERTISED_LSP_FEATURES: &[&str] = &[\n    \"lsp.completion\",\n    \"lsp.references\",\n    \"lsp.semanticTokens\",\n];")
        );

        let code_action_idx = must_some(rendered.find("id: \"lsp.codeAction\""));
        let completion_idx = must_some(rendered.find("id: \"lsp.completion\""));
        let references_idx = must_some(rendered.find("id: \"lsp.references\""));
        let semantic_idx = must_some(rendered.find("id: \"lsp.semanticTokens\""));
        assert!(completion_idx < semantic_idx);
        assert!(semantic_idx < code_action_idx);
        assert!(code_action_idx < references_idx);
    }
}
