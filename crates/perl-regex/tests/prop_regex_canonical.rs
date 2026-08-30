//! Property proof for canonical `perl-regex` analysis contracts.
//!
//! This complements the compatibility-wrapper properties by exercising arbitrary
//! valid UTF-8, structured regex triggers, operator/profile-aware modifiers, source
//! geometry, deterministic ordering, and near-overflow source offsets.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_regex::analyzer::{
    FeatureState, ModifierSequence, PerlVersion, RegexLanguageProfile, RegexOperator,
};
use perl_regex::validator::{RegexAnalysis, RegexDiagnosticClass, RegexDiagnosticCode, RegexRange};
use perl_regex::{RegexAnalyzer, RegexValidator};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestCaseResult};

fn arbitrary_utf8_pattern() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..128)
        .prop_map(|characters| characters.into_iter().collect())
}

fn structured_fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => any::<char>().prop_map(|character| character.to_string()),
        2 => prop::sample::select(vec![
            "(a+)+",
            "(a+){2,}",
            "(?{ code })",
            "(??{ later })",
            r"\Q$x\E",
            r"\p{Letter}",
            "(?x:# comment\n(a+)+)",
            "(?-x:#literal)",
            "(?<名>.)",
            "(?|(?<x>a)|(?<x>b))",
            r"\g{name}",
            "${变量}",
            "@values",
            "[é$var]",
            "[unterminated",
            r"\",
            "(unclosed",
            "(?n:(a))",
            "(?# fake (?{ code }))",
        ])
        .prop_map(|fragment| fragment.to_owned()),
    ]
}

fn structured_utf8_pattern() -> impl Strategy<Value = String> {
    prop::collection::vec(structured_fragment(), 0..32).prop_map(|fragments| {
        let mut pattern = String::new();
        for fragment in fragments {
            pattern.push_str(&fragment);
        }
        pattern
    })
}

fn modifier_spelling() -> impl Strategy<Value = String> {
    let character = prop_oneof![
        4 => prop::sample::select(vec![
            'i', 'm', 's', 'x', 'g', 'a', 'd', 'l', 'u', 'n', 'p', 'r', 'c', 'o', 'e',
            'z', ' ', '\n',
        ]),
        1 => any::<char>(),
    ];
    prop::collection::vec(character, 0..24).prop_map(|characters| characters.into_iter().collect())
}

fn regex_operator() -> impl Strategy<Value = RegexOperator> {
    prop::sample::select(vec![
        RegexOperator::BareMatch,
        RegexOperator::Match,
        RegexOperator::QuoteRegex,
        RegexOperator::Substitution,
    ])
}

fn any_operator() -> impl Strategy<Value = RegexOperator> {
    prop::sample::select(vec![
        RegexOperator::BareMatch,
        RegexOperator::Match,
        RegexOperator::QuoteRegex,
        RegexOperator::Substitution,
        RegexOperator::Transliteration,
        RegexOperator::TransliterationAlias,
    ])
}

fn language_profile() -> impl Strategy<Value = RegexLanguageProfile> {
    (
        prop::sample::select(vec![
            None,
            Some(PerlVersion::new(5, 12)),
            Some(PerlVersion::new(5, 14)),
            Some(PerlVersion::new(5, 22)),
            Some(PerlVersion::new(5, 26)),
            Some(PerlVersion::new(5, 44)),
            Some(PerlVersion::new(5, 46)),
        ]),
        prop::sample::select(vec![
            FeatureState::Enabled,
            FeatureState::Disabled,
            FeatureState::Unknown,
        ]),
    )
        .prop_map(|(version, enhanced_xx)| RegexLanguageProfile::new(version, enhanced_xx))
}

fn source_offset() -> impl Strategy<Value = usize> {
    prop_oneof![
        4 => 0usize..4096,
        1 => (0usize..64).prop_map(|distance| usize::MAX - distance),
    ]
}

fn assert_body_range(pattern: &str, range: RegexRange, label: &str) -> TestCaseResult {
    prop_assert!(range.start <= range.end, "{label} range is reversed: {range:?} in {pattern:?}");
    prop_assert!(
        range.end <= pattern.len(),
        "{label} range escapes input: {range:?}, len={} in {pattern:?}",
        pattern.len()
    );
    prop_assert!(
        pattern.is_char_boundary(range.start),
        "{label} start is not a UTF-8 boundary: {range:?} in {pattern:?}"
    );
    prop_assert!(
        pattern.is_char_boundary(range.end),
        "{label} end is not a UTF-8 boundary: {range:?} in {pattern:?}"
    );
    Ok(())
}

fn assert_analysis_contract(pattern: &str, analysis: &RegexAnalysis) -> TestCaseResult {
    for diagnostic in &analysis.diagnostics {
        assert_body_range(pattern, diagnostic.range, "diagnostic")?;
    }
    for fact in &analysis.facts.embedded_code {
        assert_body_range(pattern, fact.range, "embedded-code fact")?;
        prop_assert!(
            analysis
                .facts
                .dynamic_regions
                .iter()
                .filter(|region| region.range == fact.range)
                .count()
                == 1,
            "embedded-code fact does not have exactly one matching dynamic region: {:?} in {pattern:?}",
            fact.range
        );
    }
    prop_assert!(
        analysis.facts.dynamic_regions.len() >= analysis.facts.embedded_code.len(),
        "fewer dynamic regions than embedded-code facts in {pattern:?}"
    );
    for region in &analysis.facts.dynamic_regions {
        assert_body_range(pattern, region.range, "dynamic region")?;
    }
    for range in &analysis.facts.nested_quantifiers {
        assert_body_range(pattern, *range, "nested-quantifier fact")?;
    }

    for window in analysis.diagnostics.windows(2) {
        let left = (window[0].range.start, window[0].range.end, window[0].code);
        let right = (window[1].range.start, window[1].range.end, window[1].code);
        prop_assert!(left <= right, "diagnostics are not source ordered in {pattern:?}");
    }
    for window in analysis.facts.embedded_code.windows(2) {
        prop_assert!(
            (window[0].range.start, window[0].range.end)
                <= (window[1].range.start, window[1].range.end),
            "embedded-code facts are not source ordered in {pattern:?}"
        );
    }
    for window in analysis.facts.dynamic_regions.windows(2) {
        prop_assert!(
            (window[0].range.start, window[0].range.end)
                <= (window[1].range.start, window[1].range.end),
            "dynamic regions are not source ordered in {pattern:?}"
        );
    }
    for window in analysis.facts.nested_quantifiers.windows(2) {
        prop_assert!(
            (window[0].start, window[0].end) <= (window[1].start, window[1].end),
            "nested-quantifier facts are not source ordered in {pattern:?}"
        );
    }

    let dynamic = !analysis.facts.dynamic_regions.is_empty();
    let policy_limited = analysis.exhausted.is_some()
        || analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.class == RegexDiagnosticClass::PolicyLimit);
    prop_assert_eq!(analysis.completeness.has_dynamic_boundary(), dynamic);
    prop_assert_eq!(analysis.completeness.is_policy_limited(), policy_limited);
    prop_assert_eq!(analysis.completeness.is_complete(), !dynamic && !policy_limited);
    Ok(())
}

fn assert_modifier_range(
    raw: &str,
    source_start: usize,
    source_end: usize,
    range: RegexRange,
    label: &str,
) -> TestCaseResult {
    prop_assert!(
        range.start >= source_start,
        "{label} range starts before the modifier sequence: {range:?}"
    );
    prop_assert!(range.start <= range.end, "{label} range is reversed: {range:?}");
    prop_assert!(range.end <= source_end, "{label} range escapes the modifier sequence: {range:?}");
    let relative_start = range.start - source_start;
    let relative_end = range.end - source_start;
    prop_assert!(
        raw.is_char_boundary(relative_start),
        "{label} start is not a UTF-8 boundary: {range:?} in {raw:?}"
    );
    prop_assert!(
        raw.is_char_boundary(relative_end),
        "{label} end is not a UTF-8 boundary: {range:?} in {raw:?}"
    );
    Ok(())
}

fn assert_modifier_contract(
    raw: &str,
    source_start: usize,
    analysis: &perl_regex::analyzer::ModifierAnalysis,
) -> TestCaseResult {
    prop_assert_eq!(analysis.sequence.raw.as_str(), raw);
    prop_assert_eq!(analysis.sequence.range.start, source_start);
    prop_assert_eq!(analysis.sequence.range.end, source_start + raw.len());
    prop_assert_eq!(analysis.tokens.len(), raw.chars().count());

    for (token, (relative, value)) in analysis.tokens.iter().zip(raw.char_indices()) {
        prop_assert_eq!(token.value, value);
        prop_assert_eq!(token.range.start, source_start + relative);
        prop_assert_eq!(token.range.end, source_start + relative + value.len_utf8());
        assert_modifier_range(
            raw,
            source_start,
            analysis.sequence.range.end,
            token.range,
            "modifier token",
        )?;
    }
    for requirement in &analysis.requirements {
        assert_modifier_range(
            raw,
            source_start,
            analysis.sequence.range.end,
            requirement.range,
            "modifier requirement",
        )?;
    }
    for diagnostic in &analysis.diagnostics {
        assert_modifier_range(
            raw,
            source_start,
            analysis.sequence.range.end,
            diagnostic.range,
            "modifier diagnostic",
        )?;
    }
    for window in analysis.diagnostics.windows(2) {
        let left = (window[0].range.start, window[0].range.end, window[0].code);
        let right = (window[1].range.start, window[1].range.end, window[1].code);
        prop_assert!(left <= right, "modifier diagnostics are not source ordered in {raw:?}");
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn arbitrary_utf8_batch_is_deterministic_bounded_and_compatibility_aligned(
        pattern in arbitrary_utf8_pattern(),
        start in source_offset(),
    ) {
        let validator = RegexValidator::new();
        let first = validator.analyze(&pattern);
        let second = validator.analyze(&pattern);

        prop_assert_eq!(&first, &second);
        assert_analysis_contract(&pattern, &first)?;

        prop_assert_eq!(
            validator.detects_code_execution(&pattern),
            !first.facts.embedded_code.is_empty()
        );
        prop_assert_eq!(
            validator.detect_nested_quantifiers(&pattern),
            !first.facts.nested_quantifiers.is_empty()
        );
        prop_assert_eq!(
            validator.find_code_execution(&pattern, start).map(|finding| finding.offset),
            first
                .facts
                .embedded_code
                .first()
                .map(|fact| start.saturating_add(fact.range.start))
        );
        if let Some(first_fact) = first.facts.embedded_code.first() {
            let first_embedded_diagnostic = first.diagnostics.iter().find(|diagnostic| {
                diagnostic.code == RegexDiagnosticCode::EmbeddedCodeImmediate
                    || diagnostic.code == RegexDiagnosticCode::EmbeddedCodeDeferred
            });
            prop_assert!(
                first_embedded_diagnostic.is_some(),
                "embedded-code facts exist without an embedded-code diagnostic in {pattern:?}"
            );
            prop_assert_eq!(
                first_embedded_diagnostic.map(|diagnostic| diagnostic.range.start),
                Some(first_fact.range.start)
            );
        }
        prop_assert_eq!(
            validator.find_nested_quantifier(&pattern, start).map(|finding| finding.offset),
            first
                .facts
                .nested_quantifiers
                .first()
                .map(|range| start.saturating_add(range.start))
        );
    }

    #[test]
    fn structured_profiled_batch_preserves_the_same_geometry_contract(
        pattern in structured_utf8_pattern(),
        raw_modifiers in modifier_spelling(),
        operator in regex_operator(),
        profile in language_profile(),
    ) {
        let sequence = ModifierSequence::new(raw_modifiers, 0)
            .ok_or_else(|| TestCaseError::fail("zero-based modifier sequence overflowed"))?;
        let modifiers = RegexAnalyzer::analyze_modifiers(operator, sequence, profile).effective;
        let validator = RegexValidator::new();
        let first = validator.analyze_with_modifiers(&pattern, modifiers);
        let second = validator.analyze_with_modifiers(&pattern, modifiers);

        prop_assert_eq!(&first, &second);
        assert_analysis_contract(&pattern, &first)?;
    }

    #[test]
    fn modifier_analysis_is_lossless_profiled_and_overflow_honest(
        raw in modifier_spelling(),
        operator in any_operator(),
        profile in language_profile(),
        start in source_offset(),
    ) {
        let Some(sequence) = ModifierSequence::new(raw.clone(), start) else {
            prop_assert!(start.checked_add(raw.len()).is_none());
            return Ok(());
        };

        let first = RegexAnalyzer::analyze_modifiers(operator, sequence.clone(), profile);
        let second = RegexAnalyzer::analyze_modifiers(operator, sequence, profile);
        prop_assert_eq!(&first, &second);
        prop_assert_eq!(first.operator, operator);
        assert_modifier_contract(&raw, start, &first)?;

        for diagnostic in &first.diagnostics {
            prop_assert!(
                !diagnostic.message().is_empty(),
                "typed diagnostic {:?} rendered an empty message",
                diagnostic.code
            );
            prop_assert!(
                diagnostic.code != RegexDiagnosticCode::EmbeddedCodeImmediate
                    && diagnostic.code != RegexDiagnosticCode::EmbeddedCodeDeferred,
                "modifier analysis emitted a regex-body diagnostic: {:?}",
                diagnostic.code
            );
        }
    }
}
