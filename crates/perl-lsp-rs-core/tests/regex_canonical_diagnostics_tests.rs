//! Canonical regex diagnostics published by the provider (#7024).
//!
//! These fixtures exercise the public provider surface, not the projection helper,
//! so they fail if the wiring is removed as readily as if the mapping is wrong.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_parser::Parser;
use perl_parser_core::{RegexAnalysisTable, RetainedRegexSession};

/// One finding of each class the projection can reach from parser-retained analysis:
/// backtracking risk, embedded execution, an invalid modifier, and a clean
/// substitution that must stay silent.
const MIXED: &str = r#"my $re = qr/(a+)+b/;
my $x = /(?{ print 1 })/;
if ($s =~ m/foo/zz) { }
my $y = $s =~ s/(x)/y/gr;
"#;

/// Parse the way the server does — driving `parse()` inside a retention session —
/// and return both planes.
fn parse_with_retention(
    source: &str,
) -> (Arc<perl_parser_core::Node>, Vec<perl_parser_core::error::ParseError>, Arc<RegexAnalysisTable>)
{
    let session = RetainedRegexSession::begin(source);
    let mut parser = Parser::new(source);
    match parser.parse() {
        Ok(mut ast) => {
            let table = session.finish(Some(&mut ast));
            let errors = parser.errors().to_vec();
            (Arc::new(ast), errors, Arc::new(table))
        }
        Err(error) => {
            let table = session.finish(None);
            (
                Arc::new(perl_parser_core::Node::new(
                    perl_parser_core::ast::NodeKind::Program { statements: vec![] },
                    perl_parser_core::ast::SourceLocation { start: 0, end: 0 },
                )),
                vec![error],
                Arc::new(table),
            )
        }
    }
}

/// Diagnostics as a caller holding a retained table sees them: the parse runs inside
/// a session, and the resulting table is handed to the provider.
///
/// Pairs with [`compatibility_diagnostics`]; the two must not share a parse helper.
fn canonical_diagnostics(source: &str) -> Vec<Diagnostic> {
    let (ast, errors, table) = parse_with_retention(source);
    DiagnosticsProvider::new()
        .with_regex_analysis(table)
        .get_diagnostics(&ast, &errors, source, None)
}

/// Parse the way a caller with no retained analysis does: no session, so the
/// parser's legacy per-operator scan runs and its findings arrive as parse errors.
///
/// This must not go through `parse_with_retention`. A session suppresses the legacy
/// scan, so reusing it here would compare the canonical path against itself and the
/// "unchanged compatibility behavior" claim would be vacuous.
fn parse_without_retention(
    source: &str,
) -> (Arc<perl_parser_core::Node>, Vec<perl_parser_core::error::ParseError>) {
    let output = Parser::new(source).parse_with_recovery();
    (Arc::new(output.ast), output.diagnostics)
}

/// Diagnostics as a caller with no retained table sees them, which is what the
/// "compatibility path is unchanged" assertions compare against.
fn compatibility_diagnostics(source: &str) -> Vec<Diagnostic> {
    let (ast, errors) = parse_without_retention(source);
    DiagnosticsProvider::new().get_diagnostics(&ast, &errors, source, None)
}

/// Every diagnostic carrying exactly `code`, matched in full rather than by prefix.
fn with_code<'a>(diagnostics: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    diagnostics.iter().filter(|&diagnostic| diagnostic.code.as_deref() == Some(code)).collect()
}

/// Every identity the canonical projection can publish.
///
/// Membership is exact, never a prefix test: `PL1000` and `PL100` are different
/// codes in different categories, and `starts_with` would conflate them.
const CANONICAL_REGEX_CODES: [&str; 9] =
    ["PL609", "PL1000", "PL1001", "PL1002", "PL1003", "PL1004", "PL1005", "PL1006", "PL1007"];

/// Whether `code` is one the canonical projection can publish, by exact membership.
fn is_canonical_regex_code(code: &str) -> bool {
    CANONICAL_REGEX_CODES.contains(&code)
}

/// The correctness core. The retained record carries findings in more than one
/// coordinate space — structural and capture findings are pattern-body relative,
/// modifier findings are not. Anchoring each published range against the exact bytes
/// it names is what catches a projection that forgets to map, or that maps twice.
///
/// This fixture covers the structural and modifier spaces. Captures need source
/// *before* the pattern to be discriminating, so they are pinned separately by
/// [`a_capture_finding_names_the_capture_and_not_earlier_source`].
#[test]
fn regex_canonical_range_spaces_are_pinned_to_source_text() {
    let diagnostics = canonical_diagnostics(MIXED);

    // Structural finding: body-relative, must be mapped back to original source.
    let risk = with_code(&diagnostics, "PL1000");
    assert_eq!(risk.len(), 1, "exactly one backtracking risk: {diagnostics:#?}");
    let (start, end) = risk[0].range;
    assert_eq!(
        &MIXED[start..end],
        "+",
        "PL1000 must name the repeated group's quantifier in original source, got {:?}",
        &MIXED[start..end]
    );

    // Modifier finding: already in original-source coordinates.
    let modifiers = with_code(&diagnostics, "PL1002");
    assert_eq!(modifiers.len(), 2, "both stray modifier characters are reported");
    for diagnostic in &modifiers {
        let (start, end) = diagnostic.range;
        assert_eq!(
            &MIXED[start..end],
            "z",
            "PL1002 must name the offending modifier character itself"
        );
    }

    // Embedded code keeps its established PL609 identity, with the exact block span.
    let embedded = with_code(&diagnostics, "PL609");
    assert_eq!(embedded.len(), 1, "embedded code published exactly once: {diagnostics:#?}");
    let (start, end) = embedded[0].range;
    assert!(
        MIXED[start..end].starts_with("(?{"),
        "PL609 must name the embedded code block, got {:?}",
        &MIXED[start..end]
    );
}

/// Capture findings are body-relative too, and this is the fixture that proves it.
///
/// `MIXED` cannot: its first pattern starts at byte 11, so a body-relative range
/// published raw still lands inside the same statement and looks plausible. Here the
/// capture name sits at body offset 3 while the pattern begins far into the file, so
/// an unmapped range names bytes in the padding instead — which is exactly what this
/// projection shipped until the range space was measured rather than assumed.
#[test]
fn a_capture_finding_names_the_capture_and_not_earlier_source() {
    const PADDED: &str =
        "my $padding_variable = 1;\nmy $another_padding = 2;\nmy $re = qr/(?<9bad>x)/;\n";

    let diagnostics = canonical_diagnostics(PADDED);
    let invalid = with_code(&diagnostics, "PL1005");
    assert_eq!(invalid.len(), 1, "the malformed capture name is reported once: {diagnostics:#?}");

    let (start, end) = invalid[0].range;
    assert_eq!(
        &PADDED[start..end],
        "9bad",
        "PL1005 must name the capture itself; an unmapped body-relative range would \
         instead name {:?} near the start of the file",
        &PADDED[start..end]
    );
    // Anchor the discrimination: the finding must sit inside the pattern, not before it.
    let pattern_start = PADDED.find("(?<9bad>").unwrap_or(0);
    assert!(
        start >= pattern_start,
        "the published span must fall inside the pattern body, not in the padding"
    );
}

/// The whole point of the change: before it, the embedded-code finding covered the
/// entire pattern node. Pinning the improvement stops a silent regression back to
/// the coarse range.
#[test]
fn canonical_embedded_code_range_is_narrower_than_the_compatibility_range() {
    let source = r#"my $x = /(?{ print 1 })/;"#;
    let canonical_all = canonical_diagnostics(source);
    let canonical_published = with_code(&canonical_all, "PL609");
    assert!(
        !canonical_published.is_empty(),
        "the canonical path must publish PL609: {canonical_all:#?}"
    );
    let Some(canonical) = canonical_published.first().map(|diagnostic| diagnostic.range) else {
        return;
    };

    let compatibility_all = compatibility_diagnostics(source);
    let compatibility_published = with_code(&compatibility_all, "PL609");
    assert!(
        !compatibility_published.is_empty(),
        "the compatibility path must publish PL609: {compatibility_all:#?}"
    );
    let Some(compatibility) = compatibility_published.first().map(|diagnostic| diagnostic.range)
    else {
        return;
    };

    assert!(
        compatibility.0 <= canonical.0 && canonical.1 <= compatibility.1,
        "the canonical span must sit inside the node span: {canonical:?} vs {compatibility:?}"
    );
    assert!(
        canonical.1 - canonical.0 < compatibility.1 - compatibility.0,
        "the canonical span must be strictly narrower: {canonical:?} vs {compatibility:?}"
    );
}

/// An analyzer limit reaches the client as `PL1001`, on the construct that hit it.
///
/// Also the negative control for `PL1007`: a *limit* and an *exhaustion* are
/// different events. A limit says "I saw this and it is over the threshold";
/// exhaustion says "I stopped before the end of the evidence". Publishing both for
/// one limit would be a duplicate notice, so this asserts the limit alone.
#[test]
fn an_analyzer_limit_is_published_once_and_is_not_also_an_incompleteness() {
    let source = format!("my $re = qr/{}/;\n", "\\p{L}".repeat(60));

    let diagnostics = canonical_diagnostics(&source);
    assert_eq!(
        with_code(&diagnostics, "PL1001").len(),
        1,
        "exceeding the unicode-property budget publishes exactly one limit: {diagnostics:#?}"
    );
    assert!(
        with_code(&diagnostics, "PL1007").is_empty(),
        "a limit is not an incomplete analysis; publishing both is a duplicate notice"
    );
}

/// Both paths run in the same pass. Publishing the same execution risk under one
/// code twice is the failure this pins.
#[test]
fn embedded_code_is_published_exactly_once_when_both_paths_could_fire() {
    let diagnostics = canonical_diagnostics(MIXED);
    assert_eq!(
        with_code(&diagnostics, "PL609").len(),
        1,
        "PL609 must not be published by both the canonical projection and the AST-flag lint"
    );
}

/// Negative control for the suppression above: it must be span-matched, not blanket.
/// With no canonical finding to replace it, the compatibility finding survives —
/// otherwise the change would trade a coarse security diagnostic for none at all.
#[test]
fn compatibility_embedded_code_survives_without_a_canonical_finding() {
    let source = r#"my $x = /(?{ print 1 })/;"#;
    let (ast, errors, _table) = parse_with_retention(source);
    // The AST flag is set by the retention session from the canonical analysis, so
    // the compatibility emitter still has something to report; what is missing is a
    // canonical *finding* to supersede it.

    // An empty table is the "canonical analysis produced no record" case.
    let empty = Arc::new(RegexAnalysisTable::for_source(source));
    let diagnostics = DiagnosticsProvider::new()
        .with_regex_analysis(empty)
        .get_diagnostics(&ast, &errors, source, None);

    assert_eq!(
        with_code(&diagnostics, "PL609").len(),
        1,
        "a record-less table must not suppress the compatibility finding: {diagnostics:#?}"
    );
}

/// Classes stay distinct: a risk advisory, an execution boundary, and a modifier
/// error must not collapse into one identity or one severity.
#[test]
fn each_canonical_class_keeps_a_distinct_code_and_catalog_severity() {
    use perl_lsp_rs_core::providers::diagnostics::DiagnosticSeverity;

    let diagnostics = canonical_diagnostics(MIXED);

    assert_eq!(with_code(&diagnostics, "PL1000")[0].severity, DiagnosticSeverity::Warning);
    assert_eq!(with_code(&diagnostics, "PL1002")[0].severity, DiagnosticSeverity::Error);
    assert_eq!(with_code(&diagnostics, "PL609")[0].severity, DiagnosticSeverity::Warning);
}

/// A table retained for different source must not be believed. This is the
/// generation guard at the provider boundary: the ranges in a stale table are
/// meaningless against the current text, so the provider declines it.
#[test]
fn a_table_retained_for_other_source_is_declined() {
    let (ast, errors, table) = parse_with_retention(MIXED);
    let edited = MIXED.replace("(a+)+b", "plain");

    let (edited_ast, edited_errors, _) = parse_with_retention(&edited);
    let diagnostics = DiagnosticsProvider::new().with_regex_analysis(table).get_diagnostics(
        &edited_ast,
        &edited_errors,
        &edited,
        None,
    );

    assert!(
        with_code(&diagnostics, "PL1000").is_empty(),
        "a stale table must not publish a finding the current source no longer has: {diagnostics:#?}"
    );

    // Control: the same table against its own source does publish it, so the
    // assertion above is about staleness and not about the corpus.
    let fresh = DiagnosticsProvider::new()
        .with_regex_analysis(parse_with_retention(MIXED).2)
        .get_diagnostics(&ast, &errors, MIXED, None);
    assert_eq!(with_code(&fresh, "PL1000").len(), 1);
}

/// Without a table the provider behaves exactly as it did before: the AST-flag lint
/// publishes PL609 and no canonical code appears.
#[test]
fn the_compatibility_path_is_unchanged_without_a_table() {
    let diagnostics = compatibility_diagnostics(r#"my $x = /(?{ print 1 })/;"#);

    assert_eq!(with_code(&diagnostics, "PL609").len(), 1, "AST-flag lint still publishes PL609");
    for code in CANONICAL_REGEX_CODES.iter().filter(|code| **code != "PL609") {
        assert!(
            with_code(&diagnostics, code).is_empty(),
            "no canonical code appears without a retained table, found {code}"
        );
    }
}

/// The user-visible identity change: a backtracking risk stops being reported under
/// the generic parse-error code and gets its own.
///
/// The legacy route turned the parser's advisory into a diagnostic by matching its
/// message text; no pattern matched, so it fell back to `PL001` — the same code a
/// syntax error gets. The finding was real and correctly placed, but its identity
/// carried no information, so it could not be filtered, documented, or suppressed
/// as a distinct class.
///
/// Both routes name the same bytes. Only the identity improves.
#[test]
fn a_backtracking_risk_moves_off_the_generic_parse_error_code() {
    let source = "my $re = qr/(a+)+b/;\nmy $y = $re;\n";

    let legacy = compatibility_diagnostics(source);
    let legacy_generic = with_code(&legacy, "PL001");
    assert_eq!(
        legacy_generic.len(),
        1,
        "legacy behavior being pinned: the advisory arrives as generic PL001: {legacy:#?}"
    );
    assert!(
        with_code(&legacy, "PL1000").is_empty(),
        "and carries no canonical identity without a retained table"
    );

    let canonical = canonical_diagnostics(source);
    let canonical_risk = with_code(&canonical, "PL1000");
    assert_eq!(canonical_risk.len(), 1, "canonically it is PL1000: {canonical:#?}");
    assert!(
        with_code(&canonical, "PL001").is_empty(),
        "and is not also published under the generic code"
    );
    assert_eq!(
        canonical_risk[0].range, legacy_generic[0].range,
        "the finding names the same bytes either way; only its identity changed"
    );
}

/// An unknown regex modifier reached the client as nothing at all: the legacy
/// per-operator scan does not inspect modifiers, so `m/foo/zz` published no
/// diagnostic. Canonical analysis reports each offending character.
///
/// This direction is purely additive — no previously published finding is replaced —
/// and the negative half is what proves it.
#[test]
fn an_unknown_modifier_is_reported_where_nothing_was_reported_before() {
    let source = "my $x = 1;\nif ($s =~ m/foo/zz) { }\n";

    let legacy = compatibility_diagnostics(source);
    for code in CANONICAL_REGEX_CODES {
        assert!(
            with_code(&legacy, code).is_empty(),
            "legacy behavior being pinned: no regex diagnostic at all, found {code}: {legacy:#?}"
        );
    }
    assert!(
        with_code(&legacy, "PL001").is_empty(),
        "and no generic parse-error diagnostic either: {legacy:#?}"
    );

    let canonical = canonical_diagnostics(source);
    assert_eq!(
        with_code(&canonical, "PL1002").len(),
        2,
        "each unknown modifier character is reported: {canonical:#?}"
    );

    // Unrelated diagnostics are untouched in both directions — this change adds a
    // finding, it does not gate or suppress anything else.
    assert_eq!(with_code(&legacy, "PL100").len(), with_code(&canonical, "PL100").len());
    assert_eq!(with_code(&legacy, "PL102").len(), with_code(&canonical, "PL102").len());
}

/// A clean pattern publishes nothing. Without this, every assertion above could be
/// satisfied by a projection that reports on all regexes.
#[test]
fn a_clean_pattern_publishes_no_regex_diagnostic() {
    let diagnostics = canonical_diagnostics("my $ok = $s =~ m/^[a-z]+\\d*$/;\n");
    let regex_codes: Vec<_> = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .filter(|&code| is_canonical_regex_code(code))
        .collect();
    assert!(regex_codes.is_empty(), "clean pattern must stay silent, got {regex_codes:?}");
}

/// Transliteration shares the operator family but has no regex body. It must not
/// acquire pattern findings by being retained alongside real patterns.
#[test]
fn transliteration_publishes_no_pattern_finding() {
    let diagnostics = canonical_diagnostics("my $n = ($s =~ tr/a-z/A-Z/);\n");
    let regex_codes: Vec<_> = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .filter(|&code| is_canonical_regex_code(code))
        .collect();
    assert!(regex_codes.is_empty(), "transliteration must publish nothing, got {regex_codes:?}");
}

/// Findings arrive in deterministic source order so a client rendering them in
/// arrival order is stable across runs.
#[test]
fn canonical_findings_are_emitted_in_source_order() {
    let diagnostics = canonical_diagnostics(MIXED);
    let canonical: Vec<_> = diagnostics
        .iter()
        .filter(|&diagnostic| diagnostic.code.as_deref().is_some_and(is_canonical_regex_code))
        .map(|diagnostic| diagnostic.range.0)
        .collect();

    assert!(canonical.len() >= 4, "corpus must produce several findings: {canonical:?}");
    let mut sorted = canonical.clone();
    sorted.sort_unstable();
    assert_eq!(canonical, sorted, "canonical findings must be in source order");
}
