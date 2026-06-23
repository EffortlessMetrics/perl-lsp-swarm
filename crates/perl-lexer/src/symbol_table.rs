//! Local symbol table for same-file `sub` declaration pre-pass.
//!
//! A [`LocalSymbolTable`] is built by scanning source text before lexing to collect
//! `sub NAME` declarations. The lexer uses it to set the correct mode for known
//! function names, so a `/` after a declared-but-not-builtin function is tokenized
//! as a regex delimiter rather than a division operator.
//!
//! # Usage
//!
//! ```rust
//! use perl_lexer::{LocalSymbolTable, LexerConfig, PerlLexer};
//!
//! let input = "sub my_builder;\nmy_builder /foo|bar/;";
//! let symbol_table = LocalSymbolTable::scan_subs(input);
//! let config = LexerConfig { symbol_table: Some(symbol_table), ..LexerConfig::default() };
//! let mut lexer = PerlLexer::with_config(input, config);
//! ```

use std::collections::HashSet;

/// A lightweight, file-local symbol table for `sub` declarations.
///
/// Built by [`LocalSymbolTable::scan_subs`] as a pre-pass over the source text
/// before lexing begins. The lexer queries it when processing bare identifiers
/// to determine whether a following `/` is a regex delimiter or a division operator.
///
/// ## Scope
///
/// Only tracks `sub NAME` declarations present in the source text (including forward
/// declarations like `sub foo;`). Does **not** track:
/// - Dynamically generated subs (`eval`, `AUTOLOAD`)
/// - Imported subs (workspace symbol index is a separate concern)
/// - Method calls (`$obj->method`)
/// - Package-qualified calls (`Package::func`)
///
/// ## Forward references
///
/// Because the scan runs over the complete file before lexing, forward declarations
/// (`sub foo;` appearing after `foo /regex/;`) are correctly resolved.
#[derive(Debug, Clone, Default)]
pub struct LocalSymbolTable {
    known_subs: HashSet<String>,
}

impl LocalSymbolTable {
    /// Scan `input` for `sub NAME` declarations and return a populated symbol table.
    ///
    /// The scan is a lightweight byte-level pre-pass. It ignores:
    /// - Line comments (`# ...` up to end of line)
    /// - Single-quoted strings (`'...'`, with backslash-escape awareness)
    /// - Double-quoted strings (`"..."`, with backslash-escape awareness)
    ///
    /// Both forward declarations (`sub foo;`) and full definitions
    /// (`sub foo { ... }`) are collected.
    pub fn scan_subs(input: &str) -> Self {
        let bytes = input.as_bytes();
        let len = bytes.len();
        let mut known_subs = HashSet::new();
        let mut i = 0;

        while i < len {
            match bytes[i] {
                // Line comment — skip to end of line
                b'#' => {
                    i += 1;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                // Single-quoted string — skip to closing `'`
                b'\'' => {
                    i += 1;
                    while i < len {
                        if bytes[i] == b'\\' {
                            i += 2; // skip escaped char
                        } else if bytes[i] == b'\'' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                }
                // Double-quoted string — skip to closing `"`
                b'"' => {
                    i += 1;
                    while i < len {
                        if bytes[i] == b'\\' {
                            i += 2; // skip escaped char
                        } else if bytes[i] == b'"' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                }
                // Candidate for `sub` keyword
                b's' if i + 3 <= len && bytes[i..i + 3] == *b"sub" => {
                    // Verify `sub` is a standalone word: preceding char must not be a word char,
                    // and the char immediately after `sub` must not be a word char either
                    // (to avoid matching e.g. `substr` or `my_sub_thing`).
                    let preceded_by_word = i > 0 && is_word_byte(bytes[i - 1]);
                    let followed_by_word = i + 3 < len && is_word_byte(bytes[i + 3]);

                    if !preceded_by_word && !followed_by_word {
                        // Skip past `sub` and any trailing whitespace
                        i += 3;
                        while i < len && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
                            i += 1;
                        }
                        // Collect the identifier that follows
                        if i < len && is_identifier_start(bytes[i]) {
                            let start = i;
                            while i < len && is_word_byte(bytes[i]) {
                                i += 1;
                            }
                            if let Ok(name) = std::str::from_utf8(&bytes[start..i]) {
                                if !name.is_empty() {
                                    known_subs.insert(name.to_owned());
                                }
                            }
                        }
                    } else {
                        i += 1;
                    }
                }
                _ => {
                    i += 1;
                }
            }
        }

        Self { known_subs }
    }

    /// Return `true` if `name` was declared as a `sub` in this file.
    #[inline]
    pub fn is_known_sub(&self, name: &str) -> bool {
        self.known_subs.contains(name)
    }

    /// Return the number of known sub declarations.
    #[inline]
    pub fn len(&self) -> usize {
        self.known_subs.len()
    }

    /// Return `true` if no sub declarations were found.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.known_subs.is_empty()
    }
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[inline]
fn is_identifier_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::LocalSymbolTable;

    #[test]
    fn scan_subs_empty_input() {
        let st = LocalSymbolTable::scan_subs("");
        assert!(st.is_empty());
        assert_eq!(st.len(), 0);
    }

    #[test]
    fn scan_subs_single_forward_declaration() {
        let st = LocalSymbolTable::scan_subs("sub foo;");
        assert!(st.is_known_sub("foo"));
        assert_eq!(st.len(), 1);
    }

    #[test]
    fn scan_subs_definition_with_body() {
        let st = LocalSymbolTable::scan_subs("sub bar { return 1; }");
        assert!(st.is_known_sub("bar"));
    }

    #[test]
    fn scan_subs_multiple_declarations() {
        let input = "sub foo;\nsub bar { 1 }\nsub baz;";
        let st = LocalSymbolTable::scan_subs(input);
        assert!(st.is_known_sub("foo"));
        assert!(st.is_known_sub("bar"));
        assert!(st.is_known_sub("baz"));
        assert_eq!(st.len(), 3);
    }

    #[test]
    fn scan_subs_ignores_line_comment() {
        let input = "# sub commented_out;\nsub real_sub;";
        let st = LocalSymbolTable::scan_subs(input);
        assert!(!st.is_known_sub("commented_out"), "comment subs must not be collected");
        assert!(st.is_known_sub("real_sub"));
    }

    #[test]
    fn scan_subs_ignores_single_quoted_string() {
        let input = "my $s = 'sub not_a_sub;';\nsub real;";
        let st = LocalSymbolTable::scan_subs(input);
        assert!(!st.is_known_sub("not_a_sub"), "subs inside strings must not be collected");
        assert!(st.is_known_sub("real"));
    }

    #[test]
    fn scan_subs_ignores_double_quoted_string() {
        let input = r#"my $s = "sub not_a_sub;"; sub real;"#;
        let st = LocalSymbolTable::scan_subs(input);
        assert!(!st.is_known_sub("not_a_sub"), "subs inside strings must not be collected");
        assert!(st.is_known_sub("real"));
    }

    #[test]
    fn scan_subs_does_not_match_substr() {
        // `substr` must not cause a spurious lookup of "str"
        let input = "my $s = substr($x, 0, 3);\nsub real;";
        let st = LocalSymbolTable::scan_subs(input);
        assert!(!st.is_known_sub("str"), "substr must not match as 'sub'+'str'");
        assert!(st.is_known_sub("real"));
    }

    #[test]
    fn scan_subs_does_not_match_sub_in_identifier() {
        // `my_sub_func` must not be mistaken for a sub declaration
        let input = "my $my_sub_func = 1;\nsub real;";
        let st = LocalSymbolTable::scan_subs(input);
        assert!(!st.is_known_sub("func"), "sub inside identifier must not match");
        assert!(st.is_known_sub("real"));
    }

    #[test]
    fn is_known_sub_returns_false_for_unknown() {
        let st = LocalSymbolTable::scan_subs("sub foo;");
        assert!(!st.is_known_sub("bar"));
        assert!(!st.is_known_sub("if"));
        assert!(!st.is_known_sub(""));
    }

    #[test]
    fn scan_subs_handles_forward_reference() {
        // The pre-pass scans the whole file, so subs declared after their use are found.
        let input = "foo /regex/;\nsub foo;";
        let st = LocalSymbolTable::scan_subs(input);
        assert!(st.is_known_sub("foo"), "pre-pass must find forward-declared subs");
    }

    #[test]
    fn scan_subs_handles_escaped_quote_in_string() {
        // The \' inside a single-quoted string must not prematurely close it
        let input = r"my $s = 'it\'s a sub not_real;'; sub real;";
        let st = LocalSymbolTable::scan_subs(input);
        assert!(!st.is_known_sub("not_real"));
        assert!(st.is_known_sub("real"));
    }
}
