//! HIR storage identity for core-class `field` declarations (issue #13817,
//! slice of #6672's "core class syntax → field declarations, storage identity").
//!
//! A Perl 5.38+ `field` declaration parses to
//! `NodeKind::VariableDeclaration { declarator: "field", .. }`, so the
//! declarator reaches HIR intact. Before this fix, `storage_class_for_declarator`
//! had no `field` arm, so the declaration fell through to
//! `StorageClass::PackageGlobal`. Two consequences followed:
//!
//! 1. in the scope graph, `field $x` was indistinguishable from an undeclared
//!    package global;
//! 2. `resolve_variable_kind` maps `PackageGlobal` to `VariableKind::Package`,
//!    so a reference to `$x` inside a method resolved exactly like a reference
//!    to a never-declared `$undeclared` — a read of the package stash rather
//!    than of the field.
//!
//! Both consequences are repaired here, because they are one representation:
//! a storage class no consumer can observe is not an identity.
//!
//! Consequence 1 is the declaration side, plus the boundaries that make it
//! honest — only a `field` that is a direct statement of a class body is a
//! declaration at all, because the parser accepts `field` as a declarator
//! wherever the next token starts a variable.
//!
//! Consequence 2 is the reference side. A visible field now resolves
//! [`VariableKind::Field`], and PIR carries it as `FieldRead`/`FieldWrite`, so
//! `extract_lexical_facts` never publishes a field as a `LexicalBindingFact`.
//! "Visible" is Perl's rule, decided in the single scope walk both HIR views
//! share: a field belongs to its own class, is seen by that class's methods but
//! not by a named `sub`, and only after its own declaration.
//!
//! Two of those need more than the obvious check. Class ownership is structural
//! for siblings, whose frames are not ancestors of each other, but a *nested*
//! class's frame really is inside the outer one, so the walk has to stop
//! claiming fields once it leaves a class. And a named `sub` blocks from
//! anywhere on the path, not just innermost — a sub written inside a method
//! still gets no field access, and neither does anything inside it — while an
//! anonymous `sub` is transparent, because `sub { ... }` closes over its
//! enclosing pad the way any closure does.
//!
//! Each condition has a discriminating control below and a same-shape positive
//! case, so a resolver that simply never resolved fields would fail.
//!
//! Scope note: this names *storage and visibility*. Construction order,
//! `ADJUST`, invocant rules, MRO and dispatch remain unmodeled (#6672).
//!
//! The implementation lives in `src/hir/lower.rs`
//! (`storage_class_for_declarator`, `resolve_binding_in_scope_graph`,
//! `class_field_is_visible`).

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    Binding, BodyOwnerKind, HirBody, HirExpr, HirFile, ScopeKind, StorageClass, VariableKind,
    lower_ast,
};
use perl_parser_core::pir::{PirOperation, extract_lexical_facts, lower_hir_bodies};

type TestResult = Result<(), String>;

/// A class body exercising `field` beside the other declarators, plus a method
/// that reads a field, a lexical, and an undeclared global.
const CLASS_SOURCE: &str = "use feature 'class';\n\
     class Point {\n\
     \x20   field $x;\n\
     \x20   field $y :param = 0;\n\
     \x20   my $lex;\n\
     \x20   our $pkg;\n\
     \x20   state $st;\n\
     \x20   method show { $x + $y + $lex + $undeclared }\n\
     }\n";

fn lower_source(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// Look up the single scope-graph binding for `name`.
fn binding<'a>(file: &'a HirFile, name: &str) -> Result<&'a Binding, String> {
    let mut found = file.scope_graph.bindings.iter().filter(|b| b.name == name);
    let first = found.next().ok_or_else(|| format!("no binding recorded for `{name}`"))?;
    if found.next().is_some() {
        return Err(format!("expected exactly one binding for `{name}`"));
    }
    Ok(first)
}

/// The lowered body for `method <name>`.
fn method_body<'a>(file: &'a HirFile, name: &str) -> Result<&'a HirBody, String> {
    file.bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Method { name: n } if n == name))
        .ok_or_else(|| format!("no lowered body for method `{name}`"))
}

/// Resolved [`VariableKind`] for the single `$name` reference in a body.
fn variable_kind(body: &HirBody, name: &str) -> Result<VariableKind, String> {
    body.exprs
        .iter()
        .find_map(|expr| match expr {
            HirExpr::Variable(v) if v.name == name => Some(v.kind),
            _ => None,
        })
        .ok_or_else(|| format!("no `${name}` reference lowered in the method body"))
}

// ---------------------------------------------------------------------------
// Reference classification: a field read is neither lexical nor package
// ---------------------------------------------------------------------------

#[test]
fn field_reference_in_a_method_resolves_as_a_field() -> TestResult {
    // The whole point of the storage class. A method reading its own class's
    // field must not answer `Package` — that sends the read to the package
    // stash, the same answer a never-declared global gets — and must not
    // answer `Lexical`, which asserts downstream that a field *is* an ordinary
    // lexical binding.
    let file = lower_source(CLASS_SOURCE);
    let body = method_body(&file, "show")?;
    let kind = variable_kind(body, "x")?;
    assert_eq!(kind, VariableKind::Field, "a field read in a method must resolve as a field");
    assert_ne!(kind, VariableKind::Package, "a field read must not be a package-stash read");
    assert_ne!(kind, VariableKind::Lexical, "a field read must not claim lexical storage");
    Ok(())
}

#[test]
fn a_field_read_lowers_to_a_field_pir_operation() -> TestResult {
    // The reference must keep its identity all the way into PIR. `FieldRead`
    // exists so `pir::extractor` never folds a field into a
    // `LexicalBindingFact`, and so the reference is still present for
    // navigation rather than dropped.
    let mut parser = Parser::new(CLASS_SOURCE);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let pir = lower_hir_bodies(&hir);

    let mut saw_field_read = false;
    for node in pir.nodes.iter() {
        match &node.operation {
            PirOperation::FieldRead { name } if name.name == "x" => saw_field_read = true,
            PirOperation::LexicalRead { name } | PirOperation::LexicalWrite { name }
                if name.name == "x" =>
            {
                return Err("a field must never lower to a lexical PIR operation".to_string());
            }
            _ => {}
        }
    }
    if !saw_field_read {
        return Err("expected a `FieldRead` for the field read in `method show`".to_string());
    }
    Ok(())
}

#[test]
fn a_field_read_produces_no_lexical_binding_fact() -> TestResult {
    // The downstream consequence, asserted where it is actually consumed:
    // `LexicalBindingFact` drives navigation and reference detection, so a
    // field appearing there would be a false claim about its storage, not
    // merely a lossy one.
    let mut parser = Parser::new(CLASS_SOURCE);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let receipt = extract_lexical_facts(&hir);
    let named = |wanted: &str| {
        receipt.bodies.iter().any(|body| body.facts.iter().any(|fact| fact.name.name == wanted))
    };

    if named("x") {
        return Err("a field read must not be extracted as a lexical binding fact".to_string());
    }
    // Negative control: the ordinary lexical in the same method still is one,
    // so this test cannot pass merely because extraction produced nothing.
    if !named("lex") {
        return Err("the ordinary `my $lex` read must still be a lexical binding fact".to_string());
    }
    Ok(())
}

#[test]
fn a_field_compound_assignment_is_counted_as_a_skipped_modification() -> TestResult {
    // `skipped_node_count` exists so the receipt surface is honest about which
    // operations were filtered out of the lexical-fact model. A compound
    // read-modify-write is filtered; all three storage classes must therefore
    // count, or the receipt claims nothing was dropped when something was.
    //
    // The two existing families are the reference point rather than a fixed
    // number, so this cannot pass vacuously: if the counter stopped working
    // entirely, the lexical and stash rows would go to zero and the assertion
    // below would still hold — which is why each is asserted to be 1 as well.
    let counted = |source: &str| -> usize {
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        extract_lexical_facts(&lower_ast(&output.ast)).skipped_node_count
    };

    let lexical = counted("sub f { my $n = 1; $n += 2; }");
    let stash = counted("sub f { $n += 2; }");
    let field =
        counted("use feature 'class';\nclass C {\n    field $n;\n    method m { $n += 2; }\n}\n");

    assert_eq!(lexical, 1, "a lexical compound assignment must count as skipped");
    assert_eq!(stash, 1, "a stash compound assignment must count as skipped");
    assert_eq!(
        field, 1,
        "a field compound assignment must count as skipped like the other two, \
         got lexical={lexical}, stash={stash}, field={field}"
    );
    Ok(())
}

#[test]
fn an_aggregate_field_resolves_when_referenced_whole() -> TestResult {
    // `field @items` and `field %data` are ordinary field declarations, and a
    // whole-aggregate reference to either resolves as a field. Nothing else
    // here covers a non-scalar sigil.
    for (declaration, reference, name) in
        [("field @items;", "@items", "items"), ("field %data;", "%data", "data")]
    {
        let source = format!(
            "use feature 'class';\nclass C {{\n    {declaration}\n    method m {{ {reference} }}\n}}\n"
        );
        let file = lower_source(&source);
        let body = method_body(&file, "m")?;
        assert_eq!(
            variable_kind(body, name)?,
            VariableKind::Field,
            "a whole-aggregate field reference must resolve as a field in {source:?}"
        );
    }
    Ok(())
}

#[test]
fn an_aggregate_element_reference_resolves_the_same_for_field_and_my() -> TestResult {
    // Boundary, pinned rather than left implicit. `$items[0]` carries the
    // scalar sigil while the binding records `@items`, and the scope graph
    // matches sigils exactly — so an element reference does not reach its
    // declaration. That is a pre-existing property of the scope graph, not
    // something field storage introduced: `my @items` behaves identically, and
    // this test fails if the two ever diverge.
    //
    // The point is the *equality*. If element resolution is later repaired
    // (#14682), both sides move together and this test fails, which is the
    // signal to update it rather than a regression.
    let field_file = lower_source(
        "use feature 'class';\nclass C {\n    field @items;\n    method m { $items[0] }\n}\n",
    );
    let field_kind = variable_kind(method_body(&field_file, "m")?, "items")?;

    let my_file = lower_source("sub f { my @items; my $x = $items[0]; }");
    let my_body = my_file
        .bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Subroutine { name: Some(n) } if n == "f"))
        .ok_or_else(|| "no lowered body for sub `f`".to_string())?;
    let my_kind = variable_kind(my_body, "items")?;

    assert_eq!(
        field_kind, my_kind,
        "an aggregate element reference must resolve the same for `field` and `my`; \
         field gave {field_kind:?}, my gave {my_kind:?}"
    );
    assert_eq!(
        field_kind,
        VariableKind::Package,
        "both are currently unresolved element references (#14682)"
    );
    Ok(())
}

#[test]
fn an_initialized_field_declaration_emits_no_lexical_operation() -> TestResult {
    // `field $y = 1` looks like a declaration with an initializer, which is
    // the shape that would ordinarily lower to a `LexicalWrite` and become a
    // write-role `LexicalBindingFact`. It must not, since a field is not a
    // lexical.
    //
    // It currently cannot, for a structural reason worth pinning rather than
    // relying on: a `class` body is not lowered into a HIR body arena at all
    // (#13844), so no class-level statement — field, `my`, or otherwise —
    // reaches the body-arena declarator table. Only method bodies are lowered.
    // If that changes, the declarator table has to learn about fields, and
    // this test is what says so.
    let source = "use feature 'class';\nclass C {\n    field $y = 1;\n    method m { $y }\n}\n";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);

    for node in lower_hir_bodies(&hir).nodes.iter() {
        match &node.operation {
            PirOperation::LexicalRead { name }
            | PirOperation::LexicalWrite { name }
            | PirOperation::Modify { name, .. }
                if name.name == "y" =>
            {
                return Err(format!(
                    "an initialized field must not lower to a lexical operation, got {:?}",
                    node.operation
                ));
            }
            _ => {}
        }
    }
    let receipt = extract_lexical_facts(&hir);
    if receipt.bodies.iter().any(|body| body.facts.iter().any(|fact| fact.name.name == "y")) {
        return Err("an initialized field must not become a lexical binding fact".to_string());
    }
    Ok(())
}

#[test]
fn a_field_is_not_visible_before_its_own_declaration() -> TestResult {
    // Declaration-order visibility. A method written above the field it names
    // does not see it, so the reference falls back exactly as if no field had
    // been declared. The rule lives in the shared scope walk, so both HIR
    // views answer this the same way.
    let file =
        lower_source("use feature 'class';\nclass C {\n    method m { $x }\n    field $x;\n}\n");
    let body = method_body(&file, "m")?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Package,
        "a field declared after a method must not be visible inside it"
    );
    Ok(())
}

#[test]
fn a_field_declared_before_a_method_is_visible_inside_it() -> TestResult {
    // The discriminating half of the ordering control: same two statements,
    // opposite order. Without this, a resolver that simply never resolved
    // fields would pass the test above.
    let file =
        lower_source("use feature 'class';\nclass C {\n    field $x;\n    method m { $x }\n}\n");
    let body = method_body(&file, "m")?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Field,
        "a field declared before a method must be visible inside it"
    );
    Ok(())
}

#[test]
fn fields_do_not_leak_into_a_sibling_class() -> TestResult {
    // Class ownership. `class B`'s method names `$secret`, which belongs to
    // `class A`. A's frame is not an ancestor of B's, so the field is not
    // reachable — but that has to be proved, not assumed, because both classes
    // sit in the same file and the same package context.
    let file = lower_source(
        "use feature 'class';\nclass A {\n    field $secret;\n}\nclass B {\n    method peek { $secret }\n}\n",
    );
    let body = method_body(&file, "peek")?;
    assert_eq!(
        variable_kind(body, "secret")?,
        VariableKind::Package,
        "a sibling class's field must not be visible"
    );
    Ok(())
}

#[test]
fn a_named_sub_inside_a_method_does_not_inherit_the_method_s_fields() -> TestResult {
    // The sub is nested inside a method, so the walk out to the class frame
    // crosses a `Method` frame as well as a `Subroutine` one. Only the
    // innermost callable decides: a named sub is not a method, and an
    // enclosing method does not lend it field access.
    let file = lower_source(
        "use feature 'class';\nclass C {\n    field $x;\n    method m { sub inner { $x } }\n}\n",
    );
    let body = file
        .bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Subroutine { name: Some(n) } if n == "inner"))
        .ok_or_else(|| "no lowered body for sub `inner`".to_string())?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Package,
        "a named sub inside a method must not resolve the class's field"
    );
    Ok(())
}

#[test]
fn an_anonymous_closure_in_a_method_captures_the_field() -> TestResult {
    // `sub { ... }` closes over its enclosing pad, so a closure built inside a
    // method captures the field the way it captures a lexical. This is the
    // positive case that keeps the named-sub rule from being "any sub frame
    // blocks", which would refuse a legitimate capture.
    let file = lower_source(
        "use feature 'class';\nclass C {\n    field $x;\n    method m { return sub { $x } }\n}\n",
    );
    let body = file
        .bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Subroutine { name: None }))
        .ok_or_else(|| "no lowered body for the anonymous sub".to_string())?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Field,
        "an anonymous closure inside a method must capture the field"
    );
    Ok(())
}

#[test]
fn an_anonymous_closure_inside_an_ordinary_sub_still_sees_nothing() -> TestResult {
    // Anonymous frames are transparent, not permissive. The closure is built
    // inside an ordinary `sub`, which has no field access to lend it, so the
    // named frame on the path still blocks. A rule that looked only at the
    // innermost callable would wrongly allow this.
    let file = lower_source(
        "use feature 'class';\nclass C {\n    field $x;\n    sub f { return sub { $x } }\n}\n",
    );
    let body = file
        .bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Subroutine { name: None }))
        .ok_or_else(|| "no lowered body for the anonymous sub".to_string())?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Package,
        "a closure built inside an ordinary sub must not reach the field"
    );
    Ok(())
}

#[test]
fn only_an_anonymous_sub_opens_an_anonymous_frame() -> TestResult {
    // Negative control for the frame split: a named declaration keeps
    // `Subroutine`, so the two are actually distinguished in the scope graph
    // rather than every sub becoming anonymous.
    let file = lower_source("sub named { 1 }\nmy $anon = sub { 2 };\n");
    let kinds: Vec<_> = file
        .scope_graph
        .scopes
        .iter()
        .map(|scope| scope.kind)
        .filter(|kind| matches!(kind, ScopeKind::Subroutine | ScopeKind::AnonymousSubroutine))
        .collect();
    assert_eq!(
        kinds,
        vec![ScopeKind::Subroutine, ScopeKind::AnonymousSubroutine],
        "the named sub keeps `Subroutine` and only the anonymous one is `AnonymousSubroutine`"
    );
    Ok(())
}

#[test]
fn a_nested_class_does_not_inherit_the_outer_class_s_fields() -> TestResult {
    // The outer class's frame *is* an ancestor of the inner class's method, so
    // unlike the sibling case this one is not structural — the walk has to stop
    // claiming fields once it leaves a class.
    let file = lower_source(
        "use feature 'class';\nclass Outer {\n    field $outer_only;\n    class Inner {\n        method peek { $outer_only }\n    }\n}\n",
    );
    let body = method_body(&file, "peek")?;
    assert_eq!(
        variable_kind(body, "outer_only")?,
        VariableKind::Package,
        "a nested class must not resolve the enclosing class's field"
    );
    Ok(())
}

#[test]
fn a_nested_class_still_resolves_its_own_field() -> TestResult {
    // The discriminating half: same nesting, but the field belongs to the
    // inner class. Without this, refusing every field inside a nested class
    // would pass the test above.
    let file = lower_source(
        "use feature 'class';\nclass Outer {\n    field $outer_only;\n    class Inner {\n        field $mine;\n        method peek { $mine }\n    }\n}\n",
    );
    let body = method_body(&file, "peek")?;
    assert_eq!(
        variable_kind(body, "mine")?,
        VariableKind::Field,
        "a nested class must still resolve its own field"
    );
    Ok(())
}

#[test]
fn a_class_body_opens_a_class_scope_frame() -> TestResult {
    // The frame that owns the three visibility answers above. Before this, a
    // class body was an ordinary `Block`, so sibling isolation was incidental
    // to block nesting rather than a property of the class.
    let file = lower_source(CLASS_SOURCE);
    let field = binding(&file, "x")?;
    let scope = file
        .scope_graph
        .scopes
        .get(field.scope_id.index() as usize)
        .ok_or_else(|| "field binding names a scope that does not exist".to_string())?;
    assert_eq!(scope.kind, ScopeKind::Class, "a field must be bound in the class frame");
    Ok(())
}

#[test]
fn a_plain_block_is_still_a_plain_block() -> TestResult {
    // Negative control for the frame: only a class body earns `Class`. A bare
    // block that happens to declare a variable must not.
    let file = lower_source("{\n    my $inner;\n}\n");
    let inner = binding(&file, "inner")?;
    let scope = file
        .scope_graph
        .scopes
        .get(inner.scope_id.index() as usize)
        .ok_or_else(|| "binding names a scope that does not exist".to_string())?;
    assert_eq!(scope.kind, ScopeKind::Block, "a bare block must not become a class frame");
    Ok(())
}

#[test]
fn undeclared_global_in_method_still_resolves_to_package() -> TestResult {
    // Reference classification is untouched for every other case too.
    let file = lower_source(CLASS_SOURCE);
    let body = method_body(&file, "show")?;
    assert_eq!(
        variable_kind(body, "undeclared")?,
        VariableKind::Package,
        "an undeclared variable must still resolve as a package global"
    );
    Ok(())
}

#[test]
fn an_ordinary_sub_in_a_class_body_does_not_see_a_field() -> TestResult {
    // Perl does not give ordinary subs access to class fields. The sub sits
    // directly inside the class frame, so the field binding *is* on its scope
    // chain — the walk has to refuse it, which is exactly what makes this a
    // discriminating control rather than a restatement of scope nesting.
    let file =
        lower_source("use feature 'class';\nclass C {\n    field $x;\n    sub f { $x }\n}\n");
    let body = file
        .bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Subroutine { name: Some(n) } if n == "f"))
        .ok_or_else(|| "no lowered body for sub `f`".to_string())?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Package,
        "an ordinary sub must not resolve a class field"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Scope-graph storage identity
// ---------------------------------------------------------------------------

#[test]
fn field_binding_uses_class_field_storage() -> TestResult {
    let file = lower_source(CLASS_SOURCE);
    for name in ["x", "y"] {
        let b = binding(&file, name)?;
        assert_eq!(
            b.storage,
            StorageClass::ClassField,
            "`field ${name}` must record class-field storage, got {:?}",
            b.storage
        );
    }
    Ok(())
}

#[test]
fn field_storage_is_not_package_global_or_lexical_my() -> TestResult {
    let file = lower_source(CLASS_SOURCE);
    let field_binding = binding(&file, "x")?;

    assert_ne!(
        field_binding.storage,
        StorageClass::PackageGlobal,
        "a class field must not be modeled as a package global"
    );
    assert_ne!(
        field_binding.storage,
        binding(&file, "lex")?.storage,
        "`field $x` and `my $lex` must not share a storage identity"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: every other declarator keeps its existing identity
// ---------------------------------------------------------------------------

#[test]
fn other_declarators_keep_their_storage() -> TestResult {
    let file = lower_source(CLASS_SOURCE);

    assert_eq!(binding(&file, "lex")?.storage, StorageClass::LexicalMy, "`my` must stay LexicalMy");
    assert_eq!(
        binding(&file, "pkg")?.storage,
        StorageClass::PackageOur,
        "`our` must stay PackageOur"
    );
    assert_eq!(
        binding(&file, "st")?.storage,
        StorageClass::LexicalState,
        "`state` must stay LexicalState"
    );

    let local_file = lower_source("our $g;\nsub f { local $g = 1; }\n");
    let localized = local_file
        .scope_graph
        .bindings
        .iter()
        .find(|b| b.storage == StorageClass::LocalizedPackage)
        .ok_or_else(|| "`local` must still record LocalizedPackage".to_string())?;
    assert_eq!(localized.name, "g", "the localized binding must be `$g`");
    Ok(())
}

#[test]
fn ordinary_lexical_reference_is_unchanged() -> TestResult {
    // `my $lex` declared in the class body and read in a method was already
    // Lexical and must stay Lexical.
    let file = lower_source(CLASS_SOURCE);
    let body = method_body(&file, "show")?;
    assert_eq!(variable_kind(body, "lex")?, VariableKind::Lexical, "`my` must stay Lexical");
    Ok(())
}

// ---------------------------------------------------------------------------
// Boundary: `field` outside a class body is an ordinary identifier
// ---------------------------------------------------------------------------

#[test]
fn field_as_ordinary_identifier_records_no_field_binding() -> TestResult {
    // `field` is only a declarator when the next token starts a variable; the
    // parser disambiguates. Legacy uses must not acquire class-field storage.
    let file = lower_source("my %h = (field => 1);\nfield();\n");
    assert!(
        file.scope_graph.bindings.iter().all(|b| b.storage != StorageClass::ClassField),
        "non-declarator `field` must not record class-field storage"
    );
    Ok(())
}

#[test]
fn legacy_field_call_outside_a_class_is_not_class_field_storage() -> TestResult {
    // The parser accepts `field` as a declarator whenever the next token
    // starts a variable, with no class or feature gate. So a legacy program
    // that calls its own `field` sub with a variable argument parses exactly
    // like a field declaration.
    //
    // In Perl this is a call, and `$x` is the package variable — it must not
    // acquire class-field storage, and `$x` in `show` must stay a package
    // read. Ungated, this regressed to `Lexical`.
    let source = "sub field { 1 }\nour $x;\nfield $x;\nsub show { $x }\n";
    let file = lower_source(source);

    assert!(
        file.scope_graph.bindings.iter().all(|b| b.storage != StorageClass::ClassField),
        "`field $x` outside a class body is a call, not a class-field declaration"
    );

    let body = file
        .bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Subroutine { name: Some(n) } if n == "show"))
        .ok_or_else(|| "no lowered body for sub `show`".to_string())?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Package,
        "`$x` in a legacy sub must stay a package read"
    );
    Ok(())
}

#[test]
fn field_call_nested_inside_a_method_is_not_a_field_declaration() -> TestResult {
    // `field $x;` inside a method is a call, not a declaration: Perl's field
    // declarations belong to the class block itself and are merely *visible*
    // in methods. A descendant of a class must not manufacture a field
    // binding just by being inside one.
    let file = lower_source("use feature 'class';\nclass C {\n    method m { field $x; }\n}\n");
    assert!(
        file.scope_graph.bindings.iter().all(|b| b.storage != StorageClass::ClassField),
        "a `field $x;` statement nested in a method must not record class-field storage"
    );
    Ok(())
}

#[test]
fn field_call_nested_in_a_block_inside_a_class_is_not_a_field_declaration() -> TestResult {
    // Same boundary through a plain nested block rather than a method.
    let file = lower_source(
        "use feature 'class';\nclass C {\n    field $real;\n    sub helper { field $fake; }\n}\n",
    );
    let class_fields: Vec<&str> = file
        .scope_graph
        .bindings
        .iter()
        .filter(|b| b.storage == StorageClass::ClassField)
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(
        class_fields,
        vec!["real"],
        "only the class-level declaration is a field; the nested call is not"
    );
    Ok(())
}

#[test]
fn a_labeled_class_level_field_is_still_a_field() -> TestResult {
    // `LABEL: field $x;` parses cleanly into a `LabeledStatement` wrapper. A
    // label does not change what statement it is, so the field is still a
    // direct statement of the class body and must earn class-field storage.
    let file = lower_source(
        "use feature 'class';\nclass C {\n    LABEL: field $x;\n    field $plain;\n}\n",
    );
    for name in ["x", "plain"] {
        assert_eq!(
            binding(&file, name)?.storage,
            StorageClass::ClassField,
            "`${name}` is a class-level field declaration"
        );
    }
    Ok(())
}

#[test]
fn a_label_does_not_promote_a_nested_field_call() -> TestResult {
    // Unwrapping labels must not become "recurse until you find a
    // declaration": a labeled `field` call inside a method is still a call.
    let file = lower_source(
        "use feature 'class';\nclass C {\n    field $real;\n    method m { LABEL: field $fake; }\n}\n",
    );
    let class_fields: Vec<&str> = file
        .scope_graph
        .bindings
        .iter()
        .filter(|b| b.storage == StorageClass::ClassField)
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(
        class_fields,
        vec!["real"],
        "a label inside a method must not turn a call into a field declaration"
    );
    Ok(())
}

#[test]
fn field_in_a_nested_class_body_still_earns_class_field_storage() -> TestResult {
    // Class-body depth must be restored correctly after leaving a class, so a
    // later class in the same file is still recognized.
    let file = lower_source(
        "use feature 'class';\nclass A { field $a; }\nour $between;\nclass B { field $b; }\n",
    );
    for name in ["a", "b"] {
        assert_eq!(
            binding(&file, name)?.storage,
            StorageClass::ClassField,
            "`field ${name}` in its own class body must earn class-field storage"
        );
    }
    assert_eq!(
        binding(&file, "between")?.storage,
        StorageClass::PackageOur,
        "a declaration between two classes is not inside either body"
    );
    Ok(())
}
