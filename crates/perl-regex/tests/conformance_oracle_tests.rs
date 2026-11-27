//! Bounded real-Perl compile oracle fixtures for regex conformance (#7036).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod conformance_oracle {
    pub mod harness;
}

use conformance_oracle::harness::{
    ExpectedCompileOutcome, ORACLE_INVOCATION, check_expectation, probe_identity,
};
use perl_regex::conformance::{ConformanceCompleteness, OracleDisposition, SCHEMA_VERSION};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleFixtureRow {
    id: String,
    family: String,
    authority: String,
    positive_source: String,
    negative_source: String,
    expected_positive: String,
    expected_negative: String,
    completeness: String,
    owner_issue: u32,
    oracle_disposition: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleFixtureFile {
    schema_version: u32,
    programme: String,
    family: String,
    owner_issue: u32,
    invocation: String,
    rows: Vec<OracleFixtureRow>,
}

const FIXTURE: &str = include_str!("fixtures/conformance_oracle/compile_safe.json");

fn parse_fixture() -> OracleFixtureFile {
    serde_json::from_str(FIXTURE)
        .unwrap_or_else(|err| panic!("oracle fixture failed to parse: {err}"))
}

fn resolve_expected(token: &str, row_id: &str) -> ExpectedCompileOutcome {
    match token {
        "accepted" => ExpectedCompileOutcome::Accepted,
        "rejected" => ExpectedCompileOutcome::Rejected,
        other => panic!("row {row_id}: unknown expected outcome {other:?}"),
    }
}

#[test]
fn oracle_fixture_schema_is_load_bearing() {
    let fixture = parse_fixture();
    assert_eq!(fixture.schema_version, SCHEMA_VERSION);
    assert_eq!(fixture.programme, "perl_regex_compile_oracle");
    assert_eq!(fixture.family, "oracle.compile_safe");
    assert_eq!(fixture.owner_issue, 7036);
    assert_eq!(fixture.invocation, ORACLE_INVOCATION);
    assert!(!fixture.rows.is_empty());

    let mut ids = std::collections::BTreeSet::new();
    for row in &fixture.rows {
        assert!(ids.insert(row.id.clone()), "duplicate oracle row id {}", row.id);
        assert_eq!(row.family, fixture.family);
        assert_eq!(row.owner_issue, 7036);
        assert!(!row.authority.trim().is_empty());
        assert_eq!(row.oracle_disposition, OracleDisposition::Required.as_str());
        assert_eq!(row.completeness, ConformanceCompleteness::Proven.as_str());
        assert_ne!(row.positive_source, row.negative_source);
    }
}

#[test]
fn oracle_identity_probe_is_proven_or_skips_rows() {
    match probe_identity() {
        conformance_oracle::harness::OracleResult::Proven { identity, .. } => {
            assert!(identity.version.starts_with("5."));
            assert_eq!(identity.invocation, ORACLE_INVOCATION);
        }
        conformance_oracle::harness::OracleResult::NotProven { .. } => return,
    }

    let fixture = parse_fixture();
    for row in &fixture.rows {
        check_expectation(&row.positive_source, resolve_expected(&row.expected_positive, &row.id))
            .unwrap_or_else(|err| panic!("row {} positive probe: {err}", row.id));
        check_expectation(&row.negative_source, resolve_expected(&row.expected_negative, &row.id))
            .unwrap_or_else(|err| panic!("row {} negative probe: {err}", row.id));
    }
}
