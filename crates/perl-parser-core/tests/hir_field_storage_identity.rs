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
//! Point 2 is the behavior-bearing defect these tests pin: a declared field and
//! an undeclared global must not be the same fact.
//!
//! Scope note: this names *storage identity only*. Construction order,
//! `ADJUST`, invocant rules, MRO and dispatch remain unmodeled (#6672).
//!
//! The implementation lives in `src/hir/lower.rs`
//! (`storage_class_for_declarator`, `resolve_variable_kind`).

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    Binding, BodyOwnerKind, HirBody, HirExpr, HirFile, StorageClass, VariableKind, lower_ast,
};

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
// Behavior-bearing proof: a field reference is not an undeclared global
// ---------------------------------------------------------------------------

#[test]
fn field_reference_in_method_does_not_resolve_like_an_undeclared_global() -> TestResult {
    // This is the assertion that fails on the pre-fix implementation, where
    // `$x` and `$undeclared` both resolved to VariableKind::Package.
    let file = lower_source(CLASS_SOURCE);
    let body = method_body(&file, "show")?;

    let field_kind = variable_kind(body, "x")?;
    let undeclared_kind = variable_kind(body, "undeclared")?;

    assert_ne!(
        field_kind, undeclared_kind,
        "a declared `field $x` must not resolve like the never-declared `$undeclared`"
    );
    Ok(())
}

#[test]
fn field_reference_in_method_resolves_lexically() -> TestResult {
    let file = lower_source(CLASS_SOURCE);
    let body = method_body(&file, "show")?;

    for name in ["x", "y"] {
        assert_eq!(
            variable_kind(body, name)?,
            VariableKind::Lexical,
            "`${name}` names a field of the enclosing class, not a stash entry"
        );
    }
    Ok(())
}

#[test]
fn undeclared_global_in_method_still_resolves_to_package() -> TestResult {
    // Negative control for the test above: the fix must not make every
    // reference lexical.
    let file = lower_source(CLASS_SOURCE);
    let body = method_body(&file, "show")?;
    assert_eq!(
        variable_kind(body, "undeclared")?,
        VariableKind::Package,
        "an undeclared variable must still resolve as a package global"
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
fn field_declared_after_a_method_is_not_visible_inside_it() -> TestResult {
    // Perl 5.38 does not make a field visible before its own declaration —
    // the reference is out of scope, not an early read of the field. The
    // surrounding resolver is position-blind for `my`/`state` (#13868); this
    // candidate must not widen that wrong answer to `field`.
    let file =
        lower_source("use feature 'class';\nclass C {\n    method m { $x }\n    field $x;\n}\n");
    let body = method_body(&file, "m")?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Package,
        "a field declared after the method must not resolve as an in-scope field"
    );
    Ok(())
}

#[test]
fn field_declared_before_a_method_is_visible_inside_it() -> TestResult {
    // Opposite-direction control for the test above: position awareness must
    // not make every field reference unresolved.
    let file =
        lower_source("use feature 'class';\nclass C {\n    field $x;\n    method m { $x }\n}\n");
    let body = method_body(&file, "m")?;
    assert_eq!(
        variable_kind(body, "x")?,
        VariableKind::Lexical,
        "a field declared before the method must resolve as a field"
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
