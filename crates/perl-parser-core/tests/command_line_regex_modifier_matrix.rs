//! Modifier matrices for the compact regex-family operators used in Perl
//! command-line one-liners.
//!
//! `command_line_oneliners.rs` proves that representative one-liner bodies parse
//! cleanly and land on the intended node kind. A clean parse does not prove that
//! the *modifier payload* survives, that it keeps the author's order, or that it
//! stays bounded to the operator that owns it. This target owns that evidence for
//! `m//`, `qr//`, `s///`, `tr///`, and `y///`.
//!
//! # Boundary
//!
//! This is static parser evidence only. Successful parsing of `/e` and `/ee` is
//! **not** permission to execute the replacement: `has_embedded_code` is recorded
//! precisely so downstream authorities can refuse to evaluate it. Nothing here
//! executes Perl, models shell quoting, or claims regex-runtime support (#2356).
//!
//! Controlling issue: #13663.

use std::error::Error;

use perl_parser_core::{Node, NodeKind, Parser};

type TestResult = Result<(), Box<dyn Error>>;

/// Which regex-family node the parser produced.
///
/// These are distinct `NodeKind` variants, so a mutation that collapses `s///`
/// into a plain regex, or loses the `=~` binding, changes this value.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Family {
    Regex,
    Match,
    Substitution,
    Transliteration,
}

/// One regex-family operator exactly as the parser retained it.
///
/// `span_text` is the source text the node's byte range actually covers, not a
/// substring search. Asserting the whole struct at once means a dropped
/// modifier, a re-ordered modifier list, a span that swallows the trailing `;`,
/// and a payload that leaks into a neighbouring operator are all separate
/// observable failures.
#[derive(Debug, PartialEq, Eq)]
struct OperatorFact {
    family: Family,
    span_text: String,
    modifiers: String,
    /// `pattern` for the match families, `search` for transliteration.
    left: String,
    /// `replacement` for the match families, `replace` for transliteration.
    right: String,
    negated: bool,
    embedded_code: bool,
}

impl OperatorFact {
    fn new(
        family: Family,
        span_text: &str,
        modifiers: &str,
        left: &str,
        right: &str,
        negated: bool,
        embedded_code: bool,
    ) -> Self {
        Self {
            family,
            span_text: span_text.to_string(),
            modifiers: modifiers.to_string(),
            left: left.to_string(),
            right: right.to_string(),
            negated,
            embedded_code,
        }
    }
}

fn collect_operator_facts(
    node: &Node,
    source: &str,
    facts: &mut Vec<OperatorFact>,
) -> Result<(), Box<dyn Error>> {
    let span_text = source
        .get(node.location.start..node.location.end)
        .ok_or_else(|| {
            format!(
                "node {} range {}..{} is not a source boundary in {source:?}",
                node.kind.kind_name(),
                node.location.start,
                node.location.end
            )
        })?
        .to_string();

    // `Regex` retains its pattern with delimiters (`"/needle/"`), while the
    // `s///` and `tr///` families retain the inner text (`"foo"`). That
    // asymmetry is current parser behaviour; pinning it here makes any change
    // to it an explicit decision rather than silent drift.
    match &node.kind {
        NodeKind::Regex { pattern, replacement, modifiers, has_embedded_code } => {
            facts.push(OperatorFact::new(
                Family::Regex,
                &span_text,
                modifiers,
                pattern,
                replacement.as_deref().unwrap_or_default(),
                false,
                *has_embedded_code,
            ));
        }
        NodeKind::Match { pattern, modifiers, has_embedded_code, negated, .. } => {
            facts.push(OperatorFact::new(
                Family::Match,
                &span_text,
                modifiers,
                pattern,
                "",
                *negated,
                *has_embedded_code,
            ));
        }
        NodeKind::Substitution {
            pattern,
            replacement,
            modifiers,
            has_embedded_code,
            negated,
            ..
        } => {
            facts.push(OperatorFact::new(
                Family::Substitution,
                &span_text,
                modifiers,
                pattern,
                replacement,
                *negated,
                *has_embedded_code,
            ));
        }
        NodeKind::Transliteration { search, replace, modifiers, negated, .. } => {
            facts.push(OperatorFact::new(
                Family::Transliteration,
                &span_text,
                modifiers,
                search,
                replace,
                *negated,
                false,
            ));
        }
        _ => {}
    }

    for child in node.children() {
        collect_operator_facts(child, source, facts)?;
    }
    Ok(())
}

/// Parse `source`, require it to be clean, and return every regex-family
/// operator in source order.
fn operator_facts(source: &str) -> Result<Vec<OperatorFact>, Box<dyn Error>> {
    assert!(
        !source.contains('\n') && !source.contains('\r'),
        "one-liner modifier fixture must not contain a line boundary: {source:?}"
    );

    let mut parser = Parser::new(source);
    let ast =
        parser.parse().map_err(|error| format!("clean parse failed for {source:?}: {error:?}"))?;

    let blocking: Vec<_> =
        parser.get_errors().iter().filter(|error| error.blocks_clean_parse()).collect();
    assert!(
        blocking.is_empty(),
        "expected no blocking diagnostics for {source:?}, got {blocking:#?}"
    );

    let mut facts = Vec::new();
    collect_operator_facts(&ast, source, &mut facts)?;
    Ok(facts)
}

/// Assert the operators of `source` are exactly `expected`, in order.
fn assert_operators(source: &str, expected: &[OperatorFact]) -> TestResult {
    let actual = operator_facts(source)?;
    assert_eq!(actual.as_slice(), expected, "operator facts mismatch for {source:?}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Positive matrices
// ---------------------------------------------------------------------------

#[test]
fn match_and_qr_modifier_matrix_retains_ordered_payload_and_operator_range() -> TestResult {
    // Unbound `m//`, bare `/.../`, and `qr//` all land on `NodeKind::Regex`;
    // #14638 owns that they are not distinguishable from one another. This
    // target therefore proves the modifier payload and range, not the operator
    // identity those three share.
    let cases: [(&str, OperatorFact); 7] = [
        (
            r#"print if /needle/i;"#,
            OperatorFact::new(Family::Regex, "/needle/i", "i", "/needle/", "", false, false),
        ),
        (
            r#"print if m{needle}gimsx;"#,
            OperatorFact::new(
                Family::Regex,
                "m{needle}gimsx",
                "gimsx",
                "{needle}",
                "",
                false,
                false,
            ),
        ),
        // Same modifier set as the previous case in a different author order.
        // A mutation that sorts, canonicalises, or de-duplicates the modifier
        // list makes exactly one of these two cases fail.
        (
            r#"print if m!needle!xsmig;"#,
            OperatorFact::new(
                Family::Regex,
                "m!needle!xsmig",
                "xsmig",
                "!needle!",
                "",
                false,
                false,
            ),
        ),
        (
            r#"print while /needle/gc;"#,
            OperatorFact::new(Family::Regex, "/needle/gc", "gc", "/needle/", "", false, false),
        ),
        (
            r#"my $re = qr/needle/ix;"#,
            OperatorFact::new(Family::Regex, "qr/needle/ix", "ix", "/needle/", "", false, false),
        ),
        (
            r#"my $re = qr{needle}xi;"#,
            OperatorFact::new(Family::Regex, "qr{needle}xi", "xi", "{needle}", "", false, false),
        ),
        (
            r#"print if /needle/o;"#,
            OperatorFact::new(Family::Regex, "/needle/o", "o", "/needle/", "", false, false),
        ),
    ];

    for (source, expected) in cases {
        assert_operators(source, &[expected])?;
    }
    Ok(())
}

#[test]
fn bound_match_modifier_matrix_retains_binding_polarity() -> TestResult {
    // `=~` / `!~` produce `NodeKind::Match`, whose range covers the whole
    // binding expression. The modifier payload must still belong to the
    // operator, and `!~` must remain distinguishable from `=~`.
    assert_operators(
        r#"$line =~ /needle/i;"#,
        &[OperatorFact::new(
            Family::Match,
            "$line =~ /needle/i",
            "i",
            "/needle/",
            "",
            false,
            false,
        )],
    )?;
    assert_operators(
        r#"$line !~ m{needle}gi;"#,
        &[OperatorFact::new(
            Family::Match,
            "$line !~ m{needle}gi",
            "gi",
            "{needle}",
            "",
            true,
            false,
        )],
    )?;
    Ok(())
}

#[test]
fn substitution_modifier_matrix_retains_ordered_payload_and_operator_range() -> TestResult {
    let cases: [(&str, OperatorFact); 6] = [
        (
            r#"s/foo/bar/g;"#,
            OperatorFact::new(Family::Substitution, "s/foo/bar/g", "g", "foo", "bar", false, false),
        ),
        (
            r#"s/foo/bar/gi;"#,
            OperatorFact::new(
                Family::Substitution,
                "s/foo/bar/gi",
                "gi",
                "foo",
                "bar",
                false,
                false,
            ),
        ),
        (
            r#"s/foo/bar/r;"#,
            OperatorFact::new(Family::Substitution, "s/foo/bar/r", "r", "foo", "bar", false, false),
        ),
        (
            r#"s{foo}{bar}gx;"#,
            OperatorFact::new(
                Family::Substitution,
                "s{foo}{bar}gx",
                "gx",
                "foo",
                "bar",
                false,
                false,
            ),
        ),
        // Every modifier the parser accepts for `s///`, in one compact operator.
        (
            r#"s/foo/bar/gimsxor;"#,
            OperatorFact::new(
                Family::Substitution,
                "s/foo/bar/gimsxor",
                "gimsxor",
                "foo",
                "bar",
                false,
                false,
            ),
        ),
        (
            r#"$x =~ s/foo/bar/gr;"#,
            OperatorFact::new(
                Family::Substitution,
                "$x =~ s/foo/bar/gr",
                "gr",
                "foo",
                "bar",
                false,
                false,
            ),
        ),
    ];

    for (source, expected) in cases {
        assert_operators(source, &[expected])?;
    }
    Ok(())
}

#[test]
fn substitution_e_and_ee_are_recorded_as_embedded_code_not_permission_to_execute() -> TestResult {
    // `/e` and `/ee` evaluate the replacement as Perl. The parser records that
    // as `has_embedded_code` so downstream authorities can refuse it. This test
    // proves the flag tracks the modifier rather than being constant, which is
    // what makes a "never execute submitted Perl" claim checkable at all.
    assert_operators(
        r#"s/foo/bar/ge;"#,
        &[OperatorFact::new(Family::Substitution, "s/foo/bar/ge", "ge", "foo", "bar", false, true)],
    )?;
    assert_operators(
        r#"s/foo/bar/gee;"#,
        &[OperatorFact::new(
            Family::Substitution,
            "s/foo/bar/gee",
            "gee",
            "foo",
            "bar",
            false,
            true,
        )],
    )?;
    // Negative control: the same operator without `e` must not be flagged.
    assert_operators(
        r#"s/foo/bar/g;"#,
        &[OperatorFact::new(Family::Substitution, "s/foo/bar/g", "g", "foo", "bar", false, false)],
    )?;
    Ok(())
}

#[test]
fn transliteration_modifier_matrix_retains_ordered_payload_and_operator_range() -> TestResult {
    let cases: [(&str, OperatorFact); 6] = [
        (
            r#"tr/a-z/A-Z/;"#,
            OperatorFact::new(
                Family::Transliteration,
                "tr/a-z/A-Z/",
                "",
                "a-z",
                "A-Z",
                false,
                false,
            ),
        ),
        // `tr///d` with an empty replace list is a distinct idiom: the empty
        // `replace` must survive rather than being backfilled from `search`.
        (
            r#"tr/a-z//cd;"#,
            OperatorFact::new(Family::Transliteration, "tr/a-z//cd", "cd", "a-z", "", false, false),
        ),
        (
            r#"tr/a-z/A-Z/r;"#,
            OperatorFact::new(
                Family::Transliteration,
                "tr/a-z/A-Z/r",
                "r",
                "a-z",
                "A-Z",
                false,
                false,
            ),
        ),
        (
            r#"y{abc}{xyz}s;"#,
            OperatorFact::new(
                Family::Transliteration,
                "y{abc}{xyz}s",
                "s",
                "abc",
                "xyz",
                false,
                false,
            ),
        ),
        (
            r#"tr/a-z/A-Z/cds;"#,
            OperatorFact::new(
                Family::Transliteration,
                "tr/a-z/A-Z/cds",
                "cds",
                "a-z",
                "A-Z",
                false,
                false,
            ),
        ),
        (
            r#"$x =~ tr/a-z/A-Z/cdsr;"#,
            OperatorFact::new(
                Family::Transliteration,
                "$x =~ tr/a-z/A-Z/cdsr",
                "cdsr",
                "a-z",
                "A-Z",
                false,
                false,
            ),
        ),
    ];

    for (source, expected) in cases {
        assert_operators(source, &[expected])?;
    }
    Ok(())
}

#[test]
fn adjacent_operators_keep_separate_modifier_payloads() -> TestResult {
    // Compact one-liners chain operators against statement modifiers and
    // punctuation. If modifier scanning ever runs past its own closing
    // delimiter, the neighbouring operator absorbs it and these expectations
    // stop matching.
    assert_operators(
        r#"s/foo/bar/gr if /needle/i;"#,
        &[
            OperatorFact::new(
                Family::Substitution,
                "s/foo/bar/gr",
                "gr",
                "foo",
                "bar",
                false,
                false,
            ),
            OperatorFact::new(Family::Regex, "/needle/i", "i", "/needle/", "", false, false),
        ],
    )?;
    assert_operators(
        r#"print(/alpha/x), print(/beta/i);"#,
        &[
            OperatorFact::new(Family::Regex, "/alpha/x", "x", "/alpha/", "", false, false),
            OperatorFact::new(Family::Regex, "/beta/i", "i", "/beta/", "", false, false),
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls
// ---------------------------------------------------------------------------

/// Assert that `source` rejects an invalid modifier and that the diagnostic
/// points inside the operator that owns it.
///
/// Each fixture places the operator away from offset 0 on purpose: a mutation
/// that reports every modifier error at the start of the statement, the start
/// of the file, or the start of the binding expression fails the range check
/// rather than passing by coincidence.
fn assert_invalid_modifier_is_diagnosed_within_operator(
    source: &str,
    operator: &str,
    needle: &str,
) -> TestResult {
    let operator_start =
        source.find(operator).ok_or_else(|| format!("fixture {operator:?} not in {source:?}"))?;
    let operator_end = operator_start + operator.len();
    assert!(operator_start > 0, "negative control must not place the operator at offset 0");

    let mut parser = Parser::new(source);
    let _ast = parser.parse().map_err(|error| format!("parse of {source:?} failed: {error:?}"))?;

    let errors = parser.get_errors();
    assert!(
        errors.iter().any(|error| error.blocks_clean_parse()),
        "invalid modifier in {source:?} must block a clean parse; got {errors:#?}"
    );

    let rendered = format!("{errors:?}");
    assert!(
        rendered.contains(needle),
        "expected a diagnostic naming {needle:?} for {source:?}; got {errors:#?}"
    );

    let located = errors.iter().any(|error| {
        let rendered = format!("{error:?}");
        rendered.contains(needle)
            && rendered
                .split("location: ")
                .nth(1)
                .and_then(|tail| tail.trim_end_matches([' ', '}']).parse::<usize>().ok())
                .is_some_and(|location| (operator_start..operator_end).contains(&location))
    });
    assert!(
        located,
        "diagnostic for {needle:?} must fall inside the operator range \
         {operator_start}..{operator_end} of {source:?}; got {errors:#?}"
    );
    Ok(())
}

#[test]
fn invalid_substitution_modifiers_diagnose_inside_the_operator() -> TestResult {
    assert_invalid_modifier_is_diagnosed_within_operator(
        r#"print "x"; s/foo/bar/q;"#,
        "s/foo/bar/q",
        "Invalid substitution modifier 'q'",
    )?;
    assert_invalid_modifier_is_diagnosed_within_operator(
        r#"$x =~ s/foo/bar/q;"#,
        "s/foo/bar/q",
        "Invalid substitution modifier 'q'",
    )?;
    Ok(())
}

#[test]
fn invalid_transliteration_modifiers_diagnose_inside_the_operator() -> TestResult {
    assert_invalid_modifier_is_diagnosed_within_operator(
        r#"print "xxxxxxxxxx"; tr/a-z/A-Z/x;"#,
        "tr/a-z/A-Z/x",
        "Invalid transliteration modifier 'x'",
    )?;
    assert_invalid_modifier_is_diagnosed_within_operator(
        r#"print "x"; y/abc/xyz/q;"#,
        "y/abc/xyz/q",
        "Invalid transliteration modifier 'q'",
    )?;
    Ok(())
}

#[test]
fn match_family_modifiers_are_currently_unvalidated() -> TestResult {
    // Current-behaviour control, not an endorsement. `s///` and `tr///` reject
    // an unknown modifier letter; `m//`, bare `/.../`, and `qr//` accept any
    // ASCII letter and keep it in the payload. `z` and `q` are not Perl match
    // modifiers, so this asymmetry is a real gap, tracked by #14980.
    //
    // Pinning it here means the gap is visible in the proof surface rather than
    // hidden behind an absent test, and whoever closes #14980 gets a failure
    // here telling them to update this expectation.
    for (source, expected) in [
        (
            r#"print if /needle/z;"#,
            OperatorFact::new(Family::Regex, "/needle/z", "z", "/needle/", "", false, false),
        ),
        (
            r#"my $re = qr/needle/q;"#,
            OperatorFact::new(Family::Regex, "qr/needle/q", "q", "/needle/", "", false, false),
        ),
    ] {
        assert_operators(source, &[expected])?;
    }
    Ok(())
}

#[test]
fn valid_modifiers_are_not_diagnosed_as_invalid() -> TestResult {
    // Negative control for the two tests above: the rejection must key on the
    // modifier letter, not on the operator being present at all. A mutation
    // that rejects every `s///` or `tr///` fails here.
    for source in [
        r#"print "x"; s/foo/bar/gimsxor;"#,
        r#"print "x"; tr/a-z/A-Z/cdsr;"#,
        r#"print "x"; y/abc/xyz/cd;"#,
    ] {
        let mut parser = Parser::new(source);
        let _ast =
            parser.parse().map_err(|error| format!("parse of {source:?} failed: {error:?}"))?;
        let blocking: Vec<_> =
            parser.get_errors().iter().filter(|error| error.blocks_clean_parse()).collect();
        assert!(blocking.is_empty(), "{source:?} must parse cleanly; got {blocking:#?}");
    }
    Ok(())
}
