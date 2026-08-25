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
    use perl_tdd_support::must;

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
        assert!(sexp.contains("(call grep"), "should be a grep call: {}", sexp);
        assert!(sexp.contains("(block"), "should contain a block: {}", sexp);
        assert!(
            sexp.contains("(variable @ array)"),
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
            sexp.contains("(variable @ items)"),
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
            sexp.contains("(variable @ strings)"),
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
            sexp.contains("(call grep") && sexp.contains("(variable @ items)"),
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
        let ast = must(parser.parse());
        assert!(
            parser.errors().is_empty(),
            "user-defined block-taking call should parse without errors: {:?}",
            parser.errors()
        );

        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(call my_map"),
            "user-defined & prototype should use a call node: {sexp}"
        );
        assert!(sexp.contains("(block"), "block should be the first call argument: {sexp}");
        assert!(
            sexp.contains("(number 1)")
                && sexp.contains("(number 2)")
                && sexp.contains("(number 3)"),
            "trailing list arguments should remain inside the call: {sexp}"
        );
        assert!(
            !sexp.contains("ambiguous_function_call_expression"),
            "user-defined block-taking call should not be ambiguous: {sexp}"
        );
    }

    #[test]
    fn qualified_user_defined_block_call_uses_call_shape() {
        let code = "My::List::map { $_ * 2 } @items;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();

        assert!(
            sexp.contains("(call My::List::map"),
            "qualified block-taking call should use a call node: {sexp}"
        );
        assert!(sexp.contains("(block"), "qualified call should contain a block: {sexp}");
        assert!(
            sexp.contains("(variable @ items)"),
            "qualified call should retain the trailing list argument: {sexp}"
        );
    }

    // ---- map ----

    #[test]
    fn map_block_with_range() {
        let code = "map { $_ * 2 } 1..10;";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(sexp.contains("(call map"), "should be a map call: {}", sexp);
        assert!(sexp.contains(".."), "trailing range should be inside call: {}", sexp);
    }

    #[test]
    fn map_block_array_subscript_in_block() {
        let code = r#"map { $_->[0] + $_->[1] } @pairs;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(
            sexp.contains("(variable @ pairs)"),
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
            sexp.contains("(call map") && sexp.contains("(variable @ pairs)"),
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
        assert!(sexp.contains("(call sort"), "should be a sort call: {}", sexp);
        assert!(
            sexp.contains("(variable @ numbers)"),
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
            sexp.contains("(variable @ records)"),
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
        assert!(sexp.contains("(call sort"), "should be a sort call: {}", sexp);
        // keys %hash should be inside the sort call
        assert!(sexp.contains("(call keys"), "keys should be inside sort call: {}", sexp);
    }

    #[test]
    fn sort_block_assigned_to_variable() {
        let code = r#"my @sorted = sort { $a->{name} cmp $b->{name} } @records;"#;
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let sexp = ast.to_sexp();
        assert!(!sexp.contains("ERROR"), "should not contain ERROR: {}", sexp);
        assert!(
            sexp.contains("(call sort") && sexp.contains("(variable @ records)"),
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
            sexp.contains("(call sort") && sexp.contains("(call keys"),
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
        assert!(sexp.contains("(call grep"), "outer should be grep: {}", sexp);
        assert!(sexp.contains("(call map"), "inner should be map: {}", sexp);
