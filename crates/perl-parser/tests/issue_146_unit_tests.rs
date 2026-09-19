//! Unit tests for specific Issue #146 fixes
//!
//! These tests validate individual components and fixes implemented
//! for the architectural integrity repair.
//!
//! Note: The actual TDD workflow and refactoring implementations are in
//! the perl-tdd-support and perl-refactoring crates respectively.
//! The perl-parser crate re-exports these modules.

#[cfg(test)]
mod tdd_workflow_unit_tests {
    use std::fs;

    // Path to the actual TDD workflow implementation
    const TDD_WORKFLOW_PATH: &str = "../perl-tdd-support/src/tdd/tdd_workflow.rs";

    /// Test that tdd_workflow.rs signature variable fix is correct
    #[test]
    fn test_signature_variable_fix() -> Result<(), Box<dyn std::error::Error>> {
        let content = fs::read_to_string(TDD_WORKFLOW_PATH)?;

        // Should not contain undefined signature variable usage
        assert!(
            !content.contains("let args = signature.as_ref()"),
            "tdd_workflow.rs still contains undefined signature variable"
        );

        Ok(())
    }

    /// Test that tower_lsp imports are replaced with lsp_types
    #[test]
    fn test_lsp_imports_fix() -> Result<(), Box<dyn std::error::Error>> {
        let content = fs::read_to_string(TDD_WORKFLOW_PATH)?;

        // Should not contain tower_lsp imports
        assert!(
            !content.contains("use tower_lsp::lsp_types"),
            "tdd_workflow.rs still contains tower_lsp imports"
        );

        Ok(())
    }

    /// Test that generate_basic_test method compiles correctly
    #[test]
    fn test_generate_basic_test_method() -> Result<(), Box<dyn std::error::Error>> {
        // This test validates that the method signature and implementation are correct
        let content = fs::read_to_string(TDD_WORKFLOW_PATH)?;

        // Check that the method exists and has correct parameter usage
        if content.contains("fn generate_basic_test") {
            // The method should use the params parameter
            let method_start = content
                .find("fn generate_basic_test")
                .ok_or("generate_basic_test method not found")?;
            let method_end = content[method_start..]
                .find("\n    }")
                .ok_or("generate_basic_test method end not found")?
                + method_start;
            let method_content = &content[method_start..method_end];

            assert!(
                method_content.contains("params"),
                "generate_basic_test method does not reference params parameter"
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod lib_integration_tests {
    /// Test that lib.rs module declarations are correct
    ///
    /// Note: The modules are named `tdd` and `refactor` (not `tdd_workflow` and `refactoring`).
    /// The tdd_workflow and refactoring submodules are re-exported from these parent modules.
    #[test]
    fn test_lib_module_declarations() -> Result<(), Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string("src/lib.rs")?;

        // Should contain tdd module (parent module for tdd_workflow)
        assert!(
            content.contains("pub mod tdd;") && !content.contains("// pub mod tdd;"),
            "tdd module is missing or commented out in lib.rs"
        );

        // Should contain refactor module (parent module for refactoring)
        assert!(
            content.contains("pub mod refactor;") && !content.contains("// pub mod refactor;"),
            "refactor module is missing or commented out in lib.rs"
        );

        Ok(())
    }

    /// Test that public API exports are added correctly
    #[test]
    fn test_public_api_exports() -> Result<(), Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string("src/lib.rs")?;

        // Check for TDD workflow exports (re-exported from tdd submodule)
        assert!(
            content.contains("pub use tdd::tdd_workflow")
                || content.contains("pub use tdd_workflow"),
            "TDD workflow is not exported from lib.rs"
        );

        // The unified refactoring engine export was retired (#5231); the
        // operation-specific refactor submodules remain the exported surface.
        assert!(
            content.contains("pub use refactor::import_optimizer")
                && content.contains("pub use refactor::modernize")
                && content.contains("pub use refactor::workspace_refactor"),
            "Operation-specific refactor exports are missing from lib.rs"
        );

        Ok(())
    }
}

#[cfg(test)]
mod api_contract_validation_tests {
    /// Test TestGenerator API contract
    #[test]
    fn test_test_generator_api_contract() {
        use perl_parser::test_generator::{TestFramework, TestGenerator};

        // Validate that TestGenerator can be instantiated with available frameworks
        let _generator = TestGenerator::new(TestFramework::Test2V0);

        // If this compiles, the API contract is valid (verified by compilation)
    }

    /// Test that existing parser APIs remain stable
    #[test]
    fn test_parser_api_stability() {
        // Core parser functionality validation
        use perl_parser::error::ParseResult;

        // Test that core types are still available
        let _result: ParseResult<()> = Ok(());

        // If this compiles, core parser API is stable (verified by compilation)
    }

    /// Test LSP types availability
    #[test]
    fn test_lsp_types_availability() {
        use lsp_types::{Position, Range};

        // Test that LSP types can be used
        let _position = Position::new(0, 0);
        let _range = Range::new(Position::new(0, 0), Position::new(1, 0));

        // If this compiles, LSP types are properly available (verified by compilation)
    }
}

/// Performance and quality tests
#[cfg(test)]
mod quality_assurance_tests {
    use std::process::Command;

    /// Build a nested cargo invocation that cannot contend on the outer
    /// `cargo test` run's locks (#15663): "Blocking waiting for file lock"
    /// waits surfaced in the captured stderr and failed the warning scan as
    /// if the crate emitted warnings. The nested invocation gets its own
    /// stable target dir (removing the build-dir lock overlap with the outer
    /// run; the cache stays warm across runs) and runs `--offline` so it
    /// never blocks on registry index/package-cache work.
    fn nested_cargo_command(args: &[&str]) -> Command {
        let mut command = Command::new("cargo");
        command.args(args).arg("--offline");
        let target_dir = std::env::temp_dir().join("perl-parser-qa-nested-target");
        command.env("CARGO_TARGET_DIR", target_dir);
        command
    }

    /// Test that the crate builds without warnings after fixes
    #[test]
    fn test_build_without_warnings() -> Result<(), Box<dyn std::error::Error>> {
        let output = nested_cargo_command(&["build", "--package", "perl-parser"]).output()?;

        let stderr = String::from_utf8_lossy(&output.stderr);

        // The crate's own sources must compile warning-free. The scan is
        // anchored to this crate because the isolated build compiles the
        // whole dependency chain from source, and dependency crates carry
        // pre-existing missing-docs warnings that are not this test's claim
        // (workspace-wide warning hygiene is tracked on #15734).
        assert!(
            !warnings_anchored_in_this_crate(&stderr),
            "perl-parser build contains warnings: {stderr}"
        );

        assert!(output.status.success(), "Build failed: {stderr}");

        Ok(())
    }

    /// Whether the captured cargo output contains a compiler warning whose
    /// anchor line names this crate's own sources. Path separators are
    /// normalized so the check behaves the same on Windows and Unix.
    fn warnings_anchored_in_this_crate(stderr: &str) -> bool {
        let normalized = stderr.replace('\\', "/");
        let lines: Vec<&str> = normalized.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if line.starts_with("warning: ") {
                let anchored = lines[index + 1..]
                    .iter()
                    .find(|candidate| candidate.trim_start().starts_with("-->"));
                if anchored.is_some_and(|anchor| anchor.contains("crates/perl-parser/src")) {
                    return true;
                }
            }
        }
        false
    }

    /// Test that tests pass after architectural repair
    #[test]
    fn test_test_suite_passes() -> Result<(), Box<dyn std::error::Error>> {
        let output =
            nested_cargo_command(&["test", "--package", "perl-parser", "--lib"]).output()?;

        assert!(
            output.status.success(),
            "Test suite failed after architectural repair: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        Ok(())
    }
}
