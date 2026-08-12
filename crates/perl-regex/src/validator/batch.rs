use super::{
    analysis::{
        EmbeddedCodeFact, EmbeddedCodeKind, RegexAnalysis, RegexAnalysisCompleteness,
        RegexDiagnostic, RegexDiagnosticClass, RegexDiagnosticCode, RegexDynamicRegionFact,
        RegexDynamicRegionKind, RegexFacts, RegexRange,
    },
    code_execution, complexity,
    config::RegexValidationConfig,
    interpolation, nested_quantifier,
};

pub(crate) fn analyze(pattern: &str, config: &RegexValidationConfig) -> RegexAnalysis {
    let mut diagnostics = Vec::new();
    let mut facts = RegexFacts::default();

    for finding in code_execution::find_code_executions(pattern) {
        let (kind, code) = match finding.kind {
            code_execution::EmbeddedCodeKind::Immediate => {
                (EmbeddedCodeKind::Immediate, RegexDiagnosticCode::EmbeddedCodeImmediate)
            }
            code_execution::EmbeddedCodeKind::Deferred => {
                (EmbeddedCodeKind::Deferred, RegexDiagnosticCode::EmbeddedCodeDeferred)
            }
        };
        if let Some(range) = RegexRange::anchored(
            finding.offset,
            finding.end.saturating_sub(finding.offset),
            pattern.len(),
        ) {
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

    let embedded_ranges = facts.dynamic_regions.iter().map(|fact| fact.range).collect::<Vec<_>>();
    for range in interpolation::find_interpolations(pattern, &embedded_ranges)
        .into_iter()
        .map(|range| RegexDynamicRegionFact { kind: RegexDynamicRegionKind::Interpolation, range })
    {
        facts.dynamic_regions.push(range);
    }
    facts.dynamic_regions.sort_by_key(|fact| fact.range);
    let dynamic_ranges = facts.dynamic_regions.iter().map(|fact| fact.range).collect::<Vec<_>>();

    for offset in nested_quantifier::find_nested_quantifiers(pattern, &dynamic_ranges) {
        if let Some(range) = RegexRange::anchored(offset, 1, pattern.len()) {
            facts.nested_quantifiers.push(range);
            diagnostics.push(RegexDiagnostic::new(
                RegexDiagnosticCode::NestedQuantifierRisk,
                range,
                None,
            ));
        }
    }

    let policy_diagnostics =
        complexity::find_complexity_diagnostics(pattern, config, &dynamic_ranges);
    let dynamic = !facts.dynamic_regions.is_empty();
    let policy_limited = !policy_diagnostics.is_empty();
    diagnostics.extend(policy_diagnostics);
    diagnostics
        .sort_by_key(|diagnostic| (diagnostic.range.start, diagnostic.range.end, diagnostic.code));

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
