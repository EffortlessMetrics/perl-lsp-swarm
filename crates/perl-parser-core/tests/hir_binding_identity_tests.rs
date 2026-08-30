//! Canonical binding identity on body-HIR occurrences (#14166, family #6659).
//!
//! `ScopeGraph` already owns the source-backed binding authority: `Binding` /
//! `HirBindingId`, an eight-variant `StorageClass`, and
//! `BindingReference.resolved_binding`. Before this slice the body-HIR
//! projection dropped it — `HirVariable` and `HirStmt::Let` carried only
//! sigil/name/kind/access, so downstream had to reconstruct a weaker identity
//! from `(body, scope path, sigil, name)`.
//!
//! The discriminating property, from the #6659 implementation ruling:
//!
//! > two same-spelling lexicals in nested scopes inside the same body must
//! > produce distinct binding identities and attach each read/write/RMW to the
//! > correct one
//!
//! An implementation that groups by body + name must fail these fixtures.
//!
//! Fixtures nest through `if` blocks rather than bare `{ ... }` blocks: bare
//! blocks are not lowered into the body arena at all on current `main` (they
//! fall through to an opaque statement), which is a separate body-lowering
//! coverage gap and not this slice's claim.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, HirBindingId, HirExpr, HirExprId, HirFile, HirStmt, HirStmtId, StorageClass,
    VariableKind, lower_ast,
};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// One flattened `HirExpr::Variable` occurrence with its source anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Occurrence {
    name: String,
    access: AccessMode,
    kind: VariableKind,
    binding: Option<HirBindingId>,
    start: usize,
    end: usize,
}

/// One flattened `HirStmt::Let` declaration with its source anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Declaration {
    name: String,
    binding: Option<HirBindingId>,
    start: usize,
}

fn occurrences(file: &HirFile) -> Vec<Occurrence> {
    let mut found = Vec::new();
    for body in &file.bodies {
        for idx in 0..body.source_map.expr_ranges.len() {
            let id = HirExprId(idx as u32);
            if let Some(HirExpr::Variable(var)) = body.expr(id) {
                let range = body.source_map.expr_range(id).expect("expr range");
                found.push(Occurrence {
                    name: var.name.clone(),
                    access: var.access,
                    kind: var.kind,
                    binding: var.binding,
                    start: range.start,
                    end: range.end,
                });
            }
        }
    }
    found.sort_by_key(|o| (o.start, o.end));
    found
}

fn declarations(file: &HirFile) -> Vec<Declaration> {
    let mut found = Vec::new();
    for body in &file.bodies {
        for idx in 0..body.source_map.stmt_ranges.len() {
            let id = HirStmtId(idx as u32);
            if let Some(HirStmt::Let { name, binding, binding_range, .. }) = body.stmt(id) {
                found.push(Declaration {
                    name: name.clone(),
                    binding: *binding,
                    start: binding_range.start,
                });
            }
        }
    }
    found.sort_by_key(|d| d.start);
    found
}

/// Storage class behind a canonical binding identity.
fn storage_of(file: &HirFile, id: HirBindingId) -> StorageClass {
    file.scope_graph
        .bindings
        .iter()
        .find(|binding| binding.id == id)
        .expect("binding id refers to a real binding")
        .storage
}

/// Byte offset of the `n`-th (0-based) occurrence of `needle` in `source`.
fn nth_offset(source: &str, needle: &str, n: usize) -> usize {
    let mut from = 0usize;
    for _ in 0..n {
        let at = source[from..].find(needle).expect("occurrence exists") + from;
        from = at + needle.len();
    }
    source[from..].find(needle).expect("occurrence exists") + from
}

/// Byte span of the inner (second) brace-delimited block in `source`.
fn inner_block_span(source: &str) -> (usize, usize) {
    let outer = source.find('{').expect("outer block brace");
    let start = outer + 1 + source[outer + 1..].find('{').expect("inner block brace");
    let end = start + source[start..].find('}').expect("inner block close");
    (start, end)
}

/// The load-bearing fixture: same spelling, nested scopes, one body.
const NESTED: &str = r#"sub outer {
    my $x = 1;
    if ($cond) {
        my $x = 2;
        $x++;
        print $x;
    }
    $x = 3;
    return $x;
}
"#;

fn nested_binding_ids(file: &HirFile) -> (HirBindingId, HirBindingId) {
    let decls = declarations(file);
    let outer = decls
        .iter()
        .find(|d| d.start == nth_offset(NESTED, "$x", 0))
        .expect("outer `my $x` declaration");
    let inner = decls
        .iter()
        .find(|d| d.start == nth_offset(NESTED, "$x", 1))
        .expect("inner `my $x` declaration");
    (
        outer.binding.expect("outer declaration carries canonical binding identity"),
        inner.binding.expect("inner declaration carries canonical binding identity"),
    )
}

#[test]
fn nested_same_spelling_lexicals_receive_distinct_binding_identities() {
    let file = lower(NESTED);
    let (outer_id, inner_id) = nested_binding_ids(&file);

    // The whole point: identical body, identical sigil, identical name — and
    // still two distinct canonical bindings. A `(body, sigil, name)` identity
    // cannot tell these apart.
    assert_ne!(
        outer_id, inner_id,
        "nested same-spelling `my $x` declarations must not share one binding identity"
    );
}

#[test]
fn each_nested_occurrence_attaches_to_its_own_binding() {
    let file = lower(NESTED);
    let (outer_id, inner_id) = nested_binding_ids(&file);
    let (block_start, block_end) = inner_block_span(NESTED);

    let named: Vec<Occurrence> = occurrences(&file).into_iter().filter(|o| o.name == "x").collect();
    assert!(!named.is_empty(), "expected `$x` occurrences in body HIR");

    for occ in &named {
        let inside = occ.start > block_start && occ.start < block_end;
        let expected = if inside { inner_id } else { outer_id };
        assert_eq!(
            occ.binding,
            Some(expected),
            "occurrence of `$x` at {}..{} (access {:?}, inside inner block: {inside}) resolved \
             to the wrong binding",
            occ.start,
            occ.end,
            occ.access,
        );
    }
}

#[test]
fn read_write_and_rmw_occurrences_all_carry_identity() {
    let file = lower(NESTED);
    let occs: Vec<Occurrence> = occurrences(&file).into_iter().filter(|o| o.name == "x").collect();

    // The RMW (`$x++`), the plain writes and the reads must all be present and
    // all carry canonical identity — the PIR extractor historically dropped RMW.
    let modes: Vec<AccessMode> = occs.iter().map(|o| o.access).collect();
    for want in [AccessMode::Read, AccessMode::Write, AccessMode::ReadModifyWrite] {
        assert!(modes.contains(&want), "expected a {want:?} occurrence of `$x`, got {modes:?}");
    }

    for occ in &occs {
        assert!(
            occ.binding.is_some(),
            "every resolvable `$x` occurrence must carry canonical identity; {occ:?} did not"
        );
    }
}

/// Negative control: a name-keyed implementation collapses these into one
/// identity. This asserts the collapse does not happen.
#[test]
fn name_keyed_identity_would_collapse_and_must_not() {
    let file = lower(NESTED);
    let distinct: std::collections::BTreeSet<HirBindingId> = occurrences(&file)
        .into_iter()
        .filter(|o| o.name == "x")
        .filter_map(|o| o.binding)
        .collect();

    assert_eq!(
        distinct.len(),
        2,
        "all `$x` occurrences share one spelling in one body; canonical identity must still \
         separate them into exactly two bindings, found {distinct:?}"
    );
}

#[test]
fn my_and_state_of_the_same_spelling_remain_distinct_bindings() {
    let source = "sub f {\n  my $c = 1;\n  if ($t) { state $c = 2; print $c; }\n  print $c;\n}\n";
    let file = lower(source);
    let decls = declarations(&file);

    let ids: Vec<HirBindingId> = decls.iter().filter_map(|d| d.binding).collect();
    assert_eq!(ids.len(), 2, "expected `my $c` and `state $c` declarations, got {decls:?}");
    assert_ne!(ids[0], ids[1], "`my $c` and `state $c` must be distinct bindings");

    // The canonical storage classes stay distinct behind those identities.
    let storages: Vec<StorageClass> = ids.iter().map(|id| storage_of(&file, *id)).collect();
    assert!(
        storages.contains(&StorageClass::LexicalMy)
            && storages.contains(&StorageClass::LexicalState),
        "expected LexicalMy and LexicalState, got {storages:?}"
    );
}

#[test]
fn our_and_my_of_the_same_spelling_remain_distinct_bindings() {
    let source = "package P;\nsub f { our $v = 1; if ($t) { my $v = 2; print $v; } print $v; }\n";
    let file = lower(source);
    let decls = declarations(&file);

    let ids: Vec<HirBindingId> = decls.iter().filter_map(|d| d.binding).collect();
    assert_eq!(ids.len(), 2, "expected `our $v` and `my $v` declarations, got {decls:?}");
    assert_ne!(ids[0], ids[1], "`our $v` and `my $v` must be distinct bindings");

    let storages: Vec<StorageClass> = ids.iter().map(|id| storage_of(&file, *id)).collect();
    assert!(
        storages.contains(&StorageClass::PackageOur) && storages.contains(&StorageClass::LexicalMy),
        "expected PackageOur and LexicalMy, got {storages:?}"
    );

    // The trailing `print $v` sits outside the `if` block and must resolve back
    // to the `our` binding, not the inner lexical.
    let outer_read = occurrences(&file)
        .into_iter()
        .filter(|o| o.name == "v" && o.access == AccessMode::Read)
        .next_back()
        .expect("trailing read of `$v`");
    assert_eq!(
        outer_read.binding.map(|id| storage_of(&file, id)),
        Some(StorageClass::PackageOur),
        "the read after the inner block must resolve to the `our` binding"
    );
}

/// Known boundary, not a claim of correctness: `package NAME;` opens a child
/// scope, but the program-root body still starts at the file scope and the
/// resolution walk only ascends. Declarations made at package top level are
/// therefore invisible to program-root occurrences.
///
/// This behaviour is pre-existing — the previous `VariableKind` walk was
/// identical, and already mis-reported the `my` read below as `Package`.
/// Threading identity only makes the gap visible as `None`. Pinned here so the
/// boundary is explicit and a future fix trips this test instead of silently
/// changing consumer-visible classification.
#[test]
fn package_top_level_declarations_are_not_visible_to_program_root_occurrences() {
    let lexical = lower("package P;\nmy $lex = 1;\nprint $lex;\n");
    let read = occurrences(&lexical)
        .into_iter()
        .find(|o| o.name == "lex" && o.access == AccessMode::Read)
        .expect("read of `$lex`");
    assert_eq!(read.binding, None, "package-top-level `my` is not resolvable from program root");
    assert_eq!(
        read.kind,
        VariableKind::Package,
        "pre-existing misclassification preserved: a package-top-level `my` read reports Package"
    );

    // Without the `package` statement the very same code resolves correctly,
    // which isolates the cause to package-scope descent rather than to `my`.
    let plain = lower("my $lex = 1;\nprint $lex;\n");
    let read = occurrences(&plain)
        .into_iter()
        .find(|o| o.name == "lex" && o.access == AccessMode::Read)
        .expect("read of `$lex`");
    assert!(read.binding.is_some(), "file-scope `my` resolves to its binding");
    assert_eq!(read.kind, VariableKind::Lexical);
}

/// Known boundary, not a claim of correctness: a `foreach my $i` iterator is
/// recorded in the *enclosing* scope rather than a loop-private one, so it does
/// not shadow an outer `my $i` the way Perl does. Combined with same-scope
/// position-insensitivity, the outer declaration and the post-loop read both
/// resolve to the loop's binding.
///
/// This is a scope-graph modelling gap in pass 1, which this slice does not
/// touch — threading identity only makes it observable. Pinned so the boundary
/// is explicit and a future scope fix trips this test.
#[test]
fn foreach_iterator_shares_the_enclosing_scope_and_does_not_shadow() {
    let source = "sub f { my $i = 9; foreach my $i (1,2) { print $i; } print $i; }\n";
    let file = lower(source);

    let bindings: Vec<_> =
        file.scope_graph.bindings.iter().filter(|b| b.name == "i").map(|b| b.scope_id).collect();
    assert_eq!(bindings.len(), 2, "expected an outer `my $i` and a foreach `my $i`");
    assert_eq!(
        bindings[0], bindings[1],
        "the foreach iterator currently shares the enclosing scope rather than opening its own"
    );

    // Every `$i` occurrence therefore collapses onto the later (loop) binding.
    let ids: std::collections::BTreeSet<HirBindingId> = occurrences(&file)
        .into_iter()
        .filter(|o| o.name == "i")
        .filter_map(|o| o.binding)
        .collect();
    assert_eq!(
        ids.len(),
        1,
        "known boundary: same-scope `$i` bindings are position-insensitive, so all occurrences \
         resolve to one binding; found {ids:?}"
    );
}

#[test]
fn signature_parameter_occurrences_resolve_to_parameter_bindings() {
    let source = "use feature 'signatures';\nsub g($a, $b) { return $a + $b; }\n";
    let file = lower(source);
    let body_brace = source.find('{').expect("sub body brace");
    let occs = occurrences(&file);

    for want in ["a", "b"] {
        let occ = occs
            .iter()
            .find(|o| o.name == want && o.start > body_brace)
            .unwrap_or_else(|| panic!("expected an occurrence of `${want}` in the body"));
        let id = occ
            .binding
            .unwrap_or_else(|| panic!("`${want}` occurrence must carry canonical identity"));
        assert_eq!(
            storage_of(&file, id),
            StorageClass::Parameter,
            "`${want}` must resolve to its signature parameter binding"
        );
    }
}

#[test]
fn unresolved_package_global_carries_no_fabricated_identity() {
    let file = lower("print $Foo::bar;\n");
    let occs = occurrences(&file);

    let occ = occs.iter().find(|o| o.name.contains("Foo")).expect("package-global occurrence");
    assert_eq!(
        occ.binding, None,
        "an unresolved package global must carry no binding identity, not a fabricated one"
    );
    assert_eq!(occ.kind, VariableKind::Package, "package global keeps its existing classification");
}

/// An undeclared bare variable is also unresolved — `None`, never a stand-in.
#[test]
fn undeclared_variable_carries_no_identity() {
    let file = lower("sub f { return $never_declared; }\n");
    let occ = occurrences(&file)
        .into_iter()
        .find(|o| o.name == "never_declared")
        .expect("undeclared occurrence");
    assert_eq!(occ.binding, None, "an undeclared variable must carry no binding identity");
}

/// Regression guard: threading identity must not disturb the existing coarse
/// `VariableKind` classification that current consumers already depend on.
#[test]
fn variable_kind_classification_is_unchanged() {
    let file = lower(NESTED);
    for occ in occurrences(&file).iter().filter(|o| o.name == "x") {
        assert_eq!(occ.kind, VariableKind::Lexical, "`my $x` occurrences remain lexical: {occ:?}");
    }

    let pkg = lower("our $g = 1;\nprint $g;\n");
    for occ in occurrences(&pkg).iter().filter(|o| o.name == "g") {
        assert_eq!(occ.kind, VariableKind::Package, "`our $g` occurrences remain package: {occ:?}");
    }
}
