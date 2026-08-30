//! Load and validate the frozen fixture-identity contract.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::error::FixtureError;
use super::types::{
    CATALOG_KIND, CATALOG_VERSION, ContentStatus, DistributionKwaliteeCatalog,
    DistributionKwaliteeFixture, DistributionKwaliteeFixtureContract, ExpectationRule,
    FIXTURE_KIND, FIXTURE_SCHEMA_VERSION, FixtureKind, MetricClass,
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
    if contract.status != "frozen_identities" {
        return Err(FixtureError::Metadata(format!(
            "status: expected `frozen_identities`, observed `{}`",
            contract.status
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
    let metric_classes: BTreeMap<&str, MetricClass> =
        catalog.metric.iter().map(|metric| (metric.id.as_str(), metric.class)).collect();
    let metric_dependencies: BTreeMap<&str, Vec<&str>> = catalog
        .metric
        .iter()
        .map(|metric| (metric.id.as_str(), metric.depends_on.iter().map(String::as_str).collect()))
        .collect();

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
        if fixture.kind == FixtureKind::SingleDefect
            && let Some(primary) = fixture.primary_fail.first()
        {
            for cascade in &fixture.permitted_cascades {
                if !metric_reaches_primary(cascade, primary, &metric_dependencies) {
                    return Err(FixtureError::Binding(format!(
                        "fixture `{}` cascade `{cascade}` must be a dependent of primary failure `{primary}`",
                        fixture.id
                    )));
                }
            }
        }
        if fixture.kind == FixtureKind::SingleDefect
            && fixture.content_status == ContentStatus::Committed
            && fixture.primary_fail.iter().any(|metric_id| {
                metric_classes.get(metric_id.as_str()) == Some(&MetricClass::UnsupportedOrDeferred)
            })
        {
            return Err(FixtureError::Binding(format!(
                "fixture `{}` cannot commit bytes for an unsupported_or_deferred metric",
                fixture.id
            )));
        }
        if fixture.content_status == ContentStatus::Committed {
            validate_committed_tree(fixture, fixture_root)?;
        }
    }
    Ok(())
}

fn metric_reaches_primary(
    start: &str,
    primary: &str,
    dependencies: &BTreeMap<&str, Vec<&str>>,
) -> bool {
    let mut pending = vec![start];
    let mut visited = BTreeSet::new();
    while let Some(metric) = pending.pop() {
        if metric == primary {
            return true;
        }
        if !visited.insert(metric) {
            continue;
        }
        if let Some(dependencies) = dependencies.get(metric) {
            pending.extend(dependencies.iter().copied());
        }
    }
    false
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
        let mut has_entries = false;
        for entry in entries {
            has_entries = true;
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
        if !has_entries {
            return Err(format!(
                "committed fixture contains an empty directory: {}",
                dir.display()
            ));
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
        assert!(contract.fixture.iter().any(|fixture| fixture.id == "Acme-CatalogFreeze"));
        assert!(contract.fixture.iter().any(|fixture| fixture.id == "archive_security_failure"));
        let minimal =
            contract.fixture.iter().find(|fixture| fixture.id == "Acme-CatalogFreeze").unwrap();
        assert_eq!(minimal.content_status, ContentStatus::Committed);
        assert_eq!(minimal.expectation_rule, ExpectationRule::AllApplicableOfflineCorePass);
    }

    #[test]
    fn committed_fixture_metadata_matches_identity() {
        let contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let fixture =
            contract.fixture.iter().find(|fixture| fixture.id == "Acme-CatalogFreeze").unwrap();
        let root = committed_fixture_root().join(&fixture.id);
        let metadata: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("META.json")).unwrap())
                .unwrap();
        let json_name = metadata.get("name").and_then(serde_json::Value::as_str).unwrap();
        let yaml = std::fs::read_to_string(root.join("META.yml")).unwrap();
        let yaml_name = yaml.lines().find_map(|line| line.strip_prefix("name: ")).unwrap();
        assert_eq!(fixture.id, root.file_name().unwrap().to_str().unwrap());
        assert_eq!(json_name, fixture.id);
        assert_eq!(yaml_name, fixture.id);
    }

    #[test]
    fn non_frozen_status_fails() {
        let toml = format!(
            "{}{}",
            envelope().replace("status = \"frozen_identities\"", "status = \"draft\""),
            reserved_pass("status")
        );
        assert!(matches!(
            parse_fixture_contract(&toml),
            Err(FixtureError::Metadata(message)) if message.contains("frozen_identities")
        ));
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
            contract.fixture.iter_mut().find(|fixture| fixture.id == "Acme-CatalogFreeze").unwrap();
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
            contract.fixture.iter_mut().find(|fixture| fixture.id == "Acme-CatalogFreeze").unwrap();
        minimal.committed_files.retain(|path| path != "README");
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("undeclared file");
        assert!(matches!(error, FixtureError::InvalidFixture { .. }));
    }

    #[test]
    fn empty_committed_directory_fails_binding() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Acme-CatalogFreeze");
        std::fs::create_dir_all(root.join("empty")).unwrap();
        let error = walk_relative_files(&root).expect_err("empty directory");
        assert!(error.contains("committed fixture contains an empty directory"));
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
        catalog.metric[0].fixture_ids = vec!["Acme-CatalogFreeze".into()];
        let contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("missing defect");
        assert!(matches!(error, FixtureError::Binding(_)));
    }

    #[test]
    fn committed_deferred_defect_fails_binding() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let mut contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let deferred = contract
            .fixture
            .iter_mut()
            .find(|fixture| fixture.id == "defect_no_unauthorized_packages")
            .unwrap();
        deferred.content_status = ContentStatus::Committed;
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("committed deferred defect");
        assert!(matches!(
            error,
            FixtureError::Binding(message)
                if message.contains("defect_no_unauthorized_packages")
        ));
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
    fn prerequisite_cascade_fails_binding() {
        let catalog = load_distribution_kwalitee_catalog().expect("catalog");
        let mut contract = load_distribution_kwalitee_fixture_contract().expect("fixtures");
        let defect = contract
            .fixture
            .iter_mut()
            .find(|fixture| fixture.id == "defect_manifest_matches_dist")
            .unwrap();
        defect.permitted_cascades = vec!["cpants.has_manifest".into()];
        let error =
            validate_catalog_fixture_binding(&catalog, &contract, &committed_fixture_root())
                .expect_err("prerequisite cascade");
        assert!(matches!(
            error,
            FixtureError::Binding(message)
                if message.contains("defect_manifest_matches_dist")
                    && message.contains("cpants.has_manifest")
                    && message.contains("dependent")
        ));
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
    fn nested_fixture_id_fails() {
        let toml = format!("{}{}", envelope(), reserved_pass("nested/id"));
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
