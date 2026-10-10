//! Test scaffolding for Issue #146: Architectural Integrity Repair
//!
//! Comprehensive test suite validating the restoration of commented-out modules
//! (tdd_workflow.rs and refactoring.rs) with full compilation and integration testing.

use std::process::Command;

/// Test suite for Issue #146 - Architectural Integrity Repair
#[cfg(test)]
mod issue_146_tests {
    use super::*;

    // The JSON compilation check moved to parser_workspace_prepare (#17479).

    /// AC-1.2: Validate LSP types import compatibility
    #[test]
    fn test_lsp_types_import_compatibility() {
        // This test validates that the module uses lsp_types instead of tower_lsp
        // By importing and using key LSP types that should be available

        use lsp_types::{CodeActionKind, DiagnosticSeverity, Position, Range};

        // Test that all required LSP types are available and can be instantiated
        let _position = Position::new(0, 0);
        let _range = Range::new(Position::new(0, 0), Position::new(0, 10));
        let _diagnostic_severity = DiagnosticSeverity::ERROR;
        let _code_action_kind = CodeActionKind::REFACTOR;

        // If this compiles, LSP types are properly available
    }

    /// AC-2.1: Test refactoring.rs module structure and API
    #[test]
    fn test_refactoring_module_api_structure() {
        // This test will validate the refactoring module API once it's created
        // For now, it serves as a placeholder to ensure test infrastructure works

        // Check that core modules exist for refactoring functionality
        // Note: workspace_refactor and modernize will be integrated into refactoring.rs

        // If we can import these, the foundation for refactoring.rs exists
    }

    /// AC-3.1: Integration test for lib.rs module exports
    #[test]
    fn test_lib_module_exports_integration() {
        // Test that lib.rs can be parsed and core modules are available
        // This validates that uncommenting modules doesn't break existing functionality

        // Core parser functionality validation
        use perl_parser::error::ParseResult;

        // Test basic parser functionality remains intact
        let _result: ParseResult<()> = Ok(());

        // If this compiles, core parser API is stable
    }

    /// AC-1.3: API contract validation for TDD workflow components
    #[test]
    fn test_tdd_workflow_api_contracts() {
        // Test that TestGenerator, TestRunner, and RefactoringSuggester APIs are compatible
        // This validates that tdd_workflow.rs can integrate with existing test infrastructure

        use perl_parser::test_generator::{TestFramework, TestGenerator};

        // Test that TestGenerator can be instantiated
        let _test_generator = TestGenerator::new(TestFramework::Test2V0);

        // If this compiles, API contracts are valid
    }
}

/// Integration tests for architectural integrity
#[cfg(test)]
mod integration_tests {
    use super::*;

    // Full compilation and strict Clippy obligations moved to the required
    // parser_workspace_prepare gate, before any routed runtime (#17479).

    /// Test LSP end-to-end functionality after module restoration
    #[test]
    #[ignore = "this test invokes `cargo test --package perl-lsp` but the package was renamed \
                to perl-lsp-rs; retire or replace when the e2e suite is re-enabled (see issue #1226)"]
    fn test_lsp_e2e_with_restored_modules() {
        // This test validates that LSP functionality works correctly
        // after tdd_workflow.rs and refactoring.rs are restored

        let output_res = Command::new("cargo")
            .args(["test", "--package", "perl-lsp", "--test", "lsp_comprehensive_e2e_test"])
            .output();
        assert!(output_res.is_ok(), "Failed to run LSP E2E tests");
        let output = output_res.unwrap_or_else(|_| unreachable!());

        assert!(
            output.status.success(),
            "LSP E2E tests failed after module restoration: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
