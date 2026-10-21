#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_regex::validator::{
    EmbeddedCodeKind, RegexAnalysisCompleteness, RegexDiagnosticClass, RegexDiagnosticCode,
    RegexValidationConfig,
};
use perl_regex::{RegexError, RegexValidator};

#[test]
fn safe_pattern_returns_complete_clean_analysis() -> Result<(), Box<dyn std::error::Error>> {
    let analysis = RegexValidator::new().analyze(r"^(?<year>\d{4})-(?<month>\d{2})$");

    assert!(analysis.is_clean());
    assert!(analysis.completeness.is_complete());
    assert!(analysis.facts.embedded_code.is_empty());
    assert!(analysis.facts.nested_quantifiers.is_empty());
    Ok(())
}

#[test]
fn one_batch_preserves_dynamic_advisory_and_policy_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let pattern = r"\p{L}\p{N}(a+)+(?{ run })(??{ later })";

    let analysis = validator.analyze(pattern);
    let codes = analysis.diagnostics.iter().map(|diagnostic| diagnostic.code).collect::<Vec<_>>();

    assert_eq!(
        codes,
        vec![
            RegexDiagnosticCode::UnicodePropertyLimit,
            RegexDiagnosticCode::NestedQuantifierRisk,
            RegexDiagnosticCode::EmbeddedCodeImmediate,
            RegexDiagnosticCode::EmbeddedCodeDeferred,
        ]
    );
    assert_eq!(analysis.completeness, RegexAnalysisCompleteness::DynamicAndPolicyLimited);
    assert_eq!(analysis.facts.nested_quantifiers.len(), 1);
    assert_eq!(analysis.facts.nested_quantifiers[0].start, 14);
    assert_eq!(analysis.facts.embedded_code.len(), 2);
    assert_eq!(analysis.facts.embedded_code[0].kind, EmbeddedCodeKind::Immediate);
    assert_eq!(analysis.facts.embedded_code[0].range.start, 15);
    assert_eq!(analysis.facts.embedded_code[1].kind, EmbeddedCodeKind::Deferred);
    assert_eq!(analysis.facts.embedded_code[1].range.start, 25);
    Ok(())
}

#[test]
fn diagnostic_identity_is_separate_from_presentation_text() -> Result<(), Box<dyn std::error::Error>>
{
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let analysis = validator.analyze(r"\p{L}\p{N}");
    let diagnostic = analysis.diagnostics.first().ok_or("expected policy diagnostic")?;

    assert_eq!(diagnostic.code, RegexDiagnosticCode::UnicodePropertyLimit);
    assert_eq!(diagnostic.code.as_str(), "unicode_property_limit");
    assert_eq!(diagnostic.class, RegexDiagnosticClass::PolicyLimit);
    assert_eq!(diagnostic.class.as_str(), "policy_limit");
    assert_eq!(diagnostic.limit, Some(1));
    assert!(diagnostic.message().contains("max 1"));
    Ok(())
}

#[test]
fn diagnostics_are_deterministic_and_source_ordered() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let pattern = r"(?{ first })\p{L}\p{N}(a+)+(?{ second })";
    let first = validator.analyze(pattern);
    let second = validator.analyze(pattern);

    assert_eq!(first, second);
    for pair in first.diagnostics.windows(2) {
        assert!(pair[0].range.start <= pair[1].range.start);
    }
    Ok(())
}

#[test]
fn compatibility_validate_keeps_embedded_code_category_priority()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let pattern = r"(a+)+literal(?{ run })";
    let analysis = validator.analyze(pattern);

    assert_eq!(analysis.diagnostics[0].code, RegexDiagnosticCode::NestedQuantifierRisk);
    assert_eq!(analysis.diagnostics[1].code, RegexDiagnosticCode::EmbeddedCodeImmediate);

    let error =
        validator.validate(pattern, 100).err().ok_or("expected compatibility validation error")?;
    match error {
        RegexError::Syntax { message, offset } => {
            assert!(message.contains("Embedded code execution"));
            assert_eq!(offset, 112);
        }
    }
    Ok(())
}

#[test]
fn compatibility_finders_project_relative_batch_ranges_to_absolute_offsets()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();

    let code_analysis = validator.analyze("xx(?{ run })");
    assert_eq!(code_analysis.facts.embedded_code[0].range.start, 2);
    let code = validator
        .find_code_execution("xx(?{ run })", 20)
        .ok_or("expected embedded-code compatibility finding")?;
    assert_eq!(code.offset, 22);

    let nested_analysis = validator.analyze("abc(a+)+");
    assert_eq!(nested_analysis.facts.nested_quantifiers[0].start, 7);
    let nested = validator
        .find_nested_quantifier("abc(a+)+", 20)
        .ok_or("expected nested-quantifier compatibility finding")?;
    assert_eq!(nested.offset, 27);

    assert_eq!(
        validator.detects_code_execution("xx(?{ run })"),
        !code_analysis.facts.embedded_code.is_empty()
    );
    assert_eq!(
        validator.detect_nested_quantifiers("abc(a+)+"),
        !nested_analysis.facts.nested_quantifiers.is_empty()
    );
    Ok(())
}

#[test]
fn dynamic_and_policy_completeness_remain_distinct() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });

    let dynamic = validator.analyze("(?{ run })");
    assert_eq!(dynamic.completeness, RegexAnalysisCompleteness::Dynamic);
    assert!(dynamic.completeness.has_dynamic_boundary());
    assert!(!dynamic.completeness.is_policy_limited());

    let limited = validator.analyze(r"\p{L}\p{N}");
    assert_eq!(limited.completeness, RegexAnalysisCompleteness::PolicyLimited);
    assert!(!limited.completeness.has_dynamic_boundary());
    assert!(limited.completeness.is_policy_limited());
    assert!(
        limited
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.class != RegexDiagnosticClass::Syntax)
    );
    Ok(())
}

#[test]
fn quantifier_follows_nothing_is_syntax_at_pattern_start_after_pipe_and_after_group_open()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    // (source pattern, offset of the offending quantifier) verified against perl 5.42.
    for (pattern, offset) in [
        ("*abc", 0usize),
        ("+abc", 0),
        ("?abc", 0),
        ("a|*b", 2),
        ("a|?b", 2),
        ("|*b", 1),
        ("(?:*a)", 3),
        ("(?|*a)", 3),
        ("(?=*a)", 3),
        ("(?<n>*a)", 5),
        ("(a|*b)", 3),
        ("(?i:*a)", 4),
        ("(?#c)*a", 5),
        ("(?i)*a", 4),
    ] {
        let analysis = validator.analyze(pattern);
        assert_eq!(
            analysis.facts.quantifier_follows_nothing.len(),
            1,
            "{pattern:?} must report exactly one follows-nothing quantifier"
        );
        assert_eq!(analysis.facts.quantifier_follows_nothing[0].start, offset, "{pattern:?}");
        let diagnostic = analysis
            .diagnostics
            .iter()
            .find(|d| d.code == RegexDiagnosticCode::QuantifierFollowsNothing)
            .ok_or("missing quantifier-follows-nothing diagnostic")?;
        assert_eq!(diagnostic.class, RegexDiagnosticClass::Syntax);
        assert_eq!(diagnostic.code.as_str(), "quantifier_follows_nothing");
        assert_eq!(diagnostic.message(), "Quantifier follows nothing in regex");
        assert_eq!(diagnostic.range.start, offset);
    }
    Ok(())
}

#[test]
fn quantifier_follows_nothing_guards_valid_quantifier_positions()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    // Every pattern below compiles under perl 5.42; none may be flagged.
    for pattern in [
        "abc",
        "a*",
        "a+?",
        "a?+",
        "a*+b",
        "(?:a)*",
        "(a|b)+",
        "[*+?]",
        "[a*]+",
        r"\*abc",
        r"a\*b",
        r"\Q*abc\E",
        "{2}abc",
        "{2,3}abc",
        "a|{2}b",
        "a{2}",
        "a{2,3}?",
        "^*",
        r"\b*a",
        "(*COMMIT)a",
        "(*FAIL)",
        "a**",
        "a*?+",
        "a(?#c)*b",
        r"\Qa\E*",
        "$x*",
        "(?=a)*b",
    ] {
        let analysis = validator.analyze(pattern);
        assert!(
            analysis.facts.quantifier_follows_nothing.is_empty(),
            "{pattern:?} must not be flagged, got {:?}",
            analysis.diagnostics
        );
    }
    Ok(())
}

#[test]
fn quantifier_follows_nothing_finder_projects_to_absolute_offset()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    let finding = validator
        .find_quantifier_follows_nothing("a|*b", 20)
        .ok_or("expected quantifier-follows-nothing finding")?;
    assert_eq!(finding.offset, 22);
    assert_eq!(finding.message, "Quantifier follows nothing in regex");
    assert!(validator.find_quantifier_follows_nothing("a*b", 20).is_none());
    // `validate` surfaces it as a syntax error for compatibility callers.
    let error = validator.validate("*abc", 10).err().ok_or("expected validate error")?;
    match error {
        RegexError::Syntax { message, offset } => {
            assert!(message.contains("Quantifier follows nothing"), "{message}");
            assert_eq!(offset, 10);
        }
    }
    Ok(())
}

#[test]
fn empty_quoted_literal_is_not_a_quantifiable_atom() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    // Verified against perl 5.42: `qr/\Q\E*a/` fails to compile with
    // "Quantifier follows nothing in regex". An empty `\Q\E` quotes zero
    // characters, so the quantifier after it follows nothing.
    let analysis = validator.analyze(r"\Q\E*a");
    assert_eq!(analysis.facts.quantifier_follows_nothing.len(), 1, "{analysis:?}");
    assert_eq!(analysis.facts.quantifier_follows_nothing[0].start, 4);

    // A non-empty quoted literal remains a quantifiable atom.
    let analysis = validator.analyze(r"\Qa\E*");
    assert!(analysis.facts.quantifier_follows_nothing.is_empty(), "{analysis:?}");

    // An empty `\Q\E` preserves the preceding atom: perl accepts
    // `qr/a\Q\E*/` (`*` repeats `a`) and `qr/x\Q\E*y/`.
    for pattern in [r"a\Q\E*", r"x\Q\E*y", r"(?:a\Q\E*)", r"a\Q\E{2}"] {
        let analysis = validator.analyze(pattern);
        assert!(
            analysis.facts.quantifier_follows_nothing.is_empty(),
            "{pattern:?} must not be flagged, got {analysis:?}"
        );
    }
    // At a branch start the empty span creates no atom.
    for (pattern, offset) in [(r"\Q\E*a", 4usize), (r"a|\Q\E*b", 6), (r"(?:\Q\E*a)", 7)] {
        let analysis = validator.analyze(pattern);
        assert_eq!(
            analysis.facts.quantifier_follows_nothing.len(),
            1,
            "{pattern:?} got {analysis:?}"
        );
        assert_eq!(analysis.facts.quantifier_follows_nothing[0].start, offset, "{pattern:?}");
    }
    Ok(())
}

#[test]
fn conditional_condition_closer_does_not_authorize_a_quantifier()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    // Verified against perl 5.42: each rejects with
    // "Quantifier follows nothing in regex" at the marked offset.
    for (pattern, offset) in [
        ("(?(?=a)*b|c)", 7usize),
        ("(?(?!a)*b|c)", 7),
        ("(?(?<=a)*b|c)", 8),
        ("(?(1)*b|c)", 5),
        ("(?(?{1})*b|c)", 8),
        ("(?(?=a)|*b)", 8),
        ("(?(?=a)b|*c)", 9),
    ] {
        let analysis = validator.analyze(pattern);
        assert_eq!(
            analysis.facts.quantifier_follows_nothing.len(),
            1,
            "{pattern:?} must report one finding, got {analysis:?}"
        );
        assert_eq!(analysis.facts.quantifier_follows_nothing[0].start, offset, "{pattern:?}");
    }
    // Branch content and a quantified conditional/lookahead remain valid.
    for pattern in
        ["(?(?=a)b*c)", "(?(?=a)b|c)*d", "(?(?=a)(?:b)*c|d)", "(?(1)b*c)", "(?=a)*b", "((?=a))*b"]
    {
        let analysis = validator.analyze(pattern);
        assert!(
            analysis.facts.quantifier_follows_nothing.is_empty(),
            "{pattern:?} must not be flagged, got {analysis:?}"
        );
    }
    Ok(())
}

#[test]
fn malformed_group_close_does_not_erase_the_preceding_atom()
-> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::new();
    // Verified against perl 5.42: `a)**b` reports only "Unmatched )". The
    // malformed close must not reset the preceding atom, or the quantifiers
    // after it would be spuriously flagged as following nothing.
    let analysis = validator.analyze("a)**b");
    assert!(analysis.facts.quantifier_follows_nothing.is_empty(), "{analysis:?}");
    // The stream still records the pattern as malformed; only the spurious
    // follows-nothing finding is gone.
    assert!(analysis.malformed, "{analysis:?}");
    Ok(())
}

#[test]
fn quantifier_follows_nothing_finder_reports_first_while_facts_keep_all()
-> Result<(), Box<dyn std::error::Error>> {
    // Divergence pin, mirroring the nested-quantifier contract
    // (`find_nested_quantifier` is likewise first-only while
    // `facts.nested_quantifiers` keeps every finding): the typed analysis
    // retains both follows-nothing quantifiers in `*a|*b`, and the
    // compatibility finder projects only the first, exactly as the parser's
    // legacy per-operator scan records one error per regex body.
    let validator = RegexValidator::new();
    let analysis = validator.analyze("*a|*b");
    let starts: Vec<usize> =
        analysis.facts.quantifier_follows_nothing.iter().map(|range| range.start).collect();
    assert_eq!(starts, vec![0, 3]);

    let finding = validator
        .find_quantifier_follows_nothing("*a|*b", 20)
        .ok_or("expected quantifier-follows-nothing finding")?;
    assert_eq!(finding.offset, 20);
    Ok(())
}

#[test]
fn typed_analysis_projects_current_scanner_findings() -> Result<(), Box<dyn std::error::Error>> {
    let validator = RegexValidator::with_config(RegexValidationConfig {
        max_nesting: 10,
        max_unicode_properties: 1,
        max_branch_reset_branches: 50,
    });
    let analysis = validator.analyze(r"\p{L}\p{N}(a+)+(?{ run })");
    let codes = analysis.diagnostics.iter().map(|diagnostic| diagnostic.code).collect::<Vec<_>>();
    assert!(codes.contains(&RegexDiagnosticCode::UnicodePropertyLimit));
    assert!(codes.contains(&RegexDiagnosticCode::NestedQuantifierRisk));
    assert!(codes.contains(&RegexDiagnosticCode::EmbeddedCodeImmediate));
    assert_eq!(analysis.facts.embedded_code.len(), 1);
    assert_eq!(analysis.facts.embedded_code[0].kind, EmbeddedCodeKind::Immediate);
    assert_eq!(analysis.completeness, RegexAnalysisCompleteness::DynamicAndPolicyLimited);
    Ok(())
}
