//! Evidence-backed claim model for the LSP feature catalog (#6731).
//!
//! The catalog's `maturity` field drives runtime capability advertisement and
//! keeps exactly its historical semantics. This module adds the *claim* layer:
//! whether a public surface may call a feature proven, and on what evidence.
//!
//! Claim vocabulary (rendered by generated status surfaces):
//!
//! - `proven` — the row's feature class is GA-eligible and every evidence
//!   class the policy requires is cited by an existing, assertion-bearing
//!   named test;
//! - `preview` — shipped and advertised, but the claim is not yet earned
//!   (missing evidence classes, or the class is not GA-eligible yet);
//! - `planned` — acknowledged protocol surface, not implemented;
//! - `unsupported` — explicitly withdrawn / not applicable;
//! - `not_proven` — present in the catalog but not advertised, so no
//!   capability claim exists to evaluate.
//!
//! A percentage cannot be derived from these counts without reintroducing the
//! defect this module replaces: statuses are reported as documented counts
//! against the full catalog denominator.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// An evidence class a citation can prove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    /// Method/capability/implementation identity exists (catalog row plus a
    /// capability or dispatch receipt).
    StaticMapping,
    /// The server capability or dynamic-registration route is proven.
    CapabilityDispatch,
    /// Handler/provider state and response shape are proven in process.
    PositiveBehavior,
    /// Response payload shape/schema is proven against the protocol surface.
    ShapeSchema,
    /// The shipped binary completed the exchange in a real process.
    RealProcessWire,
    /// A realistic wrong implementation makes this evidence red.
    NegativeControl,
}

impl EvidenceClass {
    /// Stable snake_case label used in generated surfaces and citations.
    pub const fn label(self) -> &'static str {
        match self {
            Self::StaticMapping => "static_mapping",
            Self::CapabilityDispatch => "capability_dispatch",
            Self::PositiveBehavior => "positive_behavior",
            Self::ShapeSchema => "shape_schema",
            Self::RealProcessWire => "real_process_wire",
            Self::NegativeControl => "negative_control",
        }
    }

    /// Parse the snake_case label back into a class.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "static_mapping" => Self::StaticMapping,
            "capability_dispatch" => Self::CapabilityDispatch,
            "positive_behavior" => Self::PositiveBehavior,
            "shape_schema" => Self::ShapeSchema,
            "real_process_wire" => Self::RealProcessWire,
            "negative_control" => Self::NegativeControl,
            _ => return None,
        })
    }
}

/// A declared claim on a catalog row. `not_proven` is derived, never declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredClaim {
    /// Evidence-backed proof per the class policy.
    Proven,
    /// Shipped and advertised, claim not yet earned.
    Preview,
    /// Acknowledged surface, not implemented.
    Planned,
    /// Explicitly withdrawn or not applicable.
    Unsupported,
}

/// The computed claim for one catalog row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowClaim {
    /// The rendered status.
    pub status: ClaimStatus,
    /// Evidence classes still missing for `preview` rows seeking `proven`.
    pub missing_evidence: Vec<EvidenceClass>,
    /// Set when the class exists but is not GA-eligible yet.
    pub ineligible_reason: Option<String>,
}

/// The rendered claim status of a catalog row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClaimStatus {
    /// Every required evidence class is cited and the class is GA-eligible.
    Proven,
    /// Shipped and advertised; evidence incomplete or class ineligible.
    Preview,
    /// Planned work item.
    Planned,
    /// Explicitly withdrawn / not applicable.
    Unsupported,
    /// Not advertised — no capability claim exists to evaluate.
    NotProven,
}

impl ClaimStatus {
    /// Stable snake_case label used in generated surfaces and citations.
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

/// One structured evidence citation on a catalog row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceCitation {
    /// Which evidence class this citation proves.
    pub proves: EvidenceClass,
    /// Repository-relative target: `path/to/file.rs::test_name`, or a bare
    /// path when the class is proven by artifact existence alone
    /// (`static_mapping` may cite capability snapshots this way).
    pub test: String,
}

/// Per-class GA policy.
#[derive(Debug, Clone, Deserialize)]
pub struct ClassPolicy {
    /// Class identifier (`request_response`, `dap`, ...).
    pub id: String,
    /// Whether this class may claim `proven` at all.
    pub ga_eligible: bool,
    /// Evidence classes required before any `proven` claim is accepted.
    #[serde(default)]
    pub required_evidence: Vec<EvidenceClass>,
    /// Who owns landing the missing runtime proof, when ineligible.
    #[serde(default)]
    pub blocked_until: Option<String>,
}

/// The GA evidence policy: which feature classes exist, what they require,
/// and per-row classification overrides.
#[derive(Debug, Clone, Deserialize)]
pub struct GaEvidencePolicy {
    /// Policy schema marker; must be `1`.
    pub schema_version: u32,
    /// Policy name marker; must be `ga-evidence-policy`.
    pub policy: String,
    /// Class assumed when a row does not declare one and no assignment covers
    /// it.
    pub default_class: String,
    /// The declared feature classes and their evidence requirements.
    #[serde(rename = "class")]
    pub classes: Vec<ClassPolicy>,
    /// Per-row classification overrides for rows without an inline class.
    #[serde(default, rename = "class_assignment")]
    pub class_assignments: Vec<ClassAssignment>,
}

/// One policy-side classification override.
#[derive(Debug, Clone, Deserialize)]
pub struct ClassAssignment {
    /// The catalog feature id being classified.
    pub feature_id: String,
    /// The governing class id.
    pub class: String,
}

impl GaEvidencePolicy {
    /// Load and structurally validate a policy TOML file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("failed to read GA evidence policy {}: {e}", path.display()))?;
        let policy: GaEvidencePolicy =
            toml::from_str(&raw).map_err(|e| format!("failed to parse GA evidence policy: {e}"))?;
        policy.validate_structure()?;
        Ok(policy)
    }

    fn validate_structure(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!("unsupported GA evidence policy schema {}", self.schema_version));
        }
        if self.classes.is_empty() {
            return Err(String::from("policy declares no [class] entries"));
        }
        let mut seen = BTreeSet::new();
        for class in &self.classes {
            if class.id.trim().is_empty() {
                return Err(String::from("class id must not be empty"));
            }
            if !seen.insert(class.id.as_str()) {
                return Err(format!("duplicate class id {}", class.id));
            }
        }
        if !seen.contains(self.default_class.as_str()) {
            return Err(format!("default_class {:?} has no [[class]] entry", self.default_class));
        }
        let mut assigned = BTreeSet::new();
        for assignment in &self.class_assignments {
            if !seen.contains(assignment.class.as_str()) {
                return Err(format!(
                    "class_assignment {} references unknown class {:?}",
                    assignment.feature_id, assignment.class
                ));
            }
            if !assigned.insert(assignment.feature_id.as_str()) {
                return Err(format!(
                    "class_assignment duplicates feature id {}",
                    assignment.feature_id
                ));
            }
        }
        Ok(())
    }

    fn class_by_id(&self, class_id: &str) -> Option<&ClassPolicy> {
        self.classes.iter().find(|c| c.id == class_id)
    }

    /// Resolve the governing class for a row: explicit declaration wins, then
    /// a policy assignment, then `default_class`.
    pub fn class_for(&self, feature_id: &str, declared: Option<&str>) -> Result<&ClassPolicy, String> {
        let class_id = if let Some(declared) = declared {
            declared
        } else if let Some(assignment) =
            self.class_assignments.iter().find(|a| a.feature_id == feature_id)
        {
            &assignment.class
        } else {
            &self.default_class
        };
        self.class_by_id(class_id)
            .ok_or_else(|| format!("feature {feature_id} declares unknown class {class_id:?}"))
    }
}

/// A single catalog validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceViolation {
    /// The feature id the violation attaches to.
    pub feature_id: String,
    /// Human-actionable description of the failure.
    pub detail: String,
}

/// Validate one citation target: the referenced file must exist under
/// `root`; when a `::function` suffix names a test, the function must exist
/// in that file and contain at least one assertion macro.
///
/// The assertion check is a deliberate heuristic (macro-name presence inside
/// the function body): it catches the historical failure mode — a contract
/// test that documents instead of asserts — without attempting full Rust
/// parsing. A determined adversary can defeat a heuristic; the deeper
/// protection is that cited tests are real named targets CI executes.
fn validate_citation(
    root: &Path,
    feature_id: &str,
    citation: &EvidenceCitation,
    violations: &mut Vec<EvidenceViolation>,
) {
    let (path_part, fn_part) = split_citation(&citation.test);
    let absolute = root.join(&path_part);
    if !absolute.is_file() {
        violations.push(EvidenceViolation {
            feature_id: feature_id.to_string(),
            detail: format!(
                "evidence citation for {} points at missing path: {}",
                citation.proves.label(),
                path_part
            ),
        });
        return;
    }
    let Some(fn_name) = fn_part else {
        if citation.proves != EvidenceClass::StaticMapping {
            violations.push(EvidenceViolation {
                feature_id: feature_id.to_string(),
                detail: format!(
                    "evidence citation for {} must name a test function (path::fn), not just a file: {}",
                    citation.proves.label(),
                    citation.test
                ),
            });
        }
        return;
    };
    let content = match fs::read_to_string(&absolute) {
        Ok(content) => content,
        Err(e) => {
            violations.push(EvidenceViolation {
                feature_id: feature_id.to_string(),
                detail: format!("cannot read cited test {}: {e}", path_part),
            });
            return;
        }
    };
    let Some(body) = function_body(&content, &fn_name) else {
        violations.push(EvidenceViolation {
            feature_id: feature_id.to_string(),
            detail: format!(
                "cited test function {}::{} does not exist",
                path_part, fn_name
            ),
        });
        return;
    };
    // Assertion presence is checked with a deliberate heuristic over macro
    // and Result-validator idioms: `assert*!`, `ok_or(..)` chains propagated
    // by `?`, `.map_err(..)?`, and `bail!` all fail the test when reality
    // disagrees. A plain `println!` contract documents instead of proving,
    // which is exactly the historical defect being guarded against.
    let asserting =
        body.contains("assert") || body.contains("ok_or(") || body.contains("map_err(") || body.contains("bail!");
    if !asserting {
        violations.push(EvidenceViolation {
            feature_id: feature_id.to_string(),
            detail: format!(
                "cited test function {}::{} asserts nothing; documentation is not evidence",
                path_part, fn_name
            ),
        });
    }
}

fn split_citation(test: &str) -> (String, Option<String>) {
    match test.rsplit_once("::") {
        // Distinguish `path/file.rs::fn_name` from Windows drive letters and
        // plain paths without a suffix.
        Some((path, name))
            if !name.is_empty()
                && !name.contains('/')
                && !name.contains('\\')
                && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') =>
        {
            (path.to_string(), Some(name.to_string()))
        }
        _ => (test.to_string(), None),
    }
}

/// Extract the body of `fn {name}` via brace matching, ignoring a same-prefix
/// function name only when followed by a non-identifier byte.
fn function_body(content: &str, name: &str) -> Option<String> {
    let needle = format!("fn {name}");
    let mut search_from = 0;
    while let Some(pos) = content[search_from..].find(&needle) {
        let absolute = search_from + pos;
        let after = &content[absolute + needle.len()..];
        let boundary_ok = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        let open = content[absolute..].find('{');
        if let Some(open_rel) = open.filter(|_| boundary_ok) {
            let open_abs = absolute + open_rel;
            let bytes = content.as_bytes();
            let mut depth = 0usize;
            let mut in_string = false;
            let mut escape = false;
            for (idx, &b) in bytes[open_abs..].iter().enumerate() {
                let ch = b as char;
                if escape {
                    escape = false;
                    continue;
                }
                match ch {
                    '\\' if in_string => escape = true,
                    '"' => in_string = !in_string,
                    '{' if !in_string => depth += 1,
                    '}' if !in_string => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return Some(content[open_abs..=open_abs + idx].to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        search_from = absolute + needle.len();
    }
    None
}

/// Validate the whole catalog against the policy. Returns every violation so
/// a single run reports the complete repair list.
pub fn validate_catalog_evidence(
    root: &Path,
    catalog: &crate::feature_catalog::Catalog,
    policy: &GaEvidencePolicy,
) -> Result<(), Vec<EvidenceViolation>> {
    use crate::feature_catalog::Maturity;

    let mut violations = Vec::new();
    for feature in catalog.features() {
        for citation in &feature.evidence {
            validate_citation(root, &feature.id, citation, &mut violations);
        }

        if matches!(feature.maturity, Maturity::Ga | Maturity::Production)
            && feature.advertised
            && feature.counts_in_coverage
            && feature.tests.is_empty()
            && feature.evidence.is_empty()
        {
            violations.push(EvidenceViolation {
                feature_id: feature.id.clone(),
                detail: String::from(
                    "GA row advertises without citing any test or evidence; \
                     either cite receipts or declare claim = \"preview\"",
                ),
            });
        }

        if feature.claim == Some(DeclaredClaim::Proven) {
            let class = match policy.class_for(&feature.id, feature.class.as_deref()) {
                Ok(class) => class,
                Err(detail) => {
                    violations.push(EvidenceViolation {
                        feature_id: feature.id.clone(),
                        detail,
                    });
                    continue;
                }
            };
            if !class.ga_eligible {
                violations.push(EvidenceViolation {
                    feature_id: feature.id.clone(),
                    detail: format!(
                        "claim = \"proven\" rejected: class is not GA-eligible{}",
                        class
                            .blocked_until
                            .as_deref()
                            .map(|owner| format!(" ({owner})"))
                            .unwrap_or_default()
                    ),
                });
                continue;
            }
            let cited: BTreeSet<EvidenceClass> =
                feature.evidence.iter().map(|c| c.proves).collect();
            let missing: Vec<&EvidenceClass> = class
                .required_evidence
                .iter()
                .filter(|required| !cited.contains(required))
                .collect();
            if !missing.is_empty() {
                let labels = missing
                    .iter()
                    .map(|class| class.label())
                    .collect::<Vec<_>>()
                    .join(", ");
                violations.push(EvidenceViolation {
                    feature_id: feature.id.clone(),
                    detail: format!("claim = \"proven\" lacks required evidence: {labels}"),
                });
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Compute the rendered claim for one row.
pub fn effective_claim(
    feature: &crate::feature_catalog::Feature,
    policy: &GaEvidencePolicy,
) -> RowClaim {
    use crate::feature_catalog::Maturity;

    if feature.claim == Some(DeclaredClaim::Unsupported) {
        return RowClaim {
            status: ClaimStatus::Unsupported,
            missing_evidence: Vec::new(),
            ineligible_reason: None,
        };
    }
    if feature.maturity == Maturity::Planned || feature.claim == Some(DeclaredClaim::Planned) {
        return RowClaim {
            status: ClaimStatus::Planned,
            missing_evidence: Vec::new(),
            ineligible_reason: None,
        };
    }
    if !feature.advertised {
        return RowClaim {
            status: ClaimStatus::NotProven,
            missing_evidence: Vec::new(),
            ineligible_reason: None,
        };
    }

    // Claiming less than earned is always allowed: an explicit `preview`
    // declaration holds even when the cited evidence would support `proven`
    // (for example while a cited receipt is red in execution).
    if feature.claim == Some(DeclaredClaim::Preview) {
        return RowClaim {
            status: ClaimStatus::Preview,
            missing_evidence: Vec::new(),
            ineligible_reason: Some(String::from("row explicitly declares claim = \"preview\"")),
        };
    }

    let class = policy
        .class_for(&feature.id, feature.class.as_deref())
        .ok();
    let class = match class {
        Some(class) => class,
        None => {
            return RowClaim {
                status: ClaimStatus::Preview,
                missing_evidence: Vec::new(),
                ineligible_reason: Some(String::from("row declares an unknown feature class")),
            };
        }
    };
    if !class.ga_eligible {
        return RowClaim {
            status: ClaimStatus::Preview,
            missing_evidence: Vec::new(),
            ineligible_reason: class.blocked_until.clone().or(Some(String::from(
                "feature class is not GA-eligible yet",
            ))),
        };
    }

    let cited: BTreeSet<EvidenceClass> = feature.evidence.iter().map(|c| c.proves).collect();
    let missing: Vec<EvidenceClass> = class
        .required_evidence
        .iter()
        .copied()
        .filter(|required| !cited.contains(required))
        .collect();
    if missing.is_empty() {
        RowClaim { status: ClaimStatus::Proven, missing_evidence: Vec::new(), ineligible_reason: None }
    } else {
        RowClaim {
            status: ClaimStatus::Preview,
            missing_evidence: missing,
            ineligible_reason: None,
        }
    }
}

/// Per-area claim counts against the full catalog denominator.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AreaClaimCounts {
    /// Rows whose evidence-backed claim is `proven`.
    pub proven: usize,
    /// Rows shipped and advertised but not yet proven.
    pub preview: usize,
    /// Planned rows.
    pub planned: usize,
    /// Explicitly withdrawn / not applicable rows.
    pub unsupported: usize,
    /// Rows present but not advertised.
    pub not_proven: usize,
    /// The area's full denominator.
    pub total: usize,
}

/// Aggregate claim counts per area; the counts partition the catalog
/// denominator by construction.
pub fn claim_counts_by_area(
    catalog: &crate::feature_catalog::Catalog,
    policy: &GaEvidencePolicy,
) -> BTreeMap<String, AreaClaimCounts> {
    let mut areas: BTreeMap<String, AreaClaimCounts> = BTreeMap::new();
    for feature in catalog.features() {
        let entry = areas.entry(feature.area.clone()).or_default();
        entry.total += 1;
        match effective_claim(feature, policy).status {
            ClaimStatus::Proven => entry.proven += 1,
            ClaimStatus::Preview => entry.preview += 1,
            ClaimStatus::Planned => entry.planned += 1,
            ClaimStatus::Unsupported => entry.unsupported += 1,
            ClaimStatus::NotProven => entry.not_proven += 1,
        }
    }
    areas
}

/// Walk upward from `start` until a directory containing `features.toml` is
/// found; that directory is the catalog authority root.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join("features.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

// ---------------------------------------------------------------------------
// Deterministic projection rendering
// ---------------------------------------------------------------------------

impl crate::feature_catalog::Meta {
    /// Meta as re-emitted into generated projections: the declared compliance
    /// percentage is deliberately dropped — it is uncomputed decoration that
    /// can silently disagree with reality.
    fn projection_fields(&self) -> String {
        format!("version = {}\nlsp_version = {}", quote_toml(&self.version), quote_toml(&self.lsp_version))
    }
}

impl DeclaredClaim {
    /// Stable snake_case label used in generated surfaces and citations.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Preview => "preview",
            Self::Planned => "planned",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Escape a string as a TOML basic string (including the surrounding quotes).
fn quote_toml(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a crate-local vendored projection of the authority catalog.
///
/// The output is byte-deterministic for identical semantic content: fields are
/// emitted in fixed order, rows are sorted by `(area, id)`, and optional
/// fields appear only when present.
pub fn render_vendored_projection(catalog: &crate::feature_catalog::Catalog) -> String {
    let mut out = String::new();
    out.push_str("# @generated from the workspace-root `features.toml` authority by\n");
    out.push_str("# `cargo xtask catalog-authority sync-vendored`. DO NOT EDIT.\n");
    out.push_str("# Drift from the authority fails `catalog-authority check-drift`.\n\n");
    out.push_str("[meta]\n");
    out.push_str(&catalog.meta.projection_fields());
    out.push_str("\n\n");

    let mut sorted = catalog.feature.clone();
    sorted.sort_by(|a, b| a.area.cmp(&b.area).then_with(|| a.id.cmp(&b.id)));
    for feature in &sorted {
        out.push_str("[[feature]]\n");
        out.push_str(&format!("id = {}\n", quote_toml(&feature.id)));
        if !feature.spec.is_empty() {
            out.push_str(&format!("spec = {}\n", quote_toml(&feature.spec)));
        }
        if !feature.area.is_empty() {
            out.push_str(&format!("area = {}\n", quote_toml(&feature.area)));
        }
        out.push_str(&format!("maturity = {}\n", quote_toml(feature.maturity.label())));
        out.push_str(&format!("advertised = {}\n", feature.advertised));
        if !feature.counts_in_coverage {
            out.push_str("counts_in_coverage = false\n");
        }
        if let Some(class) = &feature.class {
            out.push_str(&format!("class = {}\n", quote_toml(class)));
        }
        if let Some(claim) = feature.claim {
            out.push_str(&format!("claim = {}\n", quote_toml(claim.label())));
        }
        if !feature.tests.is_empty() {
            let items =
                feature.tests.iter().map(|t| quote_toml(t)).collect::<Vec<_>>().join(", ");
            out.push_str(&format!("tests = [{items}]\n"));
        }
        if !feature.evidence.is_empty() {
            let items = feature
                .evidence
                .iter()
                .map(|c| {
                    format!(
                        "{{ proves = {}, test = {} }}",
                        quote_toml(c.proves.label()),
                        quote_toml(&c.test)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("evidence = [{items}]\n"));
        }
        out.push_str(&format!("description = {}\n", quote_toml(&feature.description)));
        out.push('\n');
    }
    out
}

/// Compare two catalogs semantically: same meta and the same set of rows with
/// equal field values, independent of declaration order. Returns a
/// human-actionable divergence description.
pub fn semantic_divergence(
    authority: &crate::feature_catalog::Catalog,
    candidate: &crate::feature_catalog::Catalog,
) -> Result<(), String> {
    use crate::feature_catalog::Feature;
    if authority.meta.version != candidate.meta.version
        || authority.meta.lsp_version != candidate.meta.lsp_version
    {
        return Err(format!(
            "meta diverges: authority version={} lsp={} vs candidate version={} lsp={}",
            authority.meta.version,
            authority.meta.lsp_version,
            candidate.meta.version,
            candidate.meta.lsp_version
        ));
    }
    let key = |f: &Feature| (f.area.clone(), f.id.clone());
    let mut a_rows: BTreeMap<(String, String), &Feature> = BTreeMap::new();
    for f in authority.features() {
        if a_rows.insert(key(f), f).is_some() {
            return Err(format!("authority declares duplicate id {}", f.id));
        }
    }
    let mut b_rows: BTreeMap<(String, String), &Feature> = BTreeMap::new();
    for f in candidate.features() {
        if b_rows.insert(key(f), f).is_some() {
            return Err(format!("candidate declares duplicate id {}", f.id));
        }
    }
    let missing_in_candidate: Vec<&str> =
        a_rows.keys().filter(|k| !b_rows.contains_key(*k)).map(|k| k.1.as_str()).collect();
    let unexpected_in_candidate: Vec<&str> =
        b_rows.keys().filter(|k| !a_rows.contains_key(*k)).map(|k| k.1.as_str()).collect();
    if !missing_in_candidate.is_empty() || !unexpected_in_candidate.is_empty() {
        return Err(format!(
            "row sets diverge: missing from candidate [{}], unexpected in candidate [{}]",
            missing_in_candidate.join(", "),
            unexpected_in_candidate.join(", ")
        ));
    }
    for (identity, a) in &a_rows {
        let Some(b) = b_rows.get(identity) else {
            return Err(format!("row {} vanished during comparison", identity.1));
        };
        if **a != **b {
            return Err(format!(
                "row {} differs from the authority (maturity/advertised/class/claim/tests/evidence/description)",
                identity.1
            ));
        }
    }
    Ok(())
}

/// Render the claim-status table shared by every generated status surface:
/// one row per area plus an Overall row, columns per claim status, totals
/// against the full catalog denominator. No percentage is rendered.
pub fn render_claim_status_table(
    catalog: &crate::feature_catalog::Catalog,
    policy: &GaEvidencePolicy,
) -> Result<String, String> {
    let areas = claim_counts_by_area(catalog, policy);
    let mut lines = Vec::new();
    lines.push(String::from(
        "| Area | proven | preview | planned | not_proven | unsupported | Total |",
    ));
    lines.push(String::from("|------|-------:|--------:|--------:|-----------:|------------:|------:|"));
    let mut overall = AreaClaimCounts::default();
    for (area, counts) in &areas {
        lines.push(format!(
            "| {area} | {} | {} | {} | {} | {} | {} |",
            counts.proven,
            counts.preview,
            counts.planned,
            counts.not_proven,
            counts.unsupported,
            counts.total
        ));
        overall.proven += counts.proven;
        overall.preview += counts.preview;
        overall.planned += counts.planned;
        overall.not_proven += counts.not_proven;
        overall.unsupported += counts.unsupported;
        overall.total += counts.total;
    }
    if overall.total != catalog.feature.len() {
        return Err(format!(
            "claim counts reconcile to {} rows but the catalog declares {}",
            overall.total,
            catalog.feature.len()
        ));
    }
    lines.push(format!(
        "| **Overall** | **{}** | **{}** | **{}** | **{}** | **{}** | **{}** |",
        overall.proven,
        overall.preview,
        overall.planned,
        overall.not_proven,
        overall.unsupported,
        overall.total
    ));
    Ok(lines.join("\n"))
}

/// Parse the Overall row of a rendered claim-status table back into counts.
/// Used by gates to verify a rendered surface against the live catalog.
pub fn parse_claim_table_overall(table: &str) -> Option<[usize; 6]> {
    let row = table.lines().find(|line| line.contains("**Overall**"))?;
    let cells: Vec<usize> = row
        .split('|')
        .filter_map(|cell| {
            let cleaned = cell.trim().trim_matches('*');
            cleaned.parse::<usize>().ok()
        })
        .collect();
    if cells.len() != 6 {
        return None;
    }
    Some([cells[0], cells[1], cells[2], cells[3], cells[4], cells[5]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature_catalog::{Catalog, Feature, Meta, Maturity};
    use perl_tdd_support::{must, must_some};

    fn row(id: &str, maturity: Maturity, advertised: bool) -> Feature {
        Feature {
            id: id.to_string(),
            spec: "LSP 3.18".to_string(),
            area: "text_document".to_string(),
            maturity,
            advertised,
            tests: Vec::new(),
            counts_in_coverage: true,
            description: String::new(),
            class: None,
            claim: None,
            evidence: Vec::new(),
        }
    }

    fn catalog(rows: Vec<Feature>) -> Catalog {
        Catalog {
            meta: Meta { version: "0.0.0".to_string(), lsp_version: "3.18".to_string(), compliance_percent: None },
            feature: rows,
        }
    }

    fn policy() -> GaEvidencePolicy {
        let raw = r##"
schema_version = 1
policy = "ga-evidence-policy"
default_class = "request_response"

[[class]]
id = "request_response"
ga_eligible = true
required_evidence = ["capability_dispatch", "positive_behavior", "shape_schema", "real_process_wire", "negative_control"]

[[class]]
id = "server_initiated_request"
ga_eligible = false
required_evidence = ["capability_dispatch", "positive_behavior"]
blocked_until = "#6722 #6724"

[[class_assignment]]
feature_id = "lsp.window_show_message_request"
class = "server_initiated_request"
"##;
        must(toml::from_str::<GaEvidencePolicy>(raw))
    }

    fn temp_workspace() -> tempfile::TempDir {
        must(tempfile::TempDir::new())
    }

    fn write_test_file(dir: &Path, rel: &str, asserts: bool) {
        let path = dir.join(rel);
        must(fs::create_dir_all(must_some(path.parent())));
        let body = if asserts {
            "fn cited_receipt() {\n    assert_eq!(1 + 1, 2);\n}\n"
        } else {
            "fn cited_receipt() {\n    println!(\"documents the contract\");\n}\n"
        };
        must(fs::write(path, format!("#[test]\n{body}")));
    }

    fn citation(proves: EvidenceClass, test: &str) -> EvidenceCitation {
        EvidenceCitation { proves, test: test.to_string() }
    }

    #[test]
    fn proven_requires_every_policy_class() {
        let dir = temp_workspace();
        write_test_file(dir.path(), "crates/x/tests/a.rs", true);
        let policy = policy();
        let required = [
            EvidenceClass::CapabilityDispatch,
            EvidenceClass::PositiveBehavior,
            EvidenceClass::ShapeSchema,
            EvidenceClass::RealProcessWire,
            EvidenceClass::NegativeControl,
        ];
        let mut feature = row("lsp.hover", Maturity::Ga, true);
        feature.claim = Some(DeclaredClaim::Proven);

        // One class short: the gate must name the missing evidence class.
        for class in required[..4].iter().copied() {
            feature.evidence.push(citation(class, "crates/x/tests/a.rs::cited_receipt"));
        }
        let incomplete = catalog(vec![feature.clone()]);
        let violations = invalidate(&dir.path(), &incomplete, &policy);
        assert!(
            violations
                .iter()
                .any(|v| v.detail.contains("lacks required evidence")
                    && v.detail.contains(required[4].label())),
            "{violations:?}"
        );

        // The complete citation set earns the claim.
        feature.evidence = required
            .iter()
            .map(|class| citation(*class, "crates/x/tests/a.rs::cited_receipt"))
            .collect();
        let complete = catalog(vec![feature]);
        assert!(invalidate(&dir.path(), &complete, &policy).is_empty());
    }

    fn invalidate(
        root: &Path,
        catalog: &Catalog,
        policy: &GaEvidencePolicy,
    ) -> Vec<EvidenceViolation> {
        match super::validate_catalog_evidence(root, catalog, policy) {
            Ok(()) => Vec::new(),
            Err(violations) => violations,
        }
    }

    #[test]
    fn unproven_row_marked_proven_is_rejected() {
        let dir = temp_workspace();
        let policy = policy();
        let mut feature = row("lsp.preview_only", Maturity::Ga, true);
        feature.claim = Some(DeclaredClaim::Proven);
        let violations = invalidate(dir.path(), &catalog(vec![feature]), &policy);
        assert!(violations.iter().any(|v| v.detail.contains("lacks required evidence")));
    }

    #[test]
    fn ineligible_class_cannot_claim_proven_even_with_full_citations() {
        let dir = temp_workspace();
        write_test_file(dir.path(), "crates/x/tests/a.rs", true);
        let policy = policy();
        let mut feature = row("lsp.window_show_message_request", Maturity::Ga, true);
        feature.claim = Some(DeclaredClaim::Proven);
        feature.evidence = vec![
            citation(EvidenceClass::CapabilityDispatch, "crates/x/tests/a.rs::cited_receipt"),
            citation(EvidenceClass::PositiveBehavior, "crates/x/tests/a.rs::cited_receipt"),
        ];
        let violations = invalidate(dir.path(), &catalog(vec![feature]), &policy);
        assert!(
            violations
                .iter()
                .any(|v| v.detail.contains("not GA-eligible") && v.detail.contains("#6722")),
            "{violations:?}"
        );
    }

    #[test]
    fn dangling_citation_path_is_rejected() {
        let dir = temp_workspace();
        let policy = policy();
        let mut feature = row("lsp.hover", Maturity::Preview, true);
        feature.evidence = vec![citation(
            EvidenceClass::PositiveBehavior,
            "crates/gone/tests/never_written.rs::some_test",
        )];
        let violations = invalidate(dir.path(), &catalog(vec![feature]), &policy);
        assert!(violations.iter().any(|v| v.detail.contains("missing path")));
    }

    #[test]
    fn missing_named_function_is_rejected() {
        let dir = temp_workspace();
        write_test_file(dir.path(), "crates/x/tests/a.rs", true);
        let policy = policy();
        let mut feature = row("lsp.hover", Maturity::Preview, true);
        feature.evidence =
            vec![citation(EvidenceClass::PositiveBehavior, "crates/x/tests/a.rs::does_not_exist")];
        let violations = invalidate(dir.path(), &catalog(vec![feature]), &policy);
        assert!(violations.iter().any(|v| v.detail.contains("does not exist")));
    }

    #[test]
    fn assertion_free_documentation_test_is_rejected() {
        let dir = temp_workspace();
        write_test_file(dir.path(), "crates/x/tests/a.rs", false);
        let policy = policy();
        let mut feature = row("lsp.hover", Maturity::Preview, true);
        feature.evidence =
            vec![citation(EvidenceClass::PositiveBehavior, "crates/x/tests/a.rs::cited_receipt")];
        let violations = invalidate(dir.path(), &catalog(vec![feature]), &policy);
        assert!(
            violations
                .iter()
                .any(|v| v.detail.contains("asserts nothing")),
            "{violations:?}"
        );
    }

    #[test]
    fn ga_row_without_any_receipt_is_rejected() {
        let dir = temp_workspace();
        let policy = policy();
        let feature = row("lsp.silent_ga", Maturity::Ga, true);
        let violations = invalidate(dir.path(), &catalog(vec![feature]), &policy);
        assert!(violations.iter().any(|v| v.detail.contains("without citing any test")));
    }

    #[test]
    fn shipped_but_unproven_rows_render_preview_with_missing_classes() {
        let policy = policy();
        let mut feature = row("lsp.hover", Maturity::Ga, true);
        feature.evidence =
            vec![citation(EvidenceClass::PositiveBehavior, "unused-because-not-validated-here")];
        let claim = effective_claim(&feature, &policy);
        assert_eq!(claim.status, ClaimStatus::Preview);
        assert_eq!(claim.missing_evidence.len(), 4);
        assert!(claim.missing_evidence.contains(&EvidenceClass::RealProcessWire));
    }

    #[test]
    fn explicit_preview_holds_even_with_complete_evidence() {
        let policy = policy();
        let mut feature = row("lsp.receipt_red", Maturity::Ga, true);
        feature.claim = Some(DeclaredClaim::Preview);
        feature.evidence = [
            EvidenceClass::CapabilityDispatch,
            EvidenceClass::PositiveBehavior,
            EvidenceClass::ShapeSchema,
            EvidenceClass::RealProcessWire,
            EvidenceClass::NegativeControl,
        ]
        .iter()
        .map(|class| citation(*class, "unused"))
        .collect();
        let claim = effective_claim(&feature, &policy);
        assert_eq!(claim.status, ClaimStatus::Preview, "claiming less than earned is allowed");
    }

    #[test]
    fn planned_not_advertised_and_unsupported_statuses_are_mechanical() {
        let policy = policy();
        let planned = row("lsp.future", Maturity::Planned, false);
        assert_eq!(effective_claim(&planned, &policy).status, ClaimStatus::Planned);

        let unadvertised = row("lsp.plumbing", Maturity::Ga, false);
        assert_eq!(effective_claim(&unadvertised, &policy).status, ClaimStatus::NotProven);

        let mut unsupported = row("lsp.withdrawn", Maturity::Ga, true);
        unsupported.claim = Some(DeclaredClaim::Unsupported);
        assert_eq!(effective_claim(&unsupported, &policy).status, ClaimStatus::Unsupported);
    }

    #[test]
    fn counts_reconcile_to_the_catalog_denominator() {
        let policy = policy();
        let mut proven = row("lsp.done", Maturity::Ga, true);
        proven.evidence = (0..5)
            .map(|i| {
                citation(
                    [
                        EvidenceClass::CapabilityDispatch,
                        EvidenceClass::PositiveBehavior,
                        EvidenceClass::ShapeSchema,
                        EvidenceClass::RealProcessWire,
                        EvidenceClass::NegativeControl,
                    ][i],
                    "unused",
                )
            })
            .collect();
        let rows = vec![proven, row("lsp.shipped", Maturity::Ga, true), row("lsp.later", Maturity::Planned, false)];
        let areas = claim_counts_by_area(&catalog(rows), &policy);
        let counts = must_some(areas.get("text_document"));
        assert_eq!(counts.total, 3);
        assert_eq!(counts.proven + counts.preview + counts.planned + counts.unsupported + counts.not_proven, counts.total);
        assert_eq!(counts.proven, 1);
        assert_eq!(counts.preview, 1);
        assert_eq!(counts.planned, 1);
    }

    #[test]
    fn function_body_ignores_same_prefix_names_and_strings() {
        let content = "fn cited_receipt_extra() { unreachable!(); }\nfn cited_receipt() {\n    let s = \"}{\";\n    assert!(s.len() == 2);\n}\n";
        let body = must_some(function_body(content, "cited_receipt"));
        assert!(body.contains("assert!(s.len() == 2)"));
        assert!(!body.contains("unreachable"));
    }

    #[test]
    fn citation_split_distinguishes_windows_paths_from_fn_suffixes() {
        let (path, func) = split_citation("crates/x/tests/a.rs::the_test");
        assert_eq!(path, "crates/x/tests/a.rs");
        assert_eq!(func.as_deref(), Some("the_test"));

        let (path, func) = split_citation("crates/x/snapshot.json");
        assert_eq!(path, "crates/x/snapshot.json");
        assert_eq!(func, None);
    }

    #[test]
    fn policy_loader_rejects_unknown_assignments() {
        let raw = r#"
schema_version = 1
policy = "ga-evidence-policy"
default_class = "request_response"
[[class]]
id = "request_response"
ga_eligible = true
[[class_assignment]]
feature_id = "x"
class = "nonexistent"
"#;
        let parsed: GaEvidencePolicy = must(toml::from_str(raw));
        assert!(parsed.validate_structure().is_err());
    }

    #[test]
    fn policy_loader_rejects_duplicate_class_ids() {
        let raw = r#"
schema_version = 1
policy = "ga-evidence-policy"
default_class = "a"
[[class]]
id = "a"
ga_eligible = true
[[class]]
id = "a"
ga_eligible = false
"#;
        let parsed: GaEvidencePolicy = must(toml::from_str(raw));
        assert!(parsed.validate_structure().is_err());
    }

    #[test]
    fn find_repo_root_locates_the_authority_catalog() {
        let dir = temp_workspace();
        must(fs::create_dir_all(dir.path().join("crates/deeper/nested")));
        must(fs::write(dir.path().join("features.toml"), "[meta]\nversion='0'\nlsp_version='3.18'\n"));
        let found = must_some(find_repo_root(&dir.path().join("crates/deeper/nested")));
        assert_eq!(found, dir.path());
    }

    fn sample_authority() -> Catalog {
        let mut proven = row("lsp.done", Maturity::Ga, true);
        proven.spec = "LSP 3.17".to_string();
        proven.class = Some("request_response".to_string());
        proven.claim = Some(DeclaredClaim::Proven);
        proven.evidence = [
            EvidenceClass::CapabilityDispatch,
            EvidenceClass::PositiveBehavior,
            EvidenceClass::ShapeSchema,
            EvidenceClass::RealProcessWire,
            EvidenceClass::NegativeControl,
        ]
        .iter()
        .map(|class| citation(*class, "crates/x/tests/a.rs::cited_receipt"))
        .collect();
        proven.tests = vec!["crates/x/tests/a.rs".to_string()];
        proven.description = "quotes \"inside\" description\nsecond line".to_string();

        let mut plumbing = row("lsp.hidden", Maturity::Preview, false);
        plumbing.counts_in_coverage = false;
        plumbing.area = "protocol".to_string();

        Catalog {
            meta: Meta { version: "9.9.9".to_string(), lsp_version: "3.18".to_string(), compliance_percent: None },
            feature: vec![proven, plumbing],
        }
    }

    #[test]
    fn vendored_projection_round_trips_semantically() {
        let authority = sample_authority();
        let rendered = render_vendored_projection(&authority);

        // Deterministic: identical content renders byte-identically.
        assert_eq!(rendered, render_vendored_projection(&authority));

        // The dropped decoration must not reappear.
        assert!(!rendered.contains("compliance_percent"));

        // Round trip: a projection parses back semantically equal.
        let reparsed: Catalog = must(toml::from_str(&rendered));
        assert!(semantic_divergence(&authority, &reparsed).is_ok());

        // Order independence: shuffling the authority rows does not change
        // the rendered bytes.
        let mut shuffled = authority.clone();
        shuffled.feature.reverse();
        assert_eq!(render_vendored_projection(&shuffled), rendered);
    }

    #[test]
    fn vendored_drift_is_detected_with_actionable_detail() {
        let authority = sample_authority();
        let rendered = render_vendored_projection(&authority);
        let drifted = rendered.replace("maturity = \"ga\"", "maturity = \"preview\"");
        let reparsed: Catalog = must(toml::from_str(&drifted));
        let message = must_some(semantic_divergence(&authority, &reparsed).err());
        assert!(message.contains("row lsp.done differs"), "{message}");
    }

    #[test]
    fn missing_and_extra_rows_are_reported() {
        let authority = sample_authority();
        let mut candidate = sample_authority();
        candidate.feature.remove(1);
        let message = must_some(semantic_divergence(&authority, &candidate).err());
        assert!(message.contains("missing from candidate") && message.contains("lsp.hidden"));

        let mut extra = sample_authority();
        extra.feature.push(row("lsp.brand_new", Maturity::Ga, true));
        let message = must_some(semantic_divergence(&extra, &sample_authority()).err());
        assert!(message.contains("unexpected in candidate") && message.contains("lsp.brand_new"));
    }

    #[test]
    fn claim_table_sums_reconcile_and_overall_row_parses_back() {
        let policy = policy();
        let catalog = sample_authority();
        let table = must_some(render_claim_status_table(&catalog, &policy).ok());
        let overall = must_some(parse_claim_table_overall(&table));
        let [proven, preview, planned, not_proven, unsupported, total] = overall;
        assert_eq!(
            proven + preview + planned + not_proven + unsupported,
            total,
            "statuses partition the denominator"
        );
        assert_eq!(total, catalog.feature.len());
        assert_eq!(proven, 1, "the fully-cited row earns proven");
        assert_eq!(not_proven, 1, "the unadvertised row is not_proven");
    }

    #[test]
    fn overall_parser_rejects_malformed_tables() {
        assert_eq!(parse_claim_table_overall("| **Overall** | 1 | 2 |"), None);
        assert_eq!(parse_claim_table_overall("no overall row here"), None);
        let table = "| Area | proven | preview | planned | not_proven | unsupported | Total |\n|---|---|---|---|---|---|\n| x | 0 | 0 | 0 | 0 | 0 | 0 |";
        assert_eq!(parse_claim_table_overall(table), None);
        let good = "| **Overall** | **2** | **3** | **4** | **5** | **6** | **20** |";
        assert_eq!(parse_claim_table_overall(good), Some([2, 3, 4, 5, 6, 20]));
    }
}
