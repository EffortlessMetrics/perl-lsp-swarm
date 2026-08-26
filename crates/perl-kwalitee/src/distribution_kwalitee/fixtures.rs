//! Load and validate the frozen fixture-identity contract.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::error::FixtureError;
use super::types::{
    CATALOG_KIND, CATALOG_VERSION, ContentStatus, DistributionKwaliteeCatalog,
    DistributionKwaliteeFixture, DistributionKwaliteeFixtureContract, ExpectationRule,
    FIXTURE_KIND, FIXTURE_SCHEMA_VERSION, FixtureKind,
};

const FIXTURE_TOML: &str = include_str!("../../distribution_kwalitee_fixtures.v1.toml");

/// Checked-in fixture-contract TOML.
pub fn fixture_contract_toml() -> &'static str {
    FIXTURE_TOML
}

/// Decode fixture-contract TOML without catalog binding.
pub fn parse_fixture_contract(
    toml: &str,
) -> Result<DistributionKwaliteeFixtureContract, FixtureError> {
    let contract: DistributionKwaliteeFixtureContract =
        toml::from_str(toml).map_err(FixtureError::InvalidToml)?;
    validate_fixture_contract(&contract)?;
    Ok(contract)
}

/// Load the frozen checked-in fixture contract.
pub fn load_distribution_kwalitee_fixture_contract()
-> Result<DistributionKwaliteeFixtureContract, FixtureError> {
    parse_fixture_contract(FIXTURE_TOML)
}

/// Reject envelope and per-row fixture invariants that do not need the catalog.
pub fn validate_fixture_contract(
    contract: &DistributionKwaliteeFixtureContract,
) -> Result<(), FixtureError> {
    if contract.schema_version != FIXTURE_SCHEMA_VERSION {
        return Err(FixtureError::Metadata(format!(
            "schema_version: expected {FIXTURE_SCHEMA_VERSION}, observed {}",
            contract.schema_version
        )));
    }
    if contract.kind != FIXTURE_KIND {
        return Err(FixtureError::Metadata(format!(
            "kind: expected `{FIXTURE_KIND}`, observed `{}`",
            contract.kind
        )));
    }
    if contract.catalog_kind != CATALOG_KIND || contract.catalog_version != CATALOG_VERSION {
        return Err(FixtureError::Metadata(format!(
            "fixture contract must bind `{CATALOG_KIND}` `{CATALOG_VERSION}`"
        )));
    }
    if contract.oracle_policy != "committed_expected_facts_only" {
        return Err(FixtureError::Metadata(
            "oracle_policy must be `committed_expected_facts_only`".into(),
        ));
    }
    if contract.fixture.is_empty() {
        return Err(FixtureError::Metadata(
            "fixture contract must name at least one identity".into(),
        ));
    }

    let mut ids = BTreeSet::new();
    for fixture in &contract.fixture {
        if !ids.insert(fixture.id.as_str()) {
            return Err(FixtureError::DuplicateIdentity(fixture.id.clone()));
        }
        validate_fixture_row(fixture)?;
    }
    Ok(())
}

fn validate_fixture_row(fixture: &DistributionKwaliteeFixture) -> Result<(), FixtureError> {
    if fixture.id.is_empty() || fixture.intended_proposition.is_empty() || fixture.owning_issue == 0
    {
        return Err(FixtureError::InvalidFixture {
            id: fixture.id.clone(),
            reason: "id, intended_proposition, and owning_issue are required".into(),
        });
    }
    if fixture.id.contains(['/', '\\'])
        || fixture.id.split(['/', '\\']).any(|part| part == "." || part == "..")
        || Path::new(&fixture.id).is_absolute()
    {
        return Err(FixtureError::InvalidFixture {
            id: fixture.id.clone(),
            reason: "fixture id must be a single relative path segment".into(),
        });
    }
    match (fixture.content_status, fixture.committed_files.is_empty()) {
        (ContentStatus::Committed, true) => {
            return Err(FixtureError::InvalidFixture {
                id: fixture.id.clone(),
                reason: "committed fixtures must list committed_files".into(),
            });
        }
        (ContentStatus::Reserved, false) => {
            return Err(FixtureError::InvalidFixture {
                id: fixture.id.clone(),
                reason: "reserved fixtures must not list committed_files".into(),
            });
        }
        (ContentStatus::Committed, false) | (ContentStatus::Reserved, true) => {}
    }
    match fixture.expectation_rule {
        ExpectationRule::AllApplicableOfflineCorePass => {
            if !fixture.primary_fail.is_empty() {
                return Err(FixtureError::InvalidFixture {
                    id: fixture.id.clone(),
                    reason: "pass-all fixtures must not name primary_fail".into(),
                });
            }
        }
        ExpectationRule::SingleDefect => {
            if fixture.primary_fail.len() != 1 {
                return Err(FixtureError::InvalidFixture {
                    id: fixture.id.clone(),
                    reason: "single-defect fixtures need exactly one primary_fail".into(),
                });
            }
            if fixture.kind != FixtureKind::SingleDefect {
                return Err(FixtureError::InvalidFixture {
                    id: fixture.id.clone(),
                    reason: "single_defect expectation requires kind `single_defect`".into(),
                });
            }
        }
        ExpectationRule::InvalidInput => {
            if fixture.kind != FixtureKind::ArchiveSecurityFailure {
                return Err(FixtureError::InvalidFixture {
                    id: fixture.id.clone(),
                    reason: "invalid_input expectation is reserved for archive security failures"
                        .into(),
                });
            }
        }
    }
    if fixture.committed_files.iter().any(|path| {
        path.is_empty()
            || Path::new(path).is_absolute()
            || path.split(['/', '\\']).any(|part| part == "..")
    }) {
        return Err(FixtureError::InvalidFixture {
            id: fixture.id.clone(),
            reason: "committed_files must be relative, non-empty, and non-escaping".into(),
        });
    }
    Ok(())
}

/// Bind catalog fixture IDs to the fixture contract and check committed trees.
pub fn validate_catalog_fixture_binding(
    catalog: &DistributionKwaliteeCatalog,
    contract: &DistributionKwaliteeFixtureContract,
    fixture_root: &Path,
) -> Result<(), FixtureError> {
    if contract.catalog_version != catalog.catalog_version || contract.catalog_kind != catalog.kind
    {
        return Err(FixtureError::Binding(
            "fixture contract catalog identity does not match the loaded catalog".into(),
        ));
    }

    let fixtures: BTreeMap<&str, &DistributionKwaliteeFixture> =
        contract.fixture.iter().map(|fixture| (fixture.id.as_str(), fixture)).collect();
    let metrics: BTreeSet<&str> = catalog.metric.iter().map(|metric| metric.id.as_str()).collect();

    for metric in &catalog.metric {
        if metric.fixture_ids.is_empty() {
            return Err(FixtureError::Binding(format!(
                "metric `{}` has no fixture_ids",
                metric.id
            )));
        }
        let mut has_own_defect = false;
        for fixture_id in &metric.fixture_ids {
            let Some(fixture) = fixtures.get(fixture_id.as_str()) else {
                return Err(FixtureError::Binding(format!(
                    "metric `{}` names unknown fixture `{fixture_id}`",
                    metric.id
                )));
            };
            if fixture.kind == FixtureKind::SingleDefect {
                if fixture.primary_fail.iter().any(|fail| fail == &metric.id) {
                    has_own_defect = true;
                } else {
                    return Err(FixtureError::Binding(format!(
                        "metric `{}` names defect fixture `{fixture_id}` that does not fail it",
                        metric.id
                    )));
                }
            }
        }
        if !has_own_defect {
            return Err(FixtureError::Binding(format!(
                "metric `{}` must name a single-defect fixture that fails it",
                metric.id
            )));
        }
    }

    for fixture in &contract.fixture {
        for metric_id in fixture.primary_fail.iter().chain(fixture.permitted_cascades.iter()) {
            if !metrics.contains(metric_id.as_str()) {
                return Err(FixtureError::Binding(format!(
                    "fixture `{}` references unknown metric `{metric_id}`",
                    fixture.id
                )));
            }
        }
        if fixture.content_status == ContentStatus::Committed {
            validate_committed_tree(fixture, fixture_root)?;
        }
    }
    Ok(())
}

fn validate_committed_tree(
    fixture: &DistributionKwaliteeFixture,
    fixture_root: &Path,
) -> Result<(), FixtureError> {
    let dir = fixture_root.join(&fixture.id);
    if !dir.is_dir() {
        return Err(FixtureError::InvalidFixture {
            id: fixture.id.clone(),
            reason: format!("committed tree `{}` is missing", dir.display()),
        });
    }
    let expected: BTreeSet<&str> = fixture.committed_files.iter().map(String::as_str).collect();
    let observed = walk_relative_files(&dir)
        .map_err(|reason| FixtureError::InvalidFixture { id: fixture.id.clone(), reason })?;
    if expected != observed.iter().map(String::as_str).collect() {
        return Err(FixtureError::InvalidFixture {
            id: fixture.id.clone(),
            reason: format!(
                "committed file set drifted: expected {expected:?}, observed {observed:?}"
            ),
        });
    }
    Ok(())
}

fn walk_relative_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|error| format!("cannot read `{}`: {error}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("cannot read entry: {error}"))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot stat `{}`: {error}", path.display()))?;
            if file_type.is_symlink() {
                return Err(format!(
                    "committed fixture must not contain symlinks: {}",
                    path.display()
                ));
            }
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                return Err(format!("committed fixture contains a non-file: {}", path.display()));
            }
            let rel = path.strip_prefix(root).map_err(|error| {
                format!("path `{}` escaped fixture root: {error}", path.display())
            })?;
            files.insert(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(files)
}

/// Directory that holds committed fixture trees.
pub fn committed_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/distribution")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]
    use super::*;
    use crate::distribution_kwalitee::catalog::load_distribution_kwalitee_catalog;

    fn envelope() -> String {
        fixture_contract_toml()
            .lines()
            .take_while(|line| !line.starts_with("[[fixture]]"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn reserved_pass(id: &str) -> String {
        format!(
            r#"
[[fixture]]
id = "{id}"
kind = "minimal_valid_distribution"
input_role = "staged_directory"
content_status = "reserved"
expectation_rule = "all_applicable_offline_core_pass"
primary_fail = []
permitted_cascades = []
owning_issue = 9220
cpants_comparability = "direct"
intended_proposition = "control"
committed_files = []
"#
        )
    }

    #[test]
    fn checked_in_contract_binds_to_the_catalog() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
            .expect("binding");
        assert!(contract.fixture.iter().any(|fixture| fixture.id == "minimal_valid"));
        assert!(contract.fixture.iter().any(|fixture| fixture.id == "archive_security_failure"));
        let minimal =
            contract.fixture.iter().find(|fixture| fixture.id == "minimal_valid").unwrap();
        assert_eq!(minimal.content_status, ContentStatus::Committed);
        assert_eq!(minimal.expectation_rule, ExpectationRule::AllApplicableOfflineCorePass);
    }

    #[test]
    fn unknown_field_fails_decode() {
        let toml = format!("{}blessed_by_evaluator = true\n{}", envelope(), reserved_pass("a"));
        assert!(matches!(parse_fixture_contract(&toml), Err(FixtureError::InvalidToml(_))));
    }

    #[test]
    fn duplicate_fixture_ids_fail() {
        let toml = format!("{}{}{}", envelope(), reserved_pass("dup"), reserved_pass("dup"));
        assert!(matches!(parse_fixture_contract(&toml), Err(FixtureError::DuplicateIdentity(_))));
    }

    #[test]
    fn reserved_fixture_cannot_claim_committed_files() {
        let mut row = reserved_pass("ghost");
        row = row.replace("committed_files = []", r#"committed_files = ["README"]"#);
        let toml = format!("{}{row}", envelope());
        assert!(matches!(parse_fixture_contract(&toml), Err(FixtureError::InvalidFixture { .. })));
    }

    #[test]
    fn committed_tree_missing_file_fails_binding() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let mut contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let minimal =
            contract.fixture.iter_mut().find(|fixture| fixture.id == "minimal_valid").unwrap();
        minimal.committed_files.push("not-really-there.txt".into());
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("missing file");
        assert!(matches!(error, FixtureError::InvalidFixture { .. }));
    }

    #[test]
    fn undeclared_committed_file_fails_binding() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let mut contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let minimal =
            contract.fixture.iter_mut().find(|fixture| fixture.id == "minimal_valid").unwrap();
        minimal.committed_files.retain(|path| path != "README");
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("undeclared file");
        assert!(matches!(error, FixtureError::InvalidFixture { .. }));
    }

    #[test]
    fn catalog_row_without_fixture_fails_binding() {
        let mut catalog = load_distribution_kwalitee_catalog().expect("catalog");
        catalog.metric[0].fixture_ids.clear();
        let contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("missing fixture");
        assert!(matches!(error, FixtureError::Binding(_)));
    }

    #[test]
    fn catalog_row_without_defect_fixture_fails_binding() {
        let mut catalog = load_distribution_kwalitee_catalog().expect("catalog");
        catalog.metric[0].fixture_ids = vec!["minimal_valid".into()];
        let contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("missing defect");
        assert!(matches!(error, FixtureError::Binding(_)));
    }

    #[test]
    fn fixture_referencing_unknown_metric_fails_binding() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let mut contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let defect = contract
            .fixture
            .iter_mut()
            .find(|fixture| fixture.id == "defect_has_manifest")
            .unwrap();
        defect.permitted_cascades.push("cpants.not_real".into());
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("unknown metric");
        assert!(matches!(error, FixtureError::Binding(_)));
    }

    #[test]
    fn absolute_committed_path_fails() {
        let mut row = reserved_pass("abs");
        row = row.replace("content_status = \"reserved\"", "content_status = \"committed\"");
        row = row.replace("committed_files = []", r#"committed_files = ["/tmp/escape"]"#);
        let toml = format!("{}{row}", envelope());
        assert!(matches!(parse_fixture_contract(&toml), Err(FixtureError::InvalidFixture { .. })));
    }

    #[test]
    fn parent_dir_committed_path_fails() {
        let mut row = reserved_pass("esc");
        row = row.replace("content_status = \"reserved\"", "content_status = \"committed\"");
        row = row.replace("committed_files = []", r#"committed_files = ["../secret"]"#);
        let toml = format!("{}{row}", envelope());
        assert!(matches!(parse_fixture_contract(&toml), Err(FixtureError::InvalidFixture { .. })));
    }

    #[test]
    fn parent_dir_fixture_id_fails() {
        let toml = format!("{}{}", envelope(), reserved_pass("../secret"));
        assert!(matches!(parse_fixture_contract(&toml), Err(FixtureError::InvalidFixture { .. })));
    }

    #[test]
    fn catalog_version_drift_fails_binding() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let mut contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        contract.catalog_version = "v2".into();
        // parse-time check already forbids v2; mutate after load.
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("version drift");
        assert!(matches!(error, FixtureError::Binding(_)));
    }
}
