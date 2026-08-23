//! Authoritative feature-catalog projections and fail-closed validation (#7029).
//!
//! The workspace-root `features.toml` is the single authority for LSP/DAP
//! feature claims. The crate-local `crates/*/features_sot.toml` files are
//! byte-exact generated projections used as fallbacks by standalone/packaged
//! builds. This task regenerates those projections and validates the catalog
//! against the #7029 schema and evidence policy:
//!
//! - every row carries the stable ownership/claim-boundary fields (or the
//!   explicit `missing` marker);
//! - `planned`/`unsupported` rows are never advertised;
//! - `proven` rows carry classified evidence receipts that exist on disk and
//!   use classes from `[policy].evidence_classes` — advertisement alone
//!   cannot promote a row;
//! - no aggregate percentage re-enters `[meta]` (#6731 recurrence control);
//! - every projection is byte-identical to the authority (drift fails closed);
//! - blanket protocol-complete claims are refused outright.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::fs;
use std::path::Path;

/// Crate-local generated projections of the authoritative catalog.
pub const PROJECTION_PATHS: &[&str] = &[
    "crates/perl-lsp-rs/features_sot.toml",
    "crates/perl-lsp-rs-core/features_sot.toml",
    "crates/perl-dap/features_sot.toml",
    "crates/perl-parser/features_sot.toml",
];

const AUTHORITY_PATH: &str = "features.toml";

const DIRECTIONS: &[&str] = &["client_to_server", "server_to_client", "both"];
const REGISTRATIONS: &[&str] = &["static", "dynamic", "none"];
const FEATURE_CLASSES: &[&str] = &[
    "request_response",
    "server_request",
    "document_workspace",
    "cancellation_progress",
    "editor_dependent",
    "debug_adapter",
];
const MATURITIES: &[&str] = &["proven", "preview", "planned", "unsupported", "not_proven"];

/// Required raw keys on every feature row. Ownership fields may hold the
/// explicit `missing` marker; they may not be absent or empty.
const REQUIRED_ROW_KEYS: &[&str] = &[
    "id",
    "spec",
    "area",
    "maturity",
    "advertised",
    "direction",
    "capability_gate",
    "registration",
    "feature_class",
    "impl_owner",
    "state_owner",
    "limitations",
    "claim_boundary",
    "description",
];

/// Blanket-claim vocabulary refused anywhere in a feature id.
const FORBIDDEN_ID_PATTERNS: &[&str] =
    &["full_318", "318_complete", "protocol_complete", "all_methods"];

/// A single catalog validation finding.
#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    pub label: String,
    pub detail: String,
}

/// Validate the authoritative catalog text plus projection byte-drift.
///
/// Receipt paths are resolved relative to `root`, mirroring how the catalog
/// cites repository-relative paths.
pub fn validate_catalog(root: &Path, authority_text: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let table = match toml::from_str::<toml::Table>(authority_text) {
        Ok(table) => table,
        Err(error) => {
            violations.push(Violation {
                label: "authority parse".to_string(),
                detail: format!("{AUTHORITY_PATH} does not parse: {error}"),
            });
            return violations;
        }
    };

    // Recurrence control for #6731/#7029: no aggregate percentage may be
    // declared in [meta]; status must derive from per-row evidence state.
    if table.get("meta").and_then(|meta| meta.get("compliance_percent")).is_some() {
        violations.push(Violation {
            label: "aggregate claim".to_string(),
            detail: "meta.compliance_percent is refused: declaration-count aggregates are not \
                     behavior evidence"
                .to_string(),
        });
    }

    let evidence_classes = policy_list(&table, "evidence_classes");
    let promotion_classes = promotion_policy_classes(&table);

    let Some(rows) = table.get("feature").and_then(|rows| rows.as_array()) else {
        violations.push(Violation {
            label: "authority shape".to_string(),
            detail: format!("{AUTHORITY_PATH} has no [[feature]] rows"),
        });
        return violations;
    };

    let mut seen_ids = std::collections::BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let Some(row) = row.as_table() else {
            violations.push(Violation {
                label: "authority shape".to_string(),
                detail: format!("feature row {index} is not a table"),
            });
            continue;
        };
        let id =
            row.get("id").and_then(|value| value.as_str()).unwrap_or("<missing id>").to_string();

        if !seen_ids.insert(id.clone()) {
            violations.push(Violation {
                label: "duplicate id".to_string(),
                detail: format!("feature id {id:?} appears more than once"),
            });
        }

        let lowered_id = id.to_lowercase();
        if let Some(pattern) =
            FORBIDDEN_ID_PATTERNS.iter().find(|pattern| lowered_id.contains(**pattern))
        {
            violations.push(Violation {
                label: "blanket claim".to_string(),
                detail: format!(
                    "row id {id:?} uses forbidden blanket-claim pattern {pattern:?}; LSP 3.18 \
                     support stays method-scoped"
                ),
            });
        }

        for key in REQUIRED_ROW_KEYS {
            let present = row.get(*key).is_some_and(|value| match value {
                toml::Value::String(text) => !text.trim().is_empty(),
                toml::Value::Boolean(_) | toml::Value::Integer(_) => true,
                _ => false,
            });
            if !present {
                violations.push(Violation {
                    label: "schema field missing".to_string(),
                    detail: format!(
                        "row {id:?} lacks required key {key:?} (record the explicit value \
                         \"missing\" when unknown)"
                    ),
                });
            }
        }

        if let Some(maturity) = row.get("maturity").and_then(|value| value.as_str()) {
            if !MATURITIES.contains(&maturity) {
                violations.push(Violation {
                    label: "unknown maturity".to_string(),
                    detail: format!(
                        "row {id:?} uses maturity {maturity:?} outside the #7029 \
                                    vocabulary {MATURITIES:?}"
                    ),
                });
            }
            let advertised = row.get("advertised").and_then(|value| value.as_bool());
            if advertised == Some(true) && matches!(maturity, "planned" | "unsupported") {
                violations.push(Violation {
                    label: "advertisement without a claim".to_string(),
                    detail: format!(
                        "row {id:?} is advertised but maturity {maturity:?} carries no \
                         implementation claim"
                    ),
                });
            }
            if maturity == "proven" {
                validate_proven_row(root, &id, row, &evidence_classes, &mut violations);
            }
        }

        if let Some(direction) = row.get("direction").and_then(|value| value.as_str())
            && !DIRECTIONS.contains(&direction)
        {
            violations.push(Violation {
                label: "unknown direction".to_string(),
                detail: format!("row {id:?} uses direction {direction:?}"),
            });
        }

        if let Some(registration) = row.get("registration").and_then(|value| value.as_str())
            && !REGISTRATIONS.contains(&registration)
        {
            violations.push(Violation {
                label: "unknown registration route".to_string(),
                detail: format!("row {id:?} uses registration {registration:?}"),
            });
        }

        if let Some(class) = row.get("feature_class").and_then(|value| value.as_str()) {
            if !FEATURE_CLASSES.contains(&class) {
                violations.push(Violation {
                    label: "unknown feature class".to_string(),
                    detail: format!("row {id:?} uses feature_class {class:?}"),
                });
            } else if !promotion_classes.iter().any(|known| known == class) {
                violations.push(Violation {
                    label: "promotion policy missing".to_string(),
                    detail: format!(
                        "row {id:?} declares feature_class {class:?} without a \
                         [policy.promotion.{class}] minimum_evidence rule"
                    ),
                });
            }
        }
    }

    violations
}

fn validate_proven_row(
    root: &Path,
    id: &str,
    row: &toml::Table,
    evidence_classes: &[String],
    violations: &mut Vec<Violation>,
) {
    let receipts = row.get("evidence").and_then(|value| value.as_array());
    let Some(receipts) = receipts else {
        violations.push(Violation {
            label: "promotion without evidence".to_string(),
            detail: format!(
                "row {id} claims proven without [[feature.evidence]] receipts (#7029); \
                 advertisement alone cannot promote a row"
            ),
        });
        return;
    };
    if receipts.is_empty() {
        violations.push(Violation {
            label: "promotion without evidence".to_string(),
            detail: format!("row {id} claims proven with an empty evidence list (#7029)"),
        });
    }
    for entry in receipts {
        let class = entry.get("class").and_then(|value| value.as_str());
        let path = entry.get("path").and_then(|value| value.as_str());
        match (class, path) {
            (Some(class), Some(path)) => {
                if !evidence_classes.iter().any(|known| known == class) {
                    violations.push(Violation {
                        label: "unknown evidence class".to_string(),
                        detail: format!(
                            "row {id} cites evidence class {class:?} outside \
                             [policy].evidence_classes"
                        ),
                    });
                }
                if !root.join(path).exists() {
                    violations.push(Violation {
                        label: "evidence receipt missing".to_string(),
                        detail: format!("row {id} cites non-existent receipt path {path:?}"),
                    });
                }
            }
            _ => violations.push(Violation {
                label: "malformed evidence receipt".to_string(),
                detail: format!("row {id} has an evidence entry without both class and path"),
            }),
        }
    }

    let tests = row.get("tests").and_then(|value| value.as_array());
    if tests.is_none_or(|tests| tests.is_empty()) {
        violations.push(Violation {
            label: "proven without receipts".to_string(),
            detail: format!(
                "row {id} claims proven but lists no test receipts; proven rows must name \
                 current behavior receipts"
            ),
        });
    }
}

fn policy_list(table: &toml::Table, key: &str) -> Vec<String> {
    table
        .get("policy")
        .and_then(|policy| policy.get(key))
        .and_then(|classes| classes.as_array())
        .map(|classes| {
            classes.iter().filter_map(|class| class.as_str().map(str::to_string)).collect()
        })
        .unwrap_or_default()
}

fn promotion_policy_classes(table: &toml::Table) -> Vec<String> {
    table
        .get("policy")
        .and_then(|policy| policy.get("promotion"))
        .and_then(|promotion| promotion.as_table())
        .map(|promotions| promotions.keys().cloned().collect())
        .unwrap_or_default()
}

/// Validate the whole tree: authority schema plus byte drift of projections.
pub fn validate_tree(root: &Path) -> Vec<Violation> {
    let mut violations = Vec::new();
    let Ok(authority_text) = fs::read_to_string(root.join(AUTHORITY_PATH)) else {
        violations.push(Violation {
            label: "authority missing".to_string(),
            detail: format!("failed to read {AUTHORITY_PATH}"),
        });
        return violations;
    };

    violations.extend(validate_catalog(root, &authority_text));

    for relative in PROJECTION_PATHS {
        match fs::read(root.join(relative)) {
            Ok(bytes) => {
                if bytes.as_slice() != authority_text.as_bytes() {
                    violations.push(Violation {
                        label: "projection drift".to_string(),
                        detail: format!(
                            "{relative} differs from the authority {AUTHORITY_PATH}; regenerate \
                             with: cargo xtask feature-sot"
                        ),
                    });
                }
            }
            Err(error) => violations.push(Violation {
                label: "projection unreadable".to_string(),
                detail: format!("{relative}: {error}"),
            }),
        }
    }

    violations
}

/// Rewrite every projection as a byte-exact copy of the authority.
pub fn regenerate_projections(root: &Path) -> Result<usize> {
    let authority =
        fs::read(root.join(AUTHORITY_PATH)).wrap_err(format!("failed to read {AUTHORITY_PATH}"))?;
    for relative in PROJECTION_PATHS {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .wrap_err_with(|| format!("failed to create parent for {relative}"))?;
        }
        fs::write(&path, &authority)
            .wrap_err_with(|| format!("failed to write projection {relative}"))?;
    }
    Ok(PROJECTION_PATHS.len())
}

pub fn run(check: bool) -> Result<()> {
    let root = project_root()?;
    if check {
        let violations = validate_tree(&root);
        if violations.is_empty() {
            println!(
                "Feature catalog OK: {} projections byte-match {} under the #7029 schema and \
                 evidence policy",
                PROJECTION_PATHS.len(),
                AUTHORITY_PATH
            );
            return Ok(());
        }
        eprintln!("FEATURE CATALOG VIOLATIONS:");
        eprintln!("{}", "=".repeat(72));
        for violation in &violations {
            eprintln!("  [{}] {}", violation.label, violation.detail);
        }
        eprintln!("{}", "=".repeat(72));
        bail!("feature catalog check failed with {} violation(s)", violations.len());
    }

    regenerate_projections(&root)?;
    let violations = validate_tree(&root);
    if !violations.is_empty() {
        for violation in &violations {
            eprintln!("  [{}] {}", violation.label, violation.detail);
        }
        bail!("regenerated projections still violate the catalog contract");
    }
    println!(
        "Regenerated {} projections from {}; all checks pass",
        PROJECTION_PATHS.len(),
        AUTHORITY_PATH
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN_CATALOG: &str = r#"[meta]
version = "0.17.0"
lsp_version = "3.18"

[policy]
evidence_classes = ["wire_e2e", "integration", "unit"]
classes_qualifying_for_proven = ["wire_e2e", "integration"]

[policy.promotion.request_response]
minimum_evidence = "integration"

[[feature]]
id = "lsp.completion"
spec = "LSP 3.0"
area = "text_document"
maturity = "not_proven"
advertised = true
direction = "client_to_server"
capability_gate = "textDocument.completion"
registration = "static"
feature_class = "request_response"
impl_owner = "perl-lsp-rs-core::providers::completion"
state_owner = "perl-lsp-rs::state::document"
limitations = "missing"
claim_boundary = "method-scoped"
description = "Completion"
"#;

    /// Writes an isolated tree: authority + all four projections.
    use std::path::PathBuf;

    fn write_tree(catalog: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let root = dir.path().to_path_buf();
        fs::write(root.join(AUTHORITY_PATH), catalog).expect("write authority");
        for relative in PROJECTION_PATHS {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("parent")).expect("create crate dir");
            fs::write(&path, catalog).expect("write projection");
        }
        (dir, root)
    }

    #[test]
    fn clean_authority_and_matching_projections_validate() {
        let (_dir, root) = write_tree(CLEAN_CATALOG);
        assert_eq!(validate_tree(&root), Vec::new());
    }

    #[test]
    fn injected_projection_drift_fails_closed() {
        let (_dir, root) = write_tree(CLEAN_CATALOG);
        let drifted = format!("{CLEAN_CATALOG}\n# stray hand edit\n");
        fs::write(root.join(PROJECTION_PATHS[0]), drifted).expect("drift projection");

        let violations = validate_tree(&root);
        assert!(
            violations.iter().any(|violation| violation.label == "projection drift"
                && violation.detail.contains(PROJECTION_PATHS[0])),
            "expected projection-drift violation, got: {violations:?}"
        );
    }

    #[test]
    fn promotion_from_advertisement_alone_fails_closed() {
        // Negative control from #7029: flipping maturity to proven while only
        // advertisement/test citations back it must fail closed.
        let promoted = CLEAN_CATALOG.replace("maturity = \"not_proven\"", "maturity = \"proven\"");
        let (_dir, root) = write_tree(&promoted);

        let violations = validate_tree(&root);
        assert!(
            violations.iter().any(|violation| violation.label == "promotion without evidence"),
            "expected promotion-without-evidence violation, got: {violations:?}"
        );
    }

    #[test]
    fn proven_row_with_existing_qualifying_receipt_validates() {
        let promoted = CLEAN_CATALOG
            .replace(
                "description = \"Completion\"",
                concat!(
                    "tests = [\"receipt.txt\"]\n",
                    "evidence = [{ class = \"integration\", path = \"receipt.txt\" }]\n",
                    "description = \"Completion\""
                ),
            )
            .replace("maturity = \"not_proven\"", "maturity = \"proven\"");
        let (_dir, root) = write_tree(&promoted);
        fs::write(root.join("receipt.txt"), "proof").expect("write receipt");

        assert_eq!(validate_tree(&root), Vec::new());
    }

    #[test]
    fn proven_row_with_unknown_evidence_class_fails_closed() {
        let promoted = CLEAN_CATALOG
            .replace(
                "description = \"Completion\"",
                concat!(
                    "tests = [\"receipt.txt\"]\n",
                    "evidence = [{ class = \"vibes\", path = \"receipt.txt\" }]\n",
                    "description = \"Completion\""
                ),
            )
            .replace("maturity = \"not_proven\"", "maturity = \"proven\"");
        let (_dir, root) = write_tree(&promoted);
        fs::write(root.join("receipt.txt"), "proof").expect("write receipt");

        let violations = validate_tree(&root);
        assert!(
            violations.iter().any(|violation| violation.label == "unknown evidence class"),
            "expected unknown-evidence-class violation, got: {violations:?}"
        );
    }

    #[test]
    fn removing_required_schema_field_fails_closed() {
        let stripped = CLEAN_CATALOG
            .lines()
            .filter(|line| !line.starts_with("impl_owner = "))
            .collect::<Vec<_>>()
            .join("\n");
        let (_dir, root) = write_tree(&stripped);

        let violations = validate_tree(&root);
        assert!(
            violations.iter().any(|violation| violation.label == "schema field missing"
                && violation.detail.contains("impl_owner")),
            "expected schema-field violation, got: {violations:?}"
        );
    }

    #[test]
    fn reintroducing_aggregate_percent_fails_closed() {
        let polluted = CLEAN_CATALOG.replace(
            "[meta]\nversion = \"0.17.0\"",
            "[meta]\ncompliance_percent = 98\nversion = \"0.17.0\"",
        );
        let (_dir, root) = write_tree(&polluted);

        let violations = validate_tree(&root);
        assert!(
            violations.iter().any(|violation| violation.label == "aggregate claim"),
            "expected aggregate-claim refusal, got: {violations:?}"
        );
    }

    #[test]
    fn advertising_an_unsupported_row_fails_closed() {
        let bad = CLEAN_CATALOG.replace(
            concat!(
                "id = \"lsp.completion\"\nspec = \"LSP 3.0\"\narea = \"text_document\"\n",
                "maturity = \"not_proven\"\nadvertised = true"
            ),
            concat!(
                "id = \"dap.restart_frame\"\nspec = \"DAP 1.0\"\narea = \"debug\"\n",
                "maturity = \"unsupported\"\nadvertised = true"
            ),
        );
        let (_dir, root) = write_tree(&bad);

        let violations = validate_tree(&root);
        assert!(
            violations.iter().any(|violation| violation.label == "advertisement without a claim"),
            "expected advertisement-without-claim violation, got: {violations:?}"
        );
    }

    #[test]
    fn blanket_318_complete_claim_fails_closed() {
        let blanket =
            CLEAN_CATALOG.replace("id = \"lsp.completion\"", "id = \"lsp.full_318_complete\"");
        let (_dir, root) = write_tree(&blanket);

        let violations = validate_tree(&root);
        assert!(
            violations.iter().any(|violation| violation.label == "blanket claim"),
            "blanket 3.18-complete claims must be refused, got: {violations:?}"
        );
    }

    #[test]
    fn regeneration_is_deterministic_and_repairs_drift() {
        let (_dir, root) = write_tree(CLEAN_CATALOG);
        let drifted = format!("{CLEAN_CATALOG}\n# stale\n");
        fs::write(root.join(PROJECTION_PATHS[2]), drifted).expect("drift projection");

        regenerate_projections(&root).expect("regenerate");

        let first = fs::read(root.join(PROJECTION_PATHS[2])).expect("read first");
        regenerate_projections(&root).expect("regenerate again");
        let second = fs::read(root.join(PROJECTION_PATHS[2])).expect("read second");
        assert_eq!(first, second, "regeneration must be deterministic");
        assert_eq!(first, CLEAN_CATALOG.as_bytes(), "projection must byte-match authority");
        assert_eq!(validate_tree(&root), Vec::new());
    }
}
