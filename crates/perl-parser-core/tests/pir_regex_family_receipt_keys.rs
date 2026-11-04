//! PIR-A receipt keys for regex-family constructs (#7136).
//!
//! Canonical body HIR models `qr//`, `m//`, `s///` and `tr///` as typed forms.
//! PIR-A does not yet own regex operations (that is #7137), so each family is
//! still counted in `unsupported_construct_counts` — but under its own AST-kind
//! name, which is what [`PirReceipt::unsupported_construct_counts`] documents
//! the body lowering path to key by.
//!
//! Before #7136 these constructs reached PIR through the generic
//! `HirExpr::Call` / `HirExpr::Opaque` fallbacks, so a match, a substitution and
//! a transliteration were all booked as `"Call"` — indistinguishable from a real
//! function call — and `qr//` as `"OpaqueExpr"`. That is a receipt-visible
//! change, and this file is its proof: it pins both what the receipt now says
//! and, as negative controls, that the old conflations are gone.
//!
//! Receipt **shape** is unchanged by all of this: `PIR_RECEIPT_VERSION` governs
//! node/edge/place/effect/receipt structure per PLSP-SPEC-0032 C1, and the
//! `unsupported_construct_counts` key namespace is an open lowering-diagnostic
//! vocabulary rather than a versioned closed domain. The version assertion below
//! records that the receipt this file reads is the current-shape one.

use perl_parser_core::Parser;
use perl_parser_core::hir::lower_ast;
use perl_parser_core::pir::{PIR_RECEIPT_VERSION, PirGraph, lower_hir_bodies};

fn parse_and_lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    lower_hir_bodies(&hir)
}

fn unsupported(source: &str, key: &str) -> usize {
    parse_and_lower(source).receipt.unsupported_construct_counts.get(key).copied().unwrap_or(0)
}

#[test]
fn each_regex_family_is_counted_under_its_own_ast_kind_name() {
    for (source, key) in [
        ("my $r = qr/foo/i;", "Regex"),
        ("$x =~ /foo/;", "Match"),
        ("$x !~ /foo/;", "Match"),
        ("$x =~ s/a/b/g;", "Substitution"),
        ("s/a/b/;", "Substitution"),
        ("$x =~ tr/a-z/A-Z/;", "Transliteration"),
    ] {
        assert_eq!(
            unsupported(source, key),
            1,
            "{source:?} must be counted once under {key:?} in the PIR receipt"
        );
    }
}

#[test]
fn a_regex_operation_is_no_longer_conflated_with_a_function_call() {
    // The negative control for the change. `"Call"` is a live key — a real call
    // still produces it — so this asserts the specific conflation is gone
    // rather than that the key vanished.
    for source in ["$x =~ /foo/;", "$x =~ s/a/b/g;", "$x =~ tr/a-z/A-Z/;"] {
        assert_eq!(
            unsupported(source, "Call"),
            0,
            "{source:?} must not be booked as an unsupported call"
        );
    }
    assert_eq!(
        unsupported("my $r = qr/foo/i;", "OpaqueExpr"),
        0,
        "qr// must not fall through to the untyped opaque bucket"
    );

    // A real call still books as one: the key itself is not what changed.
    assert!(
        unsupported("foo(1);", "Call") >= 1,
        "a genuine function call must still be counted under \"Call\""
    );
}

#[test]
fn a_bound_target_still_contributes_its_own_reads_beside_the_operator_count() {
    // The regex operation is unsupported, but its target is not opaque — the
    // operand is still walked, so a fact-bearing target keeps emitting facts.
    // If this regresses, the receipt would go quiet about the target rather
    // than about the operator.
    let graph = parse_and_lower("$x =~ s/a/b/;");
    assert!(
        !graph.nodes.is_empty(),
        "a bound substitution must still lower its target into receipt-visible nodes"
    );
    assert_eq!(graph.receipt.schema_version, PIR_RECEIPT_VERSION);
}

#[test]
fn a_body_with_no_regex_reports_no_regex_family_key() {
    // Negative control: the keys are emitted by the regex arms, not present
    // unconditionally, so their absence is meaningful.
    let counts = parse_and_lower("my $x = 1;").receipt.unsupported_construct_counts;
    for key in ["Regex", "Match", "Substitution", "Transliteration"] {
        assert!(!counts.contains_key(key), "a regex-free body must not report {key:?}");
    }
}
