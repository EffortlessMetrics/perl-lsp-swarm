mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::NodeKind;

fn initializer(source: &str) -> Result<perl_parser_core::Node, String> {
    assert_clean_parse(source);
    let ast = parse(source);
    let NodeKind::Program { statements } = ast.into_parts().0 else {
        return Err("expected program".into());
    };
    let statement = statements.into_iter().nth(1).ok_or("missing declaration")?;
    let NodeKind::VariableDeclaration { initializer: Some(initializer), .. } =
        statement.into_parts().0
    else {
        return Err("expected initialized declaration".into());
    };
    Ok(*initializer)
}

#[test]
fn unprototyped_call_takes_ternary() -> Result<(), String> {
    let node = initializer("sub unary { @_ }; my $x = unary $obj ? 1 : 0;")?;
    assert!(
        matches!(&node.kind, NodeKind::FunctionCall { args, .. } if matches!(args.first().map(|a| &a.kind), Some(NodeKind::Ternary { .. })))
    );
    Ok(())
}

#[test]
fn scalar_prototype_call_precedes_ternary() -> Result<(), String> {
    let node = initializer("sub unary ($) { $_[0] }; my $x = unary $obj ? 1 : 0;")?;
    assert!(
        matches!(&node.kind, NodeKind::Ternary { condition, .. } if matches!(condition.kind, NodeKind::FunctionCall { .. }))
    );
    Ok(())
}

#[test]
fn parentheses_delimit_unprototyped_call() -> Result<(), String> {
    let node = initializer("sub unary { @_ }; my $x = (unary $obj) ? 1 : 0;")?;
    assert!(
        matches!(&node.kind, NodeKind::Ternary { condition, .. } if matches!(condition.kind, NodeKind::FunctionCall { .. }))
    );
    Ok(())
}

#[test]
fn forward_declaration_establishes_list_call() -> Result<(), String> {
    let node = initializer("sub unary; my $x = unary $obj ? 1 : 0; sub unary ($) { $_[0] }")?;
    assert!(
        matches!(&node.kind, NodeKind::FunctionCall { args, .. } if matches!(args.first().map(|a| &a.kind), Some(NodeKind::Ternary { .. })))
    );
    Ok(())
}

fn check_named(source: &str, name: &str, list: bool) -> Result<(), String> {
    assert_clean_parse(source);
    fn find<'a>(
        node: &'a perl_parser_core::Node,
        name: &str,
    ) -> Option<&'a perl_parser_core::Node> {
        if let NodeKind::VariableDeclaration { variable, initializer: Some(init), .. } = &node.kind
            && matches!(&variable.kind, NodeKind::Variable { name: n, .. } if n == name)
        {
            return Some(init);
        }
        node.children().into_iter().find_map(|child| find(child, name))
    }
    let ast = parse(source);
    let node = find(&ast, name).ok_or_else(|| format!("missing {name}: {}", ast.to_sexp()))?;
    let is_list = matches!(&node.kind, NodeKind::FunctionCall { args, .. } if matches!(args.first().map(|a| &a.kind), Some(NodeKind::Ternary { .. })));
    let is_unary = matches!(&node.kind, NodeKind::Ternary { condition, .. } if matches!(condition.kind, NodeKind::FunctionCall { .. }));
    assert!(if list { is_list } else { is_unary }, "unexpected {name}: {}", ast.to_sexp());
    Ok(())
}

#[test]
fn declaration_order_redeclaration_and_qualified_packages() -> Result<(), String> {
    check_named("sub f ($); sub f; my $x = f $v ? 1 : 0;", "x", true)?;
    check_named("sub f ($); my $x = f $v ? 1 : 0; sub f;", "x", false)?;
    check_named(
        "package A; sub f { @_ } package B; sub f ($) { $_[0] } my $x = f $v ? 1 : 0; my $y = A::f $v ? 1 : 0;",
        "x",
        false,
    )?;
    check_named(
        "package A; sub f { @_ } package B; sub f ($) { $_[0] } my $x = f $v ? 1 : 0; my $y = A::f $v ? 1 : 0;",
        "y",
        true,
    )?;
    check_named(
        "sub f { @_ } { package B; sub f ($) { $_[0] } } my $x = f $v ? 1 : 0;",
        "x",
        true,
    )?;
    Ok(())
}

#[test]
fn recursive_and_attribute_prototype_controls() -> Result<(), String> {
    check_named("sub f ($) { my $x = f $v ? 1 : 0; }", "x", false)?;
    check_named("sub f { my $x = f $v ? 1 : 0; }", "x", true)?;
    check_named("sub f :prototype($) { $_[0] } my $x = f $v ? 1 : 0;", "x", false)?;
    Ok(())
}

#[test]
fn compile_time_and_lexical_uncertainty_keep_existing_binding() -> Result<(), String> {
    check_named("sub f { @_ } use Foo; my $x = f $v ? 1 : 0;", "x", false)?;
    check_named("sub f { @_ } BEGIN { 1 } my $x = f $v ? 1 : 0;", "x", false)?;
    check_named("sub f { @_ } { my sub f ($) { $_[0] } my $x = f $v ? 1 : 0; }", "x", false)?;
    Ok(())
}

#[test]
fn ampersand_parentheses_and_qualified_literal_paths_remain_distinct() -> Result<(), String> {
    let node = initializer("sub f ($) { $_[0] }; my $x = &f($v) ? 1 : 0;")?;
    assert!(
        matches!(&node.kind, NodeKind::Ternary { condition, .. } if matches!(condition.kind, NodeKind::AmperCall { .. }))
    );
    let node = initializer("sub A::f { @_ }; my $x = A::f 1 + 2;")?;
    assert!(
        matches!(&node.kind, NodeKind::FunctionCall { name, args } if name == "A::f" && matches!(args.first().map(|a| &a.kind), Some(NodeKind::Binary { op, .. }) if op == "+"))
    );
    Ok(())
}

#[test]
fn ternary_argument_keeps_following_call_argument() -> Result<(), String> {
    let node = initializer("sub f; my $x = f $v ? 1 : 0, 5;")?;
    assert!(matches!(&node.kind, NodeKind::FunctionCall { args, .. }
        if args.len() == 2 && matches!(args.first().map(|a| &a.kind), Some(NodeKind::Ternary { .. }))
        && matches!(args.get(1).map(|a| &a.kind), Some(NodeKind::Number { .. }))));
    Ok(())
}

#[test]
fn root_package_alias_and_later_declaration_controls() -> Result<(), String> {
    check_named("package ::A; sub f { @_ } package B; my $x = A::f $v ? 1 : 0;", "x", true)?;
    check_named("my $x = f $v ? 1 : 0; sub f { @_ }", "x", false)?;
    Ok(())
}

#[test]
fn method_calls_are_not_reclassified() -> Result<(), String> {
    let node = initializer("sub f { @_ }; my $x = $obj->f($v) ? 1 : 0;")?;
    assert!(matches!(&node.kind, NodeKind::Ternary { .. }));
    Ok(())
}

#[test]
fn class_sub_does_not_register_in_enclosing_package() -> Result<(), String> {
    check_named("class C { sub f { @_ } } my $x = f $v ? 1 : 0;", "x", false)
}

#[test]
fn builtin_block_restores_package_and_lexical_context() -> Result<(), String> {
    check_named(
        "package B; sub f { @_ } package main; sub f ($); map { package B; 1 } @v; my $x = f $v ? 1 : 0;",
        "x",
        false,
    )?;
    check_named(
        "sub f { @_ } map { my sub f ($) { $_[0] } 1 } @v; my $x = f $v ? 1 : 0;",
        "x",
        false,
    )?;
    Ok(())
}

#[test]
fn given_block_restores_enclosing_package() -> Result<(), String> {
    check_named(
        "use feature 'switch'; no warnings 'experimental::smartmatch'; package B; sub f { @_ } package main; sub f ($); given ($v) { package B; } my $x = f $v ? 1 : 0;",
        "x",
        false,
    )
}

#[test]
fn generic_block_argument_restores_enclosing_package() -> Result<(), String> {
    check_named(
        "package B; sub f { @_ } package main; sub f ($); sub h (&); h { package B; 1 }; my $x = f $v ? 1 : 0;",
        "x",
        false,
    )
}
