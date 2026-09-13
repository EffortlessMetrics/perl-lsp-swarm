//! Canonical body-HIR regex-family representation (#7136).
//!
//! Before this slice, all four regex families degraded in canonical body
//! lowering: `qr//` and bare `/.../` became `HirExpr::Opaque { "Regex" }`, and
//! match/substitution/transliteration became `HirExpr::Call` with the
//! embedded-code fact mangled into the `ast_kind` string. That fallback erased
//! negation, modifiers, `/r` mutation mode and (for the unbound form) embedded
//! code, making materially different Perl programs produce byte-identical HIR.
//!
//! Every test here is written as a *discriminating* proof: it pins a fact that
//! the previous representation could not carry. The `distinguishes_*` tests are
//! the negative controls — they compare two programs that differ only in the
//! fact under test and assert the bodies differ, so a regression that drops the
//! fact fails loudly instead of silently collapsing the two programs again.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    HIR_BODY_MODEL_VERSION, HirExpr, HirExprId, HirRegex, HirRegexMatch, HirRegexTarget,
    HirSubstitution, HirTransliteration, RegexTargetKind, ReplacementEvaluation, lower_ast,
};
use perl_tdd_support::must_some_with;

/// Lower one source string and return every expression in its bodies.
fn body_exprs(source: &str) -> Vec<HirExpr> {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    hir.bodies.iter().flat_map(|b| b.exprs.iter().cloned()).collect()
}

fn is_regex_family(expr: &HirExpr) -> bool {
    matches!(
        expr,
        HirExpr::Regex(_)
            | HirExpr::Match(_)
            | HirExpr::Substitution(_)
            | HirExpr::Transliteration(_)
    )
}

/// Return the single regex-family expression in `source`.
fn sole_regex_expr(source: &str) -> HirExpr {
    let found: Vec<HirExpr> = body_exprs(source).into_iter().filter(is_regex_family).collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one regex-family expression for {source:?}, got {found:?}"
    );
    must_some_with(found.into_iter().next(), format!("no regex-family expression in {source:?}"))
}

// ── Typed extractors ─────────────────────────────────────────────────────────
//
// These keep the assertions free of `panic!`, which the repository's lint
// ratchet denies on all targets.

fn as_regex(expr: &HirExpr) -> Option<&HirRegex> {
    match expr {
        HirExpr::Regex(r) => Some(r),
        _ => None,
    }
}

fn as_match(expr: &HirExpr) -> Option<&HirRegexMatch> {
    match expr {
        HirExpr::Match(m) => Some(m),
        _ => None,
    }
}

fn as_substitution(expr: &HirExpr) -> Option<&HirSubstitution> {
    match expr {
        HirExpr::Substitution(s) => Some(s),
        _ => None,
    }
}

fn as_transliteration(expr: &HirExpr) -> Option<&HirTransliteration> {
    match expr {
        HirExpr::Transliteration(t) => Some(t),
        _ => None,
    }
}

/// Destructure an explicitly bound target.
fn as_bound(target: &HirRegexTarget) -> Option<(HirExprId, RegexTargetKind, &'static str)> {
    match target {
        HirRegexTarget::Bound { expr, kind, ast_kind } => Some((*expr, *kind, ast_kind)),
        // `HirRegexTarget` is `#[non_exhaustive]`, so an out-of-crate consumer
        // must keep a fallback arm.
        _ => None,
    }
}

fn regex_of(source: &str) -> HirRegex {
    let expr = sole_regex_expr(source);
    must_some_with(as_regex(&expr).cloned(), format!("{source:?} must lower to HirExpr::Regex"))
}

fn match_of(source: &str) -> HirRegexMatch {
    let expr = sole_regex_expr(source);
    must_some_with(as_match(&expr).cloned(), format!("{source:?} must lower to HirExpr::Match"))
}

fn substitution_of(source: &str) -> HirSubstitution {
    let expr = sole_regex_expr(source);
    must_some_with(
        as_substitution(&expr).cloned(),
        format!("{source:?} must lower to HirExpr::Substitution"),
    )
}

fn transliteration_of(source: &str) -> HirTransliteration {
    let expr = sole_regex_expr(source);
    must_some_with(
        as_transliteration(&expr).cloned(),
        format!("{source:?} must lower to HirExpr::Transliteration"),
    )
}

fn bound_of(target: &HirRegexTarget, context: &str) -> (HirExprId, RegexTargetKind, &'static str) {
    must_some_with(as_bound(target), format!("{context} must bind an explicit target"))
}

// ── Family representation ────────────────────────────────────────────────────

#[test]
fn qr_literal_lowers_to_a_regex_form_carrying_its_modifiers() {
    let r = regex_of("my $r = qr/foo/i;");
    assert_eq!(r.modifiers, "i", "raw modifiers must survive lowering");
    assert!(!r.embedded_code);
}

#[test]
fn bound_match_lowers_to_a_match_form_with_a_place_target() {
    let m = match_of("$x =~ /foo/;");
    assert!(!m.negated);
    let (_, kind, ast_kind) = bound_of(&m.target, "`$x =~ /foo/`");
    assert_eq!(kind, RegexTargetKind::Place);
    assert_eq!(ast_kind, "Variable");
}

#[test]
fn substitution_lowers_to_a_substitution_form() {
    let s = substitution_of("$x =~ s/a/b/g;");
    assert_eq!(s.modifiers, "g");
    assert_eq!(s.replacement, ReplacementEvaluation::Literal);
    assert!(s.mutates_target(), "s///g without /r writes back to its target");
}

#[test]
fn transliteration_lowers_to_its_own_form_and_carries_no_regex_analysis_anchor() {
    // tr/// is a character-list operator, not a regex. Its distinct form is the
    // structural guarantee that it can never be routed through pattern
    // analysis: there is no analysis anchor on the variant to route.
    let t = transliteration_of("$x =~ tr/a-z/A-Z/cds;");
    assert_eq!(t.modifiers, "cds");
    assert!(t.mutates_target());
}

#[test]
fn regex_operations_are_not_modeled_as_calls() {
    // The previous representation booked every bound regex operation as
    // `HirExpr::Call`, so any consumer counting calls counted regex operations.
    for source in ["$x =~ /foo/;", "$x =~ s/a/b/;", "$x =~ tr/a/b/;"] {
        let calls = body_exprs(source)
            .into_iter()
            .filter(|e| matches!(e, HirExpr::Call { ast_kind, .. } if ast_kind != "FunctionCall"))
            .count();
        assert_eq!(calls, 0, "regex operations must not be modeled as calls in {source:?}");
    }
}

// ── Negative controls: facts the old representation collapsed ────────────────

#[test]
fn distinguishes_negated_from_plain_match() {
    // `=~` and `!~` previously produced byte-identical canonical bodies.
    assert_ne!(
        body_exprs("$x =~ /foo/;"),
        body_exprs("$x !~ /foo/;"),
        "`=~` and `!~` must not produce identical HIR"
    );
    assert!(match_of("$x !~ /foo/;").negated, "`!~` must set negated");
}

#[test]
fn distinguishes_negated_substitution_and_transliteration() {
    assert_ne!(body_exprs("$x =~ s/a/b/;"), body_exprs("$x !~ s/a/b/;"));
    assert_ne!(body_exprs("$x =~ tr/a/b/;"), body_exprs("$x !~ tr/a/b/;"));
}

#[test]
fn distinguishes_embedded_code_in_an_unbound_regex() {
    // The old arm discarded `has_embedded_code` for the unbound form outright,
    // so a code-execution site was indistinguishable from a plain pattern —
    // despite a comment claiming the fact was preserved for effect analysis.
    assert_ne!(
        body_exprs("my $r = qr/foo/;"),
        body_exprs("my $r = qr/(?{ die })/;"),
        "an embedded-code regex must not look like a plain one"
    );
    assert!(regex_of("my $r = qr/(?{ die })/;").embedded_code);
}

#[test]
fn distinguishes_mutating_substitution_from_non_destructive_r() {
    // `/r` returns a modified copy and leaves the target untouched. Both forms
    // previously produced identical HIR, making mutation unrepresentable.
    assert_ne!(
        body_exprs("$x =~ s/a/b/;"),
        body_exprs("$x =~ s/a/b/r;"),
        "`/r` must not look like a mutating substitution"
    );
    assert!(
        !substitution_of("$x =~ s/a/b/r;").mutates_target(),
        "`/r` must not write back to its target"
    );
}

#[test]
fn distinguishes_non_destructive_transliteration() {
    assert_ne!(body_exprs("$x =~ tr/a/b/;"), body_exprs("$x =~ tr/a/b/r;"));
    assert!(!transliteration_of("$x =~ tr/a/b/r;").mutates_target());
}

#[test]
fn distinguishes_modifier_sets() {
    assert_ne!(body_exprs("$x =~ /foo/i;"), body_exprs("$x =~ /foo/g;"));
    assert_ne!(body_exprs("my $r = qr/foo/i;"), body_exprs("my $r = qr/foo/x;"));
}

// ── Replacement evaluation boundaries ────────────────────────────────────────

#[test]
fn classifies_e_and_ee_replacement_evaluation() {
    // Perl escalates on the *count* of `e`: one evaluates the replacement as
    // code, two evaluate the result again.
    let cases = [
        ("$x =~ s/a/b/;", ReplacementEvaluation::Literal, false),
        ("$x =~ s/a/$c/e;", ReplacementEvaluation::Expression, true),
        ("$x =~ s/a/$c/ee;", ReplacementEvaluation::DoubleEval, true),
    ];
    for (source, expected, dynamic) in cases {
        let s = substitution_of(source);
        assert_eq!(s.replacement, expected, "wrong replacement class for {source:?}");
        assert_eq!(s.replacement.is_dynamic(), dynamic);
    }
}

#[test]
fn e_and_ee_are_distinct_dynamic_boundaries() {
    assert_ne!(
        body_exprs("$x =~ s/a/$c/e;"),
        body_exprs("$x =~ s/a/$c/ee;"),
        "`/e` and `/ee` are different evaluation boundaries"
    );
}

#[test]
fn replacement_evaluation_classifies_from_modifier_letter_count() {
    // Direct boundary proof for the classifier, independent of the parser: a
    // mutation that tests `contains('e')` instead of counting cannot pass this.
    assert_eq!(ReplacementEvaluation::from_modifiers(""), ReplacementEvaluation::Literal);
    assert_eq!(ReplacementEvaluation::from_modifiers("g"), ReplacementEvaluation::Literal);
    assert_eq!(ReplacementEvaluation::from_modifiers("e"), ReplacementEvaluation::Expression);
    assert_eq!(ReplacementEvaluation::from_modifiers("ge"), ReplacementEvaluation::Expression);
    assert_eq!(ReplacementEvaluation::from_modifiers("ee"), ReplacementEvaluation::DoubleEval);
    assert_eq!(ReplacementEvaluation::from_modifiers("gee"), ReplacementEvaluation::DoubleEval);
    assert!(!ReplacementEvaluation::Literal.is_dynamic());
}

// ── Target modeling ──────────────────────────────────────────────────────────

#[test]
fn implicit_topic_is_explicit_and_not_a_fabricated_identifier() {
    // The parser materializes an unbound `s///` target as a zero-width
    // `Identifier` node literally named "$_". Canonical body HIR must record
    // the implicit topic as its own state rather than adopting that
    // fabrication as if it were a real bound operand.
    assert_eq!(
        substitution_of("s/a/b/;").target,
        HirRegexTarget::DefaultTopic,
        "unbound s/// must record an explicit default topic"
    );
    assert_eq!(
        transliteration_of("tr/a/b/;").target,
        HirRegexTarget::DefaultTopic,
        "unbound tr/// must record an explicit default topic"
    );
}

#[test]
fn implicit_topic_is_recorded_at_a_non_zero_source_offset() {
    // Guards the synthesized-node detector against an offset-dependent match:
    // the operator here does not start at byte 0.
    assert_eq!(substitution_of("my $q = 1; s/a/b/;").target, HirRegexTarget::DefaultTopic);
}

#[test]
fn explicit_topic_is_distinguishable_from_implicit_topic() {
    // A written `$_ =~ s///` is a real place operand, not the implicit form.
    let s = substitution_of("$_ =~ s/a/b/;");
    let (_, kind, ast_kind) = bound_of(&s.target, "explicit `$_`");
    assert_eq!(kind, RegexTargetKind::Place);
    assert_eq!(ast_kind, "Variable");
    assert_ne!(
        body_exprs("s/a/b/;"),
        body_exprs("$_ =~ s/a/b/;"),
        "implicit and explicit topic must stay distinguishable"
    );
}

#[test]
fn call_produced_target_is_classified_as_an_expression_and_lowered_once() {
    // Evaluation order and evaluate-once are both load-bearing for a target
    // with side effects: `make_target()` must appear exactly once, before the
    // operator node that consumes it.
    let exprs = body_exprs("make_target() =~ /foo/;");
    let call_count = exprs
        .iter()
        .filter(|e| matches!(e, HirExpr::Call { ast_kind, .. } if ast_kind == "FunctionCall"))
        .count();
    assert_eq!(call_count, 1, "a call-produced target must be lowered exactly once");

    let m = match_of("make_target() =~ /foo/;");
    let (target_id, kind, ast_kind) = bound_of(&m.target, "`make_target() =~ /foo/`");
    assert_eq!(kind, RegexTargetKind::Expression);
    assert_eq!(ast_kind, "FunctionCall");
    // The target is allocated before the operator node that consumes it.
    assert!(
        (target_id.0 as usize) < exprs.len() - 1,
        "target must be lowered before the match node"
    );
}

#[test]
fn declaration_wrapped_target_classifies_through_the_wrapper() {
    // `(my $copy = $s) =~ s/a/b/` is the standard copy-then-modify idiom.
    // `classify_regex_target` sees through the declaration to the inner lvalue,
    // so the target classifies as a Place named "Variable" — but body lowering
    // does not model a declaration used as an expression, so the lowered child
    // is `Opaque { "VariableDeclaration" }` and the variable is not reachable
    // through it.
    //
    // Pinning both halves keeps that boundary explicit: `ast_kind` describes
    // the classified operand, `expr` is the lowered outer operand, and a
    // consumer must not assume they name the same node. If declaration
    // expressions later lower properly, this test says exactly what changed.
    let s = substitution_of("(my $copy = $s) =~ s/a/b/;");
    let (target_id, kind, ast_kind) = bound_of(&s.target, "`(my $copy = $s) =~ s/a/b/`");
    assert_eq!(kind, RegexTargetKind::Place, "a declared lvalue is still a place");
    assert_eq!(ast_kind, "Variable", "classification sees through the declaration wrapper");

    let exprs = body_exprs("(my $copy = $s) =~ s/a/b/;");
    let child = must_some_with(
        exprs.get(target_id.0 as usize).cloned(),
        "target id must index the body arena",
    );
    assert!(
        matches!(child, HirExpr::Opaque { ref ast_kind } if ast_kind == "VariableDeclaration"),
        "the lowered child is the unmodeled outer declaration, got {child:?}"
    );
}

#[test]
fn element_subscript_target_is_a_place() {
    let m = match_of("$h{k} =~ /foo/;");
    let (_, kind, _) = bound_of(&m.target, "`$h{k} =~ /foo/`");
    assert_eq!(kind, RegexTargetKind::Place);
}

// ── Analysis anchoring ───────────────────────────────────────────────────────

#[test]
fn an_unbound_regex_anchors_to_its_own_source_range() {
    // For an UNBOUND construct the anchor is the operator's own range. This is
    // the narrow case only; a bound operator's anchor also spans its target and
    // binding operator, and end-to-end resolution of every form against a real
    // `RegexAnalysisTable` is proven in `hir_regex_anchor_resolution_test.rs`.
    let source = "my $r = qr/foo/i;";
    let r = regex_of(source);
    let start = r.analysis.full_range.start;
    let end = r.analysis.full_range.end;
    let slice = must_some_with(source.get(start..end), "anchor must be an in-bounds range");
    assert_eq!(slice, "qr/foo/i", "anchor must span the construct");
}

#[test]
fn distinct_operations_in_one_body_get_distinct_anchors() {
    let source = "$a =~ /one/; $b =~ /two/;";
    let anchors: Vec<_> = body_exprs(source)
        .into_iter()
        .filter_map(|e| as_match(&e).map(|m| m.analysis.full_range))
        .collect();
    assert_eq!(anchors.len(), 2, "both matches must be modeled");
    assert_ne!(anchors[0], anchors[1], "each operation anchors to its own range");
}

// ── Determinism and versioning ───────────────────────────────────────────────

#[test]
fn lowering_is_deterministic_across_runs() {
    let source = "$a =~ /one/; $b =~ s/x/y/gr; $c =~ tr/a/b/; my $r = qr/z/i;";
    assert_eq!(body_exprs(source), body_exprs(source), "lowering must be deterministic");
}

#[test]
fn body_model_version_covers_the_regex_family_variants() {
    // The regex-family variants changed the body arena layout, so consumers
    // gated on the body model version must see a new version.
    const {
        assert!(
            HIR_BODY_MODEL_VERSION >= 4,
            "adding the regex-family body variants requires a body model version bump"
        );
    }
    let mut parser = Parser::new("$x =~ /foo/;");
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    assert_eq!(hir.body_model_version, HIR_BODY_MODEL_VERSION);
}
