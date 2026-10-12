//! Class bodies lower into HIR body arenas (#13844).
//!
//! Statements declared directly inside a `class` body (`field`, `my`) must
//! reach a body arena with their correct storage. Before the fix they
//! appeared in no body at all: `collect_sub_bodies` minted bodies only for
//! subroutines and methods, and the program-root lowering wrapped the class
//! in an opaque `Call("Class")` expression whose block argument is childless.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, BodyOwnerKind, DeclStorageClass, HirBlockId, HirBody, HirExpr, HirFile, HirStmt,
    StorageClass, VariableKind, lower_ast,
};
use perl_parser_core::pir::{PirOperation, lower_hir_bodies};

fn parse(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

fn collect_lets(body: &HirBody, block_id: HirBlockId, out: &mut Vec<(String, DeclStorageClass)>) {
    let Some(block) = body.block(block_id) else {
        return;
    };
    for stmt_id in &block.stmts {
        let Some(stmt) = body.stmt(*stmt_id) else {
            continue;
        };
        match stmt {
            HirStmt::Let { name, storage, .. } => out.push((name.clone(), storage.clone())),
            HirStmt::Block(nested) => collect_lets(body, *nested, out),
            _ => {}
        }
    }
}

fn class_body<'a>(file: &'a HirFile, name: &str) -> Result<&'a HirBody, String> {
    file.bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Class { name: n } if n == name))
        .ok_or_else(|| format!("no lowered body for class `{name}`"))
}

const REPRO: &str = "use feature 'class';\nclass Point {\n    field $x;\n    my $lex;\n    method show { $x }\n}\nmy $top;\n";

#[test]
fn class_body_declarations_reach_a_body_arena() -> Result<(), String> {
    let file = parse(REPRO);
    let mut lets = Vec::new();
    for body in &file.bodies {
        collect_lets(body, body.root_block, &mut lets);
    }
    for wanted in ["x", "lex", "top"] {
        assert!(
            lets.iter().any(|(name, _)| name == wanted),
            "class-body and top-level declarations must lower to a body arena, have {lets:?}"
        );
    }
    Ok(())
}

#[test]
fn class_body_has_its_own_owner() -> Result<(), String> {
    let file = parse(REPRO);
    let body = class_body(&file, "Point")?;
    assert_eq!(
        body.owner,
        BodyOwnerKind::Class { name: "Point".to_string() },
        "class body owner must be Class {{ name: \"Point\" }}"
    );
    Ok(())
}

#[test]
fn field_lowers_with_field_storage_and_my_with_my() -> Result<(), String> {
    let file = parse(REPRO);
    let body = class_body(&file, "Point")?;
    let mut lets = Vec::new();
    collect_lets(body, body.root_block, &mut lets);
    let storage_of = |name: &str| {
        lets.iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.clone())
            .ok_or_else(|| format!("no `{name}` declaration in the class body, have {lets:?}"))
    };
    assert!(
        matches!(storage_of("x")?, DeclStorageClass::Field),
        "class-body `field` must lower with Field storage"
    );
    assert!(
        matches!(storage_of("lex")?, DeclStorageClass::My),
        "class-body `my` must keep My storage"
    );
    Ok(())
}

#[test]
fn scope_graph_and_arena_agree_about_class_declarations() -> Result<(), String> {
    let file = parse(REPRO);
    let body = class_body(&file, "Point")?;
    let mut lets = Vec::new();
    collect_lets(body, body.root_block, &mut lets);
    for (name, _) in &lets {
        let bindings: Vec<_> =
            file.scope_graph.bindings.iter().filter(|b| b.name == *name).collect();
        assert!(!bindings.is_empty(), "arena declaration `{name}` must have a scope-graph binding");
    }
    let field_binding = file
        .scope_graph
        .bindings
        .iter()
        .find(|b| b.name == "x")
        .ok_or_else(|| "scope graph must record the field binding".to_string())?;
    assert_eq!(
        field_binding.storage,
        StorageClass::ClassField,
        "scope graph must record `field` as class-field storage"
    );
    Ok(())
}

#[test]
fn method_and_top_level_bodies_are_unchanged() -> Result<(), String> {
    let file = parse(REPRO);
    let method = file
        .bodies
        .iter()
        .find(|b| matches!(&b.owner, BodyOwnerKind::Method { name } if name == "show"))
        .ok_or_else(|| "method body must still be lowered".to_string())?;
    let root = method
        .block(method.root_block)
        .ok_or_else(|| "method root block must exist".to_string())?;
    assert!(!root.stmts.is_empty(), "method body must keep its statements");
    let root_body = file.root_body().ok_or_else(|| "program-root body must exist".to_string())?;
    let mut lets = Vec::new();
    collect_lets(root_body, root_body.root_block, &mut lets);
    assert!(
        lets.iter().any(|(name, _)| name == "top"),
        "top-level `my` must stay in the program-root body, have {lets:?}"
    );
    Ok(())
}

#[test]
fn reads_in_the_class_body_resolve_against_the_class_scope() -> Result<(), String> {
    // A read of a class-body lexical must resolve as Lexical. With the wrong
    // start scope (e.g. the program root instead of the `ScopeKind::Class`
    // frame) the binding is invisible and the read misclassifies as Package.
    let file = parse("use feature 'class';\nclass Point {\n    my $lex = 1;\n    $lex;\n}\n");
    let body = class_body(&file, "Point")?;
    let kind = body
        .exprs
        .iter()
        .find_map(|expr| match expr {
            HirExpr::Variable(v) if v.name == "lex" && v.access == AccessMode::Read => Some(v.kind),
            _ => None,
        })
        .ok_or_else(|| "expected a `$lex` read in the class body".to_string())?;
    assert!(
        matches!(kind, VariableKind::Lexical),
        "class-body read of a lexical must resolve as Lexical"
    );
    Ok(())
}

#[test]
fn field_declaration_reaches_pir_as_a_field_write() -> Result<(), String> {
    // Production-path validation: the new Class body flows through PIR-A
    // lowering (which walks every body) and the declaration write keeps its
    // per-object identity instead of becoming a lexical or stash write.
    let file = parse(REPRO);
    let graph = lower_hir_bodies(&file);
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| matches!(&n.operation, PirOperation::FieldWrite { name } if name.name == "x")),
        "PIR must carry a FieldWrite for the class-body `field` declaration"
    );
    assert!(
        !graph.nodes.iter().any(|n| matches!(
            &n.operation,
            PirOperation::LexicalWrite { name } if name.name == "x"
        )),
        "the field declaration must not publish a lexical write"
    );
    Ok(())
}

#[test]
fn initialized_field_place_is_a_field_write_place() -> Result<(), String> {
    // `field $x = 1` owns its declaration place even though the field is not
    // yet visible to ordinary resolution at its own initializer: the init
    // Assign LHS must be a Field write place, not a package write.
    let file = parse("use feature 'class';\nclass Point {\n    field $x = 1;\n}\n");
    let body = class_body(&file, "Point")?;
    let place = body
        .exprs
        .iter()
        .find_map(|expr| match expr {
            HirExpr::Assign { lhs, .. } => body.expr(*lhs),
            _ => None,
        })
        .ok_or_else(|| "expected an initializer assignment in the class body".to_string())?;
    match place {
        HirExpr::Variable(v) => {
            assert_eq!(v.name, "x");
            assert!(matches!(v.access, AccessMode::Write), "init LHS must be a Write place");
            assert!(
                matches!(v.kind, VariableKind::Field),
                "initialized field place must be a field, not a package write"
            );
        }
        other => return Err(format!("expected a Variable init place, got {other:?}").into()),
    }
    Ok(())
}

#[test]
fn legacy_field_call_outside_a_class_stays_unknown() -> Result<(), String> {
    // A `field`-shaped node outside a class body is a subroutine call, not a
    // declaration (#13817). It must keep `Unknown` storage.
    let file = parse("field $x = 1;\n");
    let root = file.root_body().ok_or_else(|| "program-root body must exist".to_string())?;
    let mut lets = Vec::new();
    collect_lets(root, root.root_block, &mut lets);
    let (_, storage) = lets
        .iter()
        .find(|(name, _)| name == "x")
        .ok_or_else(|| format!("expected a legacy `field` Let, have {lets:?}"))?;
    assert!(
        matches!(storage, DeclStorageClass::Unknown),
        "legacy `field` call must keep Unknown storage"
    );
    Ok(())
}
