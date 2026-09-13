//! Perl-effective subroutine selection for inlay-hint label-location resolve.
//!
//! `inlayHint/resolve` must land on the declaration a call actually runs, not the
//! first AST node whose name matches. Perl installs compile-time `sub` entries
//! into the package symbol table in source order, so a later same-package
//! definition replaces an earlier one. A later forward declaration (`sub foo;` /
//! `sub foo($);`) does not replace an already-installed body. Package boundaries
//! are part of that identity: `package A; sub run` and `package B; sub run` are
//! distinct.
//!
//! This is the bounded #14675 interim. It does not attach canonical call/signature
//! entity identity (#8299) and does not authenticate the resolve envelope (#14672).

use perl_parser::declaration::current_package_at;
use perl_parser_core::ast::{Node, NodeKind};

/// Select the Perl-effective named subroutine for `callable_name` at `call_site_offset`.
///
/// Last same-package body-bearing definition in `ast` wins. A trailing forward
/// declaration does not replace an already selected body. If the snapshot has
/// only a forward declaration, that forward is still returned so navigation has
/// a target. A qualified callable (`Foo::bar`, `::bar`) selects that package;
/// an unqualified name uses the package in force at the call site. Lexical
/// `my`/`state` subs are skipped.
#[must_use]
pub(super) fn effective_subroutine_declaration<'a>(
    ast: &'a Node,
    callable_name: &str,
    call_site_offset: usize,
) -> Option<&'a Node> {
    if callable_name.is_empty() {
        return None;
    }
    let (explicit_package, short_name) = split_callable_name(callable_name);
    if short_name.is_empty() {
        return None;
    }
    let call_site_package = current_package_at(ast, call_site_offset);
    let target = Target { package: explicit_package.unwrap_or(call_site_package), short_name };
    let mut last = None;
    visit_node(ast, "main", &target, &mut last);
    last
}

struct Target<'a> {
    package: &'a str,
    short_name: &'a str,
}

fn split_callable_name(name: &str) -> (Option<&str>, &str) {
    match name.rsplit_once("::") {
        None => (None, name),
        Some((_, "")) => (None, name),
        Some(("", short)) => (Some("main"), short),
        Some((pkg, short)) => {
            let pkg = pkg.strip_prefix("::").unwrap_or(pkg);
            if pkg.is_empty() { (Some("main"), short) } else { (Some(pkg), short) }
        }
    }
}

fn normalize_package(pkg: &str) -> &str {
    let pkg = pkg.strip_prefix("::").unwrap_or(pkg);
    if pkg.is_empty() { "main" } else { pkg }
}

fn package_eq(left: &str, right: &str) -> bool {
    normalize_package(left) == normalize_package(right)
}

fn is_package_scoped(declarator: Option<&str>) -> bool {
    !matches!(declarator, Some("my" | "state"))
}

fn subroutine_matches(stored_name: &str, walk_package: &str, target: &Target<'_>) -> bool {
    let (declared_package, declared_short) = split_callable_name(stored_name);
    let declared_package = declared_package.unwrap_or(walk_package);
    declared_short == target.short_name && package_eq(declared_package, target.package)
}

fn unwrap_expression_statement(node: &Node) -> &Node {
    match &node.kind {
        NodeKind::ExpressionStatement { expression } => expression,
        _ => node,
    }
}

/// The parser encodes `sub foo;` / `sub foo($);` as a `Subroutine` whose body is
/// a zero-width empty `Block` at the semicolon. A genuine `sub foo {}` also has
/// an empty statement list, but the braces give the block a non-zero span.
fn is_forward_declaration_body(body: &Node) -> bool {
    match &body.kind {
        NodeKind::Block { statements } => {
            statements.is_empty() && body.location.start == body.location.end
        }
        _ => false,
    }
}

fn subroutine_is_forward(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Subroutine { body, .. } => is_forward_declaration_body(body),
        _ => false,
    }
}

fn consider_matching_subroutine<'a>(node: &'a Node, body: &Node, last: &mut Option<&'a Node>) {
    if is_forward_declaration_body(body)
        && last.is_some_and(|selected| !subroutine_is_forward(selected))
    {
        return;
    }
    *last = Some(node);
}

fn visit_statements<'a>(
    statements: &'a [Node],
    mut package: &'a str,
    target: &Target<'_>,
    last: &mut Option<&'a Node>,
) {
    for statement in statements {
        let statement = unwrap_expression_statement(statement);
        match &statement.kind {
            NodeKind::Package { name, block: None, .. } => {
                package = name.as_str();
            }
            NodeKind::Package { name, block: Some(block), .. } => {
                visit_node(block, name.as_str(), target, last);
            }
            _ => visit_node(statement, package, target, last),
        }
    }
}

fn visit_node<'a>(
    node: &'a Node,
    package: &'a str,
    target: &Target<'_>,
    last: &mut Option<&'a Node>,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            visit_statements(statements, package, target, last);
        }
        NodeKind::ExpressionStatement { expression } => {
            visit_node(expression, package, target, last);
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            visit_node(block, name.as_str(), target, last);
        }
        NodeKind::Package { .. } => {}
        NodeKind::Subroutine { name: Some(sub_name), declarator, body, .. } => {
            if is_package_scoped(declarator.as_deref())
                && subroutine_matches(sub_name, package, target)
            {
                consider_matching_subroutine(node, body, last);
            }
            visit_node(body, package, target, last);
        }
        _ => {
            for child in node.children() {
                visit_node(child, package, target, last);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_subroutine_declaration, split_callable_name};
    use perl_parser_core::ast::{Node, NodeKind};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn parse(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
        let mut parser = perl_parser_core::Parser::new(source);
        parser.parse().map_err(|error| format!("parse failed: {error}").into())
    }

    fn first_name_match<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
        if matches!(&node.kind, NodeKind::Subroutine { name: Some(sub_name), .. } if sub_name == name)
        {
            return Some(node);
        }
        let mut found = None;
        node.for_each_child(|child| {
            if found.is_none() {
                found = first_name_match(child, name);
            }
        });
        found
    }

    fn call_offset(source: &str, needle: &str) -> Result<usize, Box<dyn std::error::Error>> {
        source.find(needle).ok_or_else(|| format!("missing call site `{needle}`").into())
    }

    fn selected_text<'a>(source: &'a str, node: &Node) -> &'a str {
        source.get(node.location.start..node.location.end).unwrap_or("")
    }

    #[test]
    fn split_unqualified_and_qualified_names() {
        assert_eq!(split_callable_name("greet"), (None, "greet"));
        assert_eq!(split_callable_name("Foo::bar"), (Some("Foo"), "bar"));
        assert_eq!(split_callable_name("Foo::Bar::baz"), (Some("Foo::Bar"), "baz"));
        assert_eq!(split_callable_name("::bar"), (Some("main"), "bar"));
        assert_eq!(split_callable_name("::DB_File::splice"), (Some("DB_File"), "splice"));
    }

    #[test]
    fn last_same_package_definition_wins() -> TestResult {
        let source = r#"sub greet($name, $greeting) { return "first"; }
sub greet($name, $greeting) { return "second"; }
greet("Alice", "Hello");
"#;
        let ast = parse(source)?;
        let selected = effective_subroutine_declaration(
            &ast,
            "greet",
            call_offset(source, r#"greet("Alice""#)?,
        )
        .ok_or("expected a declaration")?;
        assert!(
            selected_text(source, selected).contains("second"),
            "last same-package greet must be selected: {}",
            selected_text(source, selected)
        );
        let first = first_name_match(&ast, "greet").ok_or("first-match oracle")?;
        assert_ne!(
            selected.location.start, first.location.start,
            "this fixture must discriminate last-wins from first-name-match"
        );
        Ok(())
    }

    #[test]
    fn later_other_package_definition_is_not_selected() -> TestResult {
        let source = r#"package A;
sub run($x, $y) { return "A"; }
run(1, 2);
package B;
sub run($x, $y) { return "B"; }
"#;
        let ast = parse(source)?;
        let selected =
            effective_subroutine_declaration(&ast, "run", call_offset(source, "run(1, 2)")?)
                .ok_or("expected A's run")?;
        assert!(
            selected_text(source, selected).contains("return \"A\""),
            "call in A must not steal B's later run: {}",
            selected_text(source, selected)
        );
        Ok(())
    }

    #[test]
    fn call_site_package_beats_first_file_match() -> TestResult {
        let source = r#"package A;
sub run($x, $y) { return "A"; }
package B;
sub run($x, $y) { return "B"; }
run(1, 2);
"#;
        let ast = parse(source)?;
        let selected =
            effective_subroutine_declaration(&ast, "run", call_offset(source, "run(1, 2)")?)
                .ok_or("expected B's run")?;
        assert!(
            selected_text(source, selected).contains("return \"B\""),
            "call in B must select B's run, not A's first match: {}",
            selected_text(source, selected)
        );
        let first = first_name_match(&ast, "run").ok_or("first-match oracle")?;
        assert!(
            selected_text(source, first).contains("return \"A\""),
            "first-name-match oracle must still pick A's run"
        );
        Ok(())
    }

    #[test]
    fn returning_to_package_uses_that_package_last_definition() -> TestResult {
        let source = r#"package A;
sub run($x, $y) { return "A1"; }
package B;
sub run($x, $y) { return "B"; }
package A;
sub run($x, $y) { return "A2"; }
run(1, 2);
"#;
        let ast = parse(source)?;
        let selected =
            effective_subroutine_declaration(&ast, "run", call_offset(source, "run(1, 2)")?)
                .ok_or("expected A's second run")?;
        assert!(
            selected_text(source, selected).contains("return \"A2\""),
            "returning to A must select A's last run: {}",
            selected_text(source, selected)
        );
        Ok(())
    }

    #[test]
    fn block_package_call_selects_inner_definition() -> TestResult {
        let source = r#"package Outer;
sub run($x, $y) { return "outer"; }
package Inner {
  sub run($x, $y) { return "inner"; }
  run(1, 2);
}
"#;
        let ast = parse(source)?;
        let selected =
            effective_subroutine_declaration(&ast, "run", call_offset(source, "run(1, 2)")?)
                .ok_or("expected Inner::run")?;
        assert!(
            selected_text(source, selected).contains("return \"inner\""),
            "call inside Inner block must select Inner::run: {}",
            selected_text(source, selected)
        );
        Ok(())
    }

    #[test]
    fn outer_package_restored_after_block() -> TestResult {
        let source = r#"package Outer;
sub run($x, $y) { return "outer"; }
package Inner {
  sub run($x, $y) { return "inner"; }
}
run(1, 2);
"#;
        let ast = parse(source)?;
        let selected =
            effective_subroutine_declaration(&ast, "run", call_offset(source, "run(1, 2)")?)
                .ok_or("expected Outer::run")?;
        assert!(
            selected_text(source, selected).contains("return \"outer\""),
            "call after Inner block must select Outer::run: {}",
            selected_text(source, selected)
        );
        Ok(())
    }

    #[test]
    fn qualified_name_selects_that_package_from_another_call_site() -> TestResult {
        let source = r#"package A;
sub run($x, $y) { return "A"; }
package B;
sub run($x, $y) { return "B"; }
A::run(1, 2);
"#;
        let ast = parse(source)?;
        let selected =
            effective_subroutine_declaration(&ast, "A::run", call_offset(source, "A::run(1, 2)")?)
                .ok_or("expected A::run")?;
        assert!(
            selected_text(source, selected).contains("return \"A\""),
            "qualified A::run from package B must select A's run: {}",
            selected_text(source, selected)
        );
        Ok(())
    }

    #[test]
    fn qualified_declaration_in_main_matches_qualified_callable() -> TestResult {
        let source = r#"sub Foo::run($x, $y) { return "qualified"; }
package Bar;
Foo::run(1, 2);
"#;
        let ast = parse(source)?;
        let selected = effective_subroutine_declaration(
            &ast,
            "Foo::run",
            call_offset(source, "Foo::run(1, 2)")?,
        )
        .ok_or("expected Foo::run")?;
        assert!(
            selected_text(source, selected).contains("Foo::run"),
            "selected span should cover the qualified declaration"
        );
        Ok(())
    }

    #[test]
    fn lexical_my_sub_is_not_a_package_definition() -> TestResult {
        let source = r#"my sub greet($name, $greeting) { return "lexical"; }
sub greet($name, $greeting) { return "package"; }
greet("Alice", "Hello");
"#;
        let ast = parse(source)?;
        let selected = effective_subroutine_declaration(
            &ast,
            "greet",
            call_offset(source, r#"greet("Alice""#)?,
        )
        .ok_or("expected package greet")?;
        assert!(
            selected_text(source, selected).contains("package"),
            "lexical my sub must not win over a later package sub"
        );
        let first = first_name_match(&ast, "greet").ok_or("first-match oracle")?;
        assert_ne!(selected.location.start, first.location.start);
        Ok(())
    }

    #[test]
    fn missing_name_yields_none() -> TestResult {
        let source = "sub greet($name, $greeting) { 1 }\n";
        let ast = parse(source)?;
        assert!(effective_subroutine_declaration(&ast, "absent", 0).is_none());
        assert!(effective_subroutine_declaration(&ast, "", 0).is_none());
        Ok(())
    }

    #[test]
    fn sole_declaration_is_still_selected() -> TestResult {
        let source = r#"sub greet($name, $greeting) { return "only"; }
greet("Alice", "Hello");
"#;
        let ast = parse(source)?;
        let selected = effective_subroutine_declaration(
            &ast,
            "greet",
            call_offset(source, r#"greet("Alice""#)?,
        )
        .ok_or("expected the only declaration")?;
        assert!(
            selected_text(source, selected).contains("only"),
            "the sole declaration must still resolve: {}",
            selected_text(source, selected)
        );
        Ok(())
    }

    #[test]
    fn trailing_forward_declaration_does_not_replace_body() -> TestResult {
        let source = r#"sub greet($name, $greeting) { return "body"; }
sub greet;
sub greet($);
greet("Alice", "Hello");
"#;
        let ast = parse(source)?;
        let selected = effective_subroutine_declaration(
            &ast,
            "greet",
            call_offset(source, r#"greet("Alice""#)?,
        )
        .ok_or("expected the body-bearing greet")?;
        let text = selected_text(source, selected);
        assert!(
            text.contains("return \"body\""),
            "trailing `sub greet;` / `sub greet($);` must not steal the earlier body: {text}"
        );
        Ok(())
    }

    #[test]
    fn forward_then_definition_selects_the_body() -> TestResult {
        let source = r#"sub greet;
sub greet($);
sub greet($name, $greeting) { return "defined"; }
greet("Alice", "Hello");
"#;
        let ast = parse(source)?;
        let selected = effective_subroutine_declaration(
            &ast,
            "greet",
            call_offset(source, r#"greet("Alice""#)?,
        )
        .ok_or("expected the later definition")?;
        let text = selected_text(source, selected);
        assert!(
            text.contains("return \"defined\""),
            "a later body must replace an earlier forward declaration: {text}"
        );
        let first = first_name_match(&ast, "greet").ok_or("first-match oracle")?;
        assert_ne!(
            selected.location.start, first.location.start,
            "this fixture must discriminate definition-after-forward from first-name-match"
        );
        Ok(())
    }

    #[test]
    fn sole_forward_declaration_is_still_selected() -> TestResult {
        let source = "sub greet;\ngreet();\n";
        let ast = parse(source)?;
        let selected =
            effective_subroutine_declaration(&ast, "greet", call_offset(source, "greet()")?)
                .ok_or("expected the forward declaration")?;
        let NodeKind::Subroutine { name: Some(name), body, .. } = &selected.kind else {
            return Err("expected a Subroutine node".into());
        };
        assert_eq!(name.as_str(), "greet");
        let NodeKind::Block { statements } = &body.kind else {
            return Err("forward body must be a Block".into());
        };
        assert!(statements.is_empty(), "forward body has no statements");
        assert_eq!(
            body.location.start, body.location.end,
            "parser encodes a forward as a zero-width empty Block"
        );
        Ok(())
    }

    #[test]
    fn empty_brace_body_is_not_treated_as_forward() -> TestResult {
        let source = "sub greet {}\nsub greet;\ngreet();\n";
        let ast = parse(source)?;
        let selected =
            effective_subroutine_declaration(&ast, "greet", call_offset(source, "greet()")?)
                .ok_or("expected the empty-brace body")?;
        let text = selected_text(source, selected);
        assert!(
            text.contains('{'),
            "genuine `sub greet {{}}` must win over a trailing forward: {text}"
        );
        assert!(
            !text.trim_end().ends_with(';'),
            "trailing `sub greet;` must not replace an empty-brace definition: {text}"
        );
        Ok(())
    }
}
