//! Fail-closed status/config and legacy workflow deprecation (#8428).

#![allow(deprecated, reason = "tests intentionally exercise deprecated legacy workflow facades")]

use perl_tdd_support::tdd_workflow::{
    EXECUTION_UNSUPPORTED_PHASE, LEGACY_TDD_WORKFLOW_DEPRECATION, TddConfig, TddConfigError,
    TddWorkflow, WorkflowState, read_tdd_config, read_tdd_config_with_diagnostics,
};
use std::path::PathBuf;

#[test]
fn malformed_tdd_config_returns_error() -> Result<(), Box<dyn std::error::Error>> {
    let err = read_tdd_config(r#"{"coverage_threshold":"not-a-number"}"#).unwrap_err();
    assert!(matches!(err, TddConfigError::Parse(_)));

    let err = read_tdd_config(r#"{"coverage_threshold": -1.0}"#).unwrap_err();
    assert!(matches!(err, TddConfigError::InvalidCoverageThreshold));

    let err = read_tdd_config(r#"{"test_framework": ""}"#).unwrap_err();
    assert!(matches!(err, TddConfigError::EmptyTestFramework));
    Ok(())
}

#[test]
fn valid_tdd_config_roundtrip_unchanged() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = TddConfig::default();
    let json = serde_json::to_string(&cfg)?;
    let parsed = read_tdd_config(&json)?;
    assert_eq!(parsed.test_framework, cfg.test_framework);
    assert_eq!(parsed.test_file_pattern, cfg.test_file_pattern);
    assert!((parsed.coverage_threshold - cfg.coverage_threshold).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn empty_workflow_status_is_not_green() -> Result<(), Box<dyn std::error::Error>> {
    let wf = TddWorkflow::new(TddConfig::default());
    let status = wf.get_status();
    assert_ne!(status.tests_passing, Some(true));
    assert!(status.coverage.is_none());
    assert!(!status.suggestions_available);
    assert_eq!(status.state, WorkflowState::Idle);
    Ok(())
}

#[test]
fn unsupported_run_tests_does_not_authorize_red_or_green() -> Result<(), Box<dyn std::error::Error>>
{
    let mut wf = TddWorkflow::new(TddConfig::default());
    wf.start_cycle("feature");

    let result = wf.run_tests(&[PathBuf::from("t/feature.t")]);
    assert_eq!(result.phase, EXECUTION_UNSUPPORTED_PHASE);
    assert_ne!(result.phase, "Red");
    assert_ne!(result.phase, "Green");
    assert_eq!(wf.get_status().state, WorkflowState::Idle);
    assert_ne!(wf.get_status().tests_passing, Some(true));
    Ok(())
}

#[test]
fn legacy_workflow_deprecation_notice_points_at_replacements()
-> Result<(), Box<dyn std::error::Error>> {
    let note = LEGACY_TDD_WORKFLOW_DEPRECATION;
    assert!(note.contains("#8175"));
    assert!(note.contains("#4972"));
    assert!(note.contains("#8418"));
    Ok(())
}

#[test]
fn inactive_config_fields_emit_deprecation_diagnostics() -> Result<(), Box<dyn std::error::Error>> {
    let json = r#"{
        "auto_generate_tests": true,
        "test_on_save": true,
        "show_inline_coverage": true,
        "test_framework": "Test::More",
        "test_file_pattern": "t/{name}.t",
        "coverage_threshold": 80.0,
        "continuous_testing": true,
        "auto_suggest_refactorings": true
    }"#;
    let read = read_tdd_config_with_diagnostics(json)?;
    assert!(!read.deprecations.is_empty());
    assert!(read.deprecations.iter().any(|d| d.contains("test_on_save")));
    Ok(())
}
