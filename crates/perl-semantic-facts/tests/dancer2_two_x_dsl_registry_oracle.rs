//! Hermetic pinned-upstream oracle for the Dancer2 2.x core-DSL registry
//! (#13616, L2).
//!
//! The checked-in table
//! `tests/data/dancer2_two_x_dsl_registry_oracle.tsv` is generated from the
//! pinned upstream `lib/Dancer2/Core/DSL.pm` (`dsl_keywords` map plus the
//! `DEPRECATED:` runtime-croak bodies) at commit
//! `09f316678b8dd237d4c4ea0242e70e32591f5d64` (2.0.0); the same map is
//! byte-identical at `674837ce095db3bffb5acfccd50fdac57771d50b` (2.0.1) and
//! `09e9376288ddc3571f226883eb99753fecce818d` (2.1.0). The table is derived
//! independently of perl-lsp output, so agreement between the table and
//! [`DANCER2_TWO_X_DSL_KEYWORDS`] is a genuine cross-check of the registry —
//! every keyword classified, no extras, prototypes exact.
//!
//! Boundary: this is the source-derived oracle only. The runtime
//! differential (real `perl -MDancer2` export sets, croak texts, and scope
//! enforcement) has no local Dancer2 on this host and is NOT_PROVEN here; it
//! must run hermetic and version-pinned as a separate conformance surface,
//! never as part of editor analysis.

use perl_semantic_facts::framework_adapters::dancer2::DslKeywordScope;
use perl_semantic_facts::framework_adapters::dancer2_two_x::{
    DANCER2_TWO_X_DSL_KEYWORDS, DANCER2_TWO_X_KEYWORD_GLOBAL, DANCER2_TWO_X_KEYWORD_ROUTE_ONLY,
    DANCER2_TWO_X_KEYWORD_TOTAL, DANCER2_TWO_X_PINNED_COMMIT_2_0_0, Dancer2TwoXDslKeyword,
};
use perl_test_must::must;

fn oracle_path() -> String {
    format!("{}/tests/data/dancer2_two_x_dsl_registry_oracle.tsv", env!("CARGO_MANIFEST_DIR"))
}

/// One parsed oracle row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OracleRow {
    name: String,
    scope: DslKeywordScope,
    prototype: Option<String>,
    deprecation_replacement: Option<String>,
}

fn parse_oracle(source: &str) -> Result<Vec<OracleRow>, String> {
    let mut rows = Vec::new();
    for line in source.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 4 {
            return Err(format!("oracle row must have four tab-separated fields: {line:?}"));
        }
        if fields[0] == "name" {
            continue; // column header
        }
        let scope = match fields[1] {
            "global" => DslKeywordScope::Global,
            "route_handler" => DslKeywordScope::RouteHandlerOnly,
            other => return Err(format!("unknown oracle scope spelling: {other:?}")),
        };
        let dash_to_none = |field: &str| if field == "-" { None } else { Some(field.to_string()) };
        rows.push(OracleRow {
            name: fields[0].to_string(),
            scope,
            prototype: dash_to_none(fields[2]),
            deprecation_replacement: dash_to_none(fields[3]),
        });
    }
    if rows.is_empty() {
        return Err("the oracle table must not be empty".to_string());
    }
    Ok(rows)
}

fn load_oracle() -> Vec<OracleRow> {
    let path = oracle_path();
    let source = must(std::fs::read_to_string(&path));
    must(parse_oracle(&source))
}

#[test]
fn oracle_table_carries_its_pinned_provenance() {
    let path = oracle_path();
    let source = must(std::fs::read_to_string(&path));
    assert!(
        source.contains(DANCER2_TWO_X_PINNED_COMMIT_2_0_0),
        "the oracle header must anchor the pinned upstream commit"
    );
    assert!(
        source.contains("674837ce095db3bffb5acfccd50fdac57771d50b")
            && source.contains("09e9376288ddc3571f226883eb99753fecce818d"),
        "the oracle header must record all three pinned releases"
    );
}

#[test]
fn oracle_table_shape_matches_the_pinned_registry_counts() {
    let rows = load_oracle();
    assert_eq!(rows.len(), DANCER2_TWO_X_KEYWORD_TOTAL, "82 upstream keywords");
    let global = rows.iter().filter(|row| row.scope == DslKeywordScope::Global).count();
    let route_only =
        rows.iter().filter(|row| row.scope == DslKeywordScope::RouteHandlerOnly).count();
    assert_eq!(global, DANCER2_TWO_X_KEYWORD_GLOBAL, "43 is_global => 1 rows");
    assert_eq!(route_only, DANCER2_TWO_X_KEYWORD_ROUTE_ONLY, "39 route-handler-only rows");
}

fn registry_matches_oracle_row(keyword: &Dancer2TwoXDslKeyword, row: &OracleRow, context: &str) {
    assert_eq!(keyword.name, row.name, "{context}: oracle/registry row identity");
    assert_eq!(keyword.scope, row.scope, "{context}: scope for `{}`", keyword.name);
    assert_eq!(
        keyword.prototype.map(ToOwned::to_owned),
        row.prototype,
        "{context}: prototype for `{}`",
        keyword.name
    );
    assert_eq!(
        keyword.deprecation_replacement.map(ToOwned::to_owned),
        row.deprecation_replacement,
        "{context}: deprecation for `{}`",
        keyword.name
    );
}

#[test]
fn registry_equals_the_pinned_upstream_table_in_order() {
    let rows = load_oracle();
    assert_eq!(
        DANCER2_TWO_X_DSL_KEYWORDS.len(),
        rows.len(),
        "the registry and the oracle must cover the same keyword count"
    );
    for (index, (keyword, row)) in DANCER2_TWO_X_DSL_KEYWORDS.iter().zip(rows.iter()).enumerate() {
        registry_matches_oracle_row(keyword, row, &format!("row {index}"));
    }
}

#[test]
fn every_oracle_row_has_exactly_one_registry_entry_and_no_extras() {
    let rows = load_oracle();
    for row in &rows {
        let matches: Vec<_> =
            DANCER2_TWO_X_DSL_KEYWORDS.iter().filter(|keyword| keyword.name == row.name).collect();
        assert_eq!(matches.len(), 1, "keyword `{}` must appear exactly once", row.name);
        registry_matches_oracle_row(matches[0], row, "set comparison");
    }
}

#[test]
fn prototypes_in_the_oracle_are_exactly_delayed_and_prepare_app() {
    let rows = load_oracle();
    let prototyped: Vec<(String, String)> = rows
        .iter()
        .filter_map(|row| row.prototype.clone().map(|proto| (row.name.clone(), proto)))
        .collect();
    assert_eq!(
        prototyped,
        vec![
            ("delayed".to_string(), "&@".to_string()),
            ("prepare_app".to_string(), "&".to_string()),
        ]
    );
}

#[test]
fn deprecations_in_the_oracle_are_the_upstream_runtime_croaks() {
    let rows = load_oracle();
    let deprecated: Vec<(String, String)> = rows
        .iter()
        .filter_map(|row| {
            row.deprecation_replacement.clone().map(|replacement| (row.name.clone(), replacement))
        })
        .collect();
    assert_eq!(
        deprecated,
        vec![
            ("context".to_string(), "app".to_string()),
            ("header".to_string(), "response_header".to_string()),
            ("headers".to_string(), "response_headers".to_string()),
            ("push_header".to_string(), "push_response_header".to_string()),
        ]
    );
}

#[test]
fn oracle_carries_the_repo_correction_rows() {
    // #13616: the 1.x table misclassifies these; the pinned upstream map is
    // authoritative for 2.x.
    let rows = load_oracle();
    for corrected in ["cookie", "redirect"] {
        let row = rows.iter().find(|row| row.name == corrected);
        assert!(
            matches!(row, Some(row) if row.scope == DslKeywordScope::RouteHandlerOnly),
            "`{corrected}` must be route-handler-only in the pinned upstream map"
        );
    }
    for absent in ["route", "before", "after", "body"] {
        assert!(
            !rows.iter().any(|row| row.name == absent),
            "`{absent}` is not an upstream DSL keyword"
        );
    }
}
