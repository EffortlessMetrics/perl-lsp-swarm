//! Tests for grep/map/sort with block form followed by a trailing list.
//!
//! In Perl, `grep { BLOCK } LIST`, `map { BLOCK } LIST`, and
//! `sort { BLOCK } LIST` do not require a comma between the block
//! and the list.  These tests verify the parser handles this correctly
//! in both statement and expression (assignment RHS) contexts.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use crate::parser::Parser;
    use perl_ast::ast::{Node, NodeKind};
    use perl_tdd_support::must;

    fn function_call_shape(node: &mut Node, target: &str) -> Option<(usize, bool)> {
        let shape = match &node.kind {
            NodeKind::FunctionCall { name, args } if name == target => Some((
                args.len(),
                matches!(args.first().map(|arg| &arg.kind), Some(NodeKind::Block { .. })),
            )),
            _ => None,
        };
        if shape.is_some() {
            return shape;
        }

        let mut nested = None;
        node.for_each_child_mut(|child| {
            if nested.is_none() {
                nested = function_call_shape(child, target);
            }
        });
        nested
    }

    #[test]
    fn filter_simple_uppercase_block_call() {
        let code = r#"FILTER {
    s/BANG!/return "excited"/g;
    s/MAGIC/42/g;
};"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        assert!(
            parser.errors().is_empty(),
            "should not record parser errors: {:?}",
            parser.errors()
        );
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
        assert!(
            sexp.contains("ambiguous_function_call_expression"),
            "should parse FILTER as a block call: {}",
            sexp
        );
    }

    // ---- grep ----

    #[test]
    fn grep_block_simple_comparison() {
        let code = "grep { $_ > 5 } @array;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(sexp.contains("(call (name grep)"), "should be a grep call: {}", sexp);
        assert!(sexp.contains("(block"), "should contain a block: {}", sexp);
        assert!(
            sexp.contains("(variable (sigil @) (name array))"),
            "trailing list should be inside call: {}",
            sexp
        );
    }
    #[test]
    fn grep_block_method_call_in_block() {
        let code = "grep { $_->is_valid } @items;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(variable (sigil @) (name items))"),
            "trailing list should be inside call: {}",
            sexp
        );
    }

    #[test]
    fn grep_block_regex_in_block() {
        let code = r#"grep { /pattern/ } @strings;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(variable (sigil @) (name strings))"),
            "trailing list should be inside call: {}",
            sexp
        );
    }

    #[test]
    fn grep_block_assigned_to_variable() {
        let code = "my @result = grep { $_->is_valid } @items;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
        // The trailing list must be inside the grep call, not a separate statement
        assert!(
            sexp.contains("(call (name grep)")
                && sexp.contains("(variable (sigil @) (name items))"),
            "trailing list should be inside the grep call: {}",
            sexp
        );
    }

    #[test]
    fn user_defined_ampersand_prototype_uses_block_call_shape() {
        let code = r#"
            sub my_map (&@) { }
            my @result = my_map { $_ * 2 } 1, 2, 3;
        "#;
        let mut parser = Parser::new(code);
        let mut ast = must(parser.parse());
        assert!(
            parser.errors().is_empty(),
            "user-defined block-taking call should parse without errors: {:?}",
            parser.errors()
        );

        let (argument_count, starts_with_block) =
            function_call_shape(&mut ast, "my_map").expect("my_map call should be present");
        assert!(starts_with_block, "block should be the first call argument");
        assert_eq!(argument_count, 4, "block and three list arguments should be retained");
    }

    #[test]
    fn qualified_user_defined_block_call_uses_call_shape() {
        let code = "sub My::List::map (&@) { } My::List::map { $_ * 2 } @items;";
        let mut parser = Parser::new(code);
        let mut ast = must(parser.parse());
        let (argument_count, starts_with_block) = function_call_shape(&mut ast, "My::List::map")
            .expect("qualified map call should be present");
        assert!(starts_with_block, "qualified call should start with a block argument");
        assert_eq!(argument_count, 2, "qualified call should retain its block and list arguments");
    }

    // ---- map ----

    #[test]
    fn map_block_with_range() {
        let code = "map { $_ * 2 } 1..10;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(sexp.contains("(call (name map)"), "should be a map call: {}", sexp);
        assert!(sexp.contains(".."), "trailing range should be inside call: {}", sexp);
    }

    #[test]
    fn map_block_array_subscript_in_block() {
        let code = r#"map { $_->[0] + $_->[1] } @pairs;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(variable (sigil @) (name pairs))"),
            "trailing list should be inside call: {}",
            sexp
        );
    }

    #[test]
    fn map_block_assigned_to_variable() {
        let code = r#"my @mapped = map { $_->[0] + $_->[1] } @pairs;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
        assert!(
            sexp.contains("(call (name map)") && sexp.contains("(variable (sigil @) (name pairs))"),
            "trailing list should be inside the map call: {}",
            sexp
        );
    }

    // ---- sort ----

    #[test]
    fn sort_block_numeric_comparison() {
        let code = "sort { $a <=> $b } @numbers;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(sexp.contains("(call (name sort)"), "should be a sort call: {}", sexp);
        assert!(
            sexp.contains("(variable (sigil @) (name numbers))"),
            "trailing list should be inside call: {}",
            sexp
        );
    }

    #[test]
    fn sort_block_hash_access_in_block() {
        let code = r#"sort { $a->{name} cmp $b->{name} } @records;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(variable (sigil @) (name records))"),
            "trailing list should be inside call: {}",
            sexp
        );
    }

    #[test]
    fn sort_block_function_call_in_block() {
        let code = r#"sort { length($a) <=> length($b) } keys %hash;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(sexp.contains("(call (name sort)"), "should be a sort call: {}", sexp);
        // keys %hash should be inside the sort call
        assert!(sexp.contains("(call (name keys)"), "keys should be inside sort call: {}", sexp);
    }

    #[test]
    fn sort_block_assigned_to_variable() {
        let code = r#"my @sorted = sort { $a->{name} cmp $b->{name} } @records;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
        assert!(
            sexp.contains("(call (name sort)")
                && sexp.contains("(variable (sigil @) (name records))"),
            "trailing list should be inside the sort call: {}",
            sexp
        );
    }

    #[test]
    fn sort_block_with_keys_hash_assigned() {
        let code = r#"my @x = sort { length($a) <=> length($b) } keys %hash;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
        assert!(
            sexp.contains("(call (name sort)") && sexp.contains("(call (name keys)"),
            "keys call should be inside the sort call: {}",
            sexp
        );
    }

    // ---- chained builtins ----

    #[test]
    fn chained_grep_map() {
        let code = r#"grep { /pattern/ } map { lc $_ } @strings;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
        assert!(sexp.contains("(call (name grep)"), "outer should be grep: {}", sexp);
        assert!(sexp.contains("(call (name map)"), "inner should be map: {}", sexp);
    }

    #[test]
    fn chained_sort_map() {
        let code = r#"my @result = sort { $a cmp $b } map { lc } @strings;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
        assert!(sexp.contains("(call (name sort)"), "outer should be sort: {}", sexp);
        assert!(sexp.contains("(call (name map)"), "inner should be map: {}", sexp);
    }

    // ---- edge cases ----

    #[test]
    fn grep_block_with_comma_before_list() {
        // Perl also allows a comma: grep { ... }, @array
        let code = "grep { $_ > 5 }, @array;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(sexp.contains("(call (name grep)"), "should be a grep call: {}", sexp);
        assert!(
            sexp.contains("(variable (sigil @) (name array))"),
            "trailing list with comma should work: {}",
            sexp
        );
    }

    #[test]
    fn sort_block_empty_block() {
        // sort {} @array -- empty block comparison
        let code = "sort {} @array;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
    }
}
