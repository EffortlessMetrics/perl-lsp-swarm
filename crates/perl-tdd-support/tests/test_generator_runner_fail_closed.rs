//! Fail-closed regression tests for `test_generator::TestRunner`.
//!
//! Slice 1 of #4948: this runner must never report success for work it did not perform.

#![allow(deprecated, reason = "tests intentionally exercise deprecated test_generator::TestRunner")]

use perl_tdd_support::must_err;
use perl_tdd_support::test_generator::{TestExecutionError, TestRunner};

#[test]
fn run_tests_with_one_path_does_not_fake_pass() -> Result<(), Box<dyn std::error::Error>> {
    let runner = TestRunner::new();
    let err = must_err(runner.run_tests(&["t/basic.t".to_string()]));
    assert_eq!(
        err,
        TestExecutionError::Unsupported {
            operation: "run_tests",
            reason: "test_generator::TestRunner does not execute subprocesses; \
                use perl_tdd_support::test_runner::TestRunner for execution",
        }
    );
    Ok(())
}

#[test]
fn watch_with_mode_enabled_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let runner = TestRunner::with_execution_flags_for_tests(true, false);
    let err = must_err(runner.watch(&["t/basic.t".to_string()]));
    assert_eq!(
        err,
        TestExecutionError::Unsupported {
            operation: "watch",
            reason: "no file watcher is installed",
        }
    );
    Ok(())
}

#[test]
fn get_coverage_with_flag_enabled_returns_none() -> Result<(), Box<dyn std::error::Error>> {
    let runner = TestRunner::with_execution_flags_for_tests(false, true);
    let coverage = runner.get_coverage();
    assert!(coverage.is_none());
    Ok(())
}

#[test]
fn tdd_workflow_run_tests_stays_red_without_all_tests_pass_message()
-> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;

    use perl_tdd_support::tdd_workflow::{TddConfig, TddWorkflow, WorkflowState};

    let mut workflow = TddWorkflow::new(TddConfig::default());
    workflow.start_cycle("feature");

    let result = workflow.run_tests(&[PathBuf::from("t/feature.t")]);
    assert_eq!(result.phase, "Red");
    assert!(!result.message.contains("All tests pass"));
    assert_eq!(workflow.get_status().state, WorkflowState::Red);

    Ok(())
}
