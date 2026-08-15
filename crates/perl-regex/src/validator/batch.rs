use super::{
    analysis::{
        EmbeddedCodeFact, EmbeddedCodeKind, RegexAnalysis, RegexAnalysisCompleteness,
        RegexDiagnostic, RegexDiagnosticClass, RegexDiagnosticCode, RegexDynamicRegionFact,
        RegexDynamicRegionKind, RegexFacts, RegexRange,
    },
    code_execution, complexity,
    config::RegexValidationConfig,
    nested_quantifier,
};
use crate::error::RegexError;

/// Project the current fail-fast scanners into the typed analysis shell.
///
/// This slice intentionally preserves single-finding scanner behavior. Multi-finding
/// collection, dynamic-span masking, and interpolation are deferred follow-ups.
pub(crate) fn analyze(pattern: &str, config: &RegexValidationConfig) -> RegexAnalysis {
    let mut diagnostics = Vec::new();
    let mut facts = RegexFacts::default();

    if let Some(finding) = code_execution::find_code_execution(pattern, 0) {
        let (kind, code, width) = match finding.kind {
            code_execution::EmbeddedCodeKind::Immediate => {
                (EmbeddedCodeKind::Immediate, RegexDiagnosticCode::EmbeddedCodeImmediate, 3)
            }
            code_execution::EmbeddedCodeKind::Deferred => {
                (EmbeddedCodeKind::Deferred, RegexDiagnosticCode::EmbeddedCodeDeferred, 4)
            }
        };
        if let Some(range) = RegexRange::anchored(finding.offset, width, pattern.len()) {
            facts.embedded_code.push(EmbeddedCodeFact { kind, range });
            facts.dynamic_regions.push(RegexDynamicRegionFact {
                kind: match kind {
                    EmbeddedCodeKind::Immediate => RegexDynamicRegionKind::EmbeddedCodeImmediate,
                    EmbeddedCodeKind::Deferred => RegexDynamicRegionKind::EmbeddedCodeDeferred,
                },
                range,
            });
            diagnostics.push(RegexDiagnostic::new(code, range, None));
        }
    }

    if let Some(offset) = nested_quantifier::find_nested_quantifier(pattern, 0)
        && let Some(range) = RegexRange::anchored(offset, 1, pattern.len())
    {
        facts.nested_quantifiers.push(range);
        diagnostics.push(RegexDiagnostic::new(
            RegexDiagnosticCode::NestedQuantifierRisk,
            range,
            None,
        ));
    }

    let mut policy_limited = false;
    if let Err(RegexError::Syntax { message, offset }) =
        complexity::check_complexity(pattern, 0, config)
    {
        policy_limited = true;
        let code = if message.contains("Unicode properties") {
            RegexDiagnosticCode::UnicodePropertyLimit
        } else if message.contains("lookbehind") {
            RegexDiagnosticCode::LookbehindNestingLimit
        } else if message.contains("branch reset nesting") {
            RegexDiagnosticCode::BranchResetNestingLimit
        } else {
            RegexDiagnosticCode::BranchResetBranchLimit
        };
        let limit = match code {
            RegexDiagnosticCode::UnicodePropertyLimit => Some(config.max_unicode_properties),
            RegexDiagnosticCode::LookbehindNestingLimit
            | RegexDiagnosticCode::BranchResetNestingLimit => Some(config.max_nesting),
            RegexDiagnosticCode::BranchResetBranchLimit => Some(config.max_branch_reset_branches),
            _ => None,
        };
        if let Some(range) = RegexRange::anchored(offset, 1, pattern.len()) {
            diagnostics.push(RegexDiagnostic::new(code, range, limit));
        }
    }

    diagnostics
        .sort_by_key(|diagnostic| (diagnostic.range.start, diagnostic.range.end, diagnostic.code));
    let dynamic = !facts.dynamic_regions.is_empty();

    RegexAnalysis {
        diagnostics,
        facts,
        completeness: RegexAnalysisCompleteness::from_flags(dynamic, policy_limited),
    }
}

pub(crate) fn first_compatibility_diagnostic(analysis: &RegexAnalysis) -> Option<&RegexDiagnostic> {
    analysis.diagnostics.iter().min_by_key(|diagnostic| compatibility_priority(diagnostic))
}

fn compatibility_priority(diagnostic: &RegexDiagnostic) -> (u8, usize, usize, RegexDiagnosticCode) {
    let priority = match diagnostic.code {
        RegexDiagnosticCode::EmbeddedCodeImmediate | RegexDiagnosticCode::EmbeddedCodeDeferred => 0,
        RegexDiagnosticCode::NestedQuantifierRisk => 1,
        _ => match diagnostic.class {
            RegexDiagnosticClass::Syntax => 2,
            RegexDiagnosticClass::PolicyLimit => 3,
            RegexDiagnosticClass::RiskAdvisory => 4,
            RegexDiagnosticClass::DynamicBoundary => 5,
        },
    };
    (priority, diagnostic.range.start, diagnostic.range.end, diagnostic.code)
}
