//! Unparenthesized `require` module-name operand classification (#16670).
//!
//! Perl's strict `subs` exempts `require DBI` but treats `require(DBI)` as an
//! expression bareword. The diagnostic skip must follow that role, not token
//! spelling or same-line `require` text.

use super::ScopeAnalyzer;
use crate::ast::{Node, NodeKind};

impl ScopeAnalyzer {
    /// Whether `node` is the unparenthesized module-name operand of `require`.
    ///
    /// Matches the Perl form that strict `subs` exempts (`require DBI`), which
    /// is also HIR `BarewordRole::ModuleRequest` for a bare identifier target.
    /// Nested identifiers inside a computed first argument (`require(Foo . ".pm")`)
    /// stay expression barewords. Parenthesized `require(DBI)` is an expression
    /// under strict and must still be diagnosed. Presence of `require` elsewhere
    /// in the ancestor chain or on the same source line is not enough.
    pub(super) fn is_require_module_operand(
        &self,
        node: &Node,
        ancestors: &[&Node],
        source: &str,
    ) -> bool {
        let Some(parent) = ancestors.last() else {
            return false;
        };
        let NodeKind::FunctionCall { name, args } = &parent.kind else {
            return false;
        };
        if name != "require" {
            return false;
        }
        if !require_first_arg_is(args, node) {
            return false;
        }
        !Self::require_call_parenthesizes_operand(parent, node, source)
    }

    /// Perl treats `require DBI` as a module name, but `require(DBI)` as an
    /// expression. Under `use strict 'subs'` the parenthesized form is a
    /// compile error (`Bareword "DBI.pm" not allowed`).
    fn require_call_parenthesizes_operand(call: &Node, operand: &Node, source: &str) -> bool {
        let Some(between) = source.get(call.location.start..operand.location.start) else {
            return false;
        };
        between.contains('(')
    }
}

/// Direct first-argument identity for a `require` call.
///
/// Empty `args` and a first argument that is not `node` are both non-operands.
fn require_first_arg_is(args: &[Node], node: &Node) -> bool {
    let Some(first) = args.first() else {
        return false;
    };
    std::ptr::eq(first, node)
}

// ============================================================================
// #16670 — call-observation for `is_require_module_operand`.
// Integration tests drive `analyze()`; these `--lib` tests invoke the
// production predicate directly so RIPR can grip the first-arg / paren
// discriminators at the exact seam lines.
// ============================================================================
#[cfg(test)]
mod tests {
    use super::{Node, NodeKind, ScopeAnalyzer, require_first_arg_is};
    use crate::Parser;
    use crate::ast::SourceLocation;
    use perl_tdd_support::{must, must_some_with};

    fn parse(code: &str) -> Node {
        let mut parser = Parser::new(code);
        must(parser.parse())
    }

    fn identifier_hits<'a>(root: &'a Node, name: &str) -> Vec<(&'a Node, Vec<&'a Node>)> {
        let mut found = Vec::new();
        collect_identifiers(root, name, &mut Vec::new(), &mut found);
        found
    }

    fn collect_identifiers<'a>(
        node: &'a Node,
        name: &str,
        ancestors: &mut Vec<&'a Node>,
        found: &mut Vec<(&'a Node, Vec<&'a Node>)>,
    ) {
        if let NodeKind::Identifier { name: ident } = &node.kind
            && ident == name
        {
            found.push((node, ancestors.clone()));
        }
        ancestors.push(node);
        node.for_each_child(|child| collect_identifiers(child, name, ancestors, found));
        ancestors.pop();
    }

    fn first_named<'a>(root: &'a Node, name: &str) -> (&'a Node, Vec<&'a Node>) {
        must_some_with(
            identifier_hits(root, name).into_iter().next(),
            "expected Identifier with that spelling",
        )
    }

    #[test]
    fn unparenthesized_require_first_identifier_is_module_operand() {
        let code = "require DBI;";
        let ast = parse(code);
        let analyzer = ScopeAnalyzer::new();
        let (node, ancestors) = first_named(&ast, "DBI");
        assert!(
            analyzer.is_require_module_operand(node, &ancestors, code),
            "direct first-arg Identifier of unparenthesized require must match"
        );
        let parent = must_some_with(ancestors.last().copied(), "require FunctionCall parent");
        assert!(
            !ScopeAnalyzer::require_call_parenthesizes_operand(parent, node, code),
            "no '(' between require and DBI"
        );
    }

    #[test]
    fn parenthesized_require_first_identifier_is_not_module_operand() {
        let code = "require(DBI);";
        let ast = parse(code);
        let analyzer = ScopeAnalyzer::new();
        let (node, ancestors) = first_named(&ast, "DBI");
        assert!(
            !analyzer.is_require_module_operand(node, &ancestors, code),
            "parenthesized require operand stays an expression bareword"
        );
        let parent = must_some_with(ancestors.last().copied(), "require FunctionCall parent");
        assert!(
            ScopeAnalyzer::require_call_parenthesizes_operand(parent, node, code),
            "'(' between require and DBI must be observed"
        );
    }

    #[test]
    fn spaced_parenthesized_require_is_not_module_operand() {
        let code = "require (DBI);";
        let ast = parse(code);
        let analyzer = ScopeAnalyzer::new();
        let (node, ancestors) = first_named(&ast, "DBI");
        assert!(
            !analyzer.is_require_module_operand(node, &ancestors, code),
            "require (DBI) is still parenthesized"
        );
        let parent = must_some_with(ancestors.last().copied(), "require FunctionCall parent");
        assert!(ScopeAnalyzer::require_call_parenthesizes_operand(parent, node, code));
    }

    #[test]
    fn expression_identifier_is_not_module_operand() {
        let code = "my $x = DBI;";
        let ast = parse(code);
        let analyzer = ScopeAnalyzer::new();
        let (node, ancestors) = first_named(&ast, "DBI");
        assert!(
            !analyzer.is_require_module_operand(node, &ancestors, code),
            "expression-position DBI must take the non-require match arm"
        );
    }

    #[test]
    fn identifier_without_ancestors_is_not_module_operand() {
        let code = "require DBI;";
        let ast = parse(code);
        let analyzer = ScopeAnalyzer::new();
        let (node, _) = first_named(&ast, "DBI");
        assert!(
            !analyzer.is_require_module_operand(node, &[], code),
            "empty ancestor chain is not a require operand"
        );
    }

    #[test]
    fn pointer_inequality_rejects_foreign_identifier_under_require_parent() {
        let code = "require DBI; my $x = Foo;";
        let ast = parse(code);
        let analyzer = ScopeAnalyzer::new();
        let (dbi, require_ancestors) = first_named(&ast, "DBI");
        let (foo, _) = first_named(&ast, "Foo");
        assert!(
            analyzer.is_require_module_operand(dbi, &require_ancestors, code),
            "control: DBI is the require first arg"
        );
        assert!(
            !analyzer.is_require_module_operand(foo, &require_ancestors, code),
            "Foo is not ptr-eq to require's first argument"
        );
    }

    #[test]
    fn nested_identifier_in_computed_require_is_not_first_arg() {
        let code = r#"require(Foo . "/Bar.pm");"#;
        let ast = parse(code);
        let analyzer = ScopeAnalyzer::new();
        let (node, ancestors) = first_named(&ast, "Foo");
        assert!(
            !analyzer.is_require_module_operand(node, &ancestors, code),
            "Foo inside a computed first argument is not the require operand"
        );
    }

    #[test]
    fn empty_require_args_is_not_module_operand() {
        let loc = SourceLocation { start: 0, end: 7 };
        let ident = Node::new(NodeKind::Identifier { name: "DBI".to_string() }, loc);
        let call = Node::new(
            NodeKind::FunctionCall { name: "require".to_string(), args: Vec::new() },
            loc,
        );
        let analyzer = ScopeAnalyzer::new();
        assert!(
            !analyzer.is_require_module_operand(&ident, &[&call], "require"),
            "require with no arguments has no module-name operand"
        );
        assert!(
            !require_first_arg_is(&[], &ident),
            "empty args slice is not a first-arg identity match"
        );
    }

    #[test]
    fn first_arg_identity_distinguishes_same_and_foreign_nodes() {
        let loc = SourceLocation { start: 0, end: 3 };
        let dbi = Node::new(NodeKind::Identifier { name: "DBI".to_string() }, loc);
        let foo = Node::new(NodeKind::Identifier { name: "Foo".to_string() }, loc);
        assert!(
            require_first_arg_is(std::slice::from_ref(&dbi), &dbi),
            "the same Identifier node is the first arg"
        );
        assert!(
            !require_first_arg_is(std::slice::from_ref(&dbi), &foo),
            "a different Identifier node is not the first arg"
        );
    }
}
