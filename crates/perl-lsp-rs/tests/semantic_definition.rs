//! Semantic-aware textDocument/definition tests
//!
//! These tests verify that the LSP definition handler uses SemanticAnalyzer
//! for precise symbol resolution rather than heuristic-based approaches.
//!
//! The LSP handler at lsp_server.rs:3463 already uses SemanticAnalyzer::find_definition().
//! These tests validate that it works correctly for common Perl patterns.

// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stdout/print_stderr don't
// apply the way they do to production code.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod common;

#[cfg(test)]
mod semantic_definition_tests {
    use crate::common::test_utils::{
        TestServerBuilder, assertions::assert_definition_at, semantic::find_pos,
    };

    #[test]
    fn definition_finds_scalar_variable_declaration() -> Result<(), Box<dyn std::error::Error>> {
        let code = "my $x = 1;\n$x + 2;\n";
        let uri = "file:///test.pl";

        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);

        // Position on the `$x` reference in the second line
        let (line, character) = find_pos(code, "$x", 1);
        let response = server.get_definition(uri, line, character);
        println!("SCALAR DEF RESPONSE: {response:#}");

        assert_definition_at(&response, uri, 0)?;
        Ok(())
    }

    #[test]
    fn definition_finds_subroutine_declaration() -> Result<(), Box<dyn std::error::Error>> {
        let code = "sub foo { 1 }\nmy $x = foo();\n";
        let uri = "file:///test.pl";

        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);

        // Position on "foo" in the call expression
        let (line, character) = find_pos(code, "foo()", 1);
        let response = server.get_definition(uri, line, character);
        println!("SUB DEF RESPONSE: {response:#}");

        assert_definition_at(&response, uri, 0)?;
        Ok(())
    }

    #[test]
    fn definition_resolves_scoped_variables() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"my $outer = 1;
sub foo {
    my $inner = 2;
    return $inner + $outer;
}
"#;
        let uri = "file:///test.pl";

        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);

        // Position on `$inner` in the return expression (line 3)
        let (line, character) = find_pos(code, "$inner", 3);
        let response = server.get_definition(uri, line, character);
        println!("SCOPED DEF RESPONSE: {response:#}");

        assert_definition_at(&response, uri, 2)?;
        Ok(())
    }

    #[test]
    fn definition_handles_package_qualified_calls() -> Result<(), Box<dyn std::error::Error>> {
        let code = r#"package Foo {
    sub bar { 42 }
}

package main;
Foo::bar();
"#;
        let uri = "file:///test.pl";

        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);

        // Position on "bar" in Foo::bar() (line 5)
        let (line, character) = find_pos(code, "bar()", 5);
        let response = server.get_definition(uri, line, character);
        println!("PKG DEF RESPONSE: {response:#}");

        assert_definition_at(&response, uri, 1)?;
        Ok(())
    }
}
