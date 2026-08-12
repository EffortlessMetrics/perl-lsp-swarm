use super::{
    analysis::{
        EmbeddedCodeFact, EmbeddedCodeKind, RegexAnalysis, RegexAnalysisCompleteness,
        RegexDiagnostic, RegexDiagnosticClass, RegexDiagnosticCode, RegexFacts, RegexRange,
    },
    code_execution, complexity,
    config::RegexValidationConfig,
    nested_quantifier,
};

pub(crate) fn analyze(pattern: &str, config: &RegexValidationConfig) -> RegexAnalysis {
    let mut diagnostics = Vec::new();
    let mut facts = RegexFacts::default();

    for finding in code_execution::find_code_executions(pattern) {
        let (kind, code, width) = match finding.kind {
            code_execution::EmbeddedCodeKind::Immediate => (
                EmbeddedCodeKind::Immediate,
                RegexDiagnosticCode::EmbeddedCodeImmediate,
                3,
            ),
            code_execution::EmbeddedCodeKind::Deferred => (
                EmbeddedCodeKind::Deferred,
                RegexDiagnosticCode::EmbeddedCodeDeferred,
                4,
            ),
        };
        if let Some(range) = RegexRange::anchored(finding.offset, width, pattern.len()) {
            facts.embedded_code.push(EmbeddedCodeFact { kind, range });
            diagnostics.push(RegexDiagnostic::new(code, range, None));
        }
    }

    for offset in nested_quantifier::find_nested_quantifiers(pattern) {
        if let Some(range) = RegexRange::anchored(offset, 1, pattern.len()) {
            facts.nested_quantifiers.push(range);
            diagnostics.push(RegexDiagnostic::new(
                RegexDiagnosticCode::NestedQuantifierRisk,
                range,
                None,
            ));
        }
    }

    let policy_diagnostics = complexity::find_complexity_diagnostics(pattern, config);
    let dynamic = !facts.embedded_code.is_empty();
    let policy_limited = !policy_diagnostics.is_empty();
    diagnostics.extend(policy_diagnostics);
    diagnostics.sort_by_key(|diagnostic| {
        (diagnostic.range.start, diagnostic.range.end, diagnostic.code)
    });

    RegexAnalysis {
        diagnostics,
        facts,
        completeness: RegexAnalysisCompleteness::from_flags(dynamic, policy_limited),
    }
}

pub(crate) fn first_compatibility_diagnostic(
    analysis: &RegexAnalysis,
) -> Option<&RegexDiagnostic> {
    analysis
        .diagnostics
        .iter()
        .min_by_key(|diagnostic| compatibility_priority(diagnostic))
}

fn compatibility_priority(diagnostic: &RegexDiagnostic) -> (u8, usize, usize, RegexDiagnosticCode) {
    let priority = match diagnostic.code {
        RegexDiagnosticCode::EmbeddedCodeImmediate
        | RegexDiagnosticCode::EmbeddedCodeDeferred => 0,
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
