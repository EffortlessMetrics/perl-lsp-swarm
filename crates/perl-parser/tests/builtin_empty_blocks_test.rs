#[cfg(test)]
mod builtin_empty_blocks_tests {
    use perl_parser::Parser;

    fn parse_and_check(input: &str, expected_contains: &str) {
        use perl_tdd_support::must;
        let mut parser = Parser::new(input);
        let result = must(parser.parse());
        let sexp = result.to_sexp();
        assert!(
            sexp.contains(expected_contains),
            "Expected '{}' to contain '{}', but got: {}",
            sexp,
            expected_contains,
            sexp
        );
    }

    #[test]
    fn test_sort_empty_block() {
        parse_and_check(
            "sort {} @array",
            "(call (name sort) (args (block)) (args (variable (sigil @) (name array)))",
        );
    }

    #[test]
    fn test_map_empty_block() {
        parse_and_check(
            "map {} @array",
            "(call (name map) (args (block)) (args (variable (sigil @) (name array)))",
        );
    }

    #[test]
    fn test_grep_empty_block() {
        parse_and_check(
            "grep {} @array",
            "(call (name grep) (args (block)) (args (variable (sigil @) (name array)))",
        );
    }

    #[test]
    fn test_sort_with_expression() {
        parse_and_check("sort { $a cmp $b } @array", "(call (name sort) (args (block");
    }

    #[test]
    fn test_map_with_expression() {
        parse_and_check("map { $_ * 2 } @array", "(call (name map) (args (block");
    }

    #[test]
    fn test_grep_with_expression() {
        parse_and_check("grep { $_ > 5 } @array", "(call (name grep) (args (block");
    }

    #[test]
    fn test_ref_empty_hash() {
        parse_and_check("ref {}", "(call (name ref) (args (hash))");
    }

    #[test]
    fn test_defined_empty_hash() {
        parse_and_check("defined {}", "(call (name defined) (args (hash))");
    }

    #[test]
    fn test_scalar_empty_hash() {
        parse_and_check("scalar {}", "(call (name scalar) (args (hash))");
    }

    #[test]
    fn test_keys_empty_hash() {
        parse_and_check("keys {}", "(call (name keys) (args (hash))");
    }

    #[test]
    fn test_values_empty_hash() {
        parse_and_check("values {}", "(call (name values) (args (hash))");
    }

    #[test]
    fn test_each_empty_hash() {
        parse_and_check("each {}", "(call (name each) (args (hash))");
    }

    #[test]
    fn test_return_sort_empty_block() {
        parse_and_check(
            "return sort {} @array",
            "(return (value (call (name sort) (args (block)) (args (variable (sigil @) (name array)))))",
        );
    }

    #[test]
    fn test_return_map_empty_block() {
        parse_and_check(
            "return map {} @array",
            "(return (value (call (name map) (args (block)) (args (variable (sigil @) (name array)))))",
        );
    }

    #[test]
    fn test_return_grep_empty_block() {
        parse_and_check(
            "return grep {} @array",
            "(return (value (call (name grep) (args (block)) (args (variable (sigil @) (name array)))))",
        );
    }
}
