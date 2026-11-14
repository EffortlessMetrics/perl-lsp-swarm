#![deny(clippy::map_err_ignore)]

use tree_sitter_perl_c::queries::{
    folds_query_str, highlights_query_str, injections_query_str, load_folds_query,
    load_highlights_query, load_injections_query, load_matchup_query, matchup_query_str,
};

#[test]
fn queries_module_loads_injections() -> Result<(), Box<dyn std::error::Error>> {
    let query = load_injections_query()?;
    assert!(!query.capture_names().is_empty());
    assert!(
        query.capture_names().contains(&"injection.content"),
        "expected injection.content capture"
    );
    Ok(())
}

#[test]
fn queries_module_loads_highlights_reports_snapshot_drift() -> Result<(), Box<dyn std::error::Error>>
{
    let Err(error) = load_highlights_query() else {
        return Err(
            "snapshot drift resolved: expect load_highlights_query to fail until grammar refresh"
                .into(),
        );
    };
    assert_eq!(
        error.kind,
        tree_sitter::QueryErrorKind::Structure,
        "expected structural mismatch against frozen grammar, got {:?}",
        error.kind
    );
    Ok(())
}

#[test]
fn queries_module_loads_folds() -> Result<(), Box<dyn std::error::Error>> {
    let query = load_folds_query()?;
    // Folds query may use anonymous patterns; compilation success is the contract.
    let _ = query;
    Ok(())
}

#[test]
fn queries_module_loads_matchup() -> Result<(), Box<dyn std::error::Error>> {
    let query = load_matchup_query()?;
    assert!(!query.capture_names().is_empty());
    Ok(())
}

#[test]
fn queries_module_str_functions_return_non_empty() {
    assert!(!injections_query_str().is_empty());
    assert!(!highlights_query_str().is_empty());
    assert!(!folds_query_str().is_empty());
    assert!(!matchup_query_str().is_empty());
}

#[test]
fn queries_module_injections_str_matches_crate_root_constant() {
    assert_eq!(
        injections_query_str(),
        tree_sitter_perl_c::INJECTIONS_QUERY,
        "queries module and crate-root re-export must share one source"
    );
}
