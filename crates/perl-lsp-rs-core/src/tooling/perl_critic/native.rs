//! Native critic rule contract.
//!
//! These types define the Rust-native policy diagnostic surface that future
//! rules should target. They intentionally live beside the existing
//! subprocess-backed Perl::Critic adapter and built-in fallback so callers can
//! migrate rule-by-rule without changing runtime behavior in one large step.

#[cfg(test)]
use super::CriticConfig;
use super::{CriticFindingShape, Severity, insertion_range};
use crate::providers::diagnostics::known_scalar_argument_count;
use crate::providers::diagnostics::unreachable_code::check_unreachable_code;
use perl_parser_core::Node;
use perl_parser_core::NodeKind;
use perl_parser_core::position::{Position, Range};
use perl_parser_core::syntax::source_context::{SourceRegionIndex, SourceRegionKind};
use perl_pragma::PragmaTracker;
use perl_semantic_analyzer::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};

mod native_contract;
mod native_registry;
mod native_suppressions;

pub use native_contract::{
    CriticCategory, CriticContext, CriticFinding, CriticFix, CriticRelatedInformation, CriticRule,
    CriticTextEdit, FixSafety,
};
pub use native_registry::{NativeCriticProfile, NativeCriticRegistry};
/// Proof-only rebuild/reuse instrumentation; see `native_registry`. Compiled
/// for this crate's tests and for downstream crates that opt in with the
/// `test-instrumentation` feature, never in a production build.
#[cfg(any(test, feature = "test-instrumentation"))]
pub use native_registry::{
    native_critic_scope_rebuild_count, native_critic_scope_reuse_count,
    reset_native_critic_scope_rebuild_count, reset_native_critic_scope_reuse_count,
};
pub use native_suppressions::{CriticSuppression, CriticSuppressionMap, CriticSuppressionScope};

/// Resolve scope analysis issues for a rule, using pre-computed results from
/// the context when available (the common path after #4999 item 3), or
/// falling back to a fresh analysis when the context doesn't carry one.
///
/// Returns a slice of issues. When the context carries pre-computed results,
/// this is a zero-allocation borrow; otherwise it builds the pragma map and
/// runs the analyzer fresh.
fn resolve_scope_analysis_owned<'a>(
    ctx: &'a CriticContext<'a>,
) -> std::borrow::Cow<'a, [ScopeIssue]> {
    use std::borrow::Cow;
    if let Some(si) = ctx.scope_issues {
        Cow::Borrowed(si)
    } else {
        let pm = PragmaTracker::build(ctx.ast);
        let si = ScopeAnalyzer::new().analyze(ctx.ast, ctx.source, &pm);
        Cow::Owned(si)
    }
}

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
        if has_use_statement(ctx.source, "strict") || test2_provides_pragma(ctx.source, "strict") {
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
            observed_shape: CriticFindingShape::General,
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
        if has_use_statement(ctx.source, "warnings")
            || test2_provides_pragma(ctx.source, "warnings")
        {
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
            observed_shape: CriticFindingShape::General,
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
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::UnusedVariable)
                .map(|issue| unused_lexical_finding(self, ctx.source, issue)),
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
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::UnusedParameter)
                .map(|issue| unused_parameter_finding(self, ctx.source, issue)),
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
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::DuplicateParameter)
                .map(|issue| duplicate_parameter_finding(self, ctx.source, issue)),
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
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::ParameterShadowsGlobal)
                .map(|issue| parameter_shadows_global_finding(self, ctx.source, issue)),
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
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::VariableRedeclaration)
                .map(|issue| duplicate_lexical_finding(self, ctx.source, issue)),
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
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::VariableShadowing)
                .map(|issue| shadowed_lexical_finding(self, ctx.source, issue)),
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
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::CaptureVarWithoutRegexMatch)
                .map(|issue| capture_var_without_match_finding(self, ctx.source, issue)),
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
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::UndeclaredVariable)
                .map(|issue| undeclared_variable_finding(self, ctx.source, issue)),
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
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::UninitializedVariable)
                .map(|issue| uninitialized_variable_finding(self, ctx.source, issue)),
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
        let issues = resolve_scope_analysis_owned(ctx);

        out.extend(
            issues
                .iter()
                .filter(|issue| issue.kind == IssueKind::UnquotedBareword)
                .filter(|issue| !bareword_is_fat_comma_key(ctx.source, issue))
                .map(|issue| unquoted_bareword_finding(self, ctx.source, issue)),
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

/// Default maximum number of body lines allowed for one named subroutine
/// before [`SubTooLongRule`] reports it.
///
/// No per-rule threshold exists under `[critic.rules.*]` in
/// [`CriticConfig`](super::CriticConfig) today, so this named constant is
/// the single source for the limit until such plumbing exists.
pub const DEFAULT_MAX_SUB_LINES: usize = 50;

/// Native rule that reports named subroutines whose body is too long.
///
/// The body line count runs from the opening-brace line to the
/// closing-brace line inclusive, using the subroutine body node's span.
/// A subroutine whose count is exactly [`DEFAULT_MAX_SUB_LINES`] passes;
/// only a count above it fires, once, at the sub header span.
/// Anonymous subroutines have no name to report against and are ignored.
pub struct SubTooLongRule;

impl CriticRule for SubTooLongRule {
    fn id(&self) -> &'static str {
        "native.maintainability.sub_too_long"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Maintainability
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_sub_too_long_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Default maximum block nesting depth allowed inside one named
/// subroutine before [`TooManyNestedBlocksRule`] reports it.
///
/// No per-rule threshold exists under `[critic.rules.*]` in
/// [`CriticConfig`](super::CriticConfig) today, so this named constant is
/// the single source for the limit until such plumbing exists.
pub const DEFAULT_MAX_NESTED_BLOCKS: usize = 4;

/// Native rule that reports named subroutines whose blocks nest too deep.
///
/// Depth counts control-structure nesting inside the subroutine body:
/// each `if`/`unless`, `while`/`until`, `for`/`foreach`, `try`, `eval`,
/// or `given`/`when` level adds one, so a flat subroutine has depth 0.
/// Bare blocks add nothing: the parser represents a single-expression
/// hash constructor such as `{{1}}` as a `Block` node, so raw block
/// counting cannot distinguish a hash reference from an executable
/// block and would report nested hash references as deep nesting.
/// For the same reason hash and array literals never add depth. A nested
/// subroutine or method starts a fresh measurement: its blocks do not
/// inflate the enclosing subroutine, and a named nested subroutine is
/// measured on its own. A subroutine whose depth is exactly
/// [`DEFAULT_MAX_NESTED_BLOCKS`] passes; only a depth above it fires,
/// once, at the sub header span. Anonymous subroutines have no name to
/// report against and are ignored.
pub struct TooManyNestedBlocksRule;

impl CriticRule for TooManyNestedBlocksRule {
    fn id(&self) -> &'static str {
        "native.maintainability.too_many_nested_blocks"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Maintainability
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_too_many_nested_blocks_findings(self, ctx.source, ctx.ast, out);
    }
}

/// Default maximum number of signature parameters allowed for one named
/// subroutine before [`TooManyParamsRule`] reports it.
///
/// No per-rule threshold exists under `[critic.rules.*]` in
/// [`CriticConfig`](super::CriticConfig) today, so this named constant is
/// the single source for the limit until such plumbing exists.
pub const DEFAULT_MAX_PARAMS: usize = 5;

/// Native rule that reports named subroutines with too many parameters.
///
/// Only subroutines with an explicit signature (`sub name ($a, $b) { }`)
/// are measured: every classified parameter counts once, whether
/// mandatory, optional, named, or slurpy. Malformed signature items,
/// which the parser retains as error nodes rather than classifying,
/// never count. Subroutines that read `@_` directly declare
/// no parameter list, so their arity is not statically known and they are
/// never reported. A subroutine with exactly [`DEFAULT_MAX_PARAMS`]
/// parameters passes; only a count above it fires, once, at the sub
/// header span. Anonymous subroutines have no name to report against and
/// are ignored.
pub struct TooManyParamsRule;

impl CriticRule for TooManyParamsRule {
    fn id(&self) -> &'static str {
        "native.maintainability.too_many_params"
    }

    fn category(&self) -> CriticCategory {
        CriticCategory::Maintainability
    }

    fn default_severity(&self) -> Severity {
        Severity::Stern
    }

    fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
        collect_too_many_params_findings(self, ctx.source, ctx.ast, out);
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
            observed_shape: CriticFindingShape::General,
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
        observed_shape: CriticFindingShape::General,
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
        observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
        observed_shape: CriticFindingShape::General,
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
        observed_shape: CriticFindingShape::General,
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
        observed_shape: CriticFindingShape::LiteralUndefComparison,
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
            observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
        observed_shape: CriticFindingShape::Backtick,
        related: vec![CriticRelatedInformation {
            range,
            message: "Validate command arguments and avoid shell command execution when input may be user-controlled.".to_string(),
        }],
        fix: None,
    }
}

fn qx_readpipe_finding(
    rule: &QxReadpipeRule,
    source: &str,
    command_node: &Node,
    observed_shape: CriticFindingShape,
) -> CriticFinding {
    let range = range_for_byte_span(source, command_node.location.start, command_node.location.end);

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: "qx/readpipe command execution detected".to_string(),
        explanation: "qx and readpipe execute shell commands. Prefer explicit command argument lists or IPC modules when command execution is required.".to_string(),
        suppression_key: rule.id().to_string(),
        observed_shape,
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
            observed_shape: CriticFindingShape::General,
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
    let (message, observed_shape) = match name {
        "exec" => (
            "exec() replaces the current process with a shell command",
            CriticFindingShape::ExecCall,
        ),
        _ => ("system() executes a shell command", CriticFindingShape::SystemCall),
    };

    CriticFinding {
        rule_id: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        range,
        message: message.to_string(),
        explanation: "system and exec run external commands. Prefer explicit argument lists or IPC modules when command execution is required, and validate any user-controlled input.".to_string(),
        suppression_key: rule.id().to_string(),
        observed_shape,
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
        observed_shape: CriticFindingShape::General,
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
        observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
        observed_shape: CriticFindingShape::General,
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
            observed_shape: CriticFindingShape::General,
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
    let Some(arg_count) = known_scalar_argument_count(&args[1..]) else { return };
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
        NodeKind::FunctionCall { name, .. }
            if is_open_close_call(name) && !has_trailing_error_check(source, expression) =>
        {
            out.push(unchecked_open_close_finding(rule, source, expression, name));
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
            out.push(qx_readpipe_finding(rule, source, node, CriticFindingShape::Qx));
        }
        NodeKind::FunctionCall { name, .. } if name == "readpipe" => {
            out.push(qx_readpipe_finding(rule, source, node, CriticFindingShape::Readpipe));
        }
        _ => {}
    }

    for child in node.children() {
        collect_qx_readpipe_findings(rule, source, child, out);
    }
}

pub(crate) fn is_qx_string(value: &str) -> bool {
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

pub(crate) fn is_backtick_string(value: &str) -> bool {
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
            count += 1;
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index < bytes.len() && bytes[index] == b'*' {
                count += 1;
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

/// Whether a Test2 bundle in `content` provides the named pragma
/// (`strict`/`warnings`). `use Test2::V0;` turns both on for the importer
/// unless disabled via `-no_strict` / `-no_warnings` / `-no_pragmas`, so an
/// ordinary Test2 test must not trip the require-use-strict/warnings rules.
fn test2_provides_pragma(content: &str, feature: &str) -> bool {
    crate::providers::testing::test2::Test2Facts::from_source(content).provides_pragma(feature)
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

    let region_index = SourceRegionIndex::build(source);
    let mut has_pod = false;
    let mut sections = Vec::new();
    let mut first_pod_span = None;
    let mut byte_offset = 0;

    for line in source.split_inclusive('\n') {
        // POD-looking lines inside heredoc bodies and __DATA__/__END__
        // payloads are data, not documentation: they must neither satisfy
        // a required section nor mark the file as having POD. This follows
        // the SourceRegionIndex pattern used by native_suppressions.
        let line_start = byte_offset;
        byte_offset += line.len();
        if matches!(
            region_index.kind_at_offset(line_start),
            SourceRegionKind::Heredoc | SourceRegionKind::DataSection
        ) {
            continue;
        }
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        let trimmed = line_without_newline.trim_start();

        if let Some(section) = trimmed.strip_prefix("=head1") {
            has_pod = true;
            first_pod_span.get_or_insert((line_start, line_start + line_without_newline.len()));

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
            first_pod_span.get_or_insert((line_start, line_start + line_without_newline.len()));
        }
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

pub(crate) fn range_for_byte_span(content: &str, start: usize, end: usize) -> Range {
    let start = start.min(content.len());
    let end = end.min(content.len()).max(start);
    let start_position = position_for_byte_offset(content, start);
    let end_position = position_for_byte_offset(content, end);

    Range { start: start_position, end: end_position }
}

fn position_for_byte_offset(content: &str, offset: usize) -> Position {
    // Floor to a char boundary: a caller-supplied span can land
    // mid-codepoint, and slicing there would panic (#18097). The reported
    // `byte` is the floored offset so the position stays self-consistent.
    let mut offset = offset.min(content.len());
    while offset > 0 && !content.is_char_boundary(offset) {
        offset -= 1;
    }
    let prefix = &content[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |idx| idx + 1);
    let column = content[line_start..offset].chars().count();

    Position { byte: offset, line: usize_to_u32(line), column: usize_to_u32(column) }
}

fn usize_to_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

/// Physical lines spanned by `source[start..end]`, counting LF, CRLF, and
/// CR-only line breaks (the shared byte-offset position conversion is
/// LF-based, which would collapse CR-only sources to one line).
fn physical_line_count(source: &str, start: usize, end: usize) -> usize {
    let start = start.min(source.len());
    let end = end.min(source.len()).max(start);
    let bytes = &source.as_bytes()[start..end];
    let mut breaks = 0usize;
    let mut idx = 0usize;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\n' => {
                breaks += 1;
                idx += 1;
            }
            b'\r' => {
                breaks += 1;
                idx += if bytes.get(idx + 1) == Some(&b'\n') { 2 } else { 1 };
            }
            _ => idx += 1,
        }
    }
    breaks + 1
}

fn collect_sub_too_long_findings(
    rule: &SubTooLongRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::Subroutine { name: Some(name), name_span, body, .. } = &node.kind {
        let line_count = physical_line_count(source, body.location.start, body.location.end);
        if line_count > DEFAULT_MAX_SUB_LINES {
            let header_end = name_span.map_or(body.location.start, |span| span.end);
            let range = range_for_byte_span(source, node.location.start, header_end);
            out.push(CriticFinding {
                rule_id: rule.id().to_string(),
                category: rule.category(),
                severity: rule.default_severity(),
                range,
                message: format!(
                    "Subroutine '{name}' is too long ({line_count} lines, maximum is {DEFAULT_MAX_SUB_LINES})"
                ),
                explanation: format!(
                    "Subroutine '{name}' spans {line_count} lines, which exceeds the maximum of {DEFAULT_MAX_SUB_LINES}. Split it into smaller subroutines."
                ),
                suppression_key: rule.id().to_string(),
                observed_shape: CriticFindingShape::General,
                related: Vec::new(),
                fix: None,
            });
        }
    }

    for child in node.children() {
        collect_sub_too_long_findings(rule, source, child, out);
    }
}

fn collect_too_many_nested_blocks_findings(
    rule: &TooManyNestedBlocksRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::Subroutine { name: Some(name), name_span, body, .. } = &node.kind {
        let depth =
            body.children().iter().map(|child| max_block_depth(child, 0)).max().unwrap_or(0);
        if depth > DEFAULT_MAX_NESTED_BLOCKS {
            let header_end = name_span.map_or(body.location.start, |span| span.end);
            let range = range_for_byte_span(source, node.location.start, header_end);
            out.push(CriticFinding {
                rule_id: rule.id().to_string(),
                category: rule.category(),
                severity: rule.default_severity(),
                range,
                message: format!(
                    "Subroutine '{name}' has blocks nested too deeply ({depth} levels, maximum is {DEFAULT_MAX_NESTED_BLOCKS})"
                ),
                explanation: format!(
                    "Subroutine '{name}' nests blocks {depth} levels deep, which exceeds the maximum of {DEFAULT_MAX_NESTED_BLOCKS}. Extract the innermost logic into a helper subroutine."
                ),
                suppression_key: rule.id().to_string(),
                observed_shape: CriticFindingShape::General,
                related: Vec::new(),
                fix: None,
            });
        }
    }

    for child in node.children() {
        collect_too_many_nested_blocks_findings(rule, source, child, out);
    }
}

fn collect_too_many_params_findings(
    rule: &TooManyParamsRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::Subroutine {
        name: Some(name),
        name_span,
        signature: Some(signature),
        body,
        ..
    } = &node.kind
        && let NodeKind::Signature { parameters } = &signature.kind
    {
        let param_count = parameters
            .iter()
            .filter(|parameter| {
                matches!(
                    parameter.kind,
                    NodeKind::MandatoryParameter { .. }
                        | NodeKind::OptionalParameter { .. }
                        | NodeKind::NamedParameter { .. }
                        | NodeKind::SlurpyParameter { .. }
                )
            })
            .count();
        if param_count > DEFAULT_MAX_PARAMS {
            let header_end = name_span.map_or(body.location.start, |span| span.end);
            let range = range_for_byte_span(source, node.location.start, header_end);
            out.push(CriticFinding {
                rule_id: rule.id().to_string(),
                category: rule.category(),
                severity: rule.default_severity(),
                range,
                message: format!(
                    "Subroutine '{name}' has too many parameters ({param_count} parameters, maximum is {DEFAULT_MAX_PARAMS})"
                ),
                explanation: format!(
                    "Subroutine '{name}' declares {param_count} parameters, which exceeds the maximum of {DEFAULT_MAX_PARAMS}. Group related parameters into a hash or object."
                ),
                suppression_key: rule.id().to_string(),
                observed_shape: CriticFindingShape::General,
                related: Vec::new(),
                fix: None,
            });
        }
    }

    for child in node.children() {
        collect_too_many_params_findings(rule, source, child, out);
    }
}

/// Deepest control-structure nesting at or below `node`, where entering
/// an `if`, loop, `try`, `eval`, or `given`/`when` adds one level to
/// `depth`. Bare `Block` nodes add nothing (see [`TooManyNestedBlocksRule`]
/// for why), and a nested subroutine or method is a separate measurement
/// that is never descended into.
fn max_block_depth(node: &Node, depth: usize) -> usize {
    match &node.kind {
        NodeKind::Subroutine { .. } | NodeKind::Method { .. } => depth,
        NodeKind::If { .. }
        | NodeKind::While { .. }
        | NodeKind::For { .. }
        | NodeKind::Foreach { .. }
        | NodeKind::Try { .. }
        | NodeKind::Eval { .. }
        | NodeKind::Given { .. }
        | NodeKind::When { .. } => node
            .children()
            .iter()
            .map(|child| max_block_depth(child, depth + 1))
            .max()
            .unwrap_or(depth + 1),
        _ => {
            node.children().iter().map(|child| max_block_depth(child, depth)).max().unwrap_or(depth)
        }
    }
}

fn collect_leading_zeros_findings(
    rule: &ProhibitLeadingZerosRule,
    source: &str,
    node: &Node,
    out: &mut Vec<CriticFinding>,
) {
    if let NodeKind::Number { value } = &node.kind
        && is_octal_leading_zero(value)
    {
        out.push(leading_zeros_finding(rule, source, node, value));
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
        observed_shape: CriticFindingShape::General,
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

/// Build an empty AST node for tests that only exercise rule contract plumbing.
#[cfg(test)]
fn empty_program_node() -> Node {
    use perl_parser_core::{NodeKind, SourceLocation};

    Node::new(NodeKind::Program { statements: Vec::new() }, SourceLocation { start: 0, end: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_parser_core::position::{Position, Range};
    use perl_tdd_support::{must, must_some};

    /// A mid-codepoint span must floor instead of panicking (#18097).
    #[test]
    fn range_for_byte_span_floors_mid_codepoint_offsets() {
        // "é" is 2 bytes (0..2), "\n" at 2, "x" at 3.
        let content = "é\nx";
        let range = range_for_byte_span(content, 1, 1);
        assert_eq!(range.start.byte, 0, "mid-codepoint start floors to the boundary");
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.column, 0);
        assert_eq!(range.end.byte, 0);

        // Boundary-aligned inputs are unchanged.
        let aligned = range_for_byte_span(content, 3, 4);
        assert_eq!(aligned.start.byte, 3);
        assert_eq!(aligned.start.line, 1);
        assert_eq!(aligned.start.column, 0);
        assert_eq!(aligned.end.byte, 4);
        assert_eq!(aligned.end.column, 1);
    }

    struct DummyRule;

    impl CriticRule for DummyRule {
        fn id(&self) -> &'static str {
            "native.test.dummy"
        }

        fn category(&self) -> CriticCategory {
            CriticCategory::Syntax
        }

        fn default_severity(&self) -> Severity {
            Severity::Harsh
        }

        fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
            if ctx.source.contains("dummy") {
                out.push(CriticFinding {
                    rule_id: self.id().to_string(),
                    category: self.category(),
                    severity: self.default_severity(),
                    range: Range {
                        start: Position { byte: 0, line: 0, column: 0 },
                        end: Position { byte: 5, line: 0, column: 5 },
                    },
                    message: "dummy finding".to_string(),
                    explanation: "dummy explanation".to_string(),
                    suppression_key: self.id().to_string(),
                    observed_shape: CriticFindingShape::General,
                    related: Vec::new(),
                    fix: None,
                });
            }
        }
    }

    struct SecondDummyRule;

    impl CriticRule for SecondDummyRule {
        fn id(&self) -> &'static str {
            "native.test.second"
        }

        fn category(&self) -> CriticCategory {
            CriticCategory::Maintainability
        }

        fn default_severity(&self) -> Severity {
            Severity::Cruel
        }

        fn check(&self, ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
            if ctx.source.contains("second") {
                out.push(CriticFinding {
                    rule_id: self.id().to_string(),
                    category: self.category(),
                    severity: self.default_severity(),
                    range: Range {
                        start: Position { byte: 6, line: 0, column: 6 },
                        end: Position { byte: 12, line: 0, column: 12 },
                    },
                    message: "second finding".to_string(),
                    explanation: "second explanation".to_string(),
                    suppression_key: self.id().to_string(),
                    observed_shape: CriticFindingShape::General,
                    related: Vec::new(),
                    fix: None,
                });
            }
        }
    }

    fn config_with_minimum_severity(severity: u8) -> CriticConfig {
        CriticConfig { severity, ..Default::default() }
    }

    fn parse_source(source: &str) -> Node {
        let mut parser = Parser::new(source);
        must(parser.parse())
    }

    #[test]
    fn native_critic_rule_contract_emits_stable_finding_shape() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let ctx = CriticContext::new("dummy", &ast, &config);
        let mut findings = Vec::new();

        DummyRule.check(&ctx, &mut findings);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.test.dummy");
        assert_eq!(findings[0].suppression_key, "native.test.dummy");
        assert_eq!(findings[0].category, CriticCategory::Syntax);
        assert_eq!(findings[0].severity, Severity::Harsh);
    }

    #[test]
    fn native_critic_finding_serializes_agent_friendly_fields() {
        let finding = CriticFinding {
            rule_id: "native.test.fixable".to_string(),
            category: CriticCategory::Style,
            severity: Severity::Cruel,
            range: Range {
                start: Position { byte: 0, line: 0, column: 0 },
                end: Position { byte: 1, line: 0, column: 1 },
            },
            message: "style issue".to_string(),
            explanation: "style explanation".to_string(),
            suppression_key: "native.test.fixable".to_string(),
            observed_shape: CriticFindingShape::General,
            related: Vec::new(),
            fix: Some(CriticFix {
                title: "Apply style fix".to_string(),
                safety: FixSafety::Safe,
                edits: vec![CriticTextEdit {
                    range: Range {
                        start: Position { byte: 0, line: 0, column: 0 },
                        end: Position { byte: 1, line: 0, column: 1 },
                    },
                    new_text: "x".to_string(),
                }],
            }),
        };

        let value = must(serde_json::to_value(&finding));

        assert_eq!(value["rule_id"], "native.test.fixable");
        assert_eq!(value["category"], "style");
        assert_eq!(value["fix"]["safety"], "safe");
        assert_eq!(value["fix"]["edits"][0]["new_text"], "x");
    }

    #[test]
    fn native_critic_finding_converts_to_legacy_violation_shape() {
        let finding = CriticFinding {
            rule_id: "native.variables.unused_lexical".to_string(),
            category: CriticCategory::Semantic,
            severity: Severity::Stern,
            range: Range {
                start: Position { byte: 10, line: 1, column: 4 },
                end: Position { byte: 12, line: 1, column: 6 },
            },
            message: "unused lexical variable".to_string(),
            explanation: "remove or use the lexical variable".to_string(),
            suppression_key: "native.variables.unused_lexical".to_string(),
            observed_shape: CriticFindingShape::General,
            related: Vec::new(),
            fix: None,
        };

        let violation = finding.to_violation("lib/App.pm");

        assert_eq!(violation.policy, "native.variables.unused_lexical");
        assert_eq!(violation.description, "unused lexical variable");
        assert_eq!(violation.explanation, "remove or use the lexical variable");
        assert_eq!(violation.severity, Severity::Stern);
        assert_eq!(violation.range, finding.range);
        assert_eq!(violation.file, "lib/App.pm");
    }

    #[test]
    fn native_critic_registry_runs_rules_in_order() {
        let ast = empty_program_node();
        let config = config_with_minimum_severity(1);
        let ctx = CriticContext::new("dummy second", &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(DummyRule), Box::new(SecondDummyRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
        assert_eq!(registry.rule_ids(), vec!["native.test.dummy", "native.test.second"]);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].rule_id, "native.test.dummy");
        assert_eq!(findings[1].rule_id, "native.test.second");
    }

    #[test]
    fn native_critic_registry_can_be_extended_incrementally() {
        let ast = empty_program_node();
        let config = config_with_minimum_severity(1);
        let ctx = CriticContext::new("second", &ast, &config);
        let mut registry = NativeCriticRegistry::new();

        assert!(registry.is_empty());
        registry.add_rule(Box::new(SecondDummyRule));

        let findings = registry.check(&ctx);

        assert_eq!(registry.rule_ids(), vec!["native.test.second"]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, CriticCategory::Maintainability);
    }

    #[test]
    fn check_unfiltered_returns_pre_policy_findings_for_normalization() {
        // Severity threshold and suppression are post-merge policy (#7475);
        // the raw path must leave sub-threshold findings in place.
        let ast = empty_program_node();
        let config = config_with_minimum_severity(5);
        let ctx = CriticContext::new("second", &ast, &config);
        let mut registry = NativeCriticRegistry::new();
        registry.add_rule(Box::new(SecondDummyRule));

        assert!(
            registry.check(&ctx).is_empty(),
            "SecondDummyRule (Cruel=2) is below the threshold 5"
        );
        let raw = registry.check_unfiltered(&ctx);
        assert_eq!(raw.len(), 1, "unfiltered path keeps pre-policy findings");
        assert_eq!(raw[0].rule_id, "native.test.second");
    }

    #[test]
    fn native_critic_profiles_keep_recommended_lower_noise_than_strict() {
        let recommended = NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended);
        let strict = NativeCriticRegistry::for_profile(NativeCriticProfile::Strict);
        let recommended_ids = recommended.rule_ids();
        let strict_ids = strict.rule_ids();

        assert!(recommended_ids.contains(&"native.testing.require_use_strict"));
        assert!(recommended_ids.contains(&"native.security.string_eval"));
        assert!(recommended_ids.contains(&"native.io.two_arg_open"));
        assert!(!recommended_ids.contains(&"native.variables.unused_lexical"));
        assert!(!recommended_ids.contains(&"native.syntax.unquoted_bareword"));
        assert!(recommended_ids.len() < strict_ids.len());
        assert!(recommended_ids.iter().all(|id| strict_ids.contains(id)));
        assert!(strict_ids.contains(&"native.documentation.require_pod_sections"));
        assert!(!recommended_ids.contains(&"native.documentation.require_pod_sections"));
        assert_eq!(recommended_ids.len(), NativeCriticRegistry::recommended().len());
    }

    #[test]
    fn native_include_adds_strict_only_rule_to_recommended_profile() {
        // The reported bug: `profile = "recommended"` plus
        // `include = ["native.variables.unused_lexical"]` produced no findings
        // at all, because the rule was never in the profile registry for the
        // include whitelist to select.
        let source = "use strict;\nuse warnings;\nmy $unused = 1;\nprint 1;\n";
        let ast = parse_source(source);
        let config = CriticConfig {
            include: vec!["native.variables.unused_lexical".to_string()],
            ..Default::default()
        };
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::for_profile_with_config(
            NativeCriticProfile::Recommended,
            &config,
        );

        assert!(registry.rule_ids().contains(&"native.variables.unused_lexical"));

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.variables.unused_lexical");
    }

    #[test]
    fn native_include_of_in_profile_rule_does_not_duplicate_it() {
        let config = CriticConfig {
            include: vec!["native.testing.require_use_strict".to_string()],
            ..Default::default()
        };
        let widened = NativeCriticRegistry::for_profile_with_config(
            NativeCriticProfile::Recommended,
            &config,
        );

        assert_eq!(
            widened.rule_ids(),
            NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended).rule_ids()
        );
    }

    #[test]
    fn native_compatibility_include_admits_system_producer() {
        let source = "use strict;\nuse warnings;\nsystem('ls');\n";
        let ast = parse_source(source);
        let config = CriticConfig { include: vec!["PL603".to_string()], ..Default::default() };
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::for_profile_with_config(
            NativeCriticProfile::Recommended,
            &config,
        );

        let findings = registry.check_unfiltered(&ctx);

        assert_eq!(
            findings.iter().map(|finding| finding.rule_id.as_str()).collect::<Vec<_>>(),
            vec!["native.security.system_exec"],
            "PL603 must admit its native producer without running unrelated rules"
        );
    }

    #[test]
    fn native_compatibility_include_admits_shape_specific_backtick_producer() {
        let source = "use strict;\nuse warnings;\nmy $output = `ls`;\n";
        let ast = parse_source(source);
        let config = CriticConfig { include: vec!["PL601".to_string()], ..Default::default() };
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::for_profile_with_config(
            NativeCriticProfile::Recommended,
            &config,
        );

        let findings = registry.check_unfiltered(&ctx);

        assert_eq!(
            findings.iter().map(|finding| finding.rule_id.as_str()).collect::<Vec<_>>(),
            vec!["native.security.backtick_exec"],
            "PL601 must admit only the native producer for the reviewed backtick shape"
        );
    }

    #[test]
    fn native_include_of_unknown_rule_id_leaves_the_profile_roster_alone() {
        // Unknown IDs are reported by config load, not silently resolved to a
        // neighbouring rule here.
        let config = CriticConfig {
            include: vec!["native.variables.unused_lexicals".to_string()],
            ..Default::default()
        };
        let widened = NativeCriticRegistry::for_profile_with_config(
            NativeCriticProfile::Recommended,
            &config,
        );

        assert_eq!(
            widened.rule_ids(),
            NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended).rule_ids()
        );
    }

    #[test]
    fn native_empty_include_keeps_the_plain_profile_roster() {
        for profile in [NativeCriticProfile::Recommended, NativeCriticProfile::Strict] {
            let config = CriticConfig::default();
            assert_eq!(
                NativeCriticRegistry::for_profile_with_config(profile, &config).rule_ids(),
                NativeCriticRegistry::for_profile(profile).rule_ids(),
                "empty include changed the {} roster",
                profile.as_str()
            );
        }
    }

    #[test]
    fn native_exclude_still_wins_over_an_include_widened_rule() {
        let source = "use strict;\nuse warnings;\nmy $unused = 1;\nprint 1;\n";
        let ast = parse_source(source);
        let config = CriticConfig {
            include: vec!["native.variables.unused_lexical".to_string()],
            exclude: vec!["native.variables.unused_lexical".to_string()],
            ..Default::default()
        };
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::for_profile_with_config(
            NativeCriticProfile::Recommended,
            &config,
        );

        assert!(registry.check(&ctx).is_empty());
    }

    #[test]
    fn native_include_still_narrows_execution_within_the_profile() {
        // Widening the registry must not turn `include` into a no-op: with a
        // non-empty include list the other profile rules stay silent.
        let source = "print 1;\n";
        let ast = parse_source(source);
        let config = CriticConfig {
            include: vec!["native.testing.require_use_strict".to_string()],
            ..Default::default()
        };
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::for_profile_with_config(
            NativeCriticProfile::Recommended,
            &config,
        );

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.testing.require_use_strict");
    }

    #[test]
    fn native_critic_profile_parser_accepts_stable_labels() {
        assert_eq!(
            NativeCriticProfile::parse("recommended").map(NativeCriticProfile::as_str),
            Some("recommended")
        );
        assert_eq!(
            NativeCriticProfile::parse("strict").map(NativeCriticProfile::as_str),
            Some("strict")
        );
        assert_eq!(NativeCriticProfile::parse("unknown"), None);
    }

    #[test]
    fn native_require_use_strict_rule_emits_safe_fix_when_missing() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let ctx = CriticContext::new("my $x = 1;\n", &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequireUseStrictRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.testing.require_use_strict");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "Code does not use strict");
        assert_eq!(finding.suppression_key, "native.testing.require_use_strict");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Add 'use strict'");
        assert_eq!(fix.safety, FixSafety::Safe);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, insertion_range());
        assert_eq!(fix.edits[0].new_text, "use strict;\n");
    }

    #[test]
    fn native_require_use_strict_rule_accepts_exact_pragma_only() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let exact_ctx = CriticContext::new("use strict;\nmy $x = 1;\n", &ast, &config);
        let similar_ctx = CriticContext::new("use strictures;\nmy $x = 1;\n", &ast, &config);
        let commented_ctx = CriticContext::new("# use strict;\nmy $x = 1;\n", &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequireUseStrictRule)]);

        assert!(registry.check(&exact_ctx).is_empty());
        assert_eq!(registry.check(&similar_ctx).len(), 1);
        assert_eq!(registry.check(&commented_ctx).len(), 1);
    }

    #[test]
    fn native_require_use_strict_rule_suppressed_by_test2_bundle() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let registry = NativeCriticRegistry::with_rules(vec![
            Box::new(RequireUseStrictRule),
            Box::new(RequireUseWarningsRule),
        ]);

        // Plain `use Test2::V0;` turns strict + warnings on - no findings.
        let v0 = CriticContext::new("use Test2::V0;\nok(1);\ndone_testing;\n", &ast, &config);
        assert!(
            registry.check(&v0).is_empty(),
            "use Test2::V0 should satisfy both strict and warnings rules"
        );

        // `-no_strict` re-enables the strict finding but not warnings.
        let no_strict =
            CriticContext::new("use Test2::V0 -no_strict => 1;\nok(1);\n", &ast, &config);
        let findings = registry.check(&no_strict);
        assert_eq!(findings.len(), 1, "only the strict finding should return");
        assert_eq!(findings[0].rule_id, "native.testing.require_use_strict");

        // `-no_warnings` re-enables only the warnings finding.
        let no_warnings =
            CriticContext::new("use Test2::V0 -no_warnings;\nok(1);\n", &ast, &config);
        let findings = registry.check(&no_warnings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.testing.require_use_warnings");

        // `-no_pragmas` disables both - both findings return.
        let no_pragmas = CriticContext::new("use Test2::V0 -no_pragmas;\nok(1);\n", &ast, &config);
        assert_eq!(registry.check(&no_pragmas).len(), 2);

        // A bare Test2 tool (not a bundle) does not provide strict/warnings.
        let tool_only = CriticContext::new("use Test2::Tools::Basic;\nok(1);\n", &ast, &config);
        assert_eq!(registry.check(&tool_only).len(), 2, "tool imports don't imply pragmas");
    }

    #[test]
    fn native_require_use_warnings_rule_emits_safe_fix_when_missing() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let ctx = CriticContext::new("use strict;\nmy $x = 1;\n", &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequireUseWarningsRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.testing.require_use_warnings");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "Code does not use warnings");
        assert_eq!(finding.suppression_key, "native.testing.require_use_warnings");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Add 'use warnings'");
        assert_eq!(fix.safety, FixSafety::Safe);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, insertion_range());
        assert_eq!(fix.edits[0].new_text, "use warnings;\n");
    }

    #[test]
    fn native_require_use_warnings_rule_accepts_exact_pragma_only() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let exact_ctx = CriticContext::new("use warnings;\nmy $x = 1;\n", &ast, &config);
        let similar_ctx = CriticContext::new("use warningsx;\nmy $x = 1;\n", &ast, &config);
        let commented_ctx = CriticContext::new("# use warnings;\nmy $x = 1;\n", &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequireUseWarningsRule)]);

        assert!(registry.check(&exact_ctx).is_empty());
        assert_eq!(registry.check(&similar_ctx).len(), 1);
        assert_eq!(registry.check(&commented_ctx).len(), 1);
    }

    #[test]
    fn native_strict_and_warnings_rules_run_together_in_order() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let ctx = CriticContext::new("my $x = 1;\n", &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![
            Box::new(RequireUseStrictRule),
            Box::new(RequireUseWarningsRule),
        ]);

        let findings = registry.check(&ctx);

        assert_eq!(
            registry.rule_ids(),
            vec!["native.testing.require_use_strict", "native.testing.require_use_warnings"]
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].rule_id, "native.testing.require_use_strict");
        assert_eq!(findings[1].rule_id, "native.testing.require_use_warnings");
    }

    #[test]
    fn native_assignment_in_condition_rule_reports_if_assignment() {
        let source = "use strict;\nuse warnings;\nmy $x = 0;\nif ($x = 5) { print $x; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.common.assignment_in_condition");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Assignment in condition - did you mean '=='?");
        assert_eq!(finding.suppression_key, "native.common.assignment_in_condition");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$x = 5");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Change to comparison (==)");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(&source[fix.edits[0].range.start.byte..fix.edits[0].range.end.byte], "=");
        assert_eq!(fix.edits[0].new_text, "==");
        assert_eq!(finding.related.len(), 2);
    }

    #[test]
    fn native_assignment_in_condition_rule_reports_statement_modifier_assignment() {
        let source = "use strict;\nuse warnings;\nmy $x = 0;\nprint $x if $x = 5;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.common.assignment_in_condition");
        assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "$x = 5");
    }

    #[test]
    fn native_assignment_in_condition_rule_reports_while_assignment() {
        let source =
            "use strict;\nuse warnings;\nmy $x = 0;\nwhile ($x = next_value()) { print $x; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.common.assignment_in_condition");
        assert_eq!(
            &source[findings[0].range.start.byte..findings[0].range.end.byte],
            "$x = next_value()"
        );
    }

    #[test]
    fn native_assignment_in_condition_rule_accepts_comparisons() {
        let source = "use strict;\nuse warnings;\nmy $x = 0;\nif ($x == 5) { print $x; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "comparison conditions should be accepted");
    }

    #[test]
    fn native_assignment_in_condition_rule_accepts_explicitly_parenthesized_assignments() {
        let source =
            "use strict;\nuse warnings;\nmy $x = 0;\nif (($x = next_value())) { print $x; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        let findings = registry.check(&ctx);

        assert!(
            findings.is_empty(),
            "double-parenthesized assignment conditions are treated as intentional"
        );
    }

    #[test]
    fn native_assignment_in_condition_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $x = 0;\nif ($x = 5) { print $x; }\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.common.assignment_in_condition".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.common.assignment_in_condition -- intentional assignment\nuse strict;\nuse warnings;\nmy $x = 0;\nif ($x = 5) { print $x; }\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_assignment_in_condition_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $x = 0;\nif ($x = 5) { print $x; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.common.assignment_in_condition");
        assert_eq!(violations[0].description, "Assignment in condition - did you mean '=='?");
        assert_eq!(
            violations[0].explanation,
            "Assignments in conditions are usually accidental. Use '==' for numeric comparison, 'eq' for string comparison, or add parentheses if the assignment is intentional."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_printf_format_arity_rule_reports_static_mismatch() {
        let source = "use strict;\nuse warnings;\nprintf \"%s %s\", $name;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.common.printf_format_arity");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(
            finding.message,
            "`printf` format string has 2 specifiers but 1 argument supplied"
        );
        assert_eq!(finding.suppression_key, "native.common.printf_format_arity");
        assert_eq!(
            &source[finding.range.start.byte..finding.range.end.byte],
            "printf \"%s %s\", $name"
        );
        assert_eq!(finding.related.len(), 1);
        assert_eq!(finding.related[0].message, "Format string contains 2 specifiers");
        assert!(finding.fix.is_none());
    }

    #[test]
    fn native_printf_format_arity_rule_accepts_matching_and_dynamic_formats() {
        let source = "use strict;\nuse warnings;\nmy $fmt = \"%s\";\nprintf \"%s\", $name;\nprintf $fmt, $name;\nsprintf \"%s %d\", $name, $count;\nprintf \"%*s\", 10, $name;\nprintf \"%.*f\", 2, $value;\nsprintf \"%*.*s\", 10, 5, $name;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "matching and dynamic formats should be accepted");
    }

    #[test]
    fn native_printf_format_arity_skips_unknown_list_cardinality() {
        let source = "my @values = ('a', 'b'); printf '%s %s', @values; sprintf '%s %s', $prefix, @values; printf '%s %s', qx{printf 'a\\nb\\n'};";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

        assert!(registry.check(&ctx).is_empty());
    }

    #[test]
    fn native_printf_format_arity_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nprintf \"%s %s\", $name;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.common.printf_format_arity".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.common.printf_format_arity -- generated format\nuse strict;\nuse warnings;\nprintf \"%s %s\", $name;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_printf_format_arity_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nsprintf \"%s %s\", $name;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PrintfFormatArityRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.common.printf_format_arity");
        assert_eq!(
            violations[0].description,
            "`sprintf` format string has 2 specifiers but 1 argument supplied"
        );
        assert_eq!(
            violations[0].explanation,
            "Add 2 arguments to match 2 format specifiers, or adjust the format string"
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_deprecated_defined_rule_reports_array_defined() {
        let source = "use strict;\nuse warnings;\nmy @items = (1, 2);\nif (defined @items) { print @items; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.common.deprecated_defined");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Use of 'defined @items' is deprecated");
        assert_eq!(finding.suppression_key, "native.common.deprecated_defined");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "defined @items");
        assert_eq!(finding.related.len(), 1);

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Replace with '@items'");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].new_text, "@items");
    }

    #[test]
    fn native_deprecated_defined_rule_reports_parenthesized_hash_defined() {
        let source = "use strict;\nuse warnings;\nmy %seen = (a => 1);\nif (defined(%seen)) { print keys %seen; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.common.deprecated_defined");
        assert_eq!(findings[0].message, "Use of 'defined %seen' is deprecated");
    }

    #[test]
    fn native_deprecated_defined_rule_accepts_scalar_defined() {
        let source =
            "use strict;\nuse warnings;\nmy $item = 1;\nif (defined $item) { print $item; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "scalar defined checks should be accepted");
    }

    #[test]
    fn native_deprecated_defined_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy @items = (1, 2);\nif (defined @items) { print @items; }\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.common.deprecated_defined".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.common.deprecated_defined -- legacy code\nuse strict;\nuse warnings;\nmy @items = (1, 2);\nif (defined @items) { print @items; }\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_deprecated_defined_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy @items = (1, 2);\ndefined @items;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DeprecatedDefinedRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.common.deprecated_defined");
        assert_eq!(violations[0].description, "Use of 'defined @items' is deprecated");
        assert_eq!(
            violations[0].explanation,
            "Testing definedness of a whole array is deprecated because it was rarely useful and often wrong. Use the array in boolean context instead."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_undef_comparison_rule_reports_numeric_comparison_with_undef() {
        let source = "use strict;\nuse warnings;\nmy $value = maybe();\nif ($value == undef) { print $value; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.common.undef_comparison");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Using '==' with undef -- use defined() to check first");
        assert_eq!(finding.suppression_key, "native.common.undef_comparison");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$value == undef");
        assert_eq!(finding.related.len(), 2);

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Use defined() check");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].new_text, "!defined($value)");
    }

    #[test]
    fn native_undef_comparison_rule_reports_reversed_not_equal_comparison() {
        let source = "use strict;\nuse warnings;\nmy $value = maybe();\nif (undef != $value) { print $value; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.common.undef_comparison");
        assert_eq!(
            &source[findings[0].range.start.byte..findings[0].range.end.byte],
            "undef != $value"
        );
        assert_eq!(must_some(findings[0].fix.as_ref()).edits[0].new_text, "defined($value)");
    }

    #[test]
    fn native_undef_comparison_rule_accepts_defined_checks_and_other_comparisons() {
        let source = "use strict;\nuse warnings;\nmy $value = maybe();\nif (defined $value) { print $value; }\nif ($value == 0) { print $value; }\nif ($value eq undef) { print $value; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "only numeric ==/!= comparisons with undef should report");
    }

    #[test]
    fn native_undef_comparison_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $value = maybe();\nif ($value == undef) { print $value; }\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.common.undef_comparison".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.common.undef_comparison -- legacy check\nuse strict;\nuse warnings;\nmy $value = maybe();\nif ($value == undef) { print $value; }\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_undef_comparison_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $value = maybe();\n$value == undef;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndefComparisonRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.common.undef_comparison");
        assert_eq!(
            violations[0].description,
            "Using '==' with undef -- use defined() to check first"
        );
        assert_eq!(
            violations[0].explanation,
            "Numeric comparison with undef is usually wrong because undef is coerced before comparison. Use defined(...) for definedness checks, or normalize the value before comparing."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_stale_dollar_at_rule_reports_if_check_after_unlocalized_eval() {
        let source = "use strict;\nuse warnings;\neval { risky_call(); };\nif ($@) { warn $@; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.common.stale_dollar_at");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Checking $@ after eval can observe a stale error");
        assert_eq!(finding.suppression_key, "native.common.stale_dollar_at");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$@");
        assert_eq!(finding.related.len(), 1);
        assert!(finding.fix.is_none());
    }

    #[test]
    fn native_stale_dollar_at_rule_reports_statement_modifier_after_unlocalized_eval() {
        let source = "use strict;\nuse warnings;\neval { risky_call(); };\nwarn $@ if $@;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.common.stale_dollar_at");
        assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "$@");
    }

    #[test]
    fn native_stale_dollar_at_rule_accepts_localized_or_return_checked_eval() {
        let source = "use strict;\nuse warnings;\n{\nlocal $@;\neval { risky_call(); };\nif ($@) { warn $@; }\n}\nmy $ok = eval { risky_call(); };\nif (!$ok) { warn 'failed'; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "localized $@ and eval return checks should be accepted");
    }

    #[test]
    fn native_stale_dollar_at_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\neval { risky_call(); };\nif ($@) { warn $@; }\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.common.stale_dollar_at".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.common.stale_dollar_at -- legacy eval\nuse strict;\nuse warnings;\neval { risky_call(); };\nif ($@) { warn $@; }\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_stale_dollar_at_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\neval { risky_call(); };\nif ($@) { warn $@; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StaleDollarAtRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.common.stale_dollar_at");
        assert_eq!(violations[0].description, "Checking $@ after eval can observe a stale error");
        assert_eq!(
            violations[0].explanation,
            "The $@ variable is global and can retain or be clobbered by unrelated exception handling. Localize $@ around eval, or check the eval return value before inspecting the error."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_unreachable_code_rule_reports_dead_statement_after_return() {
        let source = "use strict;\nuse warnings;\nsub f {\nreturn 1;\nmy $dead = 2;\n}\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnreachableCodeRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.common.unreachable_code");
        assert_eq!(finding.category, CriticCategory::Maintainability);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "Unreachable code: this statement cannot be executed");
        assert_eq!(finding.suppression_key, "native.common.unreachable_code");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "my $dead = 2");
        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Remove unreachable code");
        assert_eq!(fix.safety, FixSafety::Safe);
        assert_eq!(
            &source[fix.edits[0].range.start.byte..fix.edits[0].range.end.byte],
            "my $dead = 2;\n"
        );
        assert_eq!(fix.edits[0].new_text, "");
    }

    #[test]
    fn native_unreachable_code_rule_accepts_conditional_return_and_eval_die() {
        let source = "use strict;\nuse warnings;\nsub f {\nreturn if $cond;\nmy $live = 1;\neval { die 'caught'; };\nmy $after_eval = 2;\nreturn $live + $after_eval;\n}\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnreachableCodeRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "conditional return and caught eval die should be accepted");
    }

    #[test]
    fn native_unreachable_code_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nsub f {\nreturn 1;\nmy $dead = 2;\n}\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.common.unreachable_code".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnreachableCodeRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.common.unreachable_code -- generated dead branch\nuse strict;\nuse warnings;\nsub f {\nreturn 1;\nmy $dead = 2;\n}\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_unreachable_code_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nsub f {\nreturn 1;\nmy $dead = 2;\n}\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnreachableCodeRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.common.unreachable_code");
        assert_eq!(
            violations[0].description,
            "Unreachable code: this statement cannot be executed"
        );
        assert_eq!(violations[0].severity, Severity::Harsh);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_bareword_filehandle_rule_reports_open_bareword() {
        let source = "use strict;\nuse warnings;\nopen(FH, '<', 'file.txt');\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(BarewordFilehandleRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.io.bareword_filehandle");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Bareword filehandle 'FH' should be lexical");
        assert_eq!(finding.suppression_key, "native.io.bareword_filehandle");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "FH");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Replace bareword filehandle 'FH' with lexical '$fh_fh'");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, finding.range);
        assert_eq!(fix.edits[0].new_text, "my $fh_fh");
    }

    #[test]
    fn native_bareword_filehandle_rule_accepts_lexical_and_standard_filehandles() {
        let source = "use strict;\nuse warnings;\nopen(my $fh, '<', 'file.txt');\nopen(STDOUT, '>', 'out.txt');\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(BarewordFilehandleRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "lexical and standard filehandles should be accepted");
    }

    #[test]
    fn native_bareword_filehandle_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nopen(FH, '<', 'file.txt');\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.io.bareword_filehandle".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(BarewordFilehandleRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.io.bareword_filehandle -- legacy handle\nuse strict;\nuse warnings;\nopen(FH, '<', 'file.txt');\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_bareword_filehandle_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nopen(FH, '<', 'file.txt');\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(BarewordFilehandleRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.io.bareword_filehandle");
        assert_eq!(violations[0].description, "Bareword filehandle 'FH' should be lexical");
        assert_eq!(
            violations[0].explanation,
            "Bareword filehandles are package globals and can be accidentally reused across scopes. Use lexical filehandles for safer IO."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_two_arg_open_rule_reports_two_arg_open() {
        let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, $path);\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TwoArgOpenRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.io.two_arg_open");
        assert_eq!(finding.category, CriticCategory::Security);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "Two-argument open should use an explicit mode");
        assert_eq!(finding.suppression_key, "native.io.two_arg_open");
        assert_eq!(
            &source[finding.range.start.byte..finding.range.end.byte],
            "open(my $fh, $path)"
        );

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Convert to three-argument open() for safety");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, finding.range);
        assert_eq!(fix.edits[0].new_text, "open(my $fh, '<', $path)");
    }

    #[test]
    fn native_two_arg_open_rule_accepts_three_arg_open() {
        let source =
            "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TwoArgOpenRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "three-argument open should be accepted");
    }

    #[test]
    fn native_two_arg_open_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, $path);\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.io.two_arg_open".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TwoArgOpenRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.io.two_arg_open -- trusted legacy filename\nuse strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, $path);\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());

        let severity_config =
            CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
        let severity_ctx = CriticContext::new(source, &ast, &severity_config);
        assert!(registry.check(&severity_ctx).is_empty());
    }

    #[test]
    fn native_two_arg_open_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, $path);\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TwoArgOpenRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.io.two_arg_open");
        assert_eq!(violations[0].description, "Two-argument open should use an explicit mode");
        assert_eq!(
            violations[0].explanation,
            "Two-argument open combines mode and filename, which can allow shell interpretation when the filename is derived from input. Use three-argument open with a separate mode."
        );
        assert_eq!(violations[0].severity, Severity::Harsh);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_pipe_open_rule_reports_pipe_open_forms() {
        let source = "use strict;\nuse warnings;\nopen(my $read_fh, '-|', 'ls');\nopen(my $write_fh, '|-', 'cat');\nopen(FH, '|cmd');\nopen(my $legacy_read_fh, 'cat |');\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PipeOpenRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 4);
        for finding in &findings {
            assert_eq!(finding.rule_id, "native.io.pipe_open");
            assert_eq!(finding.category, CriticCategory::Security);
            assert_eq!(finding.severity, Severity::Harsh);
            assert_eq!(finding.message, "Pipe-open executes a shell command");
            assert_eq!(finding.suppression_key, "native.io.pipe_open");
            assert!(finding.fix.is_none(), "pipe-open replacement is not safe to automate");
        }
        assert_eq!(
            &source[findings[0].range.start.byte..findings[0].range.end.byte],
            "open(my $read_fh, '-|', 'ls')"
        );
        assert_eq!(
            &source[findings[1].range.start.byte..findings[1].range.end.byte],
            "open(my $write_fh, '|-', 'cat')"
        );
        assert_eq!(
            &source[findings[2].range.start.byte..findings[2].range.end.byte],
            "open(FH, '|cmd')"
        );
        assert_eq!(
            &source[findings[3].range.start.byte..findings[3].range.end.byte],
            "open(my $legacy_read_fh, 'cat |')"
        );
    }

    #[test]
    fn native_pipe_open_rule_accepts_normal_open() {
        let source =
            "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PipeOpenRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "normal three-argument open should be accepted");
    }

    #[test]
    fn native_pipe_open_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nopen(my $fh, '-|', 'ls');\n";
        let ast = parse_source(source);
        let excluded_config =
            CriticConfig { exclude: vec!["native.io.pipe_open".to_string()], ..Default::default() };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PipeOpenRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.io.pipe_open -- trusted command\nuse strict;\nuse warnings;\nopen(my $fh, '-|', 'ls');\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());

        let severity_config =
            CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
        let severity_ctx = CriticContext::new(source, &ast, &severity_config);
        assert!(registry.check(&severity_ctx).is_empty());
    }

    #[test]
    fn native_pipe_open_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nopen(my $fh, '-|', 'ls');\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(PipeOpenRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.io.pipe_open");
        assert_eq!(violations[0].description, "Pipe-open executes a shell command");
        assert_eq!(
            violations[0].explanation,
            "Pipe-open forms run a command through the shell. Prefer explicit command argument lists or IPC modules when command execution is required."
        );
        assert_eq!(violations[0].severity, Severity::Harsh);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_unchecked_open_close_rule_reports_bare_statement_calls() {
        let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\nclose($fh);\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 2);
        for finding in &findings {
            assert_eq!(finding.rule_id, "native.io.unchecked_open_close");
            assert_eq!(finding.category, CriticCategory::Security);
            assert_eq!(finding.severity, Severity::Stern);
            assert_eq!(finding.suppression_key, "native.io.unchecked_open_close");
            assert!(finding.fix.is_none(), "unchecked I/O fix needs caller-specific error text");
        }
        assert_eq!(findings[0].message, "open() return value should be checked");
        assert_eq!(findings[1].message, "close() return value should be checked");
        assert_eq!(
            &source[findings[0].range.start.byte..findings[0].range.end.byte],
            "open(my $fh, '<', $path)"
        );
        assert_eq!(&source[findings[1].range.start.byte..findings[1].range.end.byte], "close($fh)");
    }

    #[test]
    fn native_unchecked_open_close_rule_accepts_or_die_checks() {
        let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path) or die $!;\nclose($fh) || die $!;\nopen(my $compact_fh, '<', $path)||die $!;\nclose($compact_fh)||die $!;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "open/close guarded by error paths should be accepted");
    }

    #[test]
    fn native_unchecked_open_close_rule_reports_argument_level_error_checks() {
        let source = "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path || die 'missing path');\nclose($fh || die 'missing handle');\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].message, "open() return value should be checked");
        assert_eq!(findings[1].message, "close() return value should be checked");
    }

    #[test]
    fn native_unchecked_open_close_rule_composes_with_config_and_suppressions() {
        let source =
            "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.io.unchecked_open_close".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let included_config = CriticConfig {
            include: vec!["native.io.unchecked_open_close".to_string()],
            ..Default::default()
        };
        let included_ctx = CriticContext::new(source, &ast, &included_config);
        assert_eq!(registry.check(&included_ctx).len(), 1);

        let other_include_config = CriticConfig {
            include: vec!["native.security.string_eval".to_string()],
            ..Default::default()
        };
        let other_include_ctx = CriticContext::new(source, &ast, &other_include_config);
        assert!(registry.check(&other_include_ctx).is_empty());

        let config = CriticConfig::default();
        let unsuppressed_ctx = CriticContext::new(source, &ast, &config);
        assert_eq!(registry.check(&unsuppressed_ctx).len(), 1);

        let suppressed_source = "## no critic native.io.unchecked_open_close -- handled by caller\nuse strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
        let suppressed_ast = parse_source(suppressed_source);
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());

        let severity_config =
            CriticConfig { severity: Severity::Gentle as u8, ..Default::default() };
        let severity_ctx = CriticContext::new(source, &ast, &severity_config);
        assert!(registry.check(&severity_ctx).is_empty());
    }

    #[test]
    fn native_unchecked_open_close_rule_flows_through_violation_bridge() {
        let source =
            "use strict;\nuse warnings;\nmy $path = 'file.txt';\nopen(my $fh, '<', $path);\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UncheckedOpenCloseRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.io.unchecked_open_close");
        assert_eq!(violations[0].description, "open() return value should be checked");
        assert_eq!(
            violations[0].explanation,
            "open and close report I/O failures through their return value. Check the result with an explicit error path such as `or die` so failures are not silently ignored."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_qx_readpipe_rule_reports_qx_and_readpipe_forms() {
        let source = "use strict;\nuse warnings;\nmy $date = qx(date);\nmy $listing = qx/ls -la/;\nmy $hash = qx#whoami#;\nmy $pipe_delim = qx|id|;\nmy $tick_delim = qx`uname`;\nmy $pipe = readpipe($date);\nprint $listing . $pipe . $hash . $pipe_delim . $tick_delim;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(QxReadpipeRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 6);
        for finding in &findings {
            assert_eq!(finding.rule_id, "native.security.qx_readpipe");
            assert_eq!(finding.category, CriticCategory::Security);
            assert_eq!(finding.severity, Severity::Harsh);
            assert_eq!(finding.message, "qx/readpipe command execution detected");
            assert_eq!(finding.suppression_key, "native.security.qx_readpipe");
            assert!(finding.fix.is_none(), "qx/readpipe replacement is not safe to automate");
        }
        assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "qx(date)");
        assert_eq!(&source[findings[1].range.start.byte..findings[1].range.end.byte], "qx/ls -la/");
        assert_eq!(&source[findings[2].range.start.byte..findings[2].range.end.byte], "qx#whoami#");
        assert_eq!(&source[findings[3].range.start.byte..findings[3].range.end.byte], "qx|id|");
        assert_eq!(&source[findings[4].range.start.byte..findings[4].range.end.byte], "qx`uname`");
        assert_eq!(
            &source[findings[5].range.start.byte..findings[5].range.end.byte],
            "readpipe($date)"
        );
    }

    #[test]
    fn native_qx_readpipe_rule_accepts_non_command_strings_and_calls() {
        let source = "use strict;\nuse warnings;\nmy $text = 'qx(date)';\nmy $value = read_line($text);\nprint $value;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(QxReadpipeRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "ordinary strings and non-readpipe calls should be accepted");
    }

    #[test]
    fn native_qx_readpipe_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $out = qx(date);\nprint $out;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.security.qx_readpipe".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(QxReadpipeRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.security.qx_readpipe -- trusted command\nuse strict;\nuse warnings;\nmy $out = readpipe('date');\nprint $out;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());

        let severity_config =
            CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
        let severity_ctx = CriticContext::new(source, &ast, &severity_config);
        assert!(registry.check(&severity_ctx).is_empty());
    }

    #[test]
    fn native_qx_readpipe_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $out = qx(date);\nprint $out;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(QxReadpipeRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.security.qx_readpipe");
        assert_eq!(violations[0].description, "qx/readpipe command execution detected");
        assert_eq!(
            violations[0].explanation,
            "qx and readpipe execute shell commands. Prefer explicit command argument lists or IPC modules when command execution is required."
        );
        assert_eq!(violations[0].severity, Severity::Harsh);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_backtick_exec_rule_reports_backtick_strings() {
        let source = "use strict;\nuse warnings;\nmy $out = `ls -la`;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(BacktickExecRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        for finding in &findings {
            assert_eq!(finding.rule_id, "native.security.backtick_exec");
            assert_eq!(finding.category, CriticCategory::Security);
            assert_eq!(finding.severity, Severity::Harsh);
            assert_eq!(finding.message, "Command execution detected");
            assert_eq!(finding.suppression_key, "native.security.backtick_exec");
            assert!(finding.fix.is_none(), "backtick command replacement is not safe to automate");
        }
        assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "`ls -la`");
    }

    #[test]
    fn native_backtick_exec_rule_accepts_normal_strings() {
        let source = "use strict;\nuse warnings;\nmy $text = 'not a command';\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(BacktickExecRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "ordinary strings should be accepted");
    }

    #[test]
    fn native_backtick_exec_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $out = `ls -la`;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.security.backtick_exec".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(BacktickExecRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.security.backtick_exec -- trusted command\nuse strict;\nuse warnings;\nmy $out = `ls -la`;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());

        let severity_config =
            CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
        let severity_ctx = CriticContext::new(source, &ast, &severity_config);
        assert!(registry.check(&severity_ctx).is_empty());
    }

    #[test]
    fn native_backtick_exec_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $out = `ls -la`;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(BacktickExecRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.security.backtick_exec");
        assert_eq!(violations[0].description, "Command execution detected");
        assert_eq!(
            violations[0].explanation,
            "Backticks execute shell commands. Prefer explicit command argument lists or IPC modules when command execution is required."
        );
        assert_eq!(violations[0].severity, Severity::Harsh);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_string_eval_rule_reports_literal_and_variable_eval() {
        let source =
            "use strict;\nuse warnings;\nmy $code = 'print 1';\neval $code;\neval 'print 2';\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 2);
        for finding in &findings {
            assert_eq!(finding.rule_id, "native.security.string_eval");
            assert_eq!(finding.category, CriticCategory::Security);
            assert_eq!(finding.severity, Severity::Harsh);
            assert_eq!(finding.message, "String eval is a security risk");
            assert_eq!(finding.suppression_key, "native.security.string_eval");
            assert!(finding.fix.is_none(), "string eval replacement is not safe to automate");
        }
        assert_eq!(&source[findings[0].range.start.byte..findings[0].range.end.byte], "eval $code");
        assert_eq!(
            &source[findings[1].range.start.byte..findings[1].range.end.byte],
            "eval 'print 2'"
        );
    }

    #[test]
    fn native_string_eval_rule_accepts_block_eval() {
        let source = "use strict;\nuse warnings;\neval { my $x = 1; };\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "block eval should be accepted");
    }

    #[test]
    fn native_string_eval_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $code = 'print 1';\neval $code;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.security.string_eval".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.security.string_eval -- generated DSL\nuse strict;\nuse warnings;\nmy $code = 'print 1';\neval $code;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());

        let severity_config =
            CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
        let severity_ctx = CriticContext::new(source, &ast, &severity_config);
        assert!(registry.check(&severity_ctx).is_empty());
    }

    #[test]
    fn native_string_eval_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $code = 'print 1';\neval $code;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.security.string_eval");
        assert_eq!(violations[0].description, "String eval is a security risk");
        assert_eq!(
            violations[0].explanation,
            "String eval executes dynamically generated Perl code and is difficult to analyze safely. Prefer block eval for exception handling or a safer dispatch mechanism."
        );
        assert_eq!(violations[0].severity, Severity::Harsh);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_system_exec_rule_reports_system_and_exec_forms() {
        let source = "use strict;\nuse warnings;\nmy $cmd = 'ls';\nsystem($cmd);\nsystem('ls', '-la');\nexec($cmd);\nexec('ls', '-la');\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SystemExecRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 4);
        for finding in &findings {
            assert_eq!(finding.rule_id, "native.security.system_exec");
            assert_eq!(finding.category, CriticCategory::Security);
            assert_eq!(finding.severity, Severity::Harsh);
            assert_eq!(finding.suppression_key, "native.security.system_exec");
            assert!(finding.fix.is_none(), "system/exec replacement is not safe to automate");
        }
        assert_eq!(findings[0].message, "system() executes a shell command");
        assert_eq!(findings[1].message, "system() executes a shell command");
        assert_eq!(findings[2].message, "exec() replaces the current process with a shell command");
        assert_eq!(findings[3].message, "exec() replaces the current process with a shell command");
        assert_eq!(
            &source[findings[0].range.start.byte..findings[0].range.end.byte],
            "system($cmd)"
        );
        assert_eq!(
            &source[findings[1].range.start.byte..findings[1].range.end.byte],
            "system('ls', '-la')"
        );
        assert_eq!(&source[findings[2].range.start.byte..findings[2].range.end.byte], "exec($cmd)");
        assert_eq!(
            &source[findings[3].range.start.byte..findings[3].range.end.byte],
            "exec('ls', '-la')"
        );
    }

    #[test]
    fn native_system_exec_rule_accepts_non_command_calls() {
        let source = "use strict;\nuse warnings;\nmy $system = 'name';\nmy $exec_ok = run_exec($system);\nprint $exec_ok;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SystemExecRule)]);

        let findings = registry.check(&ctx);

        assert!(
            findings.is_empty(),
            "ordinary variables and non-system/exec calls should be accepted"
        );
    }

    #[test]
    fn native_system_exec_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $cmd = 'ls';\nsystem($cmd);\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.security.system_exec".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SystemExecRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.security.system_exec -- trusted command\nuse strict;\nuse warnings;\nmy $cmd = 'ls';\nexec($cmd);\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());

        let severity_config =
            CriticConfig { severity: Severity::Stern as u8, ..Default::default() };
        let severity_ctx = CriticContext::new(source, &ast, &severity_config);
        assert!(registry.check(&severity_ctx).is_empty());
    }

    #[test]
    fn native_system_exec_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $cmd = 'ls';\nsystem($cmd);\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SystemExecRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.security.system_exec");
        assert_eq!(violations[0].description, "system() executes a shell command");
        assert_eq!(
            violations[0].explanation,
            "system and exec run external commands. Prefer explicit argument lists or IPC modules when command execution is required, and validate any user-controlled input."
        );
        assert_eq!(violations[0].severity, Severity::Harsh);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_require_pod_sections_rule_reports_incomplete_pod() {
        let source =
            "use strict;\nuse warnings;\n=head1 NAME\n\nApp::Demo - demo module\n\n=cut\n\n1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.documentation.require_pod_sections");
        assert_eq!(finding.category, CriticCategory::Documentation);
        assert_eq!(finding.severity, Severity::Harsh);
        assert_eq!(finding.message, "POD is missing required =head1 DESCRIPTION section");
        assert_eq!(finding.suppression_key, "native.documentation.require_pod_sections");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "=head1 NAME");
        assert!(finding.fix.is_none(), "documentation policy should not fabricate POD content");
    }

    #[test]
    fn native_require_pod_sections_rule_accepts_complete_pod_and_files_without_pod() {
        let complete_source = "use strict;\nuse warnings;\n=head1 NAME\n\nApp::Demo\n\n=head1 DESCRIPTION\n\nDemo.\n\n=cut\n\n1;\n";
        let complete_ast = parse_source(complete_source);
        let config = CriticConfig::default();
        let complete_ctx = CriticContext::new(complete_source, &complete_ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

        assert!(registry.check(&complete_ctx).is_empty());

        let no_pod_source = "use strict;\nuse warnings;\npackage App::Demo;\n1;\n";
        let no_pod_ast = parse_source(no_pod_source);
        let no_pod_ctx = CriticContext::new(no_pod_source, &no_pod_ast, &config);

        assert!(
            registry.check(&no_pod_ctx).is_empty(),
            "native rule only checks existing POD sections to avoid noisy opt-in diagnostics"
        );
    }

    #[test]
    fn native_require_pod_sections_rule_ignores_head1_inside_heredoc() {
        // =head1 lines in a heredoc body are string data, not POD: they must
        // not mark the file as having POD at all.
        let source = "use strict;\nuse warnings;\nmy $doc = <<'END_DOC';\n=head1 NAME\n\nFake - payload\nEND_DOC\nprint $doc;\n1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

        assert!(
            registry.check(&ctx).is_empty(),
            "heredoc payload =head1 lines must not count as POD"
        );
    }

    #[test]
    fn native_require_pod_sections_rule_ignores_head1_inside_data_section() {
        for marker in ["__DATA__", "__END__"] {
            let source = format!(
                "use strict;\nuse warnings;\nprint 1;\n1;\n{marker}\n=head1 NAME\n\nFake - payload\n"
            );
            let ast = parse_source(&source);
            let config = CriticConfig::default();
            let ctx = CriticContext::new(&source, &ast, &config);
            let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

            assert!(
                registry.check(&ctx).is_empty(),
                "{marker} payload =head1 lines must not count as POD"
            );
        }
    }

    #[test]
    fn native_require_pod_sections_rule_payload_does_not_satisfy_missing_section() {
        // Real POD is missing DESCRIPTION; a heredoc =head1 DESCRIPTION is
        // payload and must not mask the real missing-section finding.
        let heredoc_source = "use strict;\nuse warnings;\nmy $doc = <<'END_DOC';\n=head1 DESCRIPTION\n\nPayload.\nEND_DOC\n=head1 NAME\n\nApp::Demo - demo module\n\n=cut\n\n1;\n";
        let ast = parse_source(heredoc_source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(heredoc_source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

        let findings = registry.check(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].message, "POD is missing required =head1 DESCRIPTION section");

        // Real POD is missing NAME; a __DATA__ =head1 NAME is payload and
        // must not mask the real missing-section finding.
        let data_source = "use strict;\nuse warnings;\n=head1 DESCRIPTION\n\nDemo.\n\n=cut\n\n1;\n__DATA__\n=head1 NAME\n\nFake - payload\n";
        let ast = parse_source(data_source);
        let ctx = CriticContext::new(data_source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

        let findings = registry.check(&ctx);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].message, "POD is missing required =head1 NAME section");
    }

    #[test]
    fn native_require_pod_sections_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\n=head1 NAME\n\nApp::Demo\n\n=cut\n\n1;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.documentation.require_pod_sections".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.documentation.require_pod_sections -- generated docs\nuse strict;\nuse warnings;\n=head1 NAME\n\nApp::Demo\n\n=cut\n\n1;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_require_pod_sections_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\n=head1 DESCRIPTION\n\nDemo.\n\n=cut\n\n1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(RequirePodSectionsRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.documentation.require_pod_sections");
        assert_eq!(violations[0].description, "POD is missing required =head1 NAME section");
        assert_eq!(violations[0].severity, Severity::Harsh);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_recommended_registry_contains_initial_policy_bundle() {
        let source = "print 1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended);

        let findings = registry.check(&ctx);

        assert_eq!(
            registry.rule_ids(),
            vec![
                "native.testing.require_use_strict",
                "native.testing.require_use_warnings",
                "native.common.assignment_in_condition",
                "native.common.printf_format_arity",
                "native.common.deprecated_defined",
                "native.common.undef_comparison",
                "native.common.stale_dollar_at",
                "native.common.unreachable_code",
                "native.io.bareword_filehandle",
                "native.io.two_arg_open",
                "native.io.pipe_open",
                "native.io.unchecked_open_close",
                "native.security.qx_readpipe",
                "native.security.backtick_exec",
                "native.security.string_eval",
                "native.security.system_exec"
            ]
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].suppression_key, "native.testing.require_use_strict");
        assert_eq!(findings[1].suppression_key, "native.testing.require_use_warnings");
    }

    #[test]
    fn native_unused_lexical_rule_reports_declared_but_unread_variable() {
        let source = "use strict;\nuse warnings;\nmy $unused = 1;\nprint 1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.variables.unused_lexical");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Lexical variable '$unused' is declared but never used");
        assert_eq!(finding.suppression_key, "native.variables.unused_lexical");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$unused");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Rename to '$_unused'");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, finding.range);
        assert_eq!(fix.edits[0].new_text, "$_unused");
    }

    #[test]
    fn native_unused_lexical_rule_accepts_used_and_intentionally_unused_variables() {
        let source = "use strict;\nuse warnings;\nmy $used = 1;\nmy $_ignored = 2;\nprint $used;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "used and underscore-prefixed variables should be accepted");
    }

    #[test]
    fn native_unused_lexical_rule_reports_multiple_sigils() {
        let source = "use strict;\nuse warnings;\nmy @items = (1, 2);\nmy %seen = ();\nprint 1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

        let findings = registry.check(&ctx);
        let names = findings
            .iter()
            .map(|finding| &source[finding.range.start.byte..finding.range.end.byte])
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["@items", "%seen"]);
        assert_eq!(
            findings
                .iter()
                .map(|finding| must_some(finding.fix.as_ref()).edits[0].new_text.as_str())
                .collect::<Vec<_>>(),
            vec!["@_items", "%_seen"]
        );
    }

    #[test]
    fn native_unused_lexical_rule_composes_with_config_and_suppressions() {
        let ast = parse_source("use strict;\nuse warnings;\nmy $unused = 1;\n");
        let excluded_config = CriticConfig {
            exclude: vec!["native.variables.unused_lexical".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(
            "use strict;\nuse warnings;\nmy $unused = 1;\n",
            &ast,
            &excluded_config,
        );
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.variables.unused_lexical -- legacy fixture\nuse strict;\nuse warnings;\nmy $unused = 1;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_unused_lexical_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $unused = 1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedLexicalVariableRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.variables.unused_lexical");
        assert_eq!(
            violations[0].description,
            "Lexical variable '$unused' is declared but never used"
        );
        assert_eq!(
            violations[0].explanation,
            "Remove the lexical variable, use it, or prefix it with '_' to mark it intentionally unused."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_unused_parameter_rule_reports_unread_signature_parameter() {
        let source = "use strict;\nuse warnings;\nsub helper($used, $unused) { return $used; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedParameterRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.variables.unused_parameter");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Parameter '$unused' is never used");
        assert_eq!(finding.suppression_key, "native.variables.unused_parameter");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$unused");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Rename to '$_unused'");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, finding.range);
        assert_eq!(fix.edits[0].new_text, "$_unused");
    }

    #[test]
    fn native_unused_parameter_rule_accepts_used_and_intentionally_unused_parameters() {
        let source = "use strict;\nuse warnings;\nsub helper($used, $_ignored) { return $used; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedParameterRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "used and underscore-prefixed parameters should be accepted");
    }

    #[test]
    fn native_unused_parameter_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nsub helper($used, $unused) { return $used; }\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.variables.unused_parameter".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedParameterRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.variables.unused_parameter -- fixture\nuse strict;\nuse warnings;\nsub helper($used, $unused) { return $used; }\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_unused_parameter_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nsub helper($used, $unused) { return $used; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnusedParameterRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.variables.unused_parameter");
        assert_eq!(violations[0].description, "Parameter '$unused' is never used");
        assert_eq!(
            violations[0].explanation,
            "Use the parameter or prefix it with '_' to mark it intentionally unused."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_duplicate_parameter_rule_reports_repeated_signature_parameter() {
        let source = "use strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DuplicateParameterRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.variables.duplicate_parameter");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Parameter '$arg' appears more than once in this signature");
        assert_eq!(finding.suppression_key, "native.variables.duplicate_parameter");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$arg");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Rename duplicate parameter to '$arg_2'");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, finding.range);
        assert_eq!(fix.edits[0].new_text, "$arg_2");
    }

    #[test]
    fn native_duplicate_parameter_rule_accepts_unique_parameters() {
        let source =
            "use strict;\nuse warnings;\nsub helper($left, $right) { return $left + $right; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DuplicateParameterRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "unique parameters should be accepted");
    }

    #[test]
    fn native_duplicate_parameter_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.variables.duplicate_parameter".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DuplicateParameterRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no perl-lsp-critic native.variables.duplicate_parameter -- fixture\nuse strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_duplicate_parameter_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nsub helper($arg, $arg) { return $arg; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(DuplicateParameterRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.variables.duplicate_parameter");
        assert_eq!(
            violations[0].description,
            "Parameter '$arg' appears more than once in this signature"
        );
        assert_eq!(
            violations[0].explanation,
            "Remove the duplicate parameter or rename it so every signature binding is unique."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_parameter_shadows_global_rule_reports_parameter_shadowing() {
        let source = "use strict;\nuse warnings;\nmy $name = 'outer';\nsub helper($name) { return $name; }\nprint $name;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ParameterShadowsGlobalRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.variables.parameter_shadows_global");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Parameter '$name' shadows an outer declaration");
        assert_eq!(finding.suppression_key, "native.variables.parameter_shadows_global");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$name");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Rename parameter to '$p_name'");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, finding.range);
        assert_eq!(fix.edits[0].new_text, "$p_name");
    }

    #[test]
    fn native_parameter_shadows_global_rule_accepts_unique_parameters() {
        let source =
            "use strict;\nuse warnings;\nmy $outer = 1;\nsub helper($inner) { return $inner; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ParameterShadowsGlobalRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "non-shadowing parameters should be accepted");
    }

    #[test]
    fn native_parameter_shadows_global_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $name = 'outer';\nsub helper($name) { return $name; }\nprint $name;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.variables.parameter_shadows_global".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ParameterShadowsGlobalRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.variables.parameter_shadows_global -- fixture\nuse strict;\nuse warnings;\nmy $name = 'outer';\nsub helper($name) { return $name; }\nprint $name;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_parameter_shadows_global_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $name = 'outer';\nsub helper($name) { return $name; }\nprint $name;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ParameterShadowsGlobalRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.variables.parameter_shadows_global");
        assert_eq!(violations[0].description, "Parameter '$name' shadows an outer declaration");
        assert_eq!(
            violations[0].explanation,
            "Rename the parameter or use the outer variable directly to avoid confusing scope shadowing."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_duplicate_lexical_rule_reports_same_scope_redeclaration() {
        let source = "use strict;\nuse warnings;\nmy $dup = 1;\nmy $dup = 2;\nprint $dup;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(DuplicateLexicalDeclarationRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.variables.duplicate_lexical");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(
            finding.message,
            "Lexical variable '$dup' is declared more than once in the same scope"
        );
        assert_eq!(finding.suppression_key, "native.variables.duplicate_lexical");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$dup");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Remove duplicate 'my' declaration");
        assert_eq!(fix.safety, FixSafety::Safe);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(&source[fix.edits[0].range.start.byte..fix.edits[0].range.end.byte], "my ");
        assert_eq!(fix.edits[0].new_text, "");
    }

    #[test]
    fn native_duplicate_lexical_rule_accepts_nested_shadowing() {
        let source = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(DuplicateLexicalDeclarationRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "nested lexical shadowing is not same-scope duplication");
    }

    #[test]
    fn native_duplicate_lexical_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $dup = 1;\nmy $dup = 2;\nprint $dup;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.variables.duplicate_lexical".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(DuplicateLexicalDeclarationRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no perl-lsp-critic native.variables.duplicate_lexical -- fixture\nuse strict;\nuse warnings;\nmy $dup = 1;\nmy $dup = 2;\nprint $dup;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_duplicate_lexical_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $dup = 1;\nmy $dup = 2;\nprint $dup;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(DuplicateLexicalDeclarationRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.variables.duplicate_lexical");
        assert_eq!(
            violations[0].description,
            "Lexical variable '$dup' is declared more than once in the same scope"
        );
        assert_eq!(
            violations[0].explanation,
            "Remove the duplicate lexical declarator or assign to the existing lexical variable."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_shadowed_lexical_rule_reports_inner_shadowing() {
        let source = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(ShadowedLexicalVariableRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.variables.shadowed_lexical");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Lexical variable '$value' shadows an outer declaration");
        assert_eq!(finding.suppression_key, "native.variables.shadowed_lexical");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "$value");

        let fix = must_some(finding.fix.as_ref());
        assert_eq!(fix.title, "Rename to '$inner_value'");
        assert_eq!(fix.safety, FixSafety::Suggested);
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, finding.range);
        assert_eq!(fix.edits[0].new_text, "$inner_value");
    }

    #[test]
    fn native_shadowed_lexical_rule_accepts_unique_nested_lexicals() {
        let source = "use strict;\nuse warnings;\nmy $outer = 1;\n{ my $inner = 2; print $inner; }\nprint $outer;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(ShadowedLexicalVariableRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "unique nested lexicals should not be shadowing findings");
    }

    #[test]
    fn native_shadowed_lexical_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.variables.shadowed_lexical".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(ShadowedLexicalVariableRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.variables.shadowed_lexical -- fixture\nuse strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_shadowed_lexical_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $value = 1;\n{ my $value = 2; print $value; }\nprint $value;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(ShadowedLexicalVariableRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.variables.shadowed_lexical");
        assert_eq!(
            violations[0].description,
            "Lexical variable '$value' shadows an outer declaration"
        );
        assert_eq!(
            violations[0].explanation,
            "Rename the inner lexical variable or use the outer variable directly to avoid confusing scope shadowing."
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_capture_var_rule_reports_capture_used_without_match() {
        let source = "use strict;\nuse warnings;\nmy $x = $1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(CaptureVarWithoutRegexMatchRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.regex.capture_without_match");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(
            finding.message,
            "Capture variable '$1' used without a preceding regex match in scope"
        );
        assert_eq!(finding.suppression_key, "native.regex.capture_without_match");
        assert_eq!(finding.fix.as_ref().map(|fix| fix.title.as_str()), None);
    }

    #[test]
    fn native_capture_var_rule_accepts_capture_after_regex_match() {
        let source = "use strict;\nuse warnings;\nif ('hello' =~ /(ell)/) { my $x = $1; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(CaptureVarWithoutRegexMatchRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "capture after a regex match should be accepted");
    }

    #[test]
    fn native_capture_var_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $x = $1;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.regex.capture_without_match".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(CaptureVarWithoutRegexMatchRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.regex.capture_without_match -- fixture\nuse strict;\nuse warnings;\nmy $x = $1;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_capture_var_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $x = $1;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(CaptureVarWithoutRegexMatchRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.regex.capture_without_match");
        assert_eq!(
            violations[0].description,
            "Capture variable '$1' used without a preceding regex match in scope"
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_undeclared_variable_rule_reports_undeclared_use() {
        let source = "use strict;\nuse warnings;\nprint $undeclared;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndeclaredVariableRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.variables.undeclared");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Variable '$undeclared' is used but not declared");
        assert_eq!(finding.suppression_key, "native.variables.undeclared");
        assert_eq!(
            finding.fix.as_ref().map(|fix| (fix.title.as_str(), fix.safety)),
            Some(("Change to 'my $undeclared'", FixSafety::Suggested))
        );
    }

    #[test]
    fn native_undeclared_variable_rule_accepts_declared_variables() {
        let source = "use strict;\nuse warnings;\nmy $declared = 1;\nprint $declared;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndeclaredVariableRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "declared variables should be accepted");
    }

    #[test]
    fn native_undeclared_variable_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nprint $undeclared;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.variables.undeclared".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndeclaredVariableRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.variables.undeclared -- fixture\nuse strict;\nuse warnings;\nprint $undeclared;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_undeclared_variable_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nprint $undeclared;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UndeclaredVariableRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.variables.undeclared");
        assert_eq!(violations[0].description, "Variable '$undeclared' is used but not declared");
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_uninitialized_variable_rule_reports_use_before_init() {
        let source = "use strict;\nuse warnings;\nmy $count;\nprint $count;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UninitializedVariableRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.variables.uninitialized");
        assert_eq!(finding.category, CriticCategory::Semantic);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Variable '$count' used before initialization");
        assert_eq!(finding.suppression_key, "native.variables.uninitialized");
        assert_eq!(finding.fix.as_ref().map(|fix| fix.title.as_str()), None);
    }

    #[test]
    fn native_uninitialized_variable_rule_accepts_initialized_variables() {
        let source = "use strict;\nuse warnings;\nmy $count = 0;\nprint $count;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UninitializedVariableRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "initialized variables should be accepted");
    }

    #[test]
    fn native_uninitialized_variable_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $count;\nprint $count;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.variables.uninitialized".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UninitializedVariableRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.variables.uninitialized -- fixture\nuse strict;\nuse warnings;\nmy $count;\nprint $count;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_uninitialized_variable_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $count;\nprint $count;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UninitializedVariableRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.variables.uninitialized");
        assert_eq!(violations[0].description, "Variable '$count' used before initialization");
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_unquoted_bareword_rule_reports_bareword_under_strict() {
        let source = "use strict;\nuse warnings;\nmy $x = FOO;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.syntax.unquoted_bareword");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.message, "Bareword 'FOO' not allowed under strict");
        assert_eq!(finding.suppression_key, "native.syntax.unquoted_bareword");
        assert_eq!(
            finding.fix.as_ref().map(|fix| (fix.title.as_str(), fix.safety)),
            Some(("Quote as \"FOO\"", FixSafety::Suggested))
        );
    }

    #[test]
    fn native_unquoted_bareword_rule_accepts_quoted_strings() {
        let source = "use strict;\nuse warnings;\nmy $x = \"FOO\";\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "quoted strings should be accepted");
    }

    #[test]
    fn native_unquoted_bareword_rule_accepts_fat_comma_keys() {
        let source = "use strict;\nuse warnings;\nplan tests => 2;\nhas name => (is => 'ro');\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

        let findings = registry.check(&ctx);

        assert!(findings.is_empty(), "fat-comma keys should be accepted as quoted strings");
    }

    #[test]
    fn native_unquoted_bareword_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $x = FOO;\n";
        let ast = parse_source(source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.syntax.unquoted_bareword".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = "## no critic native.syntax.unquoted_bareword -- fixture\nuse strict;\nuse warnings;\nmy $x = FOO;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_unquoted_bareword_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $x = FOO;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(UnquotedBarewordRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.syntax.unquoted_bareword");
        assert_eq!(violations[0].description, "Bareword 'FOO' not allowed under strict");
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_critic_registry_maps_findings_to_legacy_violations() {
        let ast = empty_program_node();
        let config = config_with_minimum_severity(1);
        let ctx = CriticContext::new("dummy second", &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(DummyRule), Box::new(SecondDummyRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].policy, "native.test.dummy");
        assert_eq!(violations[0].description, "dummy finding");
        assert_eq!(violations[0].file, "lib/App.pm");
        assert_eq!(violations[1].policy, "native.test.second");
        assert_eq!(violations[1].description, "second finding");
        assert_eq!(violations[1].file, "lib/App.pm");
    }

    #[test]
    fn native_critic_registry_honors_include_and_exclude_config() {
        let ast = empty_program_node();
        let config = CriticConfig {
            severity: 1,
            include: vec!["native.test.dummy".to_string()],
            exclude: vec!["native.test.second".to_string()],
            ..Default::default()
        };
        let ctx = CriticContext::new("dummy second", &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(DummyRule), Box::new(SecondDummyRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.test.dummy");
    }

    #[test]
    fn native_critic_registry_honors_minimum_severity_config() {
        let ast = empty_program_node();
        let config = CriticConfig { severity: 3, ..Default::default() };
        let ctx = CriticContext::new("dummy second", &ast, &config);
        let registry =
            NativeCriticRegistry::with_rules(vec![Box::new(DummyRule), Box::new(SecondDummyRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.test.dummy");
        assert_eq!(findings[0].severity, Severity::Harsh);
    }

    #[test]
    fn native_critic_suppression_map_parses_directives_and_reasons() {
        let source = "\
## no critic native.testing.require_use_strict -- generated legacy file
## no perl-lsp-critic native.testing.require_use_warnings,native.test.second
my $x = 1;
";

        let suppressions = CriticSuppressionMap::from_source(source);

        assert_eq!(suppressions.suppressions().len(), 3);
        assert_eq!(suppressions.suppressions()[0].rule_id, "native.testing.require_use_strict");
        assert_eq!(suppressions.suppressions()[0].scope, CriticSuppressionScope::ToEndOfFile);
        assert_eq!(suppressions.suppressions()[0].line, 0);
        assert_eq!(suppressions.suppressions()[0].reason.as_deref(), Some("generated legacy file"));
        assert_eq!(suppressions.suppressions()[1].rule_id, "native.testing.require_use_warnings");
        assert_eq!(suppressions.suppressions()[1].line, 1);
        assert_eq!(suppressions.suppressions()[2].rule_id, "native.test.second");
        assert_eq!(suppressions.suppressions()[2].line, 1);
    }

    /// Production route (#6968): the registry filter must honor the directive's
    /// position, not merely its presence anywhere in the file.
    #[test]
    fn native_critic_registry_scopes_suppression_to_the_directive_region() {
        let source = "\
use strict;
use warnings;
my $x = 0;
if ($x = 1) { print $x; }
## no critic native.common.assignment_in_condition
if ($x = 2) { print $x; }
## use critic
if ($x = 3) { print $x; }
";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        let findings = registry.check(&ctx);
        let lines: Vec<u32> = findings.iter().map(|finding| finding.range.start.line).collect();

        // The finding above the directive survives, the one inside the region is
        // suppressed, and the one after `## use critic` is reported again.
        assert_eq!(findings.len(), 2);
        assert_eq!(lines, vec![3, 7]);
    }

    /// Negative control for the repaired contract: a directive placed below a
    /// finding must not reach back and suppress it.
    #[test]
    fn native_critic_registry_does_not_suppress_findings_above_the_directive() {
        let source = "\
use strict;
use warnings;
my $x = 0;
if ($x = 1) { print $x; }
## no critic native.common.assignment_in_condition
";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(AssignmentInConditionRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].range.start.line, 3);
    }

    #[test]
    fn native_critic_registry_filters_suppressed_findings() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let ctx = CriticContext::new(
            "## no critic native.testing.require_use_strict -- legacy file\nmy $x = 1;\n",
            &ast,
            &config,
        );
        let registry = NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "native.testing.require_use_warnings");
    }

    #[test]
    fn native_critic_registry_filters_suppressed_violations() {
        let ast = empty_program_node();
        let config = CriticConfig::default();
        let ctx = CriticContext::new(
            "## no perl-lsp-critic native.testing.require_use_warnings\nmy $x = 1;\n",
            &ast,
            &config,
        );
        let registry = NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.testing.require_use_strict");
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    #[test]
    fn native_prohibit_leading_zeros_rule_reports_octal_literal() {
        let source = "use strict;\nuse warnings;\nchmod(0755, $file);\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.syntax.prohibit_leading_zeros");
        assert_eq!(finding.category, CriticCategory::Syntax);
        assert_eq!(finding.severity, Severity::Stern);
        assert!(
            finding.message.contains("0755"),
            "message should mention the literal: {}",
            finding.message
        );
        assert!(
            finding.message.contains("octal"),
            "message should mention octal: {}",
            finding.message
        );
        assert!(
            finding.message.contains("493"),
            "message should include decimal value 493: {}",
            finding.message
        );
        assert!(
            finding.explanation.contains("evaluates to 493, not decimal 0755"),
            "explanation should compare evaluated and written values: {}",
            finding.explanation
        );
        assert!(
            finding.explanation.contains("0o755"),
            "explanation should suggest explicit octal spelling: {}",
            finding.explanation
        );
        assert_eq!(finding.suppression_key, "native.syntax.prohibit_leading_zeros");
        assert_eq!(&source[finding.range.start.byte..finding.range.end.byte], "0755");
        assert!(finding.fix.is_none(), "leading-zeros is diagnostic-only (no auto-fix)");
    }

    #[test]
    fn native_prohibit_leading_zeros_rule_normalizes_underscored_octal_hint() {
        let source = "use strict;\nuse warnings;\nmy $mode = 0_755;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert!(
            finding.message.contains("493"),
            "message should include decimal value 493: {}",
            finding.message
        );
        assert!(
            finding.explanation.contains("0o755"),
            "explicit octal suggestion should not preserve separator after the prefix: {}",
            finding.explanation
        );
    }

    #[test]
    fn native_prohibit_leading_zeros_rule_accepts_safe_numeric_forms() {
        // Hex, binary, float, plain zero, and invalid-octal-looking forms are
        // not silent-octal cases for this rule.
        let source = "use strict;\nuse warnings;\nmy $h = 0xFF;\nmy $b = 0b1010;\nmy $f = 0.5;\nmy $g = 00.5;\nmy $z = 0;\nmy $zero = 00;\nmy $small = 0007;\nmy $bad = 018;\nmy $also_bad = 0_8;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

        let findings = registry.check(&ctx);

        assert!(
            findings.is_empty(),
            "hex/binary/float/zero forms should not be flagged, got: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn native_prohibit_leading_zeros_rule_composes_with_config_and_suppressions() {
        let source = "use strict;\nuse warnings;\nmy $mode = 0644;\n";
        let ast = parse_source(source);

        let excluded_config = CriticConfig {
            exclude: vec!["native.syntax.prohibit_leading_zeros".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

        assert!(
            registry.check(&excluded_ctx).is_empty(),
            "excluded rule should produce no findings"
        );

        let suppressed_source = "## no critic native.syntax.prohibit_leading_zeros -- intentional octal\nuse strict;\nuse warnings;\nmy $mode = 0644;\n";
        let suppressed_ast = parse_source(suppressed_source);
        let default_config = CriticConfig::default();
        let suppressed_ctx =
            CriticContext::new(suppressed_source, &suppressed_ast, &default_config);

        assert!(
            registry.check(&suppressed_ctx).is_empty(),
            "suppressed rule should produce no findings"
        );
    }

    #[test]
    fn native_prohibit_leading_zeros_rule_flows_through_violation_bridge() {
        let source = "use strict;\nuse warnings;\nmy $timeout = 0300;\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(ProhibitLeadingZerosRule)]);

        let violations = registry.check_violations(&ctx, "lib/App.pm");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].policy, "native.syntax.prohibit_leading_zeros");
        assert!(
            violations[0].description.contains("0300"),
            "description should mention the literal"
        );
        assert_eq!(violations[0].severity, Severity::Stern);
        assert_eq!(violations[0].file, "lib/App.pm");
    }

    fn long_sub_source(name: &str, interior_lines: usize) -> String {
        let mut source = format!("use strict;\nuse warnings;\nsub {name} {{\n");
        for _ in 0..interior_lines {
            source.push_str("    print 1;\n");
        }
        source.push_str("}\n");
        source
    }

    #[test]
    fn native_sub_too_long_rule_reports_sub_over_the_limit() {
        // 1 opening line + 49 interior lines + 1 closing line = 51 body lines.
        let source = long_sub_source("too_long", DEFAULT_MAX_SUB_LINES - 1);
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SubTooLongRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.maintainability.sub_too_long");
        assert_eq!(finding.category, CriticCategory::Maintainability);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.suppression_key, "native.maintainability.sub_too_long");
        assert_eq!(
            &source[finding.range.start.byte..finding.range.end.byte],
            "sub too_long",
            "finding must anchor at the sub header span"
        );
        assert!(
            finding.message.contains("too_long") && finding.message.contains("51"),
            "message should name the sub and its line count: {}",
            finding.message
        );
    }

    #[test]
    fn native_sub_too_long_rule_reports_cr_only_sub_over_the_limit() {
        let source = long_sub_source("too_long", DEFAULT_MAX_SUB_LINES - 1).replace('\n', "\r");
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SubTooLongRule)]);

        let findings = registry.check(&ctx);
        assert_eq!(findings.len(), 1, "CR-only line breaks must count as physical lines");
        assert!(findings[0].message.contains("51"), "got: {}", findings[0].message);
    }

    #[test]
    fn native_sub_too_long_rule_accepts_sub_at_the_limit() {
        // 1 opening line + 48 interior lines + 1 closing line = 50 body lines.
        let source = long_sub_source("at_limit", DEFAULT_MAX_SUB_LINES - 2);
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SubTooLongRule)]);

        assert!(registry.check(&ctx).is_empty(), "a sub at exactly the limit must pass");
    }

    #[test]
    fn native_sub_too_long_rule_accepts_short_sub() {
        let source = "use strict;\nuse warnings;\nsub short { return 1; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SubTooLongRule)]);

        assert!(registry.check(&ctx).is_empty());
    }

    #[test]
    fn native_sub_too_long_rule_ignores_anonymous_subs() {
        let mut source = String::from("use strict;\nuse warnings;\nmy $handler = sub {\n");
        for _ in 0..DEFAULT_MAX_SUB_LINES {
            source.push_str("    print 1;\n");
        }
        source.push_str("};\n");
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SubTooLongRule)]);

        assert!(registry.check(&ctx).is_empty(), "anonymous subs have no header to report");
    }

    #[test]
    fn native_sub_too_long_rule_composes_with_config_and_suppressions() {
        let source = long_sub_source("too_long", DEFAULT_MAX_SUB_LINES - 1);
        let ast = parse_source(&source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.maintainability.sub_too_long".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(&source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SubTooLongRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = format!(
            "## no critic native.maintainability.sub_too_long -- legacy fixture\n{}",
            long_sub_source("too_long", DEFAULT_MAX_SUB_LINES - 1)
        );
        let suppressed_ast = parse_source(&suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(&suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_sub_too_long_rule_is_strict_only() {
        let recommended = NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended);
        let strict = NativeCriticRegistry::for_profile(NativeCriticProfile::Strict);

        assert!(!recommended.rule_ids().contains(&"native.maintainability.sub_too_long"));
        assert!(strict.rule_ids().contains(&"native.maintainability.sub_too_long"));
        assert_eq!(strict.len(), 31);
        assert_eq!(recommended.len(), 16);
    }

    fn nested_blocks_source(name: &str, depth: usize) -> String {
        let mut source = format!("use strict;\nuse warnings;\nsub {name} {{\n");
        for level in 0..depth {
            source.push_str(&format!("{}if (1) {{\n", "    ".repeat(level + 1)));
        }
        source.push_str(&format!("{}print 1;\n", "    ".repeat(depth + 1)));
        for level in (0..depth).rev() {
            source.push_str(&format!("{}}}\n", "    ".repeat(level + 1)));
        }
        source.push_str("}\n");
        source
    }

    #[test]
    fn native_too_many_nested_blocks_rule_reports_sub_over_the_limit() {
        let source = nested_blocks_source("too_deep", DEFAULT_MAX_NESTED_BLOCKS + 1);
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyNestedBlocksRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.maintainability.too_many_nested_blocks");
        assert_eq!(finding.category, CriticCategory::Maintainability);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.suppression_key, "native.maintainability.too_many_nested_blocks");
        assert_eq!(
            &source[finding.range.start.byte..finding.range.end.byte],
            "sub too_deep",
            "finding must anchor at the sub header span"
        );
        assert!(
            finding.message.contains("too_deep") && finding.message.contains("5"),
            "message should name the sub and its depth: {}",
            finding.message
        );
    }

    #[test]
    fn native_too_many_nested_blocks_rule_accepts_sub_at_the_limit() {
        let source = nested_blocks_source("at_limit", DEFAULT_MAX_NESTED_BLOCKS);
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyNestedBlocksRule)]);

        assert!(registry.check(&ctx).is_empty(), "a sub at exactly the limit must pass");
    }

    #[test]
    fn native_too_many_nested_blocks_rule_accepts_flat_sub() {
        let source =
            "use strict;\nuse warnings;\nsub flat { my $x = { a => [1, 2] }; return $x; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyNestedBlocksRule)]);

        assert!(registry.check(&ctx).is_empty(), "hash/array literals are not blocks");
    }

    #[test]
    fn native_too_many_nested_blocks_rule_ignores_nested_hash_references() {
        // The parser represents a single-expression brace constructor as a
        // Block node, so nested hash references must not count as nesting.
        let source = "use strict;\nuse warnings;\nsub build { my $x = {{{{{1}}}}}; return $x; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyNestedBlocksRule)]);

        assert!(registry.check(&ctx).is_empty(), "hash references are not nested blocks");
    }

    #[test]
    fn native_too_many_nested_blocks_rule_ignores_anonymous_subs() {
        let mut source = String::from("use strict;\nuse warnings;\nmy $handler = sub {\n");
        for level in 0..DEFAULT_MAX_NESTED_BLOCKS + 2 {
            source.push_str(&format!("{}if (1) {{\n", "    ".repeat(level + 1)));
        }
        source.push_str("    print 1;\n");
        for _ in 0..DEFAULT_MAX_NESTED_BLOCKS + 2 {
            source.push_str("}\n");
        }
        source.push_str("};\n");
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyNestedBlocksRule)]);

        assert!(registry.check(&ctx).is_empty(), "anonymous subs have no header to report");
    }

    #[test]
    fn native_too_many_nested_blocks_rule_measures_nested_subs_separately() {
        let inner = nested_blocks_source("inner_deep", DEFAULT_MAX_NESTED_BLOCKS + 1);
        let inner_body = inner.strip_prefix("use strict;\nuse warnings;\n").unwrap_or(&inner);
        let source = format!("use strict;\nuse warnings;\nsub outer {{\n{inner_body}}}\n");
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyNestedBlocksRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1, "only the nested sub itself fires");
        assert!(findings[0].message.contains("inner_deep"), "got: {}", findings[0].message);
    }

    #[test]
    fn native_too_many_nested_blocks_rule_composes_with_config_and_suppressions() {
        let source = nested_blocks_source("too_deep", DEFAULT_MAX_NESTED_BLOCKS + 1);
        let ast = parse_source(&source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.maintainability.too_many_nested_blocks".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(&source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyNestedBlocksRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = format!(
            "## no critic native.maintainability.too_many_nested_blocks -- legacy fixture\n{}",
            nested_blocks_source("too_deep", DEFAULT_MAX_NESTED_BLOCKS + 1)
        );
        let suppressed_ast = parse_source(&suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(&suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    fn params_sub_source(name: &str, param_count: usize) -> String {
        let params: Vec<String> = (0..param_count).map(|i| format!("$p{i}")).collect();
        format!("use strict;\nuse warnings;\nsub {name}({}) {{ return 1; }}\n", params.join(", "))
    }

    #[test]
    fn native_too_many_params_rule_reports_sub_over_the_limit() {
        let source = params_sub_source("too_wide", DEFAULT_MAX_PARAMS + 1);
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyParamsRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.rule_id, "native.maintainability.too_many_params");
        assert_eq!(finding.category, CriticCategory::Maintainability);
        assert_eq!(finding.severity, Severity::Stern);
        assert_eq!(finding.suppression_key, "native.maintainability.too_many_params");
        assert_eq!(
            &source[finding.range.start.byte..finding.range.end.byte],
            "sub too_wide",
            "finding must anchor at the sub header span"
        );
        assert!(
            finding.message.contains("too_wide") && finding.message.contains("6"),
            "message should name the sub and its parameter count: {}",
            finding.message
        );
    }

    #[test]
    fn native_too_many_params_rule_accepts_sub_at_the_limit() {
        let source = params_sub_source("at_limit", DEFAULT_MAX_PARAMS);
        let ast = parse_source(&source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(&source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyParamsRule)]);

        assert!(registry.check(&ctx).is_empty(), "a sub at exactly the limit must pass");
    }

    #[test]
    fn native_too_many_params_rule_counts_optional_named_and_slurpy_parameters() {
        let source = "use strict;\nuse warnings;\nsub wide($a, $b = 1, $c, :$d, :$e = 2, @rest) { return $a; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyParamsRule)]);

        let findings = registry.check(&ctx);

        assert_eq!(findings.len(), 1, "six declared parameters must fire");
        assert!(findings[0].message.contains("6"), "got: {}", findings[0].message);
    }

    #[test]
    fn native_too_many_params_rule_ignores_malformed_parameters() {
        // The parser retains a malformed signature item as an error node;
        // it is not a declared binding and must not count toward the width.
        let source =
            "use strict;\nuse warnings;\nsub wide($a, $b, $c, $d, $e, $f = ) { return $a; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyParamsRule)]);

        assert!(
            registry.check(&ctx).is_empty(),
            "a malformed parameter is not a declared parameter"
        );
    }

    #[test]
    fn native_too_many_params_rule_ignores_subs_without_signatures() {
        let source = "use strict;\nuse warnings;\nsub legacy { my ($a, $b, $c, $d, $e, $f, $g) = @_; return $a; }\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyParamsRule)]);

        assert!(registry.check(&ctx).is_empty(), "no signature means no declared parameter count");
    }

    #[test]
    fn native_too_many_params_rule_ignores_anonymous_subs() {
        let source = "use strict;\nuse warnings;\nmy $handler = sub ($a, $b, $c, $d, $e, $f) { return $a; };\n";
        let ast = parse_source(source);
        let config = CriticConfig::default();
        let ctx = CriticContext::new(source, &ast, &config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyParamsRule)]);

        assert!(registry.check(&ctx).is_empty(), "anonymous subs have no header to report");
    }

    #[test]
    fn native_too_many_params_rule_composes_with_config_and_suppressions() {
        let source = params_sub_source("too_wide", DEFAULT_MAX_PARAMS + 1);
        let ast = parse_source(&source);
        let excluded_config = CriticConfig {
            exclude: vec!["native.maintainability.too_many_params".to_string()],
            ..Default::default()
        };
        let excluded_ctx = CriticContext::new(&source, &ast, &excluded_config);
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(TooManyParamsRule)]);

        assert!(registry.check(&excluded_ctx).is_empty());

        let suppressed_source = format!(
            "## no critic native.maintainability.too_many_params -- legacy fixture\n{}",
            params_sub_source("too_wide", DEFAULT_MAX_PARAMS + 1)
        );
        let suppressed_ast = parse_source(&suppressed_source);
        let config = CriticConfig::default();
        let suppressed_ctx = CriticContext::new(&suppressed_source, &suppressed_ast, &config);

        assert!(registry.check(&suppressed_ctx).is_empty());
    }

    #[test]
    fn native_too_many_params_rule_is_strict_only() {
        let recommended = NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended);
        let strict = NativeCriticRegistry::for_profile(NativeCriticProfile::Strict);

        assert!(!recommended.rule_ids().contains(&"native.maintainability.too_many_params"));
        assert!(strict.rule_ids().contains(&"native.maintainability.too_many_params"));
    }

    #[test]
    fn native_too_many_nested_blocks_rule_is_strict_only() {
        let recommended = NativeCriticRegistry::for_profile(NativeCriticProfile::Recommended);
        let strict = NativeCriticRegistry::for_profile(NativeCriticProfile::Strict);

        assert!(!recommended.rule_ids().contains(&"native.maintainability.too_many_nested_blocks"));
        assert!(strict.rule_ids().contains(&"native.maintainability.too_many_nested_blocks"));
    }
}
