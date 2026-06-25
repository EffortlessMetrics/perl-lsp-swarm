//! Pre-lexing symbol table for bareword/regex disambiguation.
//!
//! Performs a lightweight scan of the source before lexing to collect `sub NAME`
//! declarations. The result is passed to [`LexerConfig`] so the lexer can treat
//! known function names as term-introducing, allowing `/` after them to be
//! classified as a regex delimiter rather than division.
//!
//! # Scope and limitations (v1)
//!
//! - Single-file scope only: cross-file / imported symbols require workspace-level
//!   tracking and are a follow-up.
//! - Forward references: the pre-pass scans to EOF, so `my_func /re/; sub my_func {}`
//!   is handled correctly even though the call precedes the declaration.
//! - Dynamic subs (`eval "sub foo {}"`, `AUTOLOAD`): not tracked.  These are a static
//!   analysis limitation shared by all Perl tooling.
//! - `use constant NAME => ...` constants: tracked so constant-name `/re/` is also
//!   disambiguated correctly.

use std::collections::HashSet;

/// Lightweight per-file symbol table populated by a pre-lexing scan.
///
/// After construction via [`LocalSymbolTable::scan_subs`], pass it to
/// [`crate::LexerConfig::symbol_table`] so the lexer can resolve bareword mode
/// correctly.
#[derive(Debug, Clone, Default)]
pub struct LocalSymbolTable {
    /// Named `sub` declarations found in the source.
    pub known_subs: HashSet<String>,
    /// Named constants declared via `use constant NAME =>`.
    pub known_constants: HashSet<String>,
}

impl LocalSymbolTable {
    /// Create an empty symbol table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return `true` if `name` was declared as a subroutine in this file.
    pub fn is_known_sub(&self, name: &str) -> bool {
        self.known_subs.contains(name)
    }

    /// Return `true` if `name` was declared as a constant in this file.
    pub fn is_known_constant(&self, name: &str) -> bool {
        self.known_constants.contains(name)
    }

    /// Scan `source` and collect sub/constant declarations.
    ///
    /// Uses a character-by-character state machine that skips string literals and
    /// line comments so declarations inside strings or after `#` are not collected.
    ///
    /// ```
    /// use perl_lexer::LocalSymbolTable;
    ///
    /// let table = LocalSymbolTable::scan_subs("sub my_builder; sub helper {}");
    /// assert!(table.is_known_sub("my_builder"));
    /// assert!(table.is_known_sub("helper"));
    /// assert!(!table.is_known_sub("unrelated"));
    /// ```
    pub fn scan_subs(source: &str) -> Self {
        let mut table = Self::new();
        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            match bytes[i] {
                // Line comment: skip to end of line.
                b'#' => {
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                // Double-quoted string: skip until matching unescaped `"`.
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
                // Single-quoted string: skip until matching unescaped `'`.
                b'\'' => {
                    i += 1;
                    while i < len {
                        if bytes[i] == b'\\' {
                            i += 2;
                        } else if bytes[i] == b'\'' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                }
                // Backtick: skip to matching unescaped `` ` ``.
                b'`' => {
                    i += 1;
                    while i < len {
                        if bytes[i] == b'\\' {
                            i += 2;
                        } else if bytes[i] == b'`' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                }
                b's' if source[i..].starts_with("sub") => {
                    // Check it's a real `sub` keyword (not a longer identifier).
                    let after_sub = i + 3;
                    let preceded_by_word_char = i > 0 && is_word_char(bytes[i - 1]);
                    let followed_by_word_char = after_sub < len && is_word_char(bytes[after_sub]);

                    if !preceded_by_word_char && !followed_by_word_char {
                        // Skip whitespace after `sub`.
                        let mut j = after_sub;
                        while j < len && (bytes[j] == b' ' || bytes[j] == b'\t') {
                            j += 1;
                        }
                        // Collect the identifier.
                        if j < len && is_identifier_start(bytes[j]) {
                            let name_start = j;
                            while j < len && is_word_char(bytes[j]) {
                                j += 1;
                            }
                            let name = &source[name_start..j];
                            table.known_subs.insert(name.to_owned());
                            i = j;
                            continue;
                        }
                    }
                    i += 1;
                }
                b'u' if source[i..].starts_with("use") => {
                    // Look for `use constant NAME =>` or `use constant NAME,`.
                    let after_use = i + 3;
                    let preceded_by_word_char = i > 0 && is_word_char(bytes[i - 1]);
                    let followed_by_word_char = after_use < len && is_word_char(bytes[after_use]);

                    if !preceded_by_word_char && !followed_by_word_char {
                        let rest = &source[after_use..];
                        if let Some(name) = extract_constant_name(rest) {
                            table.known_constants.insert(name.to_owned());
                        }
                    }
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }

        table
    }
}

/// Extract the constant name from the part of source after `use ` (the `constant NAME` suffix).
///
/// Returns `Some(name)` if this looks like `use constant NAME =>` or `use constant NAME,`.
fn extract_constant_name(after_use: &str) -> Option<&str> {
    let rest = after_use.trim_ascii_start();
    let rest = rest.strip_prefix("constant")?;
    let rest = rest.trim_ascii_start();

    // Single-constant form: `use constant NAME => value`
    // Hash form (multiple constants) is not tracked in v1.
    // Skip if starts with `{` (hash ref form).
    if rest.starts_with('{') {
        return None;
    }

    let name_end = rest.find(|c: char| !is_word_char(c as u8))?;
    let name = &rest[..name_end];
    if name.is_empty()
        || !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        return None;
    }
    Some(name)
}

#[inline]
fn is_word_char(b: u8) -> bool {
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
    fn scan_empty_source_yields_empty_table() {
        let t = LocalSymbolTable::scan_subs("");
        assert!(t.known_subs.is_empty());
        assert!(t.known_constants.is_empty());
    }

    #[test]
    fn scan_single_sub_declaration() {
        let t = LocalSymbolTable::scan_subs("sub my_func { 1 }");
        assert!(t.is_known_sub("my_func"));
    }

    #[test]
    fn scan_forward_declaration_semicolon_form() {
        let t = LocalSymbolTable::scan_subs("sub builder;");
        assert!(t.is_known_sub("builder"));
    }

    #[test]
    fn scan_multiple_subs() {
        let src = "sub alpha { }\nsub beta;\nsub gamma {}";
        let t = LocalSymbolTable::scan_subs(src);
        assert!(t.is_known_sub("alpha"));
        assert!(t.is_known_sub("beta"));
        assert!(t.is_known_sub("gamma"));
    }

    #[test]
    fn is_known_sub_false_for_undeclared() {
        let t = LocalSymbolTable::scan_subs("sub foo;");
        assert!(!t.is_known_sub("bar"));
    }

    #[test]
    fn sub_in_comment_not_collected() {
        let t = LocalSymbolTable::scan_subs("# sub commented_out;\nmy $x = 1;");
        assert!(!t.is_known_sub("commented_out"));
    }

    #[test]
    fn sub_in_double_quoted_string_not_collected() {
        let t = LocalSymbolTable::scan_subs(r#"my $s = "sub fake_sub { }";"#);
        assert!(!t.is_known_sub("fake_sub"));
    }

    #[test]
    fn sub_in_single_quoted_string_not_collected() {
        let t = LocalSymbolTable::scan_subs("my $s = 'sub fake_sub { }';");
        assert!(!t.is_known_sub("fake_sub"));
    }

    #[test]
    fn substring_does_not_trigger_sub_scan() {
        // `subscribe` must not be seen as `sub scribe`
        let t = LocalSymbolTable::scan_subs("my $x = subscribe();");
        assert!(!t.is_known_sub("scribe"));
    }

    #[test]
    fn scan_use_constant_single_name() {
        let t = LocalSymbolTable::scan_subs("use constant MY_CONST => 42;");
        assert!(t.is_known_constant("MY_CONST"));
    }

    #[test]
    fn scan_use_constant_does_not_collect_hash_form() {
        let t = LocalSymbolTable::scan_subs("use constant { A => 1, B => 2 };");
        assert!(!t.is_known_constant("A"));
        assert!(!t.is_known_constant("B"));
    }

    #[test]
    fn forward_reference_is_collected() {
        // Pre-pass scans whole file, so declaration after use is still found.
        let src = "builder /foo/;\nsub builder { return 1; }";
        let t = LocalSymbolTable::scan_subs(src);
        assert!(t.is_known_sub("builder"), "pre-pass must find forward declarations");
    }
}
