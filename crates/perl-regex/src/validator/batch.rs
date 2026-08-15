use crate::{
    analyzer::{CaptureMode, EffectiveModifiers, ExtendedMode},
    syntax::event::{RegexEventBudget, RegexExtendedMode, RegexModeState, parse_regex_events},
};

use super::{
    analysis::{
        EmbeddedCodeFact, EmbeddedCodeKind, RegexAnalysis, RegexAnalysisBudget,
        RegexAnalysisCompleteness, RegexDiagnostic, RegexDiagnosticClass, RegexDiagnosticCode,
        RegexDynamicRegionFact, RegexDynamicRegionKind, RegexFacts, RegexRange,
    },
    code_execution, complexity,
    config::RegexValidationConfig,
    nested_quantifier,
};

pub(crate) fn analyze(
    pattern: &str,
    config: &RegexValidationConfig,
    modifiers: EffectiveModifiers,
) -> RegexAnalysis {
    let stream = parse_regex_events(pattern, initial_mode(modifiers));
    let mut diagnostics = Vec::new();
    let mut facts = RegexFacts::default();

    for finding in code_execution::find_code_executions(&stream) {
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

    for offset in nested_quantifier::find_nested_quantifiers(&stream) {
        if let Some(range) = RegexRange::anchored(offset, 1, pattern.len()) {
            facts.nested_quantifiers.push(range);
            diagnostics.push(RegexDiagnostic::new(
                RegexDiagnosticCode::NestedQuantifierRisk,
                range,
                None,
            ));
        }
    }

    let policy_diagnostics = complexity::find_complexity_diagnostics(&stream, config);
    let exhausted = stream.exhausted.map(map_budget);
    let dynamic = !facts.dynamic_regions.is_empty();
    let policy_limited = !policy_diagnostics.is_empty() || exhausted.is_some();
    diagnostics.extend(policy_diagnostics);
    diagnostics
        .sort_by_key(|diagnostic| (diagnostic.range.start, diagnostic.range.end, diagnostic.code));

    RegexAnalysis {
        diagnostics,
        facts,
        completeness: RegexAnalysisCompleteness::from_flags(dynamic, policy_limited),
        exhausted,
        malformed: stream.malformed,
    }
}

pub(crate) fn first_compatibility_diagnostic(analysis: &RegexAnalysis) -> Option<&RegexDiagnostic> {
    analysis.diagnostics.iter().min_by_key(|diagnostic| compatibility_priority(diagnostic))
}

fn initial_mode(modifiers: EffectiveModifiers) -> RegexModeState {
    let extended = match modifiers.extended {
        ExtendedMode::Off => RegexExtendedMode::Off,
        ExtendedMode::Extended => RegexExtendedMode::Extended,
        ExtendedMode::ExtraExtended { .. } => RegexExtendedMode::ExtraExtended,
    };
    RegexModeState {
        extended,
        captures_by_default: matches!(modifiers.captures, CaptureMode::CapturingByDefault),
    }
}

fn map_budget(budget: RegexEventBudget) -> RegexAnalysisBudget {
    match budget {
        RegexEventBudget::Events => RegexAnalysisBudget::Events,
        RegexEventBudget::Nesting => RegexAnalysisBudget::Nesting,
        RegexEventBudget::Steps => RegexAnalysisBudget::Steps,
    }
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
