//! HIR lowering tests for regex/match/substitution/transliteration ops.
//!
//! Pins the item-level HIR shells for `NodeKind::Regex`, `Match`,
//! `Substitution`, and `Transliteration` (issue #2195, HIR Wave 4). Each
//! construct lowers to exactly one typed HIR shell (`RegexExpr`, `MatchExpr`,
//! `SubstitutionExpr`, `TransliterationExpr`); the genuinely-dynamic cases
//! (the `/e` modifier, embedded `(?{...})` code) additionally emit a
//! `DynamicBoundary(DynamicBoundaryKind::EmbeddedRegexCode)` item, mirroring
//! how `Eval`/`Do` emit boundaries for their expression forms.
//!
//! The implementation lives in `crates/perl-parser-core/src/hir/lower.rs`
//! (the `NodeKind::Regex`/`Match`/`Substitution`/`Transliteration` arms).

use perl_parser_core::Parser;
use perl_parser_core::hir::{DynamicBoundaryKind, HirFile, HirItem, HirKind, lower_ast};
use perl_tdd_support::must_some;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn find_dynamic_boundary(file: &HirFile, kind: DynamicBoundaryKind) -> Option<&HirItem> {
    file.items
        .iter()
        .find(|item| matches!(&item.kind, HirKind::DynamicBoundary(b) if b.kind == kind))
}

// ---------------------------------------------------------------------------
// qr// regex literal
// ---------------------------------------------------------------------------

#[test]
fn qr_regex_literal_lowers_to_regex_expr_shell() -> TestResult {
    let file = lower_source("my $re = qr/foo/i;\n");
    let regex = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::RegexExpr(regex) => Some(regex),
        _ => None,
    }));
    assert!(regex.pattern.contains("foo"), "pattern should contain 'foo', got {:?}", regex.pattern);
    assert_eq!(regex.modifiers, "i", "modifiers should be preserved");
    assert!(!regex.has_embedded_code, "plain qr// has no embedded code");
    Ok(())
}

// ---------------------------------------------------------------------------
// Basic match
// ---------------------------------------------------------------------------

#[test]
fn basic_match_lowers_to_match_expr_shell() -> TestResult {
    let file = lower_source("$x =~ /foo/;\n");
    let m = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::MatchExpr(m) => Some(m),
        _ => None,
    }));
    assert!(m.pattern.contains("foo"));
    assert!(!m.negated, "=~ is not negated");
    assert!(!m.has_embedded_code);
    Ok(())
}

// ---------------------------------------------------------------------------
// Negated match (`!~`)
// ---------------------------------------------------------------------------

#[test]
fn negated_match_records_negated_flag() -> TestResult {
    let file = lower_source("$x !~ /foo/;\n");
    let m = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::MatchExpr(m) => Some(m),
        _ => None,
    }));
    assert!(m.negated, "!~ must be recorded as negated");
    Ok(())
}

// ---------------------------------------------------------------------------
// Basic substitution
// ---------------------------------------------------------------------------

#[test]
fn basic_substitution_lowers_to_substitution_expr_shell() -> TestResult {
    let file = lower_source("$x =~ s/foo/bar/;\n");
    let s = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::SubstitutionExpr(s) => Some(s),
        _ => None,
    }));
    assert!(s.pattern.contains("foo"));
    assert!(s.replacement.contains("bar"));
    assert!(!s.negated);
    assert!(!s.has_embedded_code, "plain s/// has no embedded code");
    Ok(())
}

// ---------------------------------------------------------------------------
// Substitution with `/e` modifier -> DynamicBoundary
// ---------------------------------------------------------------------------

#[test]
fn substitution_with_e_modifier_emits_dynamic_boundary() -> TestResult {
    let file = lower_source(r#"$x =~ s/foo/1+1/e;"#);
    let s = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::SubstitutionExpr(s) => Some(s),
        _ => None,
    }));
    assert!(s.modifiers.contains('e'), "modifiers should preserve 'e'");
    assert!(s.has_embedded_code, "/e modifier must set has_embedded_code");

    let boundary = find_dynamic_boundary(&file, DynamicBoundaryKind::EmbeddedRegexCode);
    assert!(
        boundary.is_some(),
        "substitution with /e must emit DynamicBoundary::EmbeddedRegexCode.\nHIR items: {}",
        file.items.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Basic transliteration
// ---------------------------------------------------------------------------

#[test]
fn basic_transliteration_lowers_to_transliteration_expr_shell() -> TestResult {
    let file = lower_source("$x =~ tr/a-z/A-Z/;\n");
    let t = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::TransliterationExpr(t) => Some(t),
        _ => None,
    }));
    assert!(t.search.contains("a-z"));
    assert!(t.replace.contains("A-Z"));
    assert!(!t.negated);
    Ok(())
}

// ---------------------------------------------------------------------------
// Negated / `d`-modified transliteration
// ---------------------------------------------------------------------------

#[test]
fn negated_transliteration_with_d_modifier_records_flags() -> TestResult {
    let file = lower_source("$x !~ tr/a-z//d;\n");
    let t = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::TransliterationExpr(t) => Some(t),
        _ => None,
    }));
    assert!(t.negated, "!~ must be recorded as negated");
    assert!(t.modifiers.contains('d'), "modifiers should preserve 'd'");
    Ok(())
}

// ---------------------------------------------------------------------------
// Embedded-code match `m/(?{...})/` -> DynamicBoundary
// ---------------------------------------------------------------------------

#[test]
fn embedded_code_match_emits_dynamic_boundary() -> TestResult {
    let file = lower_source("$x =~ m/(?{ 1 })/;\n");
    let m = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::MatchExpr(m) => Some(m),
        _ => None,
    }));
    assert!(m.has_embedded_code, "m/(?{{...}})/ must set has_embedded_code");

    let boundary = find_dynamic_boundary(&file, DynamicBoundaryKind::EmbeddedRegexCode);
    assert!(
        boundary.is_some(),
        "embedded-code match must emit DynamicBoundary::EmbeddedRegexCode.\nHIR items: {}",
        file.items.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Modifiers preserved (substitution, multiple modifiers)
// ---------------------------------------------------------------------------

#[test]
fn substitution_modifiers_are_preserved_verbatim() -> TestResult {
    let file = lower_source("$x =~ s/foo/bar/gi;\n");
    let s = must_some(file.items.iter().find_map(|item| match &item.kind {
        HirKind::SubstitutionExpr(s) => Some(s),
        _ => None,
    }));
    assert_eq!(s.modifiers, "gi", "modifiers should be preserved verbatim in parse order");
    Ok(())
}

// ---------------------------------------------------------------------------
// expr operand is traversed: a call inside the bound expression still lowers
// ---------------------------------------------------------------------------

#[test]
fn match_traverses_bound_expr_operand() -> TestResult {
    // The bound `expr` operand (here, the call `foo()`) must still be visited
    // and lowered to its own HIR item — the MatchExpr shell must not swallow
    // it, exactly as Eval/Do traverse their bodies via visit_children.
    let file = lower_source("foo() =~ /bar/;\n");

    let has_match = file.items.iter().any(|item| matches!(&item.kind, HirKind::MatchExpr(_)));
    assert!(has_match, "expected a MatchExpr HIR item");

    let has_call_expr = file.items.iter().any(|item| matches!(&item.kind, HirKind::CallExpr(_)));
    assert!(
        has_call_expr,
        "expected the bound expr operand `foo()` to lower to its own CallExpr item.\n\
         HIR item count: {}",
        file.items.len()
    );
    Ok(())
}

#[test]
fn substitution_traverses_bound_expr_operand() -> TestResult {
    // Guard the `visit_children` traversal in the Substitution arm: the bound
    // `expr` operand (`foo()`) must still lower to its own CallExpr item.
    let file = lower_source("foo() =~ s/a/b/;\n");

    let has_subst =
        file.items.iter().any(|item| matches!(&item.kind, HirKind::SubstitutionExpr(_)));
    assert!(has_subst, "expected a SubstitutionExpr HIR item");

    let has_call_expr = file.items.iter().any(|item| matches!(&item.kind, HirKind::CallExpr(_)));
    assert!(
        has_call_expr,
        "expected the bound expr operand `foo()` to lower to its own CallExpr item.\n\
         HIR item count: {}",
        file.items.len()
    );
    Ok(())
}

#[test]
fn transliteration_traverses_bound_expr_operand() -> TestResult {
    // Guard the `visit_children` traversal in the Transliteration arm.
    let file = lower_source("foo() =~ tr/a/b/;\n");

    let has_translit =
        file.items.iter().any(|item| matches!(&item.kind, HirKind::TransliterationExpr(_)));
    assert!(has_translit, "expected a TransliterationExpr HIR item");

    let has_call_expr = file.items.iter().any(|item| matches!(&item.kind, HirKind::CallExpr(_)));
    assert!(
        has_call_expr,
        "expected the bound expr operand `foo()` to lower to its own CallExpr item.\n\
         HIR item count: {}",
        file.items.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Source range validity
// ---------------------------------------------------------------------------

#[test]
fn regex_op_shells_have_valid_source_ranges() -> TestResult {
    let file = lower_source("$x =~ s/foo/bar/;\n");
    let item =
        must_some(file.items.iter().find(|item| matches!(item.kind, HirKind::SubstitutionExpr(_))));
    assert!(
        item.range.end >= item.range.start,
        "HIR item range must be non-empty and ordered; got {:?}",
        item.range,
    );
    assert_eq!(item.anchor.node_kind, "Substitution", "anchor node_kind should be 'Substitution'");
    Ok(())
}
