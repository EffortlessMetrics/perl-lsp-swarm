#[cfg(test)]
mod bless_parsing_tests {
    use perl_parser::Parser;

    fn parse_and_check(input: &str, expected_sexp: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut parser = Parser::new(input);
        let result = parser.parse()?;
        let sexp = result.to_sexp();
        assert_eq!(sexp.trim(), expected_sexp.trim(), "Input: {}", input);
        Ok(())
    }

    #[test]
    fn test_bless_empty_hash() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "bless {}",
            "(source_file (statements (expression_statement (expression (call (name bless) (args (hash)))))))",
        )
    }

    #[test]
    fn test_bless_empty_hash_with_class() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "bless {}, $class",
            "(source_file (statements (expression_statement (expression (call (name bless) (args (hash)) (args (variable (sigil $) (name class))))))))",
        )
    }

    #[test]
    fn test_bless_with_string_literal() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "bless {}, 'Foo'",
            "(source_file (statements (expression_statement (expression (call (name bless) (args (hash)) (args (string (value 'Foo'))))))))",
        )
    }

    #[test]
    fn test_return_bless_empty_hash() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "return bless {}",
            "(source_file (statements (return (value (call (name bless) (args (hash)))))))",
        )
    }

    #[test]
    fn test_return_bless_with_class() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "return bless {}, $class",
            "(source_file (statements (return (value (call (name bless) (args (hash)) (args (variable (sigil $) (name class))))))))",
        )
    }

    #[test]
    fn test_bless_in_subroutine() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "sub new { return bless {}, shift; }",
            "(source_file (statements (sub (name new) (body (block (statements (return (value (call (name bless) (args (hash)) (args (call (name shift))))))))))))",
        )
    }

    #[test]
    fn test_bless_with_hashref_data() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "bless { foo => 1, bar => 2 }, $class",
            "(source_file (statements (expression_statement (expression (call (name bless) (args (hash (key (string (value foo))) (value (number (value 1))) (key (string (value bar))) (value (number (value 2))))) (args (variable (sigil $) (name class))))))))",
        )
    }

    #[test]
    fn test_nested_bless_calls() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "bless { inner => bless {}, 'Inner' }, 'Outer'",
            "(source_file (statements (expression_statement (expression (call (name bless) (args (hash (key (string (value inner))) (value (call (name bless) (args (hash)) (args (string (value 'Inner'))))))) (args (string (value 'Outer'))))))))",
        )
    }

    #[test]
    fn test_bless_with_variable_hashref() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "bless $data, $class",
            "(source_file (statements (expression_statement (expression (call (name bless) (args (variable (sigil $) (name data))) (args (variable (sigil $) (name class))))))))",
        )
    }

    #[test]
    fn test_my_variable_assignment_with_bless() -> Result<(), Box<dyn std::error::Error>> {
        parse_and_check(
            "my $obj = bless {}, $class",
            "(source_file (statements (my_declaration (declarator my) (variable (variable (sigil $) (name obj))) (initializer (call (name bless) (args (hash)) (args (variable (sigil $) (name class))))))))",
        )
    }
}
