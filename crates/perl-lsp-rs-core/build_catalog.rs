// Build-time feature catalog helper, included via `include!()` in build.rs.
// This is a copy of the catalog logic extracted from perl-feature-catalog.
// It exists here because build.rs is a separate compilation unit and cannot
// reference `perl_lsp_rs_core::feature_catalog` at build time.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Feature maturity state (#7029 evidence-honest vocabulary).
///
/// Maturity records evidence state, not wire behavior; advertisement is
/// decided by `advertised` plus [`Maturity::is_servable`].
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
    /// Implemented surface without qualifying behavior evidence (#7029).
    NotProven,
}

impl Maturity {
    /// Whether the feature may take part in the advertisement route.
    pub const fn is_servable(self) -> bool {
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
    /// Both directions.
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
    /// Document/workspace notification surface.
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

/// Minimum evidence a feature class demands before `proven` is allowed (#7029).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct EvidencePolicy {
    /// Feature class this policy governs.
    pub class: FeatureClass,
    /// Minimum count of cited behavior-evidence tests for `proven`.
    pub min_behavior_tests: usize,
    /// Human-readable policy statement.
    pub description: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Meta {
    pub version: String,
    pub lsp_version: String,
    #[serde(default)]
    pub compliance_percent: Option<u32>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Feature {
    pub id: String,
    #[serde(default)]
    pub spec: String,
    #[serde(default)]
    pub area: String,
    pub maturity: Maturity,
    pub direction: Direction,
    pub class: FeatureClass,
    pub route: CapabilityRoute,
    pub owner: String,
    pub state_owner: String,
    #[serde(default)]
    pub advertised: bool,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default = "default_counts_in_coverage")]
    pub counts_in_coverage: bool,
    #[serde(default)]
    pub description: String,
}

fn default_counts_in_coverage() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Catalog {
    pub meta: Meta,
    #[serde(default)]
    pub evidence_policy: Vec<EvidencePolicy>,
    pub feature: Vec<Feature>,
}

impl Catalog {
    pub fn advertised_feature_ids(&self) -> Vec<&str> {
        let mut ids = self
            .feature
            .iter()
            .filter(|f| f.advertised && f.maturity.is_servable())
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    /// Trackable feature count for BDD/compliance grids.
    /// Excludes entries explicitly marked `counts_in_coverage = false`.
    pub fn trackable_feature_count_for_grid(&self) -> usize {
        self.feature
            .iter()
            .filter(|feature| feature.maturity != Maturity::Planned && feature.counts_in_coverage)
            .count()
    }

    /// Advertised trackable count for BDD/compliance grids.
    /// Excludes entries explicitly marked `counts_in_coverage = false`.
    pub fn advertised_trackable_count_for_grid(&self) -> usize {
        self.feature
            .iter()
            .filter(|feature| {
                feature.advertised
                    && feature.maturity.is_servable()
                    && feature.counts_in_coverage
            })
            .count()
    }

    /// Compatibility-only alias for the grid-oriented trackable count.
    /// This is not a compliance, status, or reporting authority.
    #[deprecated(note = "compatibility-only; use trackable_feature_count_for_grid")]
    pub fn trackable_feature_count(&self) -> usize {
        self.feature
            .iter()
            .filter(|feature| feature.maturity != Maturity::Planned)
            .count()
    }

    /// Compatibility-only alias for the grid-oriented advertised count.
    /// This is not a compliance, status, or reporting authority.
    #[deprecated(note = "compatibility-only; use advertised_trackable_count_for_grid")]
    pub fn advertised_trackable_count(&self) -> usize {
        self.feature
            .iter()
            .filter(|feature| feature.advertised && feature.maturity.is_servable())
            .count()
    }

    /// Compatibility-only alias for the grid-oriented percentage.
    /// This is not a compliance, status, or reporting authority.
    #[deprecated(note = "compatibility-only; use compliance_percent_for_grid")]
    pub fn compliance_percent(&self) -> f32 {
        let trackable = self.trackable_feature_count_for_grid();
        if trackable == 0 {
            return 0.0;
        }
        let advertised = self.advertised_trackable_count_for_grid();
        (advertised as f64 / trackable as f64 * 100.0).round() as f32
    }

    /// Policy declared for a feature class, if any.
    pub fn evidence_policy_for(&self, class: FeatureClass) -> Option<&EvidencePolicy> {
        self.evidence_policy.iter().find(|policy| policy.class == class)
    }

    pub fn validate(&self) -> Result<(), String> {
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
            if self.evidence_policy_for(feature.class).is_none() {
                issues.push(format!(
                    "feature {}: class {} has no evidence policy",
                    feature.id,
                    feature.class.label()
                ));
            }
            if feature.advertised && !feature.maturity.is_servable() {
                issues.push(format!(
                    "feature {}: advertised rows cannot be {}",
                    feature.id,
                    feature.maturity.label()
                ));
            }
            if feature.maturity == Maturity::Proven {
                let min = self
                    .evidence_policy_for(feature.class)
                    .map_or(1, |policy| policy.min_behavior_tests.max(1));
                if feature.tests.len() < min {
                    issues.push(format!(
                        "feature {}: proven requires at least {} cited behavior test(s) per {} \
                         policy (#7029); downgrade to not_proven or cite evidence",
                        feature.id, min, feature.class.label()
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
        if issues.is_empty() { Ok(()) } else { Err(issues.join(", ")) }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogSource {
    pub path: PathBuf,
    pub kind: CatalogSourceKind,
}

impl CatalogSource {
    pub const fn comment(&self) -> &'static str {
        match self.kind {
            CatalogSourceKind::Override => "// source: FEATURES_TOML_OVERRIDE\n",
            CatalogSourceKind::Workspace => "// source: features.toml\n",
            CatalogSourceKind::Vendored => "// source: features_sot.toml\n",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CatalogSourceKind {
    Override,
    Workspace,
    Vendored,
}

pub fn resolve_catalog_source(manifest_dir: &Path) -> Result<CatalogSource, String> {
    resolve_catalog_source_with_override(
        manifest_dir,
        env::var_os("FEATURES_TOML_OVERRIDE").map(PathBuf::from),
    )
}

pub fn resolve_catalog_source_with_override(
    manifest_dir: &Path,
    override_path: Option<PathBuf>,
) -> Result<CatalogSource, String> {
    if let Some(override_path) = override_path {
        if !override_path.exists() {
            return Err(format!(
                "FEATURES_TOML_OVERRIDE path does not exist: {}",
                override_path.display()
            ));
        }
        return Ok(CatalogSource { path: override_path, kind: CatalogSourceKind::Override });
    }

    let local = manifest_dir.join("features.toml");
    if local.exists() {
        return Ok(CatalogSource { path: local, kind: CatalogSourceKind::Workspace });
    }

    let parent = manifest_dir.parent().and_then(Path::parent).and_then(|p| {
        let path = p.join("features.toml");
        path.exists().then_some(path)
    });
    if let Some(path) = parent {
        return Ok(CatalogSource { path, kind: CatalogSourceKind::Workspace });
    }

    let vendored = manifest_dir.join("features_sot.toml");
    if vendored.exists() {
        return Ok(CatalogSource { path: vendored, kind: CatalogSourceKind::Vendored });
    }

    Err(format!("features catalog not found for manifest dir: {}", manifest_dir.display()))
}

#[cfg(test)]
mod tests {
    use super::resolve_catalog_source_with_override;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn missing_explicit_override_does_not_fall_back_to_workspace_catalog() {
        let root = std::env::temp_dir().join(format!(
            "perl-lsp-build-catalog-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create test catalog directory");
        let workspace_catalog = root.join("features.toml");
        let missing_override = root.join("missing-features.toml");
        fs::write(&workspace_catalog, "[meta]\nversion = 'test'\nlsp_version = 'test'\n")
            .expect("write fallback workspace catalog");

        let result = resolve_catalog_source_with_override(
            &root,
            Some(PathBuf::from(&missing_override)),
        );

        assert!(result.is_err(), "missing explicit override must be terminal");
        assert!(result
            .expect_err("missing explicit override must be terminal")
            .contains("FEATURES_TOML_OVERRIDE path does not exist"));
        fs::remove_dir_all(root).expect("remove test catalog directory");
    }
}

pub fn read_catalog(path: &Path) -> Result<Catalog, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("failed to read features catalog: {e}"))?;
    let catalog: Catalog = toml::from_str(&content)
        .map_err(|e| format!("failed to parse features catalog: {e}"))?;
    catalog.validate()?;
    Ok(catalog)
}

pub fn load_catalog_for_build(manifest_dir: &Path) -> Result<(Catalog, CatalogSource), String> {
    let source = resolve_catalog_source(manifest_dir)?;
    let catalog = read_catalog(&source.path)?;
    Ok((catalog, source))
}

pub fn generate_lsp_catalog_module_at(
    manifest_dir: &Path,
    out_dir: &Path,
    override_path: Option<PathBuf>,
) -> Result<CatalogSource, String> {
    let source = resolve_catalog_source_with_override(manifest_dir, override_path)?;
    let catalog = read_catalog(&source.path)?;
    let code = render_lsp_feature_catalog_module(&catalog, source.comment());
    fs::write(out_dir.join("feature_contracts.rs"), code)
        .map_err(|error| format!("failed to write feature_contracts.rs: {error}"))?;
    Ok(source)
}

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
        "/// Represents a single LSP feature with its metadata and evidence state\n",
    );
    code.push_str("#[derive(Debug, Clone)]\n");
    code.push_str("pub struct Feature {\n");
    code.push_str("    pub id: &'static str,\n");
    code.push_str("    pub spec: &'static str,\n");
    code.push_str("    pub area: &'static str,\n");
    code.push_str("    pub maturity: &'static str,\n");
    code.push_str("    pub direction: &'static str,\n");
    code.push_str("    pub class: &'static str,\n");
    code.push_str("    pub route: &'static str,\n");
    code.push_str("    pub owner: &'static str,\n");
    code.push_str("    pub state_owner: &'static str,\n");
    code.push_str("    pub advertised: bool,\n");
    code.push_str("    pub description: &'static str,\n");
    code.push_str("    pub counts_in_coverage: bool,\n");
    code.push_str("    pub tests: &'static [&'static str],\n");
    code.push_str("}\n\n");
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
    code.push_str("pub const ADVERTISED_LSP_FEATURES: &[&str] = &[\n");
    for id in &advertised {
        code.push_str(&format!("    {:?},\n", id));
    }
    code.push_str("];\n\n");
    code.push_str("pub fn advertised_features() -> &'static [&'static str] { ADVERTISED_LSP_FEATURES }\n\n");
    code.push_str("pub fn has_feature(id: &str) -> bool { ADVERTISED_LSP_FEATURES.contains(&id) }\n");
    code
}
