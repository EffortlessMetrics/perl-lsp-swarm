//! Lightweight pre-pass symbol table for file-local subroutine declarations.
//!
//! Scans Perl source text for `sub NAME` declarations before lexing so the
//! lexer can set the correct mode when it encounters an unknown bareword
//! followed by `/`. Without this table every unknown bareword defaults to
//! `ExpectOperator`, causing the `/` to be lexed as division instead of a
//! regex delimiter.
//!
//! # Scope (v1)
//!
//! - Only `sub NAME` declarations present anywhere in the file are tracked.
//! - Comments (`#` to end of line) and string literals are skipped.
//! - Dynamic subs (`eval "sub foo { }"`, AUTOLOAD) are not tracked.
//! - Imports from other files are not tracked (workspace-level follow-up).
//! - Forward references are supported because the whole file is scanned before
//!   lexing begins.
//!
//! # Examples
//!
//! ```rust
//! use perl_lexer::LocalSymbolTable;
//!
//! let table = LocalSymbolTable::scan_subs("sub foo; sub bar { 1 }");
//! assert!(table.is_known_sub("foo"));
//! assert!(table.is_known_sub("bar"));
//! assert!(!table.is_known_sub("baz"));
//! ```

use std::collections::HashSet;

/// File-local subroutine name table built by scanning source text.
#[derive(Debug, Default, Clone)]
pub struct LocalSymbolTable {
    known_subs: HashSet<String>,
}

impl LocalSymbolTable {
    /// Scan `source` for `sub NAME` declarations and return a populated table.
    ///
    /// Skips `#`-style comments and single- and double-quoted string literals
    /// to avoid false positives. All other `sub NAME` patterns (including those
    /// inside blocks or nested subs) are collected, which is the correct
    /// conservative behaviour for disambiguating `/` after an unknown bareword.
    pub fn scan_subs(source: &str) -> Self {
        let mut known_subs = HashSet::new();
        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            match bytes[i] {
                // Skip line comments.
                b'#' => {
                    i += 1;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                }

                // Skip single-quoted strings (no interpolation, so no nested
                // subs can appear unless the string is `eval`-ed; we ignore that).
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

                // Skip double-quoted strings.
                b'"' => {
                    i += 1;
                    while i < len {
                        if bytes[i] == b'\\' {
                            i += 2;
                        } else if bytes[i] == b'"' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                }

                // Possible start of `sub`.
                b's' => {
                    // Check for the literal bytes `sub` followed by a whitespace
                    // character, and that the `s` is at a word boundary.
                    let at_word_boundary =
                        i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');

                    if at_word_boundary
                        && i + 3 < len
                        && bytes[i + 1] == b'u'
                        && bytes[i + 2] == b'b'
                        && (bytes[i + 3] == b' '
                            || bytes[i + 3] == b'\t'
                            || bytes[i + 3] == b'\n'
                            || bytes[i + 3] == b'\r')
                    {
                        // Skip `sub` and trailing whitespace.
                        let mut j = i + 3;
                        while j < len
                            && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\r')
                        {
                            j += 1;
                        }

                        // Skip optional newline (declaration on next line is rare but legal).
                        if j < len && bytes[j] == b'\n' {
                            j += 1;
                            while j < len
                                && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\r')
                            {
                                j += 1;
                            }
                        }

                        // Collect the identifier name.
                        let name_start = j;
                        while j < len && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                            j += 1;
                        }

                        if j > name_start {
                            if let Ok(name) = std::str::from_utf8(&bytes[name_start..j]) {
                                known_subs.insert(name.to_owned());
                            }
                        }

                        i = j;
                        continue;
                    }
                    i += 1;
                }

                _ => {
                    i += 1;
                }
            }
        }

        Self { known_subs }
    }

    /// Returns `true` if `name` was declared as a subroutine anywhere in the
    /// scanned source.
    #[inline]
    pub fn is_known_sub(&self, name: &str) -> bool {
        self.known_subs.contains(name)
    }
}
