//! TDD (Test-Driven Development) workflow integration for LSP
//!
//! Provides a complete red-green-refactor cycle support with
//! automatic test generation, continuous testing, and refactoring suggestions.

use crate::ast::Node;

// Re-use Diagnostic from tdd_basic to avoid duplication
use crate::tdd_basic::{Diagnostic, DiagnosticSeverity};
#[allow(deprecated, reason = "workflow still wraps generation runner until #4972")]
use crate::test_generator::TestRunner;
use crate::test_generator::{CoverageReport, TestResults};
use crate::test_generator::{RefactoringSuggester, RefactoringSuggestion};
use crate::test_generator::{TestCase, TestFramework, TestGenerator};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Deprecation guidance for the full legacy workflow facade (#8428).
pub const LEGACY_TDD_WORKFLOW_DEPRECATION: &str = "Legacy TDD workflow facade (#8428): \
    cycle semantics move to #8175; test generation to #8154; execution to #4972/#7564; \
    public-surface removal is governed by #8418.";

/// Phase string returned when execution is unavailable on the generation-only runner.
pub const EXECUTION_UNSUPPORTED_PHASE: &str = "Unsupported";

/// Error while reading legacy TDD workflow configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TddConfigError {
    /// JSON could not be parsed into [`TddConfig`].
    Parse(String),
    /// `coverage_threshold` is not a finite percentage in `0.0..=100.0`.
    InvalidCoverageThreshold,
    /// `test_framework` is empty after trimming.
    EmptyTestFramework,
    /// `test_file_pattern` is empty after trimming.
    EmptyTestFilePattern,
}

impl std::fmt::Display for TddConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "invalid TDD config JSON: {message}"),
            Self::InvalidCoverageThreshold => {
                write!(f, "coverage_threshold must be a finite value between 0 and 100")
            }
            Self::EmptyTestFramework => write!(f, "test_framework must not be empty"),
            Self::EmptyTestFilePattern => write!(f, "test_file_pattern must not be empty"),
        }
    }
}

impl std::error::Error for TddConfigError {}

/// Diagnostics emitted while accepting a legacy configuration for compatibility.
#[derive(Debug, Clone, PartialEq)]
pub struct TddConfigReadResult {
    /// Parsed configuration that passed structural validation.
    pub config: TddConfig,
    /// Deprecation notes for inactive automation fields present in the input.
    pub deprecations: Vec<String>,
}

/// Parse legacy workflow configuration, failing closed on malformed input.
pub fn read_tdd_config(json: &str) -> Result<TddConfig, TddConfigError> {
    read_tdd_config_with_diagnostics(json).map(|read| read.config)
}

/// Parse legacy workflow configuration and collect inactive-field deprecation notes.
pub fn read_tdd_config_with_diagnostics(json: &str) -> Result<TddConfigReadResult, TddConfigError> {
    let config: TddConfig =
        serde_json::from_str(json).map_err(|err| TddConfigError::Parse(err.to_string()))?;
    config.validate()?;
    let deprecations = config.inactive_field_deprecations();
    Ok(TddConfigReadResult { config, deprecations })
}

/// TDD workflow manager
#[deprecated(note = "Legacy TDD workflow facade (#8428); see LEGACY_TDD_WORKFLOW_DEPRECATION")]
pub struct TddWorkflow {
    /// Test generator
    generator: TestGenerator,
    /// Test runner
    #[allow(deprecated, reason = "workflow still wraps generation runner until #4972")]
    runner: TestRunner,
    /// Refactoring suggester
    suggester: RefactoringSuggester,
    /// Current workflow state
    state: WorkflowState,
    /// Test results cache
    test_cache: HashMap<PathBuf, TestResults>,
    /// Coverage tracking
    coverage_tracker: CoverageTracker,
    /// Configuration
    config: TddConfig,
}

/// Represents the current phase of the TDD (Test-Driven Development) workflow cycle.
///
/// Tracks the developer's position in the red-green-refactor cycle, enabling context-aware
/// suggestions and automation during test development and implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkflowState {
    /// Writing test (Red phase)
    Red,
    /// Making test pass (Green phase)
    Green,
    /// Refactoring code (Refactor phase)
    Refactor,
    /// Not in TDD cycle
    Idle,
}

/// Configuration options for TDD workflow automation and behavior.
///
/// Customizes how the TDD workflow manager generates tests, runs them, and provides
/// feedback. All settings have sensible defaults suitable for typical Perl development.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TddConfig {
    /// Automatically generate tests for new code
    pub auto_generate_tests: bool,
    /// Run tests on save
    pub test_on_save: bool,
    /// Show coverage inline
    pub show_inline_coverage: bool,
    /// Test framework preference
    pub test_framework: String,
    /// Test file naming pattern
    pub test_file_pattern: String,
    /// Minimum coverage threshold
    pub coverage_threshold: f64,
    /// Enable continuous testing
    pub continuous_testing: bool,
    /// Auto-suggest refactorings after green
    pub auto_suggest_refactorings: bool,
}

impl Default for TddConfig {
    fn default() -> Self {
        Self {
            auto_generate_tests: true,
            test_on_save: true,
            show_inline_coverage: true,
            test_framework: "Test::More".to_string(),
            test_file_pattern: "t/{name}.t".to_string(),
            coverage_threshold: 80.0,
            continuous_testing: true,
            auto_suggest_refactorings: true,
        }
    }
}

impl TddConfig {
    fn validate(&self) -> Result<(), TddConfigError> {
        if self.test_framework.trim().is_empty() {
            return Err(TddConfigError::EmptyTestFramework);
        }
        if self.test_file_pattern.trim().is_empty() {
            return Err(TddConfigError::EmptyTestFilePattern);
        }
        if !self.coverage_threshold.is_finite() || !(0.0..=100.0).contains(&self.coverage_threshold)
        {
            return Err(TddConfigError::InvalidCoverageThreshold);
        }
        Ok(())
    }

    fn inactive_field_deprecations(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.auto_generate_tests {
            notes.push(
                "auto_generate_tests is compatibility-ignored; generation is owned by #8154"
                    .to_string(),
            );
        }
        if self.test_on_save {
            notes.push(
                "test_on_save is compatibility-ignored; save-time execution is unavailable (#4972)"
                    .to_string(),
            );
        }
        if self.show_inline_coverage {
            notes.push(
                "show_inline_coverage is compatibility-ignored; inline coverage has no live owner"
                    .to_string(),
            );
        }
        if self.continuous_testing {
            notes.push(
                "continuous_testing is compatibility-ignored; watcher execution is unavailable (#4972)"
                    .to_string(),
            );
        }
        if self.auto_suggest_refactorings {
            notes.push(
                "auto_suggest_refactorings has no reachable effect with the generation-only runner"
                    .to_string(),
            );
        }
        notes
    }
}

/// Coverage tracking for TDD
pub struct CoverageTracker {
    /// Line coverage data
    line_coverage: HashMap<PathBuf, Vec<LineCoverage>>,
    /// Branch coverage data (architectural placeholder for future implementation)
    #[allow(dead_code)]
    branch_coverage: HashMap<PathBuf, Vec<BranchCoverage>>,
    /// Overall coverage percentage
    total_coverage: f64,
}

/// Line-level code coverage information
#[derive(Debug, Clone)]
pub struct LineCoverage {
    /// Line number in the source file
    pub line: usize,
    /// Number of times this line was executed
    pub hits: usize,
    /// Whether this line is considered covered
    pub covered: bool,
}

/// Branch-level code coverage information
#[derive(Debug, Clone)]
pub struct BranchCoverage {
    /// Line number where the branch occurs
    pub line: usize,
    /// Unique identifier for this branch within the line
    pub branch_id: usize,
    /// Whether this branch was taken at least once
    pub taken: bool,
    /// Number of times this branch was executed
    pub hits: usize,
}

#[allow(deprecated, reason = "deprecated facade remains until #8418 removal")]
impl TddWorkflow {
    /// Create a new TDD workflow manager with the given configuration
    #[allow(deprecated, reason = "workflow still wraps generation runner until #4972")]
    pub fn new(config: TddConfig) -> Self {
        let framework = match config.test_framework.as_str() {
            "Test2::V0" => TestFramework::Test2V0,
            "Test::Simple" => TestFramework::TestSimple,
            "Test::Class" => TestFramework::TestClass,
            _ => TestFramework::TestMore,
        };

        Self {
            generator: TestGenerator::new(framework),
            runner: TestRunner::new(),
            suggester: RefactoringSuggester::new(),
            state: WorkflowState::Idle,
            test_cache: HashMap::new(),
            coverage_tracker: CoverageTracker::new(),
            config,
        }
    }

    /// Start a new TDD cycle
    pub fn start_cycle(&mut self, test_name: &str) -> TddCycleResult {
        TddCycleResult {
            phase: "Intent".to_string(),
            message: format!("Starting TDD cycle for '{}'", test_name),
            actions: vec![
                TddAction::GenerateTest(test_name.to_string()),
                TddAction::CreateTestFile(self.get_test_file_path(test_name)),
            ],
        }
    }

    /// Generate tests for the given code
    pub fn generate_tests(&self, ast: &Node, source: &str) -> Vec<TestCase> {
        self.generator.generate_tests(ast, source)
    }

    /// Generate a specific test type
    pub fn generate_test_for_function(
        &self,
        function_name: &str,
        params: &[String],
        test_type: TestType,
    ) -> TestCase {
        let test_name = format!("test_{}_{:?}", function_name, test_type);
        let description = format!("{:?} test for {}", test_type, function_name);

        let code = match test_type {
            TestType::Basic => self.generate_basic_test(function_name, params),
            TestType::EdgeCase => self.generate_edge_case_test(function_name, params),
            TestType::ErrorHandling => self.generate_error_test(function_name, params),
            TestType::Performance => self.generate_performance_test(function_name),
            TestType::Integration => self.generate_integration_test(function_name, params),
        };

        TestCase {
            name: test_name,
            description,
            code,
            is_todo: matches!(test_type, TestType::Integration | TestType::Performance),
        }
    }

    fn generate_basic_test(&self, name: &str, params: &[String]) -> String {
        let args = params
            .iter()
            .enumerate()
            .map(|(i, _)| format!("'test_value_{}'", i))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "use Test::More;\n\n\
             subtest '{}' => sub {{\n    \
             my $result = {}({});\n    \
             ok(defined $result, 'Returns defined value');\n    \
             # PENDING: Add specific assertions\n\
             }};\n\n\
             done_testing();\n",
            name, name, args
        )
    }

    fn generate_edge_case_test(&self, name: &str, params: &[String]) -> String {
        let undef_args = repeated_edge_case_args(params, "undef");
        let empty_args = repeated_edge_case_args(params, "''");
        let special_args = repeated_edge_case_args(params, "\"\\n\\t\\0\"");

        format!(
            "use Test::More;\n\n\
             subtest '{} edge cases' => sub {{\n    \
             # Test with undef\n    \
             eval {{ {}({}) }};\n    \
             ok(!$@, 'Handles undef');\n    \n    \
             # Test with empty values\n    \
             eval {{ {}({}) }};\n    \
             ok(!$@, 'Handles empty string');\n    \n    \
             # Test with special characters\n    \
             eval {{ {}({}) }};\n    \
             ok(!$@, 'Handles special characters');\n\
             }};\n\n\
             done_testing();\n",
            name, name, undef_args, name, empty_args, name, special_args
        )
    }

    fn generate_error_test(&self, name: &str, _params: &[String]) -> String {
        format!(
            "use Test::More;\n\
             use Test::Exception;\n\n\
             subtest '{} error handling' => sub {{\n    \
             # Test that errors are caught\n    \
             dies_ok {{ {}(undef, undef, undef) }} 'Dies on invalid input';\n    \n    \
             # Test error message\n    \
             throws_ok {{ {}() }} qr/required/, 'Correct error message';\n\
             }};\n\n\
             done_testing();\n",
            name, name, name
        )
    }

    fn generate_performance_test(&self, name: &str) -> String {
        format!(
            "use Test::More;\n\
             use Benchmark qw(timethis);\n\n\
             subtest '{} performance' => sub {{\n    \
             my $iterations = 10000;\n    \
             my $result = timethis($iterations, sub {{ {}() }});\n    \
             \n    \
             # Check performance threshold\n    \
             my $rate = $result->iters / $result->cpu_a;\n    \
             cmp_ok($rate, '>', 1000, 'Performance exceeds 1000 ops/sec');\n\
             }};\n\n\
             done_testing();\n",
            name, name
        )
    }

    fn generate_integration_test(&self, name: &str, _params: &[String]) -> String {
        format!(
            "use Test::More;\n\n\
             subtest '{} integration' => sub {{\n    \
             # PENDING: Set up test environment\n    \
             # PENDING: Call {} with real dependencies\n    \
             # PENDING: Verify integration points\n    \
             pass('Integration test placeholder');\n\
             }};\n\n\
             done_testing();\n",
            name, name
        )
    }

    /// Run tests and update state
    pub fn run_tests(&mut self, test_files: &[PathBuf]) -> TddCycleResult {
        let file_strings: Vec<String> =
            test_files.iter().map(|p| p.to_string_lossy().to_string()).collect();

        let results = match self.runner.run_tests(&file_strings) {
            Ok(results) => results,
            Err(err) => {
                return TddCycleResult {
                    phase: EXECUTION_UNSUPPORTED_PHASE.to_string(),
                    message: err.to_string(),
                    actions: vec![],
                };
            }
        };

        // Cache results
        for file in test_files {
            self.test_cache.insert(file.clone(), results.clone());
        }

        // Update state based on the red-green-refactor cycle. Passing tests move the
        // workflow to Green; entering Refactor is an explicit next step so callers
        // can observe the complete cycle instead of skipping directly to Refactor.
        let (new_state, message) = if results.failed > 0 {
            (WorkflowState::Red, format!("{} tests failed", results.failed))
        } else if results.todo > 0 {
            (WorkflowState::Green, format!("All tests pass, {} TODOs remaining", results.todo))
        } else {
            (WorkflowState::Green, "All tests pass! Ready to refactor".to_string())
        };

        self.state = new_state.clone();

        let mut actions = vec![];

        // Suggest refactorings once the suite is green, but keep the phase explicit.
        if new_state == WorkflowState::Green && self.config.auto_suggest_refactorings {
            actions.push(TddAction::SuggestRefactorings);
        }

        TddCycleResult { phase: format!("{:?}", new_state), message, actions }
    }

    /// Move from green into the refactoring phase.
    pub fn start_refactor(&mut self) -> TddCycleResult {
        self.state = WorkflowState::Refactor;

        TddCycleResult {
            phase: "Refactor".to_string(),
            message: "Refactoring phase - improve code while keeping tests green".to_string(),
            actions: vec![TddAction::SuggestRefactorings, TddAction::UpdateCoverage],
        }
    }

    /// Complete the current TDD cycle and return to idle.
    pub fn complete_cycle(&mut self) -> TddCycleResult {
        self.state = WorkflowState::Idle;

        TddCycleResult {
            phase: "Idle".to_string(),
            message: "TDD cycle complete".to_string(),
            actions: vec![TddAction::RunTests],
        }
    }

    /// Get refactoring suggestions
    pub fn get_refactoring_suggestions(
        &mut self,
        ast: &Node,
        source: &str,
    ) -> Vec<RefactoringSuggestion> {
        self.suggester.analyze(ast, source)
    }

    /// Get current test coverage
    pub fn get_coverage(&self) -> Option<CoverageReport> {
        self.runner.get_coverage()
    }

    /// Update coverage data
    pub fn update_coverage(&mut self, file: PathBuf, coverage: Vec<LineCoverage>) {
        self.coverage_tracker.line_coverage.insert(file, coverage);
        self.coverage_tracker.calculate_total_coverage();
    }

    /// Get inline coverage annotations
    pub fn get_inline_coverage(&self, file: &Path) -> Vec<CoverageAnnotation> {
        let mut annotations = Vec::new();

        if let Some(coverage) = self.coverage_tracker.line_coverage.get(file) {
            for line_cov in coverage {
                if !line_cov.covered {
                    annotations.push(CoverageAnnotation {
                        line: line_cov.line,
                        message: "Not covered by tests".to_string(),
                        severity: AnnotationSeverity::Warning,
                    });
                } else if line_cov.hits == 0 {
                    annotations.push(CoverageAnnotation {
                        line: line_cov.line,
                        message: "Never executed".to_string(),
                        severity: AnnotationSeverity::Info,
                    });
                }
            }
        }

        annotations
    }

    /// Check if measured compatibility coverage meets the configured threshold.
    ///
    /// Returns `None` when no coverage facts were supplied.
    pub fn check_coverage_threshold(&self) -> Option<bool> {
        if !self.coverage_tracker.has_measured_facts() {
            return None;
        }
        Some(self.coverage_tracker.total_coverage >= self.config.coverage_threshold)
    }

    /// Get test file path for a given module
    fn get_test_file_path(&self, name: &str) -> PathBuf {
        let pattern = &self.config.test_file_pattern;
        let path_str = pattern.replace("{name}", name);
        PathBuf::from(path_str)
    }

    /// Get workflow status
    pub fn get_status(&self) -> WorkflowStatus {
        let tests_passing = if self.test_cache.is_empty() {
            None
        } else {
            Some(self.test_cache.values().all(|r| r.failed == 0))
        };
        let coverage = if self.coverage_tracker.has_measured_facts() {
            Some(self.coverage_tracker.total_coverage)
        } else {
            None
        };
        WorkflowStatus {
            state: self.state.clone(),
            coverage,
            tests_passing,
            suggestions_available: false,
        }
    }

    /// Generate diagnostics for uncovered code
    pub fn generate_coverage_diagnostics(&self, file: &Path) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        if let Some(coverage) = self.coverage_tracker.line_coverage.get(file) {
            for line_cov in coverage {
                if !line_cov.covered {
                    diagnostics.push(Diagnostic {
                        range: (line_cov.line, line_cov.line),
                        severity: DiagnosticSeverity::Warning,
                        code: Some("tdd.uncovered".to_string()),
                        message: "Line not covered by tests".to_string(),
                        related_information: vec![],
                        tags: vec![],
                    });
                }
            }
        }

        diagnostics
    }
}

fn repeated_edge_case_args(params: &[String], edge_case: &str) -> String {
    match params.len() {
        0 => String::new(),
        len => std::iter::repeat_n(edge_case, len).collect::<Vec<_>>().join(", "),
    }
}

impl CoverageTracker {
    fn new() -> Self {
        Self { line_coverage: HashMap::new(), branch_coverage: HashMap::new(), total_coverage: 0.0 }
    }

    fn has_measured_facts(&self) -> bool {
        !self.line_coverage.is_empty()
    }

    fn calculate_total_coverage(&mut self) {
        let mut total_lines = 0;
        let mut covered_lines = 0;

        for coverage in self.line_coverage.values() {
            for line in coverage {
                total_lines += 1;
                if line.covered {
                    covered_lines += 1;
                }
            }
        }

        self.total_coverage =
            if total_lines > 0 { (covered_lines as f64 / total_lines as f64) * 100.0 } else { 0.0 };
    }
}

/// Result of a TDD cycle action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TddCycleResult {
    /// Current phase of the TDD cycle (Red, Green, or Refactor)
    pub phase: String,
    /// Human-readable message describing the cycle state
    pub message: String,
    /// Recommended actions to take next
    pub actions: Vec<TddAction>,
}

/// Actions that can be taken during a TDD cycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TddAction {
    /// Generate a test with the given name
    GenerateTest(String),
    /// Create a new test file at the specified path
    CreateTestFile(PathBuf),
    /// Execute the test suite
    RunTests,
    /// Request refactoring suggestions for the code
    SuggestRefactorings,
    /// Refresh code coverage data
    UpdateCoverage,
    /// Display test failure details
    ShowFailures,
}

/// Types of tests that can be generated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestType {
    /// Simple happy-path test with typical inputs
    Basic,
    /// Tests for boundary conditions and unusual inputs
    EdgeCase,
    /// Tests for error conditions and exception handling
    ErrorHandling,
    /// Benchmarks and performance regression tests
    Performance,
    /// Tests involving multiple components or external dependencies
    Integration,
}

/// Inline annotation for displaying coverage information in the editor
#[derive(Debug, Clone)]
pub struct CoverageAnnotation {
    /// Line number to annotate
    pub line: usize,
    /// Description of the coverage status
    pub message: String,
    /// Severity level for display styling
    pub severity: AnnotationSeverity,
}

/// Severity levels for coverage and diagnostic annotations
#[derive(Debug, Clone)]
pub enum AnnotationSeverity {
    /// Critical issue that must be addressed
    Error,
    /// Potential problem that should be reviewed
    Warning,
    /// Informational message
    Info,
    /// Subtle suggestion or hint
    Hint,
}

/// Summary of the current TDD workflow status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStatus {
    /// Current phase of the TDD cycle
    pub state: WorkflowState,
    /// Measured coverage percentage when compatibility facts were supplied.
    pub coverage: Option<f64>,
    /// Whether cached test runs all passed; `None` when no test facts exist.
    pub tests_passing: Option<bool>,
    /// Whether refactoring suggestions are available from retained facts.
    pub suggestions_available: bool,
}

/// LSP integration for TDD workflow
#[cfg(feature = "lsp-compat")]
pub mod lsp_integration {
    use super::*;
    use lsp_types::{
        CodeAction, CodeActionKind, Command, Diagnostic as LspDiagnostic, DiagnosticSeverity,
        Position, Range,
    };

    /// Convert TDD actions to LSP code actions
    pub fn tdd_actions_to_code_actions(
        actions: Vec<TddAction>,
        _uri: &url::Url,
    ) -> Vec<CodeAction> {
        actions
            .into_iter()
            .map(|action| match action {
                TddAction::GenerateTest(name) => CodeAction {
                    title: format!("Generate test for '{}'", name),
                    kind: Some(CodeActionKind::REFACTOR),
                    command: Some(Command {
                        title: "Generate Test".to_string(),
                        command: "perl.tdd.generateTest".to_string(),
                        arguments: Some(vec![serde_json::json!(name)]),
                    }),
                    ..Default::default()
                },
                TddAction::RunTests => CodeAction {
                    title: "Run tests".to_string(),
                    kind: Some(CodeActionKind::new("test.run")),
                    command: Some(Command {
                        title: "Run Tests".to_string(),
                        command: "perl.tdd.runTests".to_string(),
                        arguments: None,
                    }),
                    ..Default::default()
                },
                TddAction::SuggestRefactorings => CodeAction {
                    title: "Get refactoring suggestions".to_string(),
                    kind: Some(CodeActionKind::REFACTOR),
                    command: Some(Command {
                        title: "Suggest Refactorings".to_string(),
                        command: "perl.tdd.suggestRefactorings".to_string(),
                        arguments: None,
                    }),
                    ..Default::default()
                },
                _ => CodeAction {
                    title: format!("{:?}", action),
                    kind: Some(CodeActionKind::EMPTY),
                    ..Default::default()
                },
            })
            .collect()
    }

    /// Convert coverage annotations to LSP diagnostics
    pub fn coverage_to_diagnostics(annotations: Vec<CoverageAnnotation>) -> Vec<LspDiagnostic> {
        annotations
            .into_iter()
            .map(|ann| LspDiagnostic {
                range: Range {
                    start: Position { line: ann.line as u32, character: 0 },
                    end: Position { line: ann.line as u32, character: 999 },
                },
                severity: Some(match ann.severity {
                    AnnotationSeverity::Error => DiagnosticSeverity::ERROR,
                    AnnotationSeverity::Warning => DiagnosticSeverity::WARNING,
                    AnnotationSeverity::Info => DiagnosticSeverity::INFORMATION,
                    AnnotationSeverity::Hint => DiagnosticSeverity::HINT,
                }),
                code: Some(lsp_types::NumberOrString::String("coverage".to_string())),
                source: Some("TDD".to_string()),
                message: ann.message,
                ..Default::default()
            })
            .collect()
    }

    /// Create status bar message for TDD state
    pub fn create_status_message(status: &WorkflowStatus) -> String {
        let coverage =
            status.coverage.map_or("unknown".to_string(), |value| format!("{value:.1}%"));
        let tests = match status.tests_passing {
            None => "?",
            Some(true) => "✓",
            Some(false) => "✗",
        };
        format!(
            "TDD: {:?} | Coverage: {coverage} | Tests: {tests} | Refactor: {}",
            status.state,
            if status.suggestions_available { "💡" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(deprecated, reason = "unit tests cover the deprecated legacy facade")]

    use super::*;
    use crate::ast::NodeKind;
    use crate::ast::SourceLocation;

    #[test]
    fn test_tdd_workflow_cycle() {
        let config = TddConfig::default();
        let mut workflow = TddWorkflow::new(config);

        // Start a new cycle
        let result = workflow.start_cycle("calculate_sum");
        assert_eq!(workflow.state, WorkflowState::Idle);
        assert_eq!(result.phase, "Intent");
        assert!(result.message.contains("calculate_sum"));
    }

    #[test]
    fn test_generate_tests() {
        let config = TddConfig::default();
        let workflow = TddWorkflow::new(config);

        let ast = Node::new(
            NodeKind::Subroutine {
                name: Some("multiply".to_string()),
                name_span: Some(SourceLocation { start: 4, end: 12 }),
                declarator: None,
                signature: None,
                body: Box::new(Node::new(
                    NodeKind::Block { statements: vec![] },
                    SourceLocation { start: 0, end: 0 },
                )),
                attributes: vec![],
                prototype: None,
            },
            SourceLocation { start: 0, end: 0 },
        );

        let tests = workflow.generate_tests(&ast, "sub multiply { }");
        assert!(!tests.is_empty());
    }

    #[test]
    fn test_coverage_tracking() {
        let config = TddConfig::default();
        let mut workflow = TddWorkflow::new(config);

        let coverage = vec![
            LineCoverage { line: 1, hits: 5, covered: true },
            LineCoverage { line: 2, hits: 0, covered: false },
            LineCoverage { line: 3, hits: 10, covered: true },
        ];

        workflow.update_coverage(PathBuf::from("test.pl"), coverage);

        let annotations = workflow.get_inline_coverage(&PathBuf::from("test.pl"));
        assert_eq!(annotations.len(), 1); // One uncovered line
        assert_eq!(annotations[0].line, 2);
    }

    #[test]
    fn test_coverage_resets_to_zero_when_all_inputs_empty() -> Result<(), Box<dyn std::error::Error>>
    {
        let config = TddConfig::default();
        let mut workflow = TddWorkflow::new(config);

        workflow.update_coverage(
            PathBuf::from("a.pl"),
            vec![LineCoverage { line: 1, hits: 1, covered: true }],
        );
        assert_eq!(workflow.check_coverage_threshold(), Some(true));

        workflow.update_coverage(PathBuf::from("a.pl"), vec![]);

        assert_eq!(workflow.check_coverage_threshold(), Some(false));
        assert_eq!(workflow.get_status().coverage, Some(0.0));
        Ok(())
    }

    #[test]
    fn test_refactoring_suggestions() {
        let config = TddConfig::default();
        let mut workflow = TddWorkflow::new(config);

        // Create a subroutine with 8 parameters to trigger TooManyParameters suggestion
        let parameters: Vec<Node> = (0..8)
            .map(|i| {
                Node::new(
                    NodeKind::MandatoryParameter {
                        variable: Box::new(Node::new(
                            NodeKind::Variable {
                                sigil: "$".to_string(),
                                name: format!("param{}", i),
                            },
                            SourceLocation { start: 0, end: 0 },
                        )),
                    },
                    SourceLocation { start: 0, end: 0 },
                )
            })
            .collect();

        let ast = Node::new(
            NodeKind::Subroutine {
                name: Some("complex_function".to_string()),
                name_span: Some(SourceLocation { start: 4, end: 20 }),
                declarator: None,
                signature: Some(Box::new(Node::new(
                    NodeKind::Signature { parameters },
                    SourceLocation { start: 0, end: 0 },
                ))),
                body: Box::new(Node::new(
                    NodeKind::Block { statements: vec![] },
                    SourceLocation { start: 0, end: 0 },
                )),
                attributes: vec![],
                prototype: None,
            },
            SourceLocation { start: 0, end: 0 },
        );

        let suggestions = workflow.get_refactoring_suggestions(&ast, "sub complex_function { }");

        // Should suggest refactoring for too many parameters
        assert!(
            suggestions.iter().any(
                |s| s.category == crate::test_generator::RefactoringCategory::TooManyParameters
            )
        );
    }

    #[test]
    fn test_specific_test_generation() {
        let config = TddConfig::default();
        let workflow = TddWorkflow::new(config);

        let test = workflow.generate_test_for_function(
            "validate_email",
            &["$email".to_string()],
            TestType::EdgeCase,
        );

        assert!(test.code.contains("edge cases"));
        assert!(test.code.contains("undef"));
        assert!(test.code.contains("empty"));
    }

    #[test]
    fn test_edge_case_generation_preserves_signature_arity() {
        let config = TddConfig::default();
        let workflow = TddWorkflow::new(config);

        let test = workflow.generate_test_for_function(
            "validate_contact",
            &["$name".to_string(), "$email".to_string()],
            TestType::EdgeCase,
        );

        assert!(test.code.contains("validate_contact(undef, undef)"));
        assert!(test.code.contains("validate_contact('', '')"));
    }
}
