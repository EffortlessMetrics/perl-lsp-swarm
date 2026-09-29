//! Source-backed native-method owner and implicit invocant identity (#16969).
//!
//! `ScopeGraph` already owned `HirBindingId`, scoped
//! `BindingReference.resolved_binding`, and `StorageClass::MethodInvocant`, but
//! `hir/lower.rs::NodeKind::Method` never emitted that binding, and nothing
//! joined a method to the `class` declaration that contains it. A consumer
//! therefore could not prove that a cursor reference belongs to an implicit
//! native invocant, or which source class owns it.
//!
//! The load-bearing properties, in the order they discriminate:
//!
//! 1. a named method of an admitted core block-form class mints **exactly one**
//!    `MethodInvocant` binding, owned by the method pad, and both the scope-graph
//!    reference and the body-HIR occurrence attach to it;
//! 2. binding, method item, and class item/scope agree on identity *and* source
//!    anchor — an implementation that matches on the method name, or that infers
//!    the class from the surrounding `package_context`, cannot;
//! 3. an explicit `$self` parameter, `my ($self) = @_`, and `shift` do **not**
//!    become the implicit binding, and inner lexical shadowing changes resolution
//!    only inside its own scope;
//! 4. an anonymous closure keeps the enclosing invocant's identity, while a
//!    *named* nested `sub` is **not** an exact current invocant even though the
//!    generic lexical scope walk records the very same binding id;
//! 5. sibling classes with the same method name cannot share an owner;
//! 6. an `ADJUST` phaser, a non-admitted or dynamic class feature environment,
//!    and a package-level or statement-form method all yield a typed
//!    unavailability, never an exact class guess.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    Binding, BindingReference, HirBindingId, HirExpr, HirExprId, HirFile, HirId, HirKind,
    HirScopeId, MethodDecl, NativeClassOwner, NativeInvocantLimitation,
    NativeMethodOwnerLimitation, ScopeFrame, ScopeKind, StorageClass, lower_ast,
};
use perl_tdd_support::must_some;

/// Parse `source` and run the canonical two-pass HIR lowering.
fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

// ──────────────────────────────────────────────────────────────────────────────
// Read helpers
// ──────────────────────────────────────────────────────────────────────────────

/// The lowered `method` item named `name`, with its id.
fn method_item(file: &HirFile, name: &str) -> (HirId, MethodDecl) {
    let item = must_some(
        file.items
            .iter()
            .find(|item| matches!(&item.kind, HirKind::MethodDecl(decl) if decl.name == name)),
    );
    let decl = must_some(match &item.kind {
        HirKind::MethodDecl(decl) => Some(decl.clone()),
        _ => None,
    });
    (item.id, decl)
}

/// Every `method` item in lowering order, with its id.
fn all_method_items(file: &HirFile) -> Vec<(HirId, MethodDecl)> {
    file.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::MethodDecl(decl) => Some((item.id, decl.clone())),
            _ => None,
        })
        .collect()
}

/// The exact owner of the method named `name`, failing if it has none.
fn exact_owner(file: &HirFile, name: &str) -> NativeClassOwner {
    let (_, decl) = method_item(file, name);
    must_some(decl.class_owner.as_ref().ok().cloned())
}

/// Scope frame for `id`.
fn frame(file: &HirFile, id: HirScopeId) -> &ScopeFrame {
    let index = must_some(usize::try_from(id.index()).ok());
    must_some(file.scope_graph.scopes.get(index))
}

/// The scope context an item was lowered in.
fn item_scope(file: &HirFile, id: HirId) -> HirScopeId {
    must_some(file.items.iter().find(|item| item.id == id).and_then(|item| item.scope_context))
}

/// Every `StorageClass::MethodInvocant` binding, in lowering order.
fn invocant_bindings(file: &HirFile) -> Vec<&Binding> {
    file.scope_graph
        .bindings
        .iter()
        .filter(|binding| binding.storage == StorageClass::MethodInvocant)
        .collect()
}

/// The first `$self` binding with the given storage class.
fn self_binding_with(file: &HirFile, storage: StorageClass) -> &Binding {
    must_some(file.scope_graph.bindings.iter().find(|binding| {
        binding.sigil == "$" && binding.name == "self" && binding.storage == storage
    }))
}

/// The first scope-graph reference to `$self` anywhere in the file.
fn first_self_reference(file: &HirFile) -> &BindingReference {
    must_some(
        file.scope_graph
            .references
            .iter()
            .find(|reference| reference.sigil == "$" && reference.name == "self"),
    )
}

/// The first scope-graph reference to `$self` that resolved to `binding`.
///
/// Asked by resolved identity rather than by the scope that *declares* the
/// binding: a body reference is recorded in the body frame, which is a
/// different scope from the signature or lexical frame that declared it.
fn reference_resolving_to(file: &HirFile, binding: HirBindingId) -> &BindingReference {
    must_some(
        file.scope_graph
            .references
            .iter()
            .find(|reference| reference.resolved_binding == Some(binding)),
    )
}

/// The first scope-graph reference to `$self` that lives inside `ancestor`,
/// including `ancestor` itself.
fn self_reference_within(file: &HirFile, ancestor: HirScopeId) -> &BindingReference {
    must_some(file.scope_graph.references.iter().find(|reference| {
        reference.sigil == "$"
            && reference.name == "self"
            && (reference.scope_id == ancestor || is_within(file, reference.scope_id, ancestor))
    }))
}

/// Every flattened body-HIR `HirExpr::Variable` named `self`, as
/// `(start offset, canonical binding)`, ordered by source span.
fn self_occurrences(file: &HirFile) -> Vec<(usize, Option<HirBindingId>)> {
    let mut found = Vec::new();
    for body in &file.bodies {
        for idx in 0..body.source_map.expr_ranges.len() {
            let id = HirExprId(idx as u32);
            let Some(HirExpr::Variable(var)) = body.expr(id) else {
                continue;
            };
            if var.name != "self" {
                continue;
            }
            let range = must_some(body.source_map.expr_range(id));
            found.push((range.start, var.binding));
        }
    }
    found.sort_by_key(|(start, _)| *start);
    found
}

/// The single scope frame of `kind`, or the first when several match.
fn first_scope_of_kind(file: &HirFile, kind: ScopeKind) -> HirScopeId {
    must_some(file.scope_graph.scopes.iter().find(|frame| frame.kind == kind).map(|frame| frame.id))
}

/// Whether `descendant` is `ancestor` or nested inside it.
fn is_within(file: &HirFile, descendant: HirScopeId, ancestor: HirScopeId) -> bool {
    let mut cursor = frame(file, descendant).parent;
    while let Some(id) = cursor {
        if id == ancestor {
            return true;
        }
        cursor = frame(file, id).parent;
    }
    false
}

/// The load-bearing fixture: one admitted core class, one named method, one
/// body reference to the implicit invocant.
const CORE_CLASS: &str = r#"use feature 'class';
class Animal {
    method speak { $self->name; }
}
"#;

// ──────────────────────────────────────────────────────────────────────────────
// 1. The implicit invocant exists at all
// ──────────────────────────────────────────────────────────────────────────────

/// A named method of an admitted core class mints exactly one `MethodInvocant`,
/// owned by the method pad and anchored at the method name.
#[test]
fn core_native_method_mints_exactly_one_implicit_invocant() {
    let file = lower(CORE_CLASS);
    let invocants = invocant_bindings(&file);
    assert_eq!(
        invocants.len(),
        1,
        "expected exactly one MethodInvocant binding, got {invocants:#?}"
    );
    let invocant = invocants[0];

    assert_eq!(invocant.sigil, "$");
    assert_eq!(invocant.name, "self");
    assert!(invocant.shadows.is_none(), "the implicit invocant shadows nothing");
    assert_eq!(
        invocant.range.start,
        must_some(CORE_CLASS.find("speak")),
        "the invocant has no token of its own, so it is anchored where Perl injects it"
    );
    assert_eq!(
        invocant.range.end, invocant.range.start,
        "the anchor is a point, not a borrowed range"
    );
    assert_eq!(
        invocant.visible_from, invocant.range.start,
        "the invocant is visible from the method declaration onward"
    );

    let owner = exact_owner(&file, "speak");
    assert_eq!(invocant.scope_id, owner.method_scope, "the invocant belongs to the method pad");
    assert_eq!(
        frame(&file, owner.method_scope).kind,
        ScopeKind::Method,
        "the invocant's owning scope is the method pad, not the class body or a block"
    );
}

/// The body reference and the body-HIR occurrence both attach to that binding,
/// so a consumer reading either surface gets the same identity.
#[test]
fn body_reference_and_body_occurrence_resolve_to_the_implicit_invocant() {
    let file = lower(CORE_CLASS);
    let invocant = must_some(invocant_bindings(&file).first().map(|binding| binding.id));

    assert_eq!(
        first_self_reference(&file).resolved_binding,
        Some(invocant),
        "the scope-graph reference must resolve to the implicit invocant"
    );
    assert_eq!(
        self_occurrences(&file),
        vec![(must_some(CORE_CLASS.rfind("$self")), Some(invocant))],
        "the body-HIR occurrence must carry the same canonical binding identity"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 2. Binding, method item, and class item/scope agree
// ──────────────────────────────────────────────────────────────────────────────

/// Identity and source anchors agree across the binding, the method item, and
/// the class item/scope.
///
/// A name-only implementation cannot satisfy this: it has no class item, no
/// class frame, and no source anchor to compare.
#[test]
fn binding_method_item_and_class_item_agree_on_identity_and_anchor() {
    let file = lower(CORE_CLASS);
    let (method_id, _) = method_item(&file, "speak");
    let owner = exact_owner(&file, "speak");

    let class_id = must_some(
        file.items
            .iter()
            .find(|item| matches!(&item.kind, HirKind::ClassDecl(decl) if decl.name == "Animal"))
            .map(|item| item.id),
    );
    assert_eq!(owner.method_item, method_id, "the owner names its own method item");
    assert_eq!(owner.class_item, class_id, "the owner names the enclosing class declaration");
    assert_eq!(owner.class_name, "Animal");
    assert_eq!(owner.method_scope, item_scope(&file, method_id));

    let class_anchor = must_some(
        file.items.iter().find(|item| item.id == class_id).and_then(|item| item.anchor.name_range),
    );
    let method_anchor = must_some(
        file.items.iter().find(|item| item.id == method_id).and_then(|item| item.anchor.name_range),
    );
    assert_eq!(
        owner.class_name_range, class_anchor,
        "the class name anchor agrees with the class item"
    );
    assert_eq!(
        owner.method_name_range,
        Some(method_anchor),
        "the method name anchor agrees with the method item"
    );
    assert_eq!(owner.method_name, "speak");

    // The class frame is a real class frame whose range covers the class body,
    // and the method pad hangs directly off it. Statement-form membership and
    // package context cannot produce either.
    let class_frame = frame(&file, owner.class_scope);
    assert_eq!(class_frame.kind, ScopeKind::Class);
    assert_eq!(class_frame.parent, Some(item_scope(&file, class_id)));
    assert_eq!(
        frame(&file, owner.method_scope).parent,
        Some(owner.class_scope),
        "the method pad is a direct member of the class body frame"
    );
    assert!(
        class_frame.range.start < class_frame.range.end,
        "the class frame is anchored to the class body source range"
    );

    // The invocant is declared by this very method item, in this very pad.
    let invocant = must_some(file.invocant_binding(&owner));
    assert_eq!(invocant.id, owner.invocant);
    assert_eq!(
        invocant.declaration_item,
        Some(method_id),
        "the invocant is declared by the method item"
    );
    assert_eq!(invocant.scope_id, owner.method_scope);

    // And the same owner is reachable from the pad's own scope.
    assert_eq!(file.exact_invocant_owner(owner.method_scope), Ok(&owner));
}

// ──────────────────────────────────────────────────────────────────────────────
// 3. The implicit binding is not any other `$self`
// ──────────────────────────────────────────────────────────────────────────────

/// A plain signature parameter stays an ordinary `Parameter`, and the implicit
/// invocant is untouched.
#[test]
fn signature_parameters_stay_ordinary_parameters() {
    let file = lower(
        "use feature 'class';\nclass Animal {\n    method speak($arg) { $self->name($arg); }\n}\n",
    );
    let invocants = invocant_bindings(&file);
    assert_eq!(invocants.len(), 1, "a signature parameter must not disturb the invocant count");
    assert_eq!((invocants[0].sigil.as_str(), invocants[0].name.as_str()), ("$", "self"));

    let arg = must_some(
        file.scope_graph
            .bindings
            .iter()
            .find(|binding| binding.sigil == "$" && binding.name == "arg"),
    );
    assert_eq!(arg.storage, StorageClass::Parameter, "`$arg` is an ordinary parameter");
    assert_ne!(arg.id, invocants[0].id, "the parameter must not reuse the invocant identity");
}

/// An explicit same-name `$self` parameter shadows the implicit binding; it
/// does not become it.
#[test]
fn explicit_invocant_parameter_shadows_rather_than_replaces() {
    let file = lower(
        "use feature 'class';\nclass Animal {\n    method speak($self) { $self->name; }\n}\n",
    );
    let invocants = invocant_bindings(&file);
    assert_eq!(invocants.len(), 1, "the implicit invocant still exists exactly once");

    let explicit = self_binding_with(&file, StorageClass::Parameter);
    assert_eq!(
        explicit.shadows,
        Some(invocants[0].id),
        "the explicit parameter shadows the implicit invocant"
    );
    assert_ne!(explicit.id, invocants[0].id, "the explicit parameter is not the implicit binding");
    assert_eq!(
        reference_resolving_to(&file, explicit.id).resolved_binding,
        Some(explicit.id),
        "the body reference resolves to the explicit parameter, not the implicit invocant"
    );
}

/// `my ($self) = @_` likewise shadows rather than becoming the implicit binding.
#[test]
fn explicit_lexical_invocant_does_not_become_the_implicit_binding() {
    let file = lower(
        "use feature 'class';\nclass Animal {\n    method speak { my ($self) = @_; $self->name; }\n}\n",
    );
    let invocants = invocant_bindings(&file);
    assert_eq!(invocants.len(), 1, "the implicit invocant still exists exactly once");

    let lexical = self_binding_with(&file, StorageClass::LexicalMy);
    assert_eq!(lexical.shadows, Some(invocants[0].id));
    assert_ne!(lexical.id, invocants[0].id);
    assert_eq!(reference_resolving_to(&file, lexical.id).resolved_binding, Some(lexical.id));
}

/// Neither a first nor a second `shift` mints an invocant binding, and neither
/// consumes the one implicit binding the method already has.
#[test]
fn shift_never_becomes_the_implicit_invocant_binding() {
    let file = lower(
        "use feature 'class';\nclass Animal {\n    method speak { my $a = shift; my $b = shift @_; $self->name($a, $b); }\n}\n",
    );
    let invocants = invocant_bindings(&file);
    assert_eq!(invocants.len(), 1, "a first and a second `shift` must not add invocant bindings");
    assert_eq!((invocants[0].sigil.as_str(), invocants[0].name.as_str()), ("$", "self"));
    assert_eq!(
        file.scope_graph
            .bindings
            .iter()
            .filter(|binding| binding.sigil == "$" && binding.name == "self")
            .count(),
        1,
        "there is exactly one `$self` binding in the file"
    );
}

/// Inner lexical shadowing changes the resolved binding only inside its own
/// scope; the implicit invocant itself is unchanged.
#[test]
fn inner_shadow_changes_resolution_only_inside_its_scope() {
    let source = "use feature 'class';\nclass Animal {\n    method speak { if ($ok) { my $self = 1; $self } $self->name; }\n}\n";
    let file = lower(source);
    let invocant = must_some(invocant_bindings(&file).first().map(|binding| binding.id));
    let shadow = self_binding_with(&file, StorageClass::LexicalMy);
    let shadow_range = frame(&file, shadow.scope_id).range;

    assert_eq!(shadow.shadows, Some(invocant), "the inner lexical shadows the invocant");
    assert_ne!(shadow.id, invocant, "the shadow is a distinct binding identity");
    assert_eq!(invocant_bindings(&file).len(), 1, "shadowing must not mint a second invocant");

    // Split the body occurrences by whether they sit inside the shadowing
    // block. Asking "where does this occurrence resolve?" per position is the
    // discriminating property; counting occurrences is not, because lowering a
    // declaration also emits an occurrence for it.
    let (inside, outside): (Vec<_>, Vec<_>) = self_occurrences(&file)
        .into_iter()
        .partition(|(start, _)| *start > shadow_range.start && *start < shadow_range.end);

    assert!(!inside.is_empty(), "premise: the shadowing block has a `$self` occurrence");
    assert!(
        !outside.is_empty(),
        "premise: the method body has a `$self` occurrence after the block"
    );
    assert!(
        inside.iter().all(|(_, binding)| *binding == Some(shadow.id)),
        "every occurrence inside the shadowing block resolves to the shadow, got {inside:?}"
    );
    assert!(
        outside.iter().all(|(_, binding)| *binding == Some(invocant)),
        "every occurrence outside the block still resolves to the invocant, got {outside:?}"
    );
    assert_eq!(
        reference_resolving_to(&file, shadow.id).scope_id,
        shadow.scope_id,
        "the scope-graph reference inside the block attaches to the shadow"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 4. The typed lookup is more than a lexical scope walk
// ──────────────────────────────────────────────────────────────────────────────

/// An anonymous closure keeps the enclosing invocant's identity.
#[test]
fn anonymous_closure_retains_outer_invocant_identity() {
    let file = lower(
        "use feature 'class';\nclass Animal {\n    method speak { my $c = sub { $self }; }\n}\n",
    );
    let owner = exact_owner(&file, "speak");
    let closure_scope = first_scope_of_kind(&file, ScopeKind::AnonymousSubroutine);

    assert_eq!(
        file.exact_invocant_owner(closure_scope),
        Ok(&owner),
        "an anonymous closure runs inside the enclosing method's invocant"
    );
    // The closure really does lexically see the invocant, so this is not a
    // fixture where the name happens to be absent.
    assert_eq!(self_reference_within(&file, closure_scope).resolved_binding, Some(owner.invocant));
}

/// A *named* nested `sub` is not an exact current invocant — even though the
/// generic lexical scope walk records the very same binding id for its `$self`.
///
/// Both halves are load-bearing. Without the first, a `resolved_binding`-only
/// implementation would pass; without the second, an implementation that
/// blanket-reports "not a method" for every nested callable would pass.
#[test]
fn named_nested_sub_is_not_an_exact_current_invocant() {
    let file = lower(
        "use feature 'class';\nclass Animal {\n    method speak { sub inner { $self } $self->name; }\n}\n",
    );
    let invocant = must_some(invocant_bindings(&file).first().map(|binding| binding.id));
    let nested_scope = first_scope_of_kind(&file, ScopeKind::Subroutine);

    assert_eq!(
        self_reference_within(&file, nested_scope).resolved_binding,
        Some(invocant),
        "premise: the lexical walk really does record the outer invocant id here"
    );
    assert_eq!(
        file.exact_invocant_owner(nested_scope),
        Err(NativeInvocantLimitation::NamedSubroutineBoundary),
        "a named sub is a package-level declaration, not code inside that invocant"
    );

    // Descendants keep the boundary rather than inheriting the method's
    // invocant through the frames between them.
    let descendants: Vec<HirScopeId> = file
        .scope_graph
        .scopes
        .iter()
        .filter(|candidate| is_within(&file, candidate.id, nested_scope))
        .map(|candidate| candidate.id)
        .collect();
    assert!(!descendants.is_empty(), "premise: the named sub has frames nested inside it");
    for scope in descendants {
        assert_eq!(
            file.exact_invocant_owner(scope),
            Err(NativeInvocantLimitation::NamedSubroutineBoundary),
            "descendant scope {} must not inherit the method's invocant",
            scope.index()
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// 5. Sibling classes cannot share an owner
// ──────────────────────────────────────────────────────────────────────────────

/// Two classes declaring the same method name get distinct owners, invocants,
/// and class frames.
///
/// This is the falsifier for a name-only or `package_context`-derived owner.
#[test]
fn sibling_classes_with_the_same_method_name_cannot_share_an_owner() {
    let file = lower(
        "use feature 'class';\nclass A { method speak { $self } }\nclass B { method speak { $self } }\n",
    );
    let methods = all_method_items(&file);
    assert_eq!(methods.len(), 2, "expected two method items, got {methods:#?}");
    let first = &methods[0].1;
    let second = &methods[1].1;
    let a = must_some(first.class_owner.as_ref().ok().cloned());
    let b = must_some(second.class_owner.as_ref().ok().cloned());

    assert_eq!(a.method_name, b.method_name, "premise: both methods share a name");
    assert_eq!((a.class_name.as_str(), b.class_name.as_str()), ("A", "B"));
    assert_ne!(a.class_item, b.class_item, "a name-only owner cannot tell these classes apart");
    assert_ne!(a.class_scope, b.class_scope, "the owner is keyed by class frame, not by name");
    assert_ne!(a.invocant, b.invocant, "each method mints its own invocant");
    assert_eq!(invocant_bindings(&file).len(), 2, "one invocant per method, not one per name");

    assert_eq!(a.method_scope, item_scope(&file, a.method_item));
    assert_eq!(b.method_scope, item_scope(&file, b.method_item));
    assert_eq!(
        file.exact_invocant_owner(a.method_scope),
        Ok(&a),
        "each method pad resolves to its own owner"
    );
    assert_eq!(file.exact_invocant_owner(b.method_scope), Ok(&b));
}

// ──────────────────────────────────────────────────────────────────────────────
// 6. Typed unavailability, never a guess
// ──────────────────────────────────────────────────────────────────────────────

/// An `ADJUST` phaser reaches this arm as a `NodeKind::Method` with no name
/// span. It must not gain an owner, and it must not consume the sibling
/// method's single implicit invocant.
#[test]
fn adjust_phaser_gets_no_invocant_owner_and_no_invocant_binding() {
    let file = lower(
        "use feature 'class';\nclass Animal {\n    ADJUST { 1 }\n    method speak { $self->name; }\n}\n",
    );
    let (adjust_id, adjust) = method_item(&file, "ADJUST");
    assert_eq!(
        adjust.class_owner,
        Err(NativeMethodOwnerLimitation::UnnamedMethodAnchor),
        "a phaser is not a named callable method"
    );
    assert_eq!(
        file.method_owner_limitation(adjust_id),
        Some(NativeMethodOwnerLimitation::UnnamedMethodAnchor)
    );
    assert_eq!(file.native_class_owner(adjust_id), None);

    let invocants = invocant_bindings(&file);
    assert_eq!(invocants.len(), 1, "only the named method mints an invocant");
    let speak = exact_owner(&file, "speak");
    assert_eq!(
        invocants[0].scope_id, speak.method_scope,
        "the invocant belongs to `speak`, not to the phaser pad"
    );
    assert_ne!(
        invocants[0].scope_id,
        item_scope(&file, adjust_id),
        "the phaser pad mints no invocant"
    );
}

/// A class whose `class` feature is not in effect is a typed limitation, not a
/// name-based guess.
#[test]
fn class_feature_must_be_admitted() {
    let file = lower("class Animal {\n    method speak { $self->name; }\n}\n");
    let (method_id, decl) = method_item(&file, "speak");
    assert_eq!(decl.class_owner, Err(NativeMethodOwnerLimitation::ClassFeatureNotAdmitted));
    assert_eq!(file.native_class_owner(method_id), None);
    assert!(invocant_bindings(&file).is_empty(), "an unadmitted class mints no invocant");
    assert_eq!(
        file.exact_invocant_owner(item_scope(&file, method_id)),
        Err(NativeInvocantLimitation::OwnerUnavailable(
            NativeMethodOwnerLimitation::ClassFeatureNotAdmitted
        ))
    );
}

/// A dynamic pragma argument before the class leaves the feature set
/// undecidable, which is its own limitation rather than "not admitted".
#[test]
fn dynamic_pragma_environment_is_its_own_limitation() {
    let file = lower(
        "use feature 'class';\nuse feature $dynamic;\nclass Animal { method speak { $self } }\n",
    );
    let (_, decl) = method_item(&file, "speak");
    assert_eq!(decl.class_owner, Err(NativeMethodOwnerLimitation::DynamicPragmaEnvironment));
    assert_ne!(
        decl.class_owner,
        Err(NativeMethodOwnerLimitation::ClassFeatureNotAdmitted),
        "a dynamic environment is not the same claim as an unadmitted feature"
    );
}

/// A package-level `method` has no class frame to own it, so it is unavailable.
/// This is the falsifier for a surrounding-`package_context` implementation.
#[test]
fn package_level_method_has_no_native_class_owner() {
    let file = lower("use feature 'class';\npackage Foo;\nmethod speak { $self; }\n");
    let (method_id, decl) = method_item(&file, "speak");
    assert_eq!(decl.class_owner, Err(NativeMethodOwnerLimitation::NotInBlockFormClass));
    assert!(invocant_bindings(&file).is_empty());
    assert!(
        file.items.iter().all(|item| !matches!(&item.kind, HirKind::ClassDecl(_))),
        "premise: there is no class declaration to own this method"
    );
    // The package frame is a real frame carrying the right package context —
    // the exact data a `package_context`-based owner would key on — and it
    // still cannot own the method.
    let package_scope = first_scope_of_kind(&file, ScopeKind::Package);
    assert_eq!(frame(&file, package_scope).package_context.as_deref(), Some("Foo"));
    assert_eq!(frame(&file, item_scope(&file, method_id)).parent, Some(package_scope));
}

/// A `method` with no enclosing class body — the shape statement-form class
/// membership would produce once #10346 supplies its source-order semantic
/// owner — is unavailable, not guessed.
#[test]
fn method_without_an_enclosing_class_frame_has_no_owner() {
    let file = lower("use feature 'class';\nmethod speak { $self; }\n");
    let method_id = must_some(all_method_items(&file).first().map(|(id, _)| *id));
    assert_eq!(
        file.method_owner_limitation(method_id),
        Some(NativeMethodOwnerLimitation::NotInBlockFormClass)
    );
    assert_eq!(
        file.exact_invocant_owner(item_scope(&file, method_id)),
        Err(NativeInvocantLimitation::OwnerUnavailable(
            NativeMethodOwnerLimitation::NotInBlockFormClass
        ))
    );
    assert!(invocant_bindings(&file).is_empty());
}

/// A scope outside any method — the file scope itself — is not inside an
/// invocant.
#[test]
fn file_scope_is_not_inside_an_invocant() {
    let file = lower(CORE_CLASS);
    assert_eq!(
        file.exact_invocant_owner(HirScopeId::from_index(0)),
        Err(NativeInvocantLimitation::NotInMethod)
    );
}
