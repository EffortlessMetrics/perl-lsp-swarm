//! HIR lowering proof for `tie`/`untie` dynamic boundaries (issue #14786).
//!
//! `tie` binds a place to a tie class, after which every read, write,
//! iteration, and destruction of that place dispatches to hidden `TIE*`
//! methods. Before this slice, `NodeKind::Tie` and `NodeKind::Untie` had no
//! arm in `crates/perl-parser-core/src/hir/lower.rs`: both fell through to
//! `_ => visit_children` and vanished from flat HIR, so a consumer walking
//! HIR saw a tied `%hash` as ordinary storage.
//!
//! These tests pin that both constructs now emit an explicit
//! `HirKind::DynamicBoundary`, in the same way `Eval`/`Do`/symbolic-deref
//! already do, while still traversing children so the tie class expression
//! and the `TIE*` arguments keep lowering normally.
//!
//! Claim boundary: this proves only that a boundary exists and is anchored
//! to the right construct. The tied place identity, the hidden constructor
//! dispatch, and the tied access classes remain unmodeled and belong to
//! parent issue #6683.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    DynamicBoundary, DynamicBoundaryKind, HirFile, HirItem, HirKind, disposition, lower_ast,
};
use perl_tdd_support::must_some;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn boundaries_of(file: &HirFile, kind: DynamicBoundaryKind) -> Vec<&HirItem> {
    file.items
        .iter()
        .filter(|item| matches!(&item.kind, HirKind::DynamicBoundary(b) if b.kind == kind))
        .collect()
}

fn sole_boundary(
    file: &HirFile,
    kind: DynamicBoundaryKind,
) -> Option<(&HirItem, &DynamicBoundary)> {
    let matches = boundaries_of(file, kind);
    if matches.len() != 1 {
        return None;
    }
    let item = matches[0];
    match &item.kind {
        HirKind::DynamicBoundary(boundary) => Some((item, boundary)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// tie emits a boundary
// ---------------------------------------------------------------------------

#[test]
fn tie_emits_exactly_one_tied_place_binding_boundary() -> TestResult {
    let file = lower_source("tie %hash, 'Tie::StdHash';\n");
    let (item, boundary) = must_some(sole_boundary(&file, DynamicBoundaryKind::TiedPlaceBinding));
    assert_eq!(
        item.anchor.node_kind, "Tie",
        "the tie boundary must be anchored to the Tie construct, got {:?}",
        item.anchor.node_kind
    );
    assert!(!boundary.reason.trim().is_empty(), "a dynamic boundary must carry a non-empty reason");
    Ok(())
}

#[test]
fn tie_boundary_is_emitted_for_every_tied_sigil_family() -> TestResult {
    for source in [
        "tie $scalar, 'Tie::StdScalar';\n",
        "tie @array, 'Tie::StdArray';\n",
        "tie %hash, 'Tie::StdHash';\n",
        "tie *HANDLE, 'Tie::StdHandle';\n",
    ] {
        let file = lower_source(source);
        assert_eq!(
            boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).len(),
            1,
            "expected exactly one tie boundary for {source:?}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// untie emits a distinct boundary
// ---------------------------------------------------------------------------

#[test]
fn untie_emits_exactly_one_tied_place_release_boundary() -> TestResult {
    let file = lower_source("untie %hash;\n");
    let (item, boundary) = must_some(sole_boundary(&file, DynamicBoundaryKind::TiedPlaceRelease));
    assert_eq!(
        item.anchor.node_kind, "Untie",
        "the untie boundary must be anchored to the Untie construct, got {:?}",
        item.anchor.node_kind
    );
    assert!(!boundary.reason.trim().is_empty(), "a dynamic boundary must carry a non-empty reason");
    Ok(())
}

/// `tie` and `untie` are different propositions: binding a place to a class
/// is not the same event as releasing it. A single shared boundary kind would
/// let a consumer confuse the two, so the two kinds must stay distinct.
#[test]
fn tie_and_untie_boundaries_are_independently_classified() -> TestResult {
    let file = lower_source("tie %hash, 'Tie::StdHash';\nuntie %hash;\n");
    assert_eq!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).len(),
        1,
        "one tie in the source must produce exactly one binding boundary"
    );
    assert_eq!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceRelease).len(),
        1,
        "one untie in the source must produce exactly one release boundary"
    );
    assert_ne!(
        DynamicBoundaryKind::TiedPlaceBinding,
        DynamicBoundaryKind::TiedPlaceRelease,
        "tie and untie must not share a boundary kind"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Children keep lowering
// ---------------------------------------------------------------------------

/// The boundary must not swallow the construct. `tie`'s arguments are ordinary
/// Perl and are evaluated before the hidden constructor runs, so they must
/// still reach HIR.
#[test]
fn tie_boundary_still_traverses_arguments() -> TestResult {
    let file = lower_source("tie %hash, 'Tie::StdHash', build_args();\n");
    assert_eq!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).len(),
        1,
        "the tie itself must still produce its boundary"
    );
    let has_call = file.items.iter().any(|item| match &item.kind {
        HirKind::CallExpr(call) => call.name == "build_args",
        _ => false,
    });
    assert!(
        has_call,
        "build_args() inside the tie argument list must still lower to a CallExpr; \
         anchored HIR items: {:?}",
        file.items.iter().map(|item| item.anchor.node_kind).collect::<Vec<_>>()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls
// ---------------------------------------------------------------------------

/// Ordinary hash use must not be reported as tied. Without this control the
/// implementation could emit a boundary unconditionally and still pass every
/// positive test above.
#[test]
fn untied_source_emits_no_tie_boundary() -> TestResult {
    let file = lower_source("my %hash = (a => 1);\nmy $value = $hash{a};\nfoo();\n");
    assert!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).is_empty(),
        "a source with no tie must not emit a tie boundary"
    );
    assert!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceRelease).is_empty(),
        "a source with no untie must not emit an untie boundary"
    );
    Ok(())
}

/// A bareword `tied(%hash)` inspection is a different proposition from `tie`
/// and is not part of this slice; it must not be silently classified as a
/// binding.
#[test]
fn tie_boundary_does_not_fire_for_an_unrelated_call() -> TestResult {
    let file = lower_source("my $obj = tied(%hash);\n");
    assert!(
        boundaries_of(&file, DynamicBoundaryKind::TiedPlaceBinding).is_empty(),
        "tied() is an inspection, not a tie binding"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The disposition registry must agree with the lowerer
// ---------------------------------------------------------------------------

#[test]
fn tie_and_untie_dispositions_report_dynamic_boundary() -> TestResult {
    for kind_name in ["Tie", "Untie"] {
        let disposition = must_some(disposition::disposition_for(kind_name));
        assert!(
            disposition.may_emit_boundary,
            "{kind_name} must record may_emit_boundary=true now that lower.rs emits one"
        );
        assert!(
            disposition.is_intentional,
            "{kind_name}'s lowering is a deliberate decision, not a fallthrough"
        );
        assert!(
            disposition.traverses_children,
            "{kind_name} must keep traversing children so arguments still lower"
        );
        assert_eq!(
            disposition.legacy_category(),
            disposition::LegacyCategory::DynamicBoundary,
            "{kind_name} must no longer be classified NotYetModeled"
        );
    }
    Ok(())
}
