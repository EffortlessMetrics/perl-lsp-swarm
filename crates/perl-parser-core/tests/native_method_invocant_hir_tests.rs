//! Canonical HIR identity for the implicit invocant of native block-class methods.

use perl_parser_core::hir::{
    BodyOwnerKind, HirExpr, HirExprId, HirFile, HirKind, NativeMethodInvocantBoundary,
    NativeMethodInvocantLookup, NativeMethodOwner, StorageClass, VariableKind, lower_ast,
    lower_ast_with_parse_diagnostics,
};
use perl_parser_core::{Node, NodeKind, Parser};

type TestResult = Result<(), String>;

fn some<T>(value: Option<T>, context: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("missing {context}"))
}

fn same<T: std::fmt::Debug + PartialEq>(actual: T, expected: T, context: &str) -> TestResult {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{context}: got {actual:?}, want {expected:?}"))
    }
}

fn require(condition: bool, context: &str) -> TestResult {
    if condition { Ok(()) } else { Err(context.to_string()) }
}

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast_with_parse_diagnostics(&output.ast, &output.diagnostics)
}

fn nth_offset(source: &str, needle: &str, nth: usize) -> Result<usize, String> {
    let mut from = 0;
    for _ in 0..nth {
        from = some(source.get(from..).and_then(|part| part.find(needle)), needle)?
            + from
            + needle.len();
    }
    Ok(some(source.get(from..).and_then(|part| part.find(needle)), needle)? + from)
}

#[test]
fn named_block_method_joins_binding_reference_body_and_class_source() -> TestResult {
    let source = "use feature 'class'; class Animal { method speak { $self->sound; } }";
    let file = lower(source);
    let self_start = nth_offset(source, "$self", 0)?;
    let lookup = file.native_method_invocant_at(self_start + 1);
    let NativeMethodInvocantLookup::Exact {
        binding,
        method_item,
        method_scope,
        class_item,
        class_scope,
    } = lookup
    else {
        return Err(format!("native `$self` needs exact source owner, got {lookup:?}"));
    };

    let method = some(file.items.iter().find(|item| item.id == method_item), "method item")?;
    let class = some(file.items.iter().find(|item| item.id == class_item), "class item")?;
    let declared =
        some(file.scope_graph.bindings.iter().find(|item| item.id == binding), "implicit binding")?;
    let reference = some(
        file.scope_graph.references.iter().find(|item| item.range.start == self_start),
        "body reference",
    )?;
    require(
        matches!(&method.kind, HirKind::MethodDecl(decl)
        if decl.native_owner == NativeMethodOwner::Exact { class_item, class_scope, invocant_binding: binding }),
        "method owner relation",
    )?;
    require(
        matches!(&class.kind, HirKind::ClassDecl(decl) if decl.name == "Animal"),
        "class declaration",
    )?;
    same(
        some(class.anchor.name_range, "class name range")?.start,
        nth_offset(source, "Animal", 0)?,
        "class source anchor",
    )?;
    same(
        some(method.anchor.name_range, "method name range")?.start,
        nth_offset(source, "speak", 0)?,
        "method source anchor",
    )?;
    same(
        declared.range,
        some(method.anchor.name_range, "method name range")?,
        "implicit binding source anchor",
    )?;
    same(declared.storage, StorageClass::MethodInvocant, "implicit storage")?;
    same(declared.declaration_item, Some(method_item), "binding declaration item")?;
    same(declared.scope_id, method_scope, "binding method scope")?;
    same(reference.resolved_binding, Some(binding), "reference binding")?;
    same(method.scope_context, Some(method_scope), "method item scope")?;
    same(
        some(
            file.scope_graph.scopes.iter().find(|scope| scope.id == method_scope),
            "method frame",
        )?
        .parent,
        Some(class_scope),
        "method class parent",
    )?;
    require(
        some(file.compile_environment.pragma_state_at(class.range.start), "class feature state")?
            .has_feature("class"),
        "class feature evidence",
    )?;
    require(
        some(file.compile_environment.pragma_state_at(method.range.start), "method feature state")?
            .has_feature("class"),
        "method feature evidence",
    )?;

    let body_id = some(
        file.body_owners.iter().find_map(|(owner, id)| {
            matches!(&owner.kind, BodyOwnerKind::Method { name } if name == "speak").then_some(*id)
        }),
        "speak method body ID",
    )?;
    let body = some(file.bodies.get(body_id.0 as usize), "speak method body")?;
    let body_occurrence = some(
        std::iter::once(body)
            .flat_map(|body| {
                (0..body.source_map.expr_ranges.len()).filter_map(move |index| {
                    let id = HirExprId(index as u32);
                    let range = body.source_map.expr_range(id)?;
                    match body.expr(id) {
                        Some(HirExpr::Variable(var)) if range.start == self_start => Some(var),
                        _ => None,
                    }
                })
            })
            .next(),
        "body-HIR `$self` occurrence",
    )?;
    same(body_occurrence.binding, Some(binding), "body occurrence binding")?;
    same(body_occurrence.kind, VariableKind::Lexical, "body occurrence kind")?;
    same(
        file.scope_graph
            .bindings
            .iter()
            .filter(|item| item.storage == StorageClass::MethodInvocant)
            .count(),
        1,
        "implicit binding count",
    )
}

#[test]
fn explicit_signature_at_and_first_or_second_shift_remain_ordinary_arguments() -> TestResult {
    let cases = [
        ("method speak($self, $arg) { $self->sound; $arg->sound; }", StorageClass::Parameter),
        ("method speak { my ($self) = @_; $self->sound; }", StorageClass::LexicalMy),
        ("method speak { my $self = shift; $self->sound; }", StorageClass::LexicalMy),
        (
            "method speak { my $arg = shift; my $self = shift; $self->sound; }",
            StorageClass::LexicalMy,
        ),
    ];
    for (method, expected_storage) in cases {
        let source = format!("use feature 'class'; class Animal {{ {method} }}");
        let file = lower(&source);
        let read = nth_offset(&source, "$self", 1)?;
        same(
            file.native_method_invocant_at(read + 1),
            NativeMethodInvocantLookup::Unavailable,
            "explicit `$self` receiver",
        )?;
        let reference = some(
            file.scope_graph.references.iter().find(|item| item.range.start == read),
            "explicit `$self` reference",
        )?;
        let binding = some(
            file.scope_graph
                .bindings
                .iter()
                .find(|item| Some(item.id) == reference.resolved_binding),
            "ordinary argument binding",
        )?;
        same(binding.storage, expected_storage, "ordinary argument storage")?;
        same(
            file.scope_graph
                .bindings
                .iter()
                .filter(|item| item.storage == StorageClass::MethodInvocant)
                .count(),
            1,
            "implicit binding remains separate",
        )?;
        if method.contains("$arg)") {
            let arg_read = nth_offset(&source, "$arg", 1)?;
            let arg = some(
                file.scope_graph.references.iter().find(|item| item.range.start == arg_read),
                "signature `$arg` read",
            )?;
            let parameter = some(
                file.scope_graph.bindings.iter().find(|item| Some(item.id) == arg.resolved_binding),
                "signature `$arg` binding",
            )?;
            same(parameter.storage, StorageClass::Parameter, "signature `$arg` storage")?;
        }
    }
    Ok(())
}

#[test]
fn shadow_and_named_sub_refuse_exact_while_anonymous_closure_captures() -> TestResult {
    let source = "use feature 'class'; class Animal { method speak { $self->a; if (1) { my $self = 1; $self->b; } $self->c; my $cb = sub { $self->d; }; sub inner { $self->e; } } }";
    let file = lower(source);
    let outer = file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1);
    same(
        file.native_method_invocant_at(nth_offset(source, "$self", 3)? + 1),
        outer,
        "after shadow scope",
    )?;
    same(
        file.native_method_invocant_at(nth_offset(source, "$self", 4)? + 1),
        outer,
        "anonymous closure capture",
    )?;
    require(matches!(outer, NativeMethodInvocantLookup::Exact { .. }), "outer reference exact")?;
    same(
        file.native_method_invocant_at(nth_offset(source, "$self", 2)? + 1),
        NativeMethodInvocantLookup::Unavailable,
        "inner lexical shadow",
    )?;
    let nested = nth_offset(source, "$self", 5)?;
    same(
        file.native_method_invocant_at(nested + 1),
        NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::NamedSubroutine),
        "named sub boundary",
    )?;
    let reference = some(
        file.scope_graph.references.iter().find(|item| item.range.start == nested),
        "nested generic reference",
    )?;
    require(reference.resolved_binding.is_some(), "generic lexical walk may retain outer identity")
}

#[test]
fn named_nested_sub_local_self_is_unavailable_instead_of_outer_pad_boundary() -> TestResult {
    let cases = [
        ("sub inner { my $self = 1; $self->sound; }", StorageClass::LexicalMy),
        ("sub inner($self) { $self->sound; }", StorageClass::Parameter),
    ];
    for (nested_sub, storage) in cases {
        let source = format!(
            "use feature 'class'; use feature 'signatures'; class Animal {{ method speak {{ {nested_sub} }} }}"
        );
        let file = lower(&source);
        let read = nth_offset(&source, "$self", 1)?;
        same(
            file.native_method_invocant_at(read + 1),
            NativeMethodInvocantLookup::Unavailable,
            "nested sub local `$self` has no native invocant",
        )?;
        let reference = some(
            file.scope_graph.references.iter().find(|item| item.range.start == read),
            "nested sub `$self` reference",
        )?;
        let binding = some(
            file.scope_graph
                .bindings
                .iter()
                .find(|item| Some(item.id) == reference.resolved_binding),
            "nested sub local binding",
        )?;
        same(binding.storage, storage, "nested sub local storage")?;
    }
    Ok(())
}

#[test]
fn same_spelling_sibling_classes_have_distinct_source_owners() -> TestResult {
    let source = "use feature 'class'; class Animal { method speak { $self->a; } } class Animal { method speak { $self->b; } }";
    let file = lower(source);
    let first = file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1);
    let second = file.native_method_invocant_at(nth_offset(source, "$self", 1)? + 1);
    let (
        NativeMethodInvocantLookup::Exact {
            binding: first_binding, class_item: first_class, ..
        },
        NativeMethodInvocantLookup::Exact {
            binding: second_binding, class_item: second_class, ..
        },
    ) = (first, second)
    else {
        return Err(format!("both block classes need exact owners: {first:?} {second:?}"));
    };
    require(first_binding != second_binding, "sibling binding IDs must differ")?;
    require(first_class != second_class, "sibling class item IDs must differ")?;
    same(
        some(file.items.iter().find(|item| item.id == first_class), "first class")?
            .anchor
            .name_range
            .map(|range| range.start),
        Some(nth_offset(source, "Animal", 0)?),
        "first class anchor",
    )?;
    same(
        some(file.items.iter().find(|item| item.id == second_class), "second class")?
            .anchor
            .name_range
            .map(|range| range.start),
        Some(nth_offset(source, "Animal", 1)?),
        "second class anchor",
    )
}

#[test]
fn adjust_package_method_disabled_feature_and_statement_class_are_not_exact() -> TestResult {
    let cases = [
        (
            "use feature 'class'; class Animal { ADJUST { $self->a; } }",
            NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::MissingNameAnchor),
        ),
        (
            "use feature 'class'; package Animal; method speak { $self->a; }",
            NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::NoBlockClassOwner),
        ),
        (
            "class Animal { method speak { $self->a; } }",
            NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::ClassFeatureDisabled),
        ),
    ];
    for (source, expected) in cases {
        let file = lower(source);
        same(
            file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1),
            expected,
            "unsupported owner",
        )?;
        require(
            file.scope_graph
                .bindings
                .iter()
                .all(|item| item.storage != StorageClass::MethodInvocant),
            "unsupported method must not declare implicit invocant",
        )?;
    }
    let nested = "use feature 'class'; class Animal { if (1) { method speak { $self->a; } } }";
    let file = lower(nested);
    same(
        file.native_method_invocant_at(nth_offset(nested, "$self", 0)? + 1),
        NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::NoBlockClassOwner),
        "method below nested block lacks direct class membership",
    )?;
    let statement = "use feature 'class'; class Animal; method speak { $self->a; }";
    let file = lower(statement);
    require(
        !matches!(
            file.native_method_invocant_at(nth_offset(statement, "$self", 0)? + 1),
            NativeMethodInvocantLookup::Exact { .. }
        ),
        "statement-form class lacks source owner",
    )?;

    let transitioned =
        "use feature 'class'; no feature 'class'; class Animal { method speak { $self->a; } }";
    let file = lower(transitioned);
    same(
        file.native_method_invocant_at(nth_offset(transitioned, "$self", 0)? + 1),
        NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::ClassFeatureDisabled),
        "feature disabled before later class",
    )
}

#[test]
fn method_attributes_and_overlapping_keyword_provider_are_ambiguous() -> TestResult {
    let cases = [
        (
            "use feature 'class'; class Animal { method speak :common { $self->a; } }",
            NativeMethodInvocantBoundary::MethodAttributes,
        ),
        (
            "use feature 'class'; class Animal { method speak($arg) :lvalue { $self->a; } }",
            NativeMethodInvocantBoundary::MethodAttributes,
        ),
        (
            "use feature 'class'; use Object::Pad; class Animal { method speak { $self->a; } }",
            NativeMethodInvocantBoundary::AmbiguousClassProfile,
        ),
        (
            "use feature 'class'; use Object::Pad 0.825; class Animal { method speak { $self->a; } }",
            NativeMethodInvocantBoundary::AmbiguousClassProfile,
        ),
        (
            "use feature 'class'; use if 1, 'Object::Pad'; class Animal { method speak { $self->a; } }",
            NativeMethodInvocantBoundary::AmbiguousClassProfile,
        ),
        (
            "use feature 'class'; use unless 0, 'Object::Pad'; class Animal { method speak { $self->a; } }",
            NativeMethodInvocantBoundary::AmbiguousClassProfile,
        ),
        (
            "use feature 'class'; use if 1, $keyword_provider; class Animal { method speak { $self->a; } }",
            NativeMethodInvocantBoundary::AmbiguousClassProfile,
        ),
        (
            "use feature 'class'; use if 0, 'DBI'; class Animal { method speak { $self->a; } }",
            NativeMethodInvocantBoundary::AmbiguousClassProfile,
        ),
    ];
    for (source, reason) in cases {
        let file = lower(source);
        same(
            file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1),
            NativeMethodInvocantLookup::Partial(reason),
            "ambiguous method profile",
        )?;
        require(
            file.scope_graph
                .bindings
                .iter()
                .all(|item| item.storage != StorageClass::MethodInvocant),
            "ambiguous method must not mint native `$self`",
        )?;
    }
    Ok(())
}

#[test]
fn recovered_method_wrapper_cannot_publish_exact_binding() -> TestResult {
    let source = "use feature 'class'; class Animal { method speak { $self->sound; } }";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut ast = output.ast;
    let NodeKind::Program { statements } = &mut ast.kind else {
        return Err("missing program".to_string());
    };
    let class = some(
        statements.iter_mut().find(|node| matches!(&node.kind, NodeKind::Class { .. })),
        "class node",
    )?;
    let NodeKind::Class { body, .. } = &mut class.kind else {
        return Err("missing class body".to_string());
    };
    let NodeKind::Block { statements } = &mut body.kind else {
        return Err("missing block body".to_string());
    };
    let method = some(
        statements.iter_mut().find(|node| matches!(&node.kind, NodeKind::Method { .. })),
        "method node",
    )?;
    let range = method.location;
    let partial = std::mem::replace(method, Node::new(NodeKind::MissingStatement, range));
    *method = Node::new(
        NodeKind::Error {
            message: "recovered method".to_string(),
            expected: Vec::new(),
            found: None,
            partial: Some(Box::new(partial)),
        },
        range,
    );
    let file = lower_ast_with_parse_diagnostics(&ast, &output.diagnostics);
    same(
        file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1),
        NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::RecoveredSyntax),
        "recovered owner",
    )?;
    require(
        file.scope_graph.bindings.iter().all(|item| item.storage != StorageClass::MethodInvocant),
        "recovered owner has no implicit binding",
    )
}

#[test]
fn recovered_class_wrapper_cannot_publish_exact_binding() -> TestResult {
    let source = "use feature 'class'; class Animal { method speak { $self->sound; } }";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let mut ast = output.ast;
    let NodeKind::Program { statements } = &mut ast.kind else {
        return Err("missing program".to_string());
    };
    let class = some(
        statements.iter_mut().find(|node| matches!(&node.kind, NodeKind::Class { .. })),
        "class node",
    )?;
    let range = class.location;
    let partial = std::mem::replace(class, Node::new(NodeKind::MissingStatement, range));
    *class = Node::new(
        NodeKind::Error {
            message: "recovered class".to_string(),
            expected: Vec::new(),
            found: None,
            partial: Some(Box::new(partial)),
        },
        range,
    );
    let file = lower_ast_with_parse_diagnostics(&ast, &output.diagnostics);
    same(
        file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1),
        NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::RecoveredSyntax),
        "recovered class owner",
    )?;
    require(
        file.scope_graph.bindings.iter().all(|item| item.storage != StorageClass::MethodInvocant),
        "recovered class has no implicit binding",
    )
}

#[test]
fn unclosed_method_body_delimiter_cannot_publish_exact_binding() -> TestResult {
    // The parser's real delimiter recovery: `parse_block` records "Unclosed
    // block" and still returns an ordinary `Block`, so no Error wrapper ever
    // reaches the lowerer. A method body that reaches the input end was never
    // closed and must not mint an exact invocant.
    let source = "use feature 'class'; class Animal { method speak { $self->sound;";
    let file = lower(source);
    same(
        file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1),
        NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::RecoveredSyntax),
        "unclosed method body delimiter",
    )?;
    require(
        file.scope_graph.bindings.iter().all(|item| item.storage != StorageClass::MethodInvocant),
        "unclosed method body has no implicit binding",
    )
}

#[test]
fn unclosed_class_body_delimiter_cannot_publish_exact_binding() -> TestResult {
    // The method body itself closed, but its `}` is the last input byte: the
    // class body's own closer is missing, so the declaration is still inside
    // recovered delimiters.
    let source = "use feature 'class'; class Animal { method speak { $self->sound; }";
    let file = lower(source);
    same(
        file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1),
        NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::RecoveredSyntax),
        "unclosed class body delimiter",
    )?;
    require(
        file.scope_graph.bindings.iter().all(|item| item.storage != StorageClass::MethodInvocant),
        "unclosed class body has no implicit binding",
    )
}

#[test]
fn unclosed_class_delimiter_refuses_also_the_complete_method_before_it() -> TestResult {
    // A closed method followed by another method, with the class body never
    // closed. The parser records "Unclosed block" for the class brace, so the
    // whole file is delimiter-recovered: the earlier complete method must not
    // mint an exact owner either, even though its own body span ends well
    // before the input boundary.
    let source = "use feature 'class'; class Animal { method speak { $self->sound; } method bark { $self->yelp; }";
    let file = lower(source);
    for (method, nth) in [("speak", 0), ("bark", 1)] {
        same(
            file.native_method_invocant_at(nth_offset(source, "$self", nth)? + 1),
            NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::RecoveredSyntax),
            &format!("{method} on a delimiter-recovered file"),
        )?;
    }
    require(
        file.scope_graph.bindings.iter().all(|item| item.storage != StorageClass::MethodInvocant),
        "delimiter-recovered file has no implicit binding",
    )
}

#[test]
fn lower_ast_without_parse_authority_never_publishes_exact() -> TestResult {
    // Even a perfectly clean source never admits an exact owner through the
    // entry that carries no parse diagnostics: cleanliness is unverifiable
    // there, and unverifiable must mean refused.
    let source = "use feature 'class'; class Animal { method speak { $self->sound; } }";
    let mut parser = Parser::new(source);
    let file = lower_ast(&parser.parse_with_recovery().ast);
    same(
        file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1),
        NativeMethodInvocantLookup::Partial(NativeMethodInvocantBoundary::ParseAuthorityUnsupplied),
        "authority unsupplied",
    )?;
    require(
        file.scope_graph.bindings.iter().all(|item| item.storage != StorageClass::MethodInvocant),
        "authority-unsupplied lowering mints no implicit binding",
    )
}

#[test]
fn incomplete_arrow_does_not_invalidate_local_class_and_method_anchors() -> TestResult {
    let source = "use feature 'class'; class Animal { method speak { $self-> } }";
    let file = lower(source);
    let lookup = file.native_method_invocant_at(nth_offset(source, "$self", 0)? + 1);
    require(
        matches!(lookup, NativeMethodInvocantLookup::Exact { .. }),
        &format!("incomplete receiver still has local invocant authority: {lookup:?}"),
    )
}
