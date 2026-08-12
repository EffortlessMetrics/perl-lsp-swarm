//! This crate provides perl language support for the [tree-sitter][] parsing library.
//!
//! Typically, you will use the [language][language func] function to add this language to a
//! tree-sitter [Parser][], and then use the parser to parse some code:
//!
//! ```
//! let code = "";
//! let mut parser = tree_sitter::Parser::new();
//! let language: tree_sitter::Language = tree_sitter_perl::language().into();
//! parser.set_language(&language).expect("Error loading perl grammar");
//! let tree = parser.parse(code, None).unwrap();
//! ```
//!
//! [Language]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Language.html
//! [language func]: fn.language.html
//! [Parser]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Parser.html
//! [tree-sitter]: https://tree-sitter.github.io/

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_perl() -> *const ();
}

/// Get the tree-sitter [LanguageFn][] for this grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_perl) };

/// Get the tree-sitter [LanguageFn][] for this grammar.
pub fn language() -> LanguageFn {
    LANGUAGE
}

/// The content of the [`node-types.json`][] file for this grammar.
///
/// [`node-types.json`]: https://tree-sitter.github.io/tree-sitter/using-parsers#static-node-types
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

// Uncomment these to include any queries that this grammar contains

pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");
// pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");
// pub const TAGS_QUERY: &str = include_str!("../../queries/tags.scm");

#[cfg(test)]
mod tests {
    use tree_sitter::Parser;

    #[test]
    fn test_can_load_grammar() {
        let mut parser = Parser::new();
        let language: tree_sitter::Language = super::language().into();
        parser.set_language(&language).expect("Error loading perl language");
    }

    #[test]
    fn deep_quote_stack_preserves_the_64_byte_delimiter() {
        let delimiter = "A".repeat(64);
        let mut nested = String::from("${qq{");
        for _ in 0..78 {
            nested.push_str("${qq{");
        }
        nested.push('X');
        for _ in 0..79 {
            nested.push_str("}}");
        }

        let source = format!("my $value = <<{delimiter};\n{nested}\n{delimiter}\nprint $value;\n");
        let mut parser = Parser::new();
        let language: tree_sitter::Language = super::language().into();
        parser.set_language(&language).expect("Error loading perl language");
        let tree = parser.parse(source, None).expect("tree-sitter returned no tree");
        assert!(
            !tree.root_node().has_error(),
            "unexpected parse error: {}",
            tree.root_node().to_sexp()
        );
    }
}
