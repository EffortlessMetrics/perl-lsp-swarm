//! HIR aggregate-element binding identity (#14682): a singular element
//! subscript over a `$`-sigil container (`$arr[0]`, `$h{k}`, `$ref->[0]`)
//! must resolve to the binding of the underlying aggregate declaration when no
//! same-sigil scalar is in scope.
//!
//! Without this fallback the two HIR views disagreed:
//!   - the first-pass `BindingReference` for `$arr` had no `resolved_binding`
//!     because the lookup used sigil `$` against a declaration on sigil `@`,
//!   - the body view's `HirVariable` reported `VariableKind::Package` with no
//!     `binding` ID, which is the same shape an undeclared package global has.
//!
//! Every fixture in this file is one of:
//!   - a positive test pinning both views to the aggregate's `HirBindingId`,
//!   - a falsifier pinning the negative case (a `$`-sigil in scope still wins
//!     over the aggregate, an array slice stays a slice, etc.),
//!   - a regression guard against the two HIR views disagreeing for the same
//!     source position.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, HirExpr, HirFile, HirVariable, SubscriptKind, VariableKind, lower_ast,
};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// Find the `HirExpr::Subscript` that has `$name` as its container expression.
///
/// Panics if not exactly one matches; the tests below assert this directly
/// because every fixture is constructed so the element access is unique in
/// the lowered program.
fn subscript_for_container(
    file: &HirFile,
    container_name: &str,
) -> (HirVariable, SubscriptKind, AccessMode) {
    let mut found: Option<(HirVariable, SubscriptKind, AccessMode)> = None;
    for body in &file.bodies {
        let expr_count = body.source_map.expr_ranges.len();
        for idx in 0..expr_count {
            let id = perl_parser_core::hir::HirExprId(idx as u32);
            if let Some(HirExpr::Subscript(sub)) = body.expr(id) {
                let container = body.expr(sub.container);
                if let Some(HirExpr::Variable(var)) = container {
                    if var.name == container_name {
                        assert!(
                            found.is_none(),
                            "found two subscripts with container {container_name}; tests must scope to one"
                        );
                        found = Some((var.clone(), sub.kind, sub.access));
                    }
                }
            }
        }
    }
    found.unwrap_or_else(|| panic!("no subscript with container {container_name}"))
}

/// Every `BindingReference` whose `name` equals `name` (across all bodies).
///
/// Used to assert that the first-pass walker recorded the aggregate's binding
/// for the container — not a package-global `None`.
fn binding_references_for_name<'a>(
    file: &'a HirFile,
    name: &str,
) -> Vec<&'a perl_parser_core::hir::BindingReference> {
    let mut out = Vec::new();
    for reference in &file.scope_graph.references {
        if reference.name == name {
            out.push(reference);
        }
    }
    out
}

/// The single `BindingReference` for `$name` whose sigil is the source-side
/// `$` (the one a subscript element access produces). Used to assert the
/// first-pass view's `resolved_binding` agrees with the body view.
fn dollar_reference_for_name<'a>(
    file: &'a HirFile,
    name: &str,
) -> &'a perl_parser_core::hir::BindingReference {
    binding_references_for_name(file, name)
        .into_iter()
        .find(|r| r.sigil == "$")
        .unwrap_or_else(|| panic!("no `$`-sigil BindingReference for {name}"))
}

fn first_binding_for_name<'a>(file: &'a HirFile, name: &str) -> &'a perl_parser_core::hir::Binding {
    file.scope_graph
        .bindings
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("no binding named {name}"))
}

/// `my @arr; my $x = $arr[0]` — the body view's `$arr` container now resolves
/// to the `@arr` binding instead of falling through to `VariableKind::Package`
/// with no `binding` ID (#14682).
#[test]
fn array_element_resolves_to_array_decl_binding() {
    let file = lower("my @arr = (1, 2, 3); my $x = $arr[0];");
    let (container, kind, access) = subscript_for_container(&file, "arr");
    assert_eq!(kind, SubscriptKind::Array, "subscript kind");
    assert_eq!(access, AccessMode::Read, "subscript access");
    assert_eq!(container.sigil, perl_parser_core::hir::Sigil::Scalar);
    assert_eq!(container.name, "arr");
    assert_eq!(
        container.kind,
        VariableKind::Lexical,
        "container must reflect the lexical `@arr` declaration, not an undeclared package global"
    );
    let arr_binding = first_binding_for_name(&file, "arr");
    assert_eq!(arr_binding.sigil, "@", "declaration is `@arr`");
    assert_eq!(
        container.binding,
        Some(arr_binding.id),
        "container carries the `@arr` binding identity"
    );

    // First-pass view must agree.
    let dollar_ref = dollar_reference_for_name(&file, "arr");
    assert_eq!(
        dollar_ref.resolved_binding,
        Some(arr_binding.id),
        "first-pass BindingReference for `$arr` resolves to the `@arr` binding"
    );
}

/// `my %h; my $v = $h{k}` — hash-element container resolves to the `%h`
/// binding with the same body-view agreement as the array case (#14682).
#[test]
fn hash_element_resolves_to_hash_decl_binding() {
    let file = lower("my %h = (k => 1); my $v = $h{k};");
    let (container, kind, access) = subscript_for_container(&file, "h");
    assert_eq!(kind, SubscriptKind::Hash, "subscript kind");
    assert_eq!(access, AccessMode::Read, "subscript access");
    assert_eq!(container.sigil, perl_parser_core::hir::Sigil::Scalar);
    assert_eq!(container.name, "h");
    assert_eq!(container.kind, VariableKind::Lexical);
    let h_binding = first_binding_for_name(&file, "h");
    assert_eq!(h_binding.sigil, "%");
    assert_eq!(container.binding, Some(h_binding.id));

    let dollar_ref = dollar_reference_for_name(&file, "h");
    assert_eq!(dollar_ref.resolved_binding, Some(h_binding.id));
}

/// If a `$name` scalar IS in scope, the scalar's binding wins — the aggregate
/// fallback only activates when the scalar lookup misses. This is the
/// negative-control guard for the bug-flip: we don't want a `@arr` declaration
/// to shadow a `$arr` declaration just because `$arr` is the container of an
/// element access (#14682).
#[test]
fn scalar_in_scope_wins_over_aggregate() {
    let file = lower("my @arr; my $arr; my $x = $arr[0];");
    let (container, _kind, _access) = subscript_for_container(&file, "arr");
    assert_eq!(
        container.sigil,
        perl_parser_core::hir::Sigil::Scalar,
        "container sigil stays `$` because the source spelling is `$arr`"
    );
    assert_eq!(container.kind, VariableKind::Lexical);
    let scalar_binding = file
        .scope_graph
        .bindings
        .iter()
        .find(|b| b.name == "arr" && b.sigil == "$")
        .expect("`$arr` scalar binding must exist");
    assert_eq!(
        container.binding,
        Some(scalar_binding.id),
        "container must resolve to the `$arr` scalar, not the `@arr` aggregate"
    );
}

/// Array slices (`@arr[0, 1]`) keep their existing shape — the aggregate
/// fallback is only for singular element access, never for slices (#14682).
#[test]
fn array_slice_is_not_misresolved_as_element() {
    let file = lower("my @arr = (1, 2, 3); my @x = @arr[0, 1];");
    // No subscript containers named `arr` should exist; this is a slice, not
    // an element access.
    let mut saw_subscript_for_arr = false;
    for body in &file.bodies {
        let expr_count = body.source_map.expr_ranges.len();
        for idx in 0..expr_count {
            let id = perl_parser_core::hir::HirExprId(idx as u32);
            if let Some(HirExpr::Subscript(sub)) = body.expr(id) {
                if let Some(HirExpr::Variable(var)) = body.expr(sub.container) {
                    if var.name == "arr" {
                        saw_subscript_for_arr = true;
                    }
                }
            }
        }
    }
    assert!(
        !saw_subscript_for_arr,
        "array slice `@arr[0, 1]` must not produce a Subscript with container `arr`"
    );
}

/// Write-place: `$arr[0] = 99` keeps the container's identity anchored to the
/// `@arr` binding through both views (#14682).
#[test]
fn array_element_write_place_keeps_aggregate_binding() {
    let file = lower("my @arr; $arr[0] = 99;");
    let (container, kind, access) = subscript_for_container(&file, "arr");
    assert_eq!(kind, SubscriptKind::Array);
    assert_eq!(access, AccessMode::Write, "assignment LHS subscript is a write place");
    let arr_binding = first_binding_for_name(&file, "arr");
    assert_eq!(container.binding, Some(arr_binding.id));
    let dollar_ref = dollar_reference_for_name(&file, "arr");
    assert_eq!(dollar_ref.resolved_binding, Some(arr_binding.id));
}

/// Arrow-deref form `$ref->[0]` does NOT take the aggregate fallback — the
/// container is a scalar reference, not an aggregate (`$ref` and `@arr` are
/// different bindings), and substituting the aggregate would be wrong.
/// The aggregate fallback applies only to direct `$arr[i]` / `$h{k}` shapes
/// (#14682).
#[test]
fn arrow_deref_does_not_take_aggregate_fallback() {
    let file = lower("my $ref; my @arr; my $x = $ref->[0];");
    // The container for `$ref->[0]` is `$ref`, which is a regular scalar
    // lookup; the `@arr` declaration must not leak onto it.
    let (container, _kind, _access) = subscript_for_container(&file, "ref");
    let scalar_binding = file
        .scope_graph
        .bindings
        .iter()
        .find(|b| b.name == "ref" && b.sigil == "$")
        .expect("`$ref` scalar binding must exist");
    assert_eq!(container.binding, Some(scalar_binding.id));
    let dollar_ref = dollar_reference_for_name(&file, "ref");
    assert_eq!(dollar_ref.resolved_binding, Some(scalar_binding.id));

    // And no spurious `BindingReference` for `arr` should appear from this
    // expression — the aggregate fallback does not fire on arrow-deref.
    let arr_refs: Vec<_> =
        binding_references_for_name(&file, "arr").into_iter().filter(|r| r.sigil == "$").collect();
    assert!(arr_refs.is_empty(), "no `$arr` BindingReference should be produced by `$ref->[0]`");
}
