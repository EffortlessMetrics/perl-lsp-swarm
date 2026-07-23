//! PIR v0 lowering tests for regex/match/substitution/transliteration ops.
//!
//! Slice 1 of #4848: turns the four regex HirKinds (`RegexExpr`, `MatchExpr`,
//! `SubstitutionExpr`, `TransliterationExpr`) into first-class PIR-A
//! operations (`PirOperation::RegexLiteral`/`Match`/`Substitution`/
//! `Transliteration`) with a dedicated `PirDynamicBoundaryKind::
//! EmbeddedRegexCode` boundary, instead of falling into the flat-path
//! `other =>` unsupported-count fallback.
//!
//! Slice 1 is strictly PIR-only (no `hir/model.rs` or `hir/lower.rs` edits):
//! target/place resolution is deferred, so every op's `target` is always
//! `PirRegexTarget::Unknown` here. See `docs/specs/PLSP-SPEC-0025-pir-v0.md`
//! and the issue #4848 spec for the claim boundary.

use perl_parser_core::Parser;
use perl_parser_core::hir::{HirFile, lower_ast};
use perl_parser_core::pir::{
    PirContext, PirDynamicBoundaryKind, PirEdgeKind, PirGraph, PirOperation, PirRegexTarget,
    PirTargetAccess, lower_hir,
};
use perl_tdd_support::must_some;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn lower(source: &str) -> PirGraph {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir: HirFile = lower_ast(&output.ast);
    lower_hir(&hir)
}

#[test]
fn qr_literal_lowers_to_regex_literal_op() -> TestResult {
    let graph = lower("my $re = qr/foo/i;");
    let (modifiers, embedded_code, node) =
        must_some(graph.nodes.iter().find_map(|node| match &node.operation {
            PirOperation::RegexLiteral { modifiers, embedded_code } => {
                Some((modifiers.clone(), *embedded_code, node))
            }
            _ => None,
        }));
    assert!(modifiers.i, "expected /i modifier flag set, got {modifiers:?}");
    assert!(!embedded_code, "plain qr// has no embedded code");
    assert!(node.source_anchor.is_anchored());
    assert_eq!(graph.receipt.operation_counts.get("RegexLiteral"), Some(&1));
    Ok(())
}

#[test]
fn match_and_negated_match_are_distinct_ops() -> TestResult {
    let plain = lower("$x =~ /f/;");
    let negated = must_some(plain.nodes.iter().find_map(|node| match &node.operation {
        PirOperation::Match { negated, .. } => Some(*negated),
        _ => None,
    }));
    assert!(!negated, "`=~` should lower with negated == false");

    let bang = lower("$x !~ /f/;");
    let negated = must_some(bang.nodes.iter().find_map(|node| match &node.operation {
        PirOperation::Match { negated, .. } => Some(*negated),
        _ => None,
    }));
    assert!(negated, "`!~` should lower with negated == true");
    Ok(())
}

#[test]
fn match_context_is_unknown_without_surrounding_context() -> TestResult {
    // A match returns a boolean in scalar context but a list of captures (or
    // `()`/`(1)`) in list context — with or without `/g`. PIR v0 cannot see the
    // surrounding expression context here, so it must never guess: Unknown is
    // the honest answer in every case.
    for src in ["$x =~ /f/;", "$x =~ /f/g;"] {
        let graph = lower(src);
        let node = must_some(
            graph.nodes.iter().find(|node| matches!(node.operation, PirOperation::Match { .. })),
        );
        assert_eq!(
            node.context,
            PirContext::Unknown,
            "match context must never be guessed (src: {src})"
        );
    }
    Ok(())
}

#[test]
fn match_access_is_read_only() -> TestResult {
    // A match reads its target without reassigning it — access is ReadOnly,
    // distinct from s///'s Mutate/MutateCopy.
    let graph = lower("$x =~ /f/;");
    let (access, target) = must_some(graph.nodes.iter().find_map(|node| match &node.operation {
        PirOperation::Match { access, target, .. } => Some((*access, target.clone())),
        _ => None,
    }));
    assert_eq!(access, PirTargetAccess::ReadOnly);
    // Slice 1 leaves targets unresolved pending HIR target-descriptor enrichment.
    assert_eq!(target, PirRegexTarget::Unknown);
    Ok(())
}

#[test]
fn substitution_is_mutate_access() -> TestResult {
    let plain = lower("$x =~ s/a/b/;");
    let (access, target) = must_some(plain.nodes.iter().find_map(|node| match &node.operation {
        PirOperation::Substitution { access, target, .. } => Some((*access, target.clone())),
        _ => None,
    }));
    assert_eq!(access, PirTargetAccess::Mutate);
    assert_eq!(target, PirRegexTarget::Unknown);

    let copy = lower("$x =~ s/a/b/r;");
    let (access, target) = must_some(copy.nodes.iter().find_map(|node| match &node.operation {
        PirOperation::Substitution { access, target, .. } => Some((*access, target.clone())),
        _ => None,
    }));
    assert_eq!(access, PirTargetAccess::MutateCopy);
    assert_eq!(target, PirRegexTarget::Unknown);
    Ok(())
}

#[test]
fn transliteration_is_distinct_op() -> TestResult {
    let graph = lower("$x =~ tr/a-z/A-Z/;");
    let (access, target) = must_some(graph.nodes.iter().find_map(|node| match &node.operation {
        PirOperation::Transliteration { access, target, .. } => Some((*access, target.clone())),
        _ => None,
    }));
    assert_eq!(access, PirTargetAccess::Mutate);
    assert_eq!(target, PirRegexTarget::Unknown);
    assert_eq!(graph.receipt.operation_counts.get("Transliteration"), Some(&1));

    let copy = lower("$x =~ tr/a-z/A-Z/r;");
    let (access, target) = must_some(copy.nodes.iter().find_map(|node| match &node.operation {
        PirOperation::Transliteration { access, target, .. } => Some((*access, target.clone())),
        _ => None,
    }));
    assert_eq!(access, PirTargetAccess::MutateCopy);
    assert_eq!(target, PirRegexTarget::Unknown);
    Ok(())
}

#[test]
fn modifiers_preserved_verbatim_and_normalized() -> TestResult {
    let graph = lower("$x =~ s/a/b/gi;");
    let modifiers = must_some(graph.nodes.iter().find_map(|node| match &node.operation {
        PirOperation::Substitution { modifiers, .. } => Some(modifiers.clone()),
        _ => None,
    }));
    assert!(modifiers.g);
    assert!(modifiers.i);
    assert!(!modifiers.m && !modifiers.s && !modifiers.r);
    assert!(modifiers.unknown.is_empty());
    // The HIR shell exposes the modifier text verbatim; PIR preserves it
    // rather than re-serializing from the parsed flags.
    assert_eq!(modifiers.raw, "gi");
    Ok(())
}

/// Assert that the single embedded-code regex op in `graph` carries
/// `embedded_code: true` and links to a dedicated `EmbeddedRegexCode` boundary
/// node. Covers the `Match` / `Substitution` / `RegexLiteral` owner paths.
fn assert_embedded_code_boundary(graph: &PirGraph, source: &str) -> TestResult {
    assert_eq!(
        graph.receipt.dynamic_boundary_counts.get("EmbeddedRegexCode"),
        Some(&1),
        "expected one EmbeddedRegexCode boundary for {source:?}"
    );
    let owner = must_some(graph.nodes.iter().find(|node| {
        matches!(
            node.operation,
            PirOperation::Match { .. }
                | PirOperation::Substitution { .. }
                | PirOperation::RegexLiteral { .. }
        )
    }));
    // The direct `embedded_code` flag on the op must agree with the link.
    let embedded = match &owner.operation {
        PirOperation::Match { embedded_code, .. }
        | PirOperation::Substitution { embedded_code, .. }
        | PirOperation::RegexLiteral { embedded_code, .. } => *embedded_code,
        _ => false,
    };
    assert!(embedded, "owning op's embedded_code flag must be true for {source:?}");
    let boundary_id = must_some(owner.dynamic_boundary);
    let boundary = must_some(graph.node(boundary_id));
    assert!(
        matches!(
            &boundary.operation,
            PirOperation::DynamicBoundary { kind, .. }
                if *kind == PirDynamicBoundaryKind::EmbeddedRegexCode
        ),
        "owning op's dynamic_boundary must point at the EmbeddedRegexCode \
         boundary node, not Unknown, for {source:?}"
    );
    Ok(())
}

#[test]
fn embedded_code_uses_dedicated_boundary() -> TestResult {
    // Verified against real parser/HIR output: each source sets
    // `has_embedded_code` and HIR emits a `DynamicBoundary(EmbeddedRegexCode)`
    // item immediately after the owning regex item. Covers the Substitution
    // (`/e`, `/ee` double-eval), Match (`(?{...})`), and RegexLiteral
    // (`qr/(?{...})/`) owner paths — one assertion helper for all four.
    for source in
        ["$x =~ s/a/1+1/e;", "$x =~ m/(?{1})/;", "$x =~ s/a/1+1/ee;", "my $re = qr/(?{1})/;"]
    {
        assert_embedded_code_boundary(&lower(source), source)?;
    }
    Ok(())
}

#[test]
fn construct_target_operand_lowers_and_op_is_anchored() -> TestResult {
    // Verified against real HIR/PIR output: HIR lowers `Match`'s bound
    // expression via `visit_children`, which emits the `foo()` CallExpr item
    // *after* the MatchExpr item (not before it, and not via the
    // Literal-only operand-splice machinery `push_operand_node` reuses for
    // literal operands nested inside Call/Deref/Assign). So for a call target
    // the Call op follows the Match op in the fallthrough chain, rather than
    // preceding it the way a nested Literal would. This is a real,
    // parser/HIR-verified limitation of Slice 1's flat-path lowering, not an
    // assumption: only `lower_literal` currently calls
    // `enclosing_expression_parent`/`push_operand_node`; `lower_call` always
    // pushes its own node unconditionally. Slice 2 (or a follow-up) can widen
    // the operand-splice reuse to Call targets once target resolution lands.
    let graph = lower("foo() =~ /b/;");
    let match_node = must_some(
        graph.nodes.iter().find(|node| matches!(node.operation, PirOperation::Match { .. })),
    );
    assert!(match_node.source_anchor.is_anchored());

    let call_node = must_some(
        graph.nodes.iter().find(|node| matches!(node.operation, PirOperation::Call { .. })),
    );
    assert!(call_node.source_anchor.is_anchored());

    // Real observed order: Match (id 0) falls through to Call (id 1) — the
    // op precedes its construct target here, the opposite of the
    // Literal-in-Call operand order. Document, don't fabricate, the shape.
    assert!(match_node.id.index() < call_node.id.index());
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == PirEdgeKind::Fallthrough
            && edge.from == match_node.id
            && edge.to == Some(call_node.id)
    }));
    Ok(())
}

#[test]
fn regex_ops_removed_from_unsupported_counts() -> TestResult {
    let graph = lower(
        r#"
my $re = qr/foo/i;
$x =~ /f/;
$x =~ s/a/b/;
$x =~ tr/a-z/A-Z/;
"#,
    );
    for kind in ["RegexExpr", "MatchExpr", "SubstitutionExpr", "TransliterationExpr"] {
        assert_eq!(
            graph.receipt.unsupported_construct_counts.get(kind),
            None,
            "{kind} should no longer appear in unsupported_construct_counts"
        );
    }
    assert_eq!(graph.receipt.operation_counts.get("RegexLiteral"), Some(&1));
    assert_eq!(graph.receipt.operation_counts.get("Match"), Some(&1));
    assert_eq!(graph.receipt.operation_counts.get("Substitution"), Some(&1));
    assert_eq!(graph.receipt.operation_counts.get("Transliteration"), Some(&1));
    Ok(())
}

#[test]
fn all_regex_nodes_anchored() -> TestResult {
    let graph = lower(
        r#"
my $re = qr/foo/i;
$x =~ /f/;
$x !~ /f/g;
$x =~ s/a/b/r;
$x =~ tr/a-z/A-Z/;
"#,
    );
    let regex_nodes: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.operation,
                PirOperation::RegexLiteral { .. }
                    | PirOperation::Match { .. }
                    | PirOperation::Substitution { .. }
                    | PirOperation::Transliteration { .. }
            )
        })
        .collect();
    assert_eq!(regex_nodes.len(), 5);
    for node in regex_nodes {
        assert!(node.source_anchor.is_anchored(), "{:?} should be anchored", node.operation);
    }
    Ok(())
}

#[test]
fn regex_lowering_never_changes_provider_behavior() -> TestResult {
    let graph = lower("$x =~ s/a/1+1/e;");
    assert!(!graph.receipt.provider_behavior_changed);
    Ok(())
}
