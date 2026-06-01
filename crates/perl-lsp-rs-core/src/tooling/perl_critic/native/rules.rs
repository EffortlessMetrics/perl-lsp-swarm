//! Built-in native critic rules and rule-local helpers.
//!
//! This module keeps rule implementations separate from the native critic
//! contract, registry, and suppression plumbing so each native critic concern
//! can evolve independently.

use super::{
    CriticCategory, CriticContext, CriticFinding, CriticFix, CriticRelatedInformation, CriticRule,
    CriticTextEdit, FixSafety,
};
use crate::providers::diagnostics::unreachable_code::check_unreachable_code;
use crate::tooling::perl_critic::{Severity, insertion_range};
use perl_parser_core::position::{Position, Range};
use perl_parser_core::{Node, NodeKind};
use perl_pragma::PragmaTracker;
use perl_semantic_analyzer::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};

/// Native rule that requires a file-level `use strict;` pragma.
///
/// This is the first built-in rule expressed through the native critic
/// contract. It deliberately does not replace the existing legacy built-in
/// analyzer yet; callers can opt into it through `NativeCriticRegistry` while
/// runtime diagnostic migration remains incremental.
pub struct RequireUseStrictRule;

impl CriticRule for RequireUseStrictRule {
    fn id(&self) -> &'static str {
        "native.testing.require_use_strict"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        if has_use_statement(ctx.source, "strict") {
            return;
        }

        let range = insertion_range();
        out.push(CriticFinding {
            rule_id: self.id().to_string(),
            category: self.category(),
            severity: self.default_severity(),
            range,
            message: "Code does not use strict".to_string(),
            explanation: "Always use strict to catch common mistakes".to_string(),
            suppression_key: self.id().to_string(),
            related: Vec::new(),
            fix: Some(CriticFix {
                title: "Add 'use strict'".to_string(),
                safety: FixSafety::Safe,
                edits: vec![CriticTextEdit { range, new_text: "use strict;\n".to_string() }],
            }),
        });
    }
}

/// Native rule that requires a file-level `use warnings;` pragma.
///
/// Like [`RequireUseStrictRule`], this is exposed through the native critic
/// contract without replacing the existing legacy built-in analyzer yet.
pub struct RequireUseWarningsRule;

impl CriticRule for RequireUseWarningsRule {
    fn id(&self) -> &'static str {
        "native.testing.require_use_warnings"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        if has_use_statement(ctx.source, "warnings") {
            return;
        }

        let range = insertion_range();
        out.push(CriticFinding {
            rule_id: self.id().to_string(),
            category: self.category(),
            severity: self.default_severity(),
            range,
            message: "Code does not use warnings".to_string(),
            explanation: "Always use warnings to catch potential issues".to_string(),
            suppression_key: self.id().to_string(),
            related: Vec::new(),
            fix: Some(CriticFix {
                title: "Add 'use warnings'".to_string(),
                safety: FixSafety::Safe,
                edits: vec![CriticTextEdit { range, new_text: "use warnings;\n".to_string() }],
            }),
        });
    }
}

/// Native rule that reports assignments used directly as conditions.
///
/// This mirrors the existing common-mistake diagnostic through the native
/// critic contract so native critic users get stable rule IDs,
/// suppressions, severity filtering, and fix metadata.
pub struct AssignmentInConditionRule;

impl CriticRule for AssignmentInConditionRule {
    fn id(&self) -> &'static str {
        "native.common.assignment_in_condition"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_assignment_in_condition_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports static printf/sprintf format arity mismatches.
///
/// This wraps the existing parser-backed PL405 lint through the native critic
/// contract so native critic users get stable rule IDs, suppression and
/// severity filtering, violation bridge coverage, and LSP parity.
pub struct PrintfFormatArityRule;

impl CriticRule for PrintfFormatArityRule {
    fn id(&self) -> &'static str {
        "native.common.printf_format_arity"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_printf_format_arity_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports deprecated `defined @array` / `defined %hash` use.
///
/// This wraps the existing PL500 deprecated-syntax lint through the native
/// critic contract so native critic users get stable rule IDs,
/// suppressions, severity filtering, violation bridge coverage, and quick-fix
/// metadata.
pub struct DeprecatedDefinedRule;

impl CriticRule for DeprecatedDefinedRule {
    fn id(&self) -> &'static str {
        "native.common.deprecated_defined"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_deprecated_defined_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports numeric comparison with explicit `undef`.
///
/// This wraps the parser-confirmed PL404 surface through the native critic
/// contract. It intentionally starts with literal `undef` comparisons; broader
/// maybe-undef semantic inference can be added as a separate rule slice.
pub struct UndefComparisonRule;

impl CriticRule for UndefComparisonRule {
    fn id(&self) -> &'static str {
        "native.common.undef_comparison"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_undef_comparison_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports unlocalized `$@` checks after block `eval`.
///
/// Checking `$@` after `eval` without localizing it can observe a stale or
/// clobbered error value. This rule starts with the parser-confirmed
/// `eval { ... }; if ($@) { ... }` family and leaves broader exception-flow
/// analysis for later semantic rule slices.
pub struct StaleDollarAtRule;

impl CriticRule for StaleDollarAtRule {
    fn id(&self) -> &'static str {
        "native.common.stale_dollar_at"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_stale_dollar_at_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports unreachable statements after unconditional exits.
///
/// This delegates reachability to the existing PL406 analysis so the native
/// critic path and built-in diagnostic path share the same control-flow model.
pub struct UnreachableCodeRule;

impl CriticRule for UnreachableCodeRule {
    fn id(&self) -> &'static str {
        "native.common.unreachable_code"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Maintainability
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_unreachable_code_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports bareword filehandles passed to `open`.
///
/// This mirrors the existing common-mistake and Perl::Critic policy surfaces
/// while giving native critic users a stable rule ID, precise handle
/// span, suppression key, and fix metadata.
pub struct BarewordFilehandleRule;

impl CriticRule for BarewordFilehandleRule {
    fn id(&self) -> &'static str {
        "native.io.bareword_filehandle"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_bareword_filehandle_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports two-argument `open` calls.
///
/// The rule keeps the current native critic migration pattern: stable native
/// policy ID, suppression/config filtering, violation bridge support, and
/// quick-fix metadata while explicit legacy critic behavior remains available.
pub struct TwoArgOpenRule;

impl CriticRule for TwoArgOpenRule {
    fn id(&self) -> &'static str {
        "native.io.two_arg_open"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_two_arg_open_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports pipe-open command execution.
///
/// Pipe-open forms execute shell commands through `open`. The rule mirrors the
/// existing security diagnostic through the native critic contract and remains
/// diagnostic-only because rewriting command execution safely needs user
/// intent.
pub struct PipeOpenRule;

impl CriticRule for PipeOpenRule {
    fn id(&self) -> &'static str {
        "native.io.pipe_open"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_pipe_open_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports statement-level `open` and `close` calls whose
/// return value is ignored.
///
/// The rule intentionally starts narrow: it catches bare `open(...);` and
/// `close(...);` statements and accepts common checked idioms such as
/// `open(...) or die ...`.
pub struct UncheckedOpenCloseRule;

impl CriticRule for UncheckedOpenCloseRule {
    fn id(&self) -> &'static str {
        "native.io.unchecked_open_close"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_unchecked_open_close_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports `qx` and `readpipe` command execution.
///
/// This complements the backtick rule with parser-confirmed quote-command and
/// function-call forms. It stays diagnostic-only because replacing shell
/// command execution safely requires user intent.
pub struct QxReadpipeRule;

impl CriticRule for QxReadpipeRule {
    fn id(&self) -> &'static str {
        "native.security.qx_readpipe"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_qx_readpipe_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports backtick command execution.
///
/// This mirrors the existing security diagnostic through the native critic
/// contract. The rule intentionally stays diagnostic-only because replacing
/// shell command execution safely requires user intent.
pub struct BacktickExecRule;

impl CriticRule for BacktickExecRule {
    fn id(&self) -> &'static str {
        "native.security.backtick_exec"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_backtick_exec_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports string-based `eval`.
///
/// This mirrors the existing security diagnostic and built-in policy through
/// the native critic contract. The rule intentionally does not attach an
/// automatic edit: replacing string eval safely requires user intent.
pub struct StringEvalRule;

impl CriticRule for StringEvalRule {
    fn id(&self) -> &'static str {
        "native.security.string_eval"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_string_eval_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports `system` and `exec` command execution.
///
/// This mirrors the existing security diagnostics through the native critic
/// contract. The rule stays diagnostic-only because replacing process
/// execution safely requires user intent.
pub struct SystemExecRule;

impl CriticRule for SystemExecRule {
    fn id(&self) -> &'static str {
        "native.security.system_exec"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Security
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_system_exec_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Native rule that reports lexical variables declared but never read.
///
/// This rule delegates scope reasoning to the existing semantic analyzer so the
/// native critic path reuses the same declaration/use facts as core diagnostics.
pub struct UnusedLexicalVariableRule;

impl CriticRule for UnusedLexicalVariableRule {
    fn id(&self) -> &'static str {
        "native.variables.unused_lexical"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::UnusedVariable)
                .map(|issue| unused_lexical_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports subroutine parameters declared but never read.
///
/// This rule delegates parameter-use reasoning to the semantic scope analyzer
/// so native critic diagnostics reuse the same signature facts as existing
/// PL108 diagnostics.
pub struct UnusedParameterRule;

impl CriticRule for UnusedParameterRule {
    fn id(&self) -> &'static str {
        "native.variables.unused_parameter"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::UnusedParameter)
                .map(|issue| unused_parameter_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports subroutine parameters repeated in one signature.
///
/// This rule delegates duplicate-parameter detection to the semantic scope
/// analyzer so native critic diagnostics reuse existing signature facts while
/// exposing a stable native policy ID.
pub struct DuplicateParameterRule;

impl CriticRule for DuplicateParameterRule {
    fn id(&self) -> &'static str {
        "native.variables.duplicate_parameter"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Gentle
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::DuplicateParameter)
                .map(|issue| duplicate_parameter_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports subroutine parameters shadowing outer variables.
///
/// This rule delegates parameter shadowing detection to the semantic scope
/// analyzer so native critic diagnostics reuse existing binding facts while
/// exposing a stable native policy ID.
pub struct ParameterShadowsGlobalRule;

impl CriticRule for ParameterShadowsGlobalRule {
    fn id(&self) -> &'static str {
        "native.variables.parameter_shadows_global"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::ParameterShadowsGlobal)
                .map(|issue| parameter_shadows_global_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports lexical variables declared more than once in a scope.
///
/// This rule delegates redeclaration detection to the semantic scope analyzer so
/// native critic diagnostics reuse the same binding facts as existing PL105
/// diagnostics while exposing a stable native policy ID.
pub struct DuplicateLexicalDeclarationRule;

impl CriticRule for DuplicateLexicalDeclarationRule {
    fn id(&self) -> &'static str {
        "native.variables.duplicate_lexical"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Gentle
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::VariableRedeclaration)
                .map(|issue| duplicate_lexical_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports lexical variables that shadow outer declarations.
///
/// This rule delegates shadowing detection to the semantic scope analyzer so
/// native critic diagnostics reuse existing scope facts while exposing a stable
/// native policy ID.
pub struct ShadowedLexicalVariableRule;

impl CriticRule for ShadowedLexicalVariableRule {
    fn id(&self) -> &'static str {
        "native.variables.shadowed_lexical"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::VariableShadowing)
                .map(|issue| shadowed_lexical_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports capture variables used without a preceding regex match.
///
/// This rule delegates capture-variable tracking to the semantic scope analyzer
/// so native critic diagnostics reuse existing control-flow facts while
/// exposing a stable native policy ID.
pub struct CaptureVarWithoutRegexMatchRule;

impl CriticRule for CaptureVarWithoutRegexMatchRule {
    fn id(&self) -> &'static str {
        "native.regex.capture_without_match"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::CaptureVarWithoutRegexMatch)
                .map(|issue| capture_var_without_match_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports variables used without a prior declaration in scope.
///
/// This rule delegates undeclared-variable detection to the semantic scope
/// analyzer so native critic diagnostics reuse the same binding facts as
/// existing strict-mode diagnostics while exposing a stable native policy ID.
pub struct UndeclaredVariableRule;

impl CriticRule for UndeclaredVariableRule {
    fn id(&self) -> &'static str {
        "native.variables.undeclared"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::UndeclaredVariable)
                .map(|issue| undeclared_variable_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports variables read before an initializing assignment.
///
/// This rule delegates uninitialized-variable detection to the semantic scope
/// analyzer so native critic diagnostics reuse existing binding facts while
/// exposing a stable native policy ID.
pub struct UninitializedVariableRule;

impl CriticRule for UninitializedVariableRule {
    fn id(&self) -> &'static str {
        "native.variables.uninitialized"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Semantic
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::UninitializedVariable)
                .map(|issue| uninitialized_variable_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that reports barewords used where strict mode requires clarity.
///
/// This rule delegates bareword detection to the semantic scope analyzer so
/// native critic diagnostics reuse existing strict-mode facts while exposing a
/// stable native policy ID and suggested quoting fix.
pub struct UnquotedBarewordRule;

impl CriticRule for UnquotedBarewordRule {
    fn id(&self) -> &'static str {
        "native.syntax.unquoted_bareword"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let pragma_map = PragmaTracker::build(ctx.ast);
        let issues = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pragma_map);

        out.extend(
            issues
                .into_iter()
                .filter(|issue| issue.kind == IssueKind::UnquotedBareword)
                .filter(|issue| !bareword_is_fat_comma_key(ctx.source, issue))
                .map(|issue| unquoted_bareword_finding(self, ctx.source, &issue)),
        );
    }
}

/// Native rule that checks required sections inside existing POD blocks.
pub struct RequirePodSectionsRule;

impl CriticRule for RequirePodSectionsRule {
    fn id(&self) -> &'static str {
        "native.documentation.require_pod_sections"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Documentation
    }

    fn default_severity(&self) -> Severity {
        Severity::Harsh
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        out.extend(
            missing_pod_sections(ctx.source)
                .into_iter()
                .map(|missing| require_pod_sections_finding(self, ctx.source, &missing)),
        );
    }
}

/// Native rule that flags integer literals with a leading zero.
///
/// In Perl, an integer literal that starts with `0` followed by additional
/// digits is interpreted as **base-8 (octal)**, not decimal. For example
/// `chmod(0755, $file)` sets permission bits to 493 decimal, which is correct
/// for `chmod`, but `$timeout = 0300` silently evaluates to 192 instead of
/// 300, causing a hard-to-spot bug. This rule flags such literals and asks the
/// developer to be explicit about intent.
///
/// Excluded forms:
/// - `0x...` / `0X...` - hexadecimal literals
/// - `0b...` / `0B...` - binary literals
/// - `0.N` / `0e+N` / `0e-N` - floating-point literals
/// - Plain `0` - unambiguous zero
pub struct ProhibitLeadingZerosRule;

impl CriticRule for ProhibitLeadingZerosRule {
    fn id(&self) -> &'static str {
        "native.syntax.prohibit_leading_zeros"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Syntax
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_leading_zeros_findings(self, ctx.source, ctx.ast, out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MissingPodSection {
    name: &'static str,
    range_start: usize,
    range_end: usize,
}

fn unused_lexical_finding(
    rule: &UnusedLexicalVariableRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);
    let unused_name = prefixed_unused_name(&issue.variable_name);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Lexical variable '{}' is declared but never used", issue.variable_name),
        explanation: "Remove the lexical variable, use it, or prefix it with '_' to mark it intentionally unused.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: format!("Rename to '{unused_name}'"),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: unused_name }],
        }),
    }
}

fn unused_parameter_finding(
    rule: &UnusedParameterRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);
    let unused_name = prefixed_unused_name(&issue.variable_name);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Parameter '{}' is never used", issue.variable_name),
        explanation: "Use the parameter or prefix it with '_' to mark it intentionally unused."
            .to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: format!("Rename to '{unused_name}'"),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: unused_name }],
        }),
    }
}

fn duplicate_parameter_finding(
    rule: &DuplicateParameterRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);
    let replacement = numbered_duplicate_name(&issue.variable_name);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!(
            "Parameter '{}' appears more than once in this signature",
            issue.variable_name
        ),
        explanation:
            "Remove the duplicate parameter or rename it so every signature binding is unique."
                .to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: format!("Rename duplicate parameter to '{replacement}'"),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: replacement }],
        }),
    }
}

fn parameter_shadows_global_finding(
    rule: &ParameterShadowsGlobalRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);
    let replacement = parameter_shadow_name(&issue.variable_name);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Parameter '{}' shadows an outer declaration", issue.variable_name),
        explanation: "Rename the parameter or use the outer variable directly to avoid confusing scope shadowing.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: format!("Rename parameter to '{replacement}'"),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: replacement }],
        }),
    }
}

fn assignment_in_condition_finding(
    rule: &AssignmentInConditionRule,
    source: &str,
    condition: &Node,
) -> CriticFinding {
    let range = range_for_byte_span(source, condition.location.start, condition.location.end);
    let fix = assignment_comparison_fix(source, condition.location.start, condition.location.end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "Assignment in condition - did you mean '=='?".to_string(),
        explanation: "Assignments in conditions are usually accidental. Use '==' for numeric comparison, 'eq' for string comparison, or add parentheses if the assignment is intentional.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![
            CriticRelatedInformation {
                range,
                message: "Use '==' for numeric comparison or 'eq' for string comparison.".to_string(),
            },
            CriticRelatedInformation {
                range,
                message: "If the assignment is intentional, wrap it in parentheses.".to_string(),
            },
        ],
        fix,
    }
}

fn printf_format_arity_finding(
    rule: &PrintfFormatArityRule,
    source: &str,
    call_node: &Node,
    format_node: &Node,
    call_name: &str,
    specifier_count: usize,
    arg_count: usize,
) -> CriticFinding {
    let range = range_for_byte_span(source, call_node.location.start, call_node.location.end);
    let format_range =
        range_for_byte_span(source, format_node.location.start, format_node.location.end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!(
            "`{}` format string has {} specifier{} but {} argument{} supplied",
            call_name,
            specifier_count,
            if specifier_count == 1 { "" } else { "s" },
            arg_count,
            if arg_count == 1 { "" } else { "s" },
        ),
        explanation: format!(
            "Add {} argument{} to match {} format specifier{}, or adjust the format string",
            specifier_count,
            if specifier_count == 1 { "" } else { "s" },
            specifier_count,
            if specifier_count == 1 { "" } else { "s" },
        ),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range: format_range,
            message: format!(
                "Format string contains {} specifier{}",
                specifier_count,
                if specifier_count == 1 { "" } else { "s" }
            ),
        }],
        fix: None,
    }
}

fn deprecated_defined_finding(
    rule: &DeprecatedDefinedRule,
    source: &str,
    call_node: &Node,
    arg_node: &Node,
    sigil: &str,
    name: &str,
) -> CriticFinding {
    let range = range_for_byte_span(source, call_node.location.start, call_node.location.end);
    let arg_range = range_for_byte_span(source, arg_node.location.start, arg_node.location.end);
    let variable_text = format!("{sigil}{name}");
    let type_name = if sigil == "@" { "array" } else { "hash" };

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Use of 'defined {variable_text}' is deprecated"),
        explanation: format!(
            "Testing definedness of a whole {type_name} is deprecated because it was rarely useful and often wrong. Use the {type_name} in boolean context instead."
        ),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range: arg_range,
            message: format!("Use 'if ({variable_text})' instead"),
        }],
        fix: Some(CriticFix {
            title: format!("Replace with '{variable_text}'"),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: variable_text }],
        }),
    }
}

fn undef_comparison_finding(
    rule: &UndefComparisonRule,
    source: &str,
    comparison_node: &Node,
    op: &str,
    compared_node: &Node,
    undef_node: &Node,
) -> CriticFinding {
    let range =
        range_for_byte_span(source, comparison_node.location.start, comparison_node.location.end);
    let compared_range =
        range_for_byte_span(source, compared_node.location.start, compared_node.location.end);
    let undef_range =
        range_for_byte_span(source, undef_node.location.start, undef_node.location.end);
    let compared_text =
        source[compared_node.location.start..compared_node.location.end].trim().to_string();
    let replacement = match op {
        "==" => format!("!defined({compared_text})"),
        "!=" => format!("defined({compared_text})"),
        _ => compared_text,
    };

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Using '{op}' with undef -- use defined() to check first"),
        explanation: "Numeric comparison with undef is usually wrong because undef is coerced before comparison. Use defined(...) for definedness checks, or normalize the value before comparing.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![
            CriticRelatedInformation {
                range: undef_range,
                message: "This undef literal should be checked with defined(...) instead.".to_string(),
            },
            CriticRelatedInformation {
                range: compared_range,
                message: "Check this expression with defined(...) before comparing values.".to_string(),
            },
        ],
        fix: Some(CriticFix {
            title: "Use defined() check".to_string(),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: replacement }],
        }),
    }
}

fn stale_dollar_at_finding(
    rule: &StaleDollarAtRule,
    source: &str,
    eval_node: &Node,
    dollar_at_node: &Node,
) -> CriticFinding {
    let range =
        range_for_byte_span(source, dollar_at_node.location.start, dollar_at_node.location.end);
    let eval_range = range_for_byte_span(source, eval_node.location.start, eval_node.location.end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "Checking $@ after eval can observe a stale error".to_string(),
        explanation: "The $@ variable is global and can retain or be clobbered by unrelated exception handling. Localize $@ around eval, or check the eval return value before inspecting the error.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range: eval_range,
            message: "This eval should localize $@ or have its return value checked.".to_string(),
        }],
        fix: None,
    }
}

fn unreachable_code_finding(
    rule: &UnreachableCodeRule,
    source: &str,
    start: usize,
    end: usize,
) -> CriticFinding {
    let range = range_for_byte_span(source, start, end);
    let removal_range = full_line_range_for_byte_span(source, start, end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "Unreachable code: this statement cannot be executed".to_string(),
        explanation: "This statement follows an unconditional control-flow exit such as return, die, exit, last, next, redo, croak, or confess. Remove it or move it before the exit if it is still needed.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: "Remove unreachable code".to_string(),
            safety: FixSafety::Safe,
            edits: vec![CriticTextEdit { range: removal_range, new_text: String::new() }],
        }),
    }
}

fn bareword_filehandle_finding(
    rule: &BarewordFilehandleRule,
    source: &str,
    handle: &Node,
    handle_name: &str,
) -> CriticFinding {
    let range = range_for_byte_span(source, handle.location.start, handle.location.end);
    let lexical_name = bareword_filehandle_lexical_name(handle_name);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Bareword filehandle '{handle_name}' should be lexical"),
        explanation: "Bareword filehandles are package globals and can be accidentally reused across scopes. Use lexical filehandles for safer IO.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: format!(
                "Replace bareword filehandle '{handle_name}' with lexical '{lexical_name}'"
            ),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit {
                range,
                new_text: format!("my {lexical_name}"),
            }],
        }),
    }
}

fn two_arg_open_finding(
    rule: &TwoArgOpenRule,
    source: &str,
    call: &Node,
    open_args: &[Node],
) -> CriticFinding {
    let range = range_for_byte_span(source, call.location.start, call.location.end);
    let fix = two_arg_open_fix_text(source, open_args).map(|new_text| CriticFix {
        title: "Convert to three-argument open() for safety".to_string(),
        safety: FixSafety::Suggested,
        edits: vec![CriticTextEdit { range, new_text }],
    });

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "Two-argument open should use an explicit mode".to_string(),
        explanation: "Two-argument open combines mode and filename, which can allow shell interpretation when the filename is derived from input. Use three-argument open with a separate mode.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix,
    }
}

fn two_arg_open_fix_text(source: &str, open_args: &[Node]) -> Option<String> {
    let [handle, path] = open_args else {
        return None;
    };

    let handle_text = source.get(handle.location.start..handle.location.end)?.trim();
    let path_text = source.get(path.location.start..path.location.end)?.trim();

    if handle_text.is_empty() || path_text.is_empty() {
        return None;
    }

    Some(format!("open({handle_text}, '<', {path_text})"))
}

fn pipe_open_finding(rule: &PipeOpenRule, source: &str, open_node: &Node) -> CriticFinding {
    let range = range_for_byte_span(source, open_node.location.start, open_node.location.end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "Pipe-open executes a shell command".to_string(),
        explanation: "Pipe-open forms run a command through the shell. Prefer explicit command argument lists or IPC modules when command execution is required.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range,
            message: "Validate command arguments and avoid string command execution when input may be user-controlled.".to_string(),
        }],
        fix: None,
    }
}

fn unchecked_open_close_finding(
    rule: &UncheckedOpenCloseRule,
    source: &str,
    call_node: &Node,
    name: &str,
) -> CriticFinding {
    let range = range_for_byte_span(source, call_node.location.start, call_node.location.end);
    let message = match name {
        "close" => "close() return value should be checked",
        _ => "open() return value should be checked",
    };

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: message.to_string(),
        explanation: "open and close report I/O failures through their return value. Check the result with an explicit error path such as `or die` so failures are not silently ignored.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range,
            message: "Unchecked I/O calls can hide missing files, permission failures, and failed flushes.".to_string(),
        }],
        fix: None,
    }
}

fn backtick_exec_finding(
    rule: &BacktickExecRule,
    source: &str,
    string_node: &Node,
) -> CriticFinding {
    let range = range_for_byte_span(source, string_node.location.start, string_node.location.end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "Command execution detected".to_string(),
        explanation: "Backticks execute shell commands. Prefer explicit command argument lists or IPC modules when command execution is required.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range,
            message: "Validate command arguments and avoid shell command execution when input may be user-controlled.".to_string(),
        }],
        fix: None,
    }
}

fn qx_readpipe_finding(rule: &QxReadpipeRule, source: &str, command_node: &Node) -> CriticFinding {
    let range = range_for_byte_span(source, command_node.location.start, command_node.location.end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "qx/readpipe command execution detected".to_string(),
        explanation: "qx and readpipe execute shell commands. Prefer explicit command argument lists or IPC modules when command execution is required.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range,
            message: "Validate command arguments and avoid shell command execution when input may be user-controlled.".to_string(),
        }],
        fix: None,
    }
}

fn string_eval_finding(rule: &StringEvalRule, source: &str, eval_node: &Node) -> CriticFinding {
    let range = range_for_byte_span(source, eval_node.location.start, eval_node.location.end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "String eval is a security risk".to_string(),
        explanation: "String eval executes dynamically generated Perl code and is difficult to analyze safely. Prefer block eval for exception handling or a safer dispatch mechanism.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range,
            message: "String eval executes arbitrary Perl code at runtime when the string contains user-controlled input.".to_string(),
        }],
        fix: None,
    }
}

fn system_exec_finding(
    rule: &SystemExecRule,
    source: &str,
    call_node: &Node,
    name: &str,
) -> CriticFinding {
    let range = range_for_byte_span(source, call_node.location.start, call_node.location.end);
    let message = match name {
        "exec" => "exec() replaces the current process with a shell command",
        _ => "system() executes a shell command",
    };

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: message.to_string(),
        explanation: "system and exec run external commands. Prefer explicit argument lists or IPC modules when command execution is required, and validate any user-controlled input.".to_string(),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range,
            message: "List form avoids shell interpolation, but command execution still needs an explicit security review.".to_string(),
        }],
        fix: None,
    }
}

fn require_pod_sections_finding(
    rule: &RequirePodSectionsRule,
    source: &str,
    missing: &MissingPodSection,
) -> CriticFinding {
    let range = range_for_byte_span(source, missing.range_start, missing.range_end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("POD is missing required =head1 {} section", missing.name),
        explanation: format!(
            "Native critic checks existing POD for required high-level sections. Add an =head1 {} section or suppress this documentation policy for generated or internal-only files.",
            missing.name
        ),
        suppression_key: rule.id().to_string(),
        related: vec![CriticRelatedInformation {
            range,
            message: "This POD block is missing a required documentation section.".to_string(),
        }],
        fix: None,
    }
}

fn duplicate_lexical_finding(
    rule: &DuplicateLexicalDeclarationRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);
    let fix = duplicate_my_fix(source, issue.range.0);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!(
            "Lexical variable '{}' is declared more than once in the same scope",
            issue.variable_name
        ),
        explanation:
            "Remove the duplicate lexical declarator or assign to the existing lexical variable."
                .to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix,
    }
}

fn shadowed_lexical_finding(
    rule: &ShadowedLexicalVariableRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);
    let replacement = shadowed_lexical_name(&issue.variable_name);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Lexical variable '{}' shadows an outer declaration", issue.variable_name),
        explanation: "Rename the inner lexical variable or use the outer variable directly to avoid confusing scope shadowing.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: format!("Rename to '{replacement}'"),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: replacement }],
        }),
    }
}

fn capture_var_without_match_finding(
    rule: &CaptureVarWithoutRegexMatchRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!(
            "Capture variable '{}' used without a preceding regex match in scope",
            issue.variable_name
        ),
        explanation: "Capture variables are set by the most recent successful regex match. Using them without a match in scope may read undef or a stale value.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: None,
    }
}

fn undeclared_variable_finding(
    rule: &UndeclaredVariableRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);
    let declared = format!("my {}", issue.variable_name);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Variable '{}' is used but not declared", issue.variable_name),
        explanation: "Declare the variable with 'my', 'our', or 'local' before use. Under 'use strict' this is a compile-time error.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: format!("Change to '{declared}'"),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: declared }],
        }),
    }
}

fn uninitialized_variable_finding(
    rule: &UninitializedVariableRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Variable '{}' used before initialization", issue.variable_name),
        explanation:
            "Assign a value to the variable before its first use to avoid unintended undef reads."
                .to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: None,
    }
}

fn unquoted_bareword_finding(
    rule: &UnquotedBarewordRule,
    source: &str,
    issue: &ScopeIssue,
) -> CriticFinding {
    let range = range_for_byte_span(source, issue.range.0, issue.range.1);
    let quoted = format!("\"{}\"", issue.variable_name);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!("Bareword '{}' not allowed under strict", issue.variable_name),
        explanation: "Barewords are ambiguous under use strict. Quote the string explicitly, declare a filehandle, or import the symbol.".to_string(),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: Some(CriticFix {
            title: format!("Quote as {quoted}"),
            safety: FixSafety::Suggested,
            edits: vec![CriticTextEdit { range, new_text: quoted }],
        }),
    }
}

fn bareword_is_fat_comma_key(source: &str, issue: &ScopeIssue) -> bool {
    source.get(issue.range.1..).is_some_and(|rest| rest.trim_start().starts_with("=>"))
}

fn collect_assignment_in_condition_findings(
    rule: &AssignmentInConditionRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    match &node.kind {
        NodeKind::If { condition, elsif_branches, .. } => {
            push_assignment_condition_finding(rule, source, condition, out);
            for (elsif_condition, _) in elsif_branches {
                push_assignment_condition_finding(rule, source, elsif_condition, out);
            }
        }
        NodeKind::While { condition, .. } => {
            push_assignment_condition_finding(rule, source, condition, out);
        }
        NodeKind::For { condition: Some(condition), .. } => {
            push_assignment_condition_finding(rule, source, condition, out);
        }
        NodeKind::StatementModifier { modifier, condition, .. }
            if matches!(modifier.as_str(), "if" | "unless" | "while" | "until") =>
        {
            push_assignment_condition_finding(rule, source, condition, out);
        }
        _ => {}
    }

    for child in node.children() {
        collect_assignment_in_condition_findings(rule, source, child, out);
    }
}

fn collect_printf_format_arity_findings(
    rule: &PrintfFormatArityRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    match &node.kind {
        NodeKind::FunctionCall { name, args } if name == "printf" || name == "sprintf" => {
            push_printf_format_arity_finding(rule, source, node, name, args, out);
        }
        NodeKind::IndirectCall { method, args, .. } if method == "printf" => {
            push_printf_format_arity_finding(rule, source, node, method, args, out);
        }
        _ => {}
    }

    for child in node.children() {
        collect_printf_format_arity_findings(rule, source, child, out);
    }
}

fn collect_deprecated_defined_findings(
    rule: &DeprecatedDefinedRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && name == "defined"
    {
        let effective_args = effective_call_args(args);
        if let Some(arg) = effective_args.first()
            && let NodeKind::Variable { sigil, name } = &arg.kind
            && matches!(sigil.as_str(), "@" | "%")
        {
            out.push(deprecated_defined_finding(rule, source, node, arg, sigil, name));
        }
    }

    for child in node.children() {
        collect_deprecated_defined_findings(rule, source, child, out);
    }
}

fn collect_undef_comparison_findings(
    rule: &UndefComparisonRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::Binary { op, left, right } = &node.kind
        && matches!(op.as_str(), "==" | "!=")
    {
        match (&left.kind, &right.kind) {
            (NodeKind::Undef, _) => {
                out.push(undef_comparison_finding(rule, source, node, op, right, left));
            }
            (_, NodeKind::Undef) => {
                out.push(undef_comparison_finding(rule, source, node, op, left, right));
            }
            _ => {}
        }
    }

    for child in node.children() {
        collect_undef_comparison_findings(rule, source, child, out);
    }
}

fn collect_stale_dollar_at_findings(
    rule: &StaleDollarAtRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            collect_stale_dollar_at_in_statements(rule, source, statements, out);
        }
        _ => {}
    }

    for child in node.children() {
        collect_stale_dollar_at_findings(rule, source, child, out);
    }
}

fn collect_unreachable_code_findings(
    rule: &UnreachableCodeRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    let mut diagnostics = Vec::new();
    check_unreachable_code(node, &mut diagnostics);
    out.extend(diagnostics.into_iter().map(|diagnostic| {
        unreachable_code_finding(rule, source, diagnostic.range.0, diagnostic.range.1)
    }));
}

fn collect_stale_dollar_at_in_statements(
    rule: &StaleDollarAtRule,
    source: &str,
    statements: &[Node],
    out: &mut Vec<CriticFinding>,
) {
    for (idx, statement) in statements.iter().enumerate() {
        let Some(eval_node) = eval_statement_node(statement) else {
            continue;
        };
        if idx > 0 && localizes_dollar_at(&statements[idx - 1]) {
            continue;
        }
        let Some(next_statement) = statements.get(idx + 1) else {
            continue;
        };
        if let Some(dollar_at) = condition_dollar_at_read(next_statement) {
            out.push(stale_dollar_at_finding(rule, source, eval_node, dollar_at));
        }
    }
}

fn eval_statement_node(statement: &Node) -> Option<&Node> {
    match &statement.kind {
        NodeKind::ExpressionStatement { expression }
            if matches!(&expression.kind, NodeKind::Eval { .. }) =>
        {
            Some(expression)
        }
        NodeKind::Eval { .. } => Some(statement),
        _ => None,
    }
}

fn localizes_dollar_at(statement: &Node) -> bool {
    match &statement.kind {
        NodeKind::VariableDeclaration { declarator, variable, .. } if declarator == "local" => {
            is_dollar_at_variable(variable)
        }
        NodeKind::ExpressionStatement { expression } => localizes_dollar_at(expression),
        _ => false,
    }
}

fn condition_dollar_at_read(statement: &Node) -> Option<&Node> {
    match &statement.kind {
        NodeKind::If { condition, .. } | NodeKind::While { condition, .. } => {
            first_dollar_at_variable(condition)
        }
        NodeKind::StatementModifier { condition, .. } => first_dollar_at_variable(condition),
        _ => None,
    }
}

fn first_dollar_at_variable(node: &Node) -> Option<&Node> {
    if is_dollar_at_variable(node) {
        return Some(node);
    }

    node.children().into_iter().find_map(first_dollar_at_variable)
}

fn is_dollar_at_variable(node: &Node) -> bool {
    matches!(
        &node.kind,
        NodeKind::Variable { sigil, name } if sigil == "$" && name == "@"
    )
}

fn full_line_range_for_byte_span(source: &str, start: usize, end: usize) -> Range {
    let line_start = source[..start].rfind('\n').map_or(0, |pos| pos + 1);
    let line_end = source[end..].find('\n').map_or(source.len(), |pos| end + pos + 1);
    range_for_byte_span(source, line_start, line_end)
}

fn push_printf_format_arity_finding(
    rule: &PrintfFormatArityRule,
    source: &str,
    call_node: &Node,
    call_name: &str,
    args: &[Node],
    out: &mut Vec<CriticFinding>,
) {
    let Some(format_node) = args.first() else {
        return;
    };
    let NodeKind::String { value, .. } = &format_node.kind else {
        return;
    };

    let format_content = unquote_string(value);
    if format_content.contains('$') {
        return;
    }

    let specifier_count = count_format_specifiers(format_content);
    let arg_count = args.len().saturating_sub(1);
    if specifier_count != arg_count {
        out.push(printf_format_arity_finding(
            rule,
            source,
            call_node,
            format_node,
            call_name,
            specifier_count,
            arg_count,
        ));
    }
}

fn collect_bareword_filehandle_findings(
    rule: &BarewordFilehandleRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::FunctionCall { name, args } = &node.kind {
        push_bareword_filehandle_finding(rule, source, name, args, out);
    }

    for child in node.children() {
        collect_bareword_filehandle_findings(rule, source, child, out);
    }
}

fn push_bareword_filehandle_finding(
    rule: &BarewordFilehandleRule,
    source: &str,
    function_name: &str,
    args: &[Node],
    out: &mut Vec<CriticFinding>,
) {
    if function_name != "open" {
        return;
    }

    let open_args = effective_call_args(args);

    let Some(handle) = open_args.first() else {
        return;
    };
    let NodeKind::Identifier { name } = &handle.kind else {
        return;
    };
    if open_args.len() < 2 || is_standard_filehandle(name) {
        return;
    }

    out.push(bareword_filehandle_finding(rule, source, handle, name));
}

fn collect_two_arg_open_findings(
    rule: &TwoArgOpenRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && name == "open"
    {
        let open_args = effective_call_args(args);
        if open_args.len() == 2 {
            out.push(two_arg_open_finding(rule, source, node, open_args));
        }
    }

    for child in node.children() {
        collect_two_arg_open_findings(rule, source, child, out);
    }
}

fn collect_pipe_open_findings(
    rule: &PipeOpenRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::FunctionCall { name, args } = &node.kind
        && name == "open"
    {
        let open_args = effective_call_args(args);
        if is_pipe_open_args(open_args) {
            out.push(pipe_open_finding(rule, source, node));
        }
    }

    for child in node.children() {
        collect_pipe_open_findings(rule, source, child, out);
    }
}

fn collect_unchecked_open_close_findings(
    rule: &UncheckedOpenCloseRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::ExpressionStatement { expression } = &node.kind {
        push_unchecked_open_close_statement_finding(rule, source, expression, out);
        return;
    }

    for child in node.children() {
        collect_unchecked_open_close_findings(rule, source, child, out);
    }
}

fn push_unchecked_open_close_statement_finding(
    rule: &UncheckedOpenCloseRule,
    source: &str,
    expression: &Node,
    out: &mut Vec<CriticFinding>,
) {
    match &expression.kind {
        NodeKind::FunctionCall { name, .. } if is_open_close_call(name) => {
            if !has_trailing_error_check(source, expression) {
                out.push(unchecked_open_close_finding(rule, source, expression, name));
            }
        }
        _ => {}
    }
}

fn is_open_close_call(name: &str) -> bool {
    matches!(name, "open" | "close")
}

fn has_trailing_error_check(source: &str, call_node: &Node) -> bool {
    let Some(call_text) = source.get(call_node.location.start..call_node.location.end) else {
        return false;
    };
    let Some(close_paren) = call_text.trim_end().rfind(')') else {
        return false;
    };
    let trailing = call_text[close_paren + 1..].trim_start();
    starts_with_error_check_operator(trailing)
}

fn starts_with_error_check_operator(text: &str) -> bool {
    text.strip_prefix("||").is_some()
        || text
            .strip_prefix("or")
            .is_some_and(|rest| rest.chars().next().is_none_or(|ch| !is_identifier_continue(ch)))
}

fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_pipe_open_args(open_args: &[Node]) -> bool {
    match open_args.len() {
        // 3+ arg form: open(my $fh, "|-", "cmd") or open(my $fh, "-|", "cmd")
        n if n >= 3 => open_args.get(1).is_some_and(is_pipe_mode_string),
        // 2-arg form: open(FH, "|cmd")
        2 => open_args.get(1).is_some_and(is_pipe_two_arg_string),
        _ => false,
    }
}

fn is_pipe_mode_string(node: &Node) -> bool {
    match &node.kind {
        NodeKind::String { value, .. } => {
            let trimmed = value.trim_matches(['"', '\'']);
            trimmed == "|-" || trimmed == "-|"
        }
        _ => false,
    }
}

fn is_pipe_two_arg_string(node: &Node) -> bool {
    match &node.kind {
        NodeKind::String { value, .. } => {
            let trimmed = value.trim_matches(['"', '\'']);
            trimmed.starts_with('|') || trimmed.ends_with('|')
        }
        _ => false,
    }
}

fn collect_qx_readpipe_findings(
    rule: &QxReadpipeRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    match &node.kind {
        NodeKind::String { value, interpolated: true } if is_qx_string(value) => {
            out.push(qx_readpipe_finding(rule, source, node));
        }
        NodeKind::FunctionCall { name, .. } if name == "readpipe" => {
            out.push(qx_readpipe_finding(rule, source, node));
        }
        _ => {}
    }

    for child in node.children() {
        collect_qx_readpipe_findings(rule, source, child, out);
    }
}

fn is_qx_string(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("qx") else {
        return false;
    };
    rest.chars().find(|ch| !ch.is_whitespace()).is_some_and(is_quote_like_delimiter)
}

fn is_quote_like_delimiter(delimiter: char) -> bool {
    !delimiter.is_ascii_alphanumeric() && !delimiter.is_whitespace()
}

fn collect_backtick_exec_findings(
    rule: &BacktickExecRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::String { value, interpolated: true } = &node.kind
        && is_backtick_string(value)
    {
        out.push(backtick_exec_finding(rule, source, node));
    }

    for child in node.children() {
        collect_backtick_exec_findings(rule, source, child, out);
    }
}

fn is_backtick_string(value: &str) -> bool {
    value.starts_with('`') && value.ends_with('`') && value.len() >= 2
}

fn collect_string_eval_findings(
    rule: &StringEvalRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    match &node.kind {
        NodeKind::Eval { block } if is_string_eval_expression(block) => {
            out.push(string_eval_finding(rule, source, node));
        }
        NodeKind::FunctionCall { name, args } if name == "eval" => {
            let eval_args = effective_call_args(args);
            if eval_args.first().is_some_and(is_string_eval_expression) {
                out.push(string_eval_finding(rule, source, node));
            }
        }
        _ => {}
    }

    for child in node.children() {
        collect_string_eval_findings(rule, source, child, out);
    }
}

fn collect_system_exec_findings(
    rule: &SystemExecRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::FunctionCall { name, .. } = &node.kind
        && matches!(name.as_str(), "system" | "exec")
    {
        out.push(system_exec_finding(rule, source, node, name));
    }

    for child in node.children() {
        collect_system_exec_findings(rule, source, child, out);
    }
}

fn is_string_eval_expression(node: &Node) -> bool {
    match &node.kind {
        NodeKind::String { .. } | NodeKind::Variable { .. } => true,
        NodeKind::Binary { op, .. } => op == ".",
        _ => false,
    }
}

fn effective_call_args(args: &[Node]) -> &[Node] {
    if args.len() == 1
        && let NodeKind::ArrayLiteral { elements } = &args[0].kind
    {
        return elements;
    }

    args
}

fn is_standard_filehandle(name: &str) -> bool {
    matches!(name, "STDIN" | "STDOUT" | "STDERR" | "ARGV" | "ARGVOUT" | "DATA")
}

fn push_assignment_condition_finding(
    rule: &AssignmentInConditionRule,
    source: &str,
    condition: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if is_assignment_condition(source, condition) {
        out.push(assignment_in_condition_finding(rule, source, condition));
    }
}

fn is_assignment_condition(source: &str, condition: &Node) -> bool {
    let is_assignment = matches!(
        &condition.kind,
        NodeKind::Binary { op, .. } if op == "="
    ) || matches!(&condition.kind, NodeKind::Assignment { .. });

    is_assignment
        && !has_extra_condition_parentheses(
            source,
            condition.location.start,
            condition.location.end,
        )
}

fn has_extra_condition_parentheses(source: &str, start: usize, end: usize) -> bool {
    preceding_open_parens(source, start) >= 2 && following_close_parens(source, end) >= 2
}

fn preceding_open_parens(source: &str, start: usize) -> usize {
    let mut count = 0;
    let mut cursor = start.min(source.len());
    while cursor > 0 {
        let Some((idx, ch)) = source[..cursor].char_indices().next_back() else {
            break;
        };
        if ch.is_whitespace() {
            cursor = idx;
            continue;
        }
        if ch == '(' {
            count += 1;
            cursor = idx;
            continue;
        }
        break;
    }
    count
}

fn following_close_parens(source: &str, end: usize) -> usize {
    let mut count = 0;
    let mut cursor = end.min(source.len());
    while cursor < source.len() {
        let Some(ch) = source[cursor..].chars().next() else {
            break;
        };
        if ch.is_whitespace() {
            cursor += ch.len_utf8();
            continue;
        }
        if ch == ')' {
            count += 1;
            cursor += ch.len_utf8();
            continue;
        }
        break;
    }
    count
}

fn assignment_comparison_fix(source: &str, start: usize, end: usize) -> Option<CriticFix> {
    let start = start.min(source.len());
    let end = end.min(source.len()).max(start);
    let equals_offset = source[start..end].find('=')?;
    let equals_start = start + equals_offset;
    let range = range_for_byte_span(source, equals_start, equals_start + 1);

    Some(CriticFix {
        title: "Change to comparison (==)".to_string(),
        safety: FixSafety::Suggested,
        edits: vec![CriticTextEdit { range, new_text: "==".to_string() }],
    })
}

fn count_format_specifiers(format: &str) -> usize {
    let bytes = format.as_bytes();
    let mut count = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }

        index += 1;
        if index >= bytes.len() {
            break;
        }
        if bytes[index] == b'%' {
            index += 1;
            continue;
        }

        while index < bytes.len() && matches!(bytes[index], b'-' | b'+' | b' ' | b'0' | b'#') {
            index += 1;
        }
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'*' {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index < bytes.len() && bytes[index] == b'*' {
                index += 1;
            }
        }
        if index < bytes.len()
            && matches!(bytes[index], b'h' | b'l' | b'L' | b'q' | b'v' | b'z' | b't')
        {
            index += 1;
            if index < bytes.len() && matches!(bytes[index], b'h' | b'l') {
                index += 1;
            }
        }
        if index < bytes.len()
            && matches!(
                bytes[index],
                b's' | b'd'
                    | b'i'
                    | b'u'
                    | b'o'
                    | b'x'
                    | b'X'
                    | b'e'
                    | b'E'
                    | b'f'
                    | b'F'
                    | b'g'
                    | b'G'
                    | b'c'
                    | b'p'
                    | b'n'
                    | b'b'
            )
        {
            count += 1;
        }
        index += 1;
    }

    count
}

fn unquote_string(raw: &str) -> &str {
    if raw.len() >= 2 {
        let bytes = raw.as_bytes();
        let first = bytes[0];
        let last = bytes[raw.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &raw[1..raw.len() - 1];
        }
    }

    raw
}

fn duplicate_my_fix(source: &str, variable_start: usize) -> Option<CriticFix> {
    let (start, end) = duplicate_my_span(source, variable_start)?;

    Some(CriticFix {
        title: "Remove duplicate 'my' declaration".to_string(),
        safety: FixSafety::Safe,
        edits: vec![CriticTextEdit {
            range: range_for_byte_span(source, start, end),
            new_text: String::new(),
        }],
    })
}

fn duplicate_my_span(source: &str, variable_start: usize) -> Option<(usize, usize)> {
    let variable_start = variable_start.min(source.len());
    let line_start = source[..variable_start].rfind('\n').map_or(0, |pos| pos + 1);
    let before_var = &source[line_start..variable_start];
    let my_offset = before_var.rfind("my ")?;

    if before_var[my_offset + 3..].chars().all(char::is_whitespace) {
        let start = line_start + my_offset;
        Some((start, start + 3))
    } else {
        None
    }
}

fn shadowed_lexical_name(name: &str) -> String {
    let (sigil, base_name) = split_sigil(name);
    format!("{sigil}inner_{base_name}")
}

fn numbered_duplicate_name(name: &str) -> String {
    let (sigil, base_name) = split_sigil(name);
    format!("{sigil}{base_name}_2")
}

fn parameter_shadow_name(name: &str) -> String {
    let (sigil, base_name) = split_sigil(name);
    format!("{sigil}p_{base_name}")
}

fn prefixed_unused_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(sigil @ ('$' | '@' | '%' | '&' | '*')) => {
            let rest = chars.as_str();
            format!("{sigil}_{rest}")
        }
        _ => format!("_{name}"),
    }
}

fn bareword_filehandle_lexical_name(name: &str) -> String {
    format!("${}_fh", name.to_lowercase())
}

fn split_sigil(name: &str) -> (&str, &str) {
    let bare = name.trim_start_matches(['$', '@', '%', '&', '*']);
    let sigil_len = name.len() - bare.len();
    (&name[..sigil_len], bare)
}

fn has_use_statement(content: &str, feature: &str) -> bool {
    content.lines().any(|line| has_use_statement_line(line, feature))
}

fn has_use_statement_line(line: &str, feature: &str) -> bool {
    let code_portion = line.split('#').next().unwrap_or_default();
    let mut tokens = code_portion.split_whitespace();
    let Some(first) = tokens.next() else {
        return false;
    };
    if first != "use" {
        return false;
    }
    let Some(module) = tokens.next() else {
        return false;
    };
    module.trim_end_matches(';') == feature
}

fn missing_pod_sections(source: &str) -> Vec<MissingPodSection> {
    const REQUIRED: &[&str] = &["NAME", "DESCRIPTION"];

    let mut has_pod = false;
    let mut sections = Vec::new();
    let mut first_pod_span = None;
    let mut byte_offset = 0;

    for line in source.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        let trimmed = line_without_newline.trim_start();

        if let Some(section) = trimmed.strip_prefix("=head1") {
            has_pod = true;
            first_pod_span.get_or_insert((byte_offset, byte_offset + line_without_newline.len()));

            let section_name = section
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_end_matches(|ch: char| !ch.is_alphanumeric())
                .to_ascii_uppercase();
            if !section_name.is_empty() {
                sections.push(section_name);
            }
        } else if trimmed.starts_with("=pod")
            || trimmed.starts_with("=over")
            || trimmed.starts_with("=item")
            || trimmed.starts_with("=begin")
        {
            has_pod = true;
            first_pod_span.get_or_insert((byte_offset, byte_offset + line_without_newline.len()));
        }

        byte_offset += line.len();
    }

    if !has_pod {
        return Vec::new();
    }

    let (range_start, range_end) = first_pod_span.unwrap_or((0, source.len().min(1)));

    REQUIRED
        .iter()
        .filter(|required| !sections.iter().any(|section| section == **required))
        .map(|name| MissingPodSection { name, range_start, range_end })
        .collect()
}

fn range_for_byte_span(content: &str, start: usize, end: usize) -> Range {
    let start = start.min(content.len());
    let end = end.min(content.len()).max(start);
    let start_position = position_for_byte_offset(content, start);
    let end_position = position_for_byte_offset(content, end);

    Range { start: start_position, end: end_position }
}

fn position_for_byte_offset(content: &str, offset: usize) -> Position {
    let offset = offset.min(content.len());
    let prefix = &content[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |idx| idx + 1);
    let column = content[line_start..offset].chars().count();

    Position { byte: offset, line: usize_to_u32(line), column: usize_to_u32(column) }
}

fn usize_to_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

fn collect_leading_zeros_findings(
    rule: &ProhibitLeadingZerosRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::Number { value } = &node.kind {
        if is_octal_leading_zero(value) {
            out.push(leading_zeros_finding(rule, source, node, value));
        }
    }

    for child in node.children() {
        collect_leading_zeros_findings(rule, source, child, out);
    }
}

/// Return `true` if `value` is an integer literal with a silent leading-zero
/// octal interpretation.
///
/// Exempted:
/// - `0x...` / `0X...` - explicit hex
/// - `0b...` / `0B...` - explicit binary
/// - `0...` with a decimal point or exponent - decimal float
/// - Plain `"0"` - unambiguous zero
fn is_octal_leading_zero(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'0' {
        return false;
    }
    let second = bytes[1];
    // Skip explicit radix prefixes and float forms
    if matches!(second, b'x' | b'X' | b'b' | b'B' | b'.' | b'e' | b'E') {
        return false;
    }
    // The rest must be a valid octal literal after ignoring numeric
    // separators. Invalid octal digits are left to parser/compiler diagnostics
    // instead of getting a misleading explicit-octal suggestion.
    let mut has_octal_digit = false;
    for byte in &bytes[1..] {
        match byte {
            b'_' => {}
            b'0'..=b'7' => has_octal_digit = true,
            _ => return false,
        }
    }
    if !has_octal_digit {
        return false;
    }

    let normalized = normalized_octal_digits(value);
    match (u64::from_str_radix(&normalized, 8).ok(), normalized.parse::<u64>().ok()) {
        (Some(octal), Some(decimal)) => octal != decimal,
        _ => true,
    }
}

fn leading_zeros_finding(
    rule: &ProhibitLeadingZerosRule,
    source: &str,
    node: &Node,
    value: &str,
) -> CriticFinding {
    let range = range_for_byte_span(source, node.location.start, node.location.end);
    let decimal_value = octal_literal_to_decimal(value);
    let decimal_hint = decimal_value.map(|value| format!(" ({value} decimal)")).unwrap_or_default();
    let evaluated_value = decimal_value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "an octal value".to_string());
    let explicit_octal = normalized_octal_digits(value);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: format!(
            "Integer literal '{value}' has a leading zero and is interpreted as octal{decimal_hint}"
        ),
        explanation: format!(
            "In Perl, integer literals starting with '0' are base-8 (octal). \
             '{value}' evaluates to {evaluated_value}, not decimal {value}. \
             For intentional octal use 0o{explicit_octal} (Perl 5.34+) or spell out \
             the decimal value directly.",
        ),
        suppression_key: rule.id().to_string(),
        related: Vec::new(),
        fix: None,
    }
}

/// Parse a leading-zero octal literal (e.g. `"0755"`) and return its decimal
/// value. Strips Perl numeric-separator underscores before parsing.
fn octal_literal_to_decimal(value: &str) -> Option<u64> {
    let digits = normalized_octal_digits(value);
    u64::from_str_radix(&digits, 8).ok()
}

fn normalized_octal_digits(value: &str) -> String {
    let digits =
        value.chars().skip(1).filter(|&c| c != '_').skip_while(|&c| c == '0').collect::<String>();
    if digits.is_empty() { "0".to_string() } else { digits }
}
