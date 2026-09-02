//! Validated literal relative file requests.
//!
//! `require "Foo/Bar.pm"` and `require "Foo::Bar"` are *filename* lookups
//! against `@INC`, not module-name lookups. Perl does not translate `::` to `/`
//! for a quoted operand, so a [`ModuleFilePath`] deliberately preserves the
//! literal spelling and never converts into a [`ModuleName`].
//!
//! [`ModuleName`]: super::ModuleName

use std::fmt;

/// Why a candidate string is not a validated literal relative file request.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleFilePathError {
    /// The input was empty or contained only separators.
    Empty,
    /// The input contained an interior NUL byte.
    InteriorNul,
    /// The input contained a control character.
    ControlCharacter {
        /// The offending character.
        character: char,
    },
    /// The input was an absolute path.
    Absolute,
    /// The input used a UNC path prefix.
    UncPrefix,
    /// The input used drive-qualified syntax (`C:` or `C:\`).
    DriveQualified,
    /// The input contained a `..` traversal component.
    Traversal,
    /// The input is still wrapped in its source quote delimiters.
    ///
    /// A quoted `require` operand must be handed over *decoded*. An undecoded
    /// token would otherwise validate as a filename that literally contains the
    /// quote bytes, and the resolver would search for `'Foo/Bar.pm'` rather than
    /// `Foo/Bar.pm`.
    UndecodedQuoting {
        /// The delimiter the input is wrapped in.
        delimiter: char,
    },
}

impl fmt::Display for ModuleFilePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("literal file request is empty"),
            Self::InteriorNul => f.write_str("literal file request contains an interior NUL"),
            Self::ControlCharacter { character } => write!(
                f,
                "literal file request contains control character U+{:04X}",
                *character as u32
            ),
            Self::Absolute => f.write_str("literal file request is an absolute path"),
            Self::UncPrefix => f.write_str("literal file request uses a UNC prefix"),
            Self::DriveQualified => f.write_str("literal file request is drive-qualified"),
            Self::Traversal => f.write_str("literal file request contains a `..` component"),
            Self::UndecodedQuoting { delimiter } => write!(
                f,
                "literal file request is still wrapped in `{delimiter}` delimiters; \
                 pass the decoded operand, not the raw source token"
            ),
        }
    }
}

impl std::error::Error for ModuleFilePathError {}

impl ModuleFilePathError {
    /// Stable identifier for evidence rows and diagnostics.
    #[must_use]
    pub const fn boundary_id(&self) -> &'static str {
        match self {
            Self::Empty => "module_file_path.empty",
            Self::InteriorNul => "module_file_path.interior_nul",
            Self::ControlCharacter { .. } => "module_file_path.control_character",
            Self::Absolute => "module_file_path.absolute",
            Self::UncPrefix => "module_file_path.unc_prefix",
            Self::DriveQualified => "module_file_path.drive_qualified",
            Self::Traversal => "module_file_path.traversal",
            Self::UndecodedQuoting { .. } => "module_file_path.undecoded_quoting",
        }
    }
}

/// A validated literal relative file request.
///
/// The literal spelling is preserved exactly. Validation proves the request is
/// *relative* and free of traversal, NUL, and control characters; it proves
/// nothing about the filesystem. Existence, root admission, and containment
/// remain the resolver's job.
///
/// # Input contract
///
/// The input is the **decoded** operand — the string value Perl would look up —
/// not the raw source token. `require 'Foo/Bar.pm'` must be presented as
/// `Foo/Bar.pm`, with its delimiters already removed by the lexer or by the
/// caller's HIR conversion. Handing over an undecoded token is rejected as
/// [`ModuleFilePathError::UndecodedQuoting`] rather than accepted as a filename
/// that happens to contain quote bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleFilePath {
    literal: String,
}

impl ModuleFilePath {
    /// Validate `text` as a literal relative file request.
    ///
    /// # Errors
    ///
    /// Returns the classified [`ModuleFilePathError`] for the first rule `text`
    /// violates.
    pub fn parse(text: &str) -> Result<Self, ModuleFilePathError> {
        if text.is_empty() {
            return Err(ModuleFilePathError::Empty);
        }

        for character in text.chars() {
            if character == '\0' {
                return Err(ModuleFilePathError::InteriorNul);
            }
            if character.is_control() {
                return Err(ModuleFilePathError::ControlCharacter { character });
            }
        }

        if let Some(delimiter) = undecoded_quote_delimiter(text) {
            return Err(ModuleFilePathError::UndecodedQuoting { delimiter });
        }

        if text.starts_with("\\\\") || text.starts_with("//") {
            return Err(ModuleFilePathError::UncPrefix);
        }
        if text.starts_with('/') || text.starts_with('\\') {
            return Err(ModuleFilePathError::Absolute);
        }
        if is_drive_qualified(text) {
            return Err(ModuleFilePathError::DriveQualified);
        }

        let mut has_component = false;
        for component in text.split(['/', '\\']) {
            if component == ".." {
                return Err(ModuleFilePathError::Traversal);
            }
            if !component.is_empty() && component != "." {
                has_component = true;
            }
        }
        if !has_component {
            return Err(ModuleFilePathError::Empty);
        }

        Ok(Self { literal: text.to_string() })
    }

    /// The literal request text exactly as written in source.
    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }

    /// The literal request with `\` separators rewritten to `/`.
    ///
    /// Separator normalization is a spelling convenience only. It does not prove
    /// containment, admission, or filesystem authority.
    #[must_use]
    pub fn with_forward_separators(&self) -> String {
        self.literal.replace('\\', "/")
    }
}

impl fmt::Display for ModuleFilePath {
    /// Renders the literal relative file request.
    ///
    /// This is a filename, never a logical module identity.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.literal)
    }
}

impl TryFrom<&str> for ModuleFilePath {
    type Error = ModuleFilePathError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

/// Detect an operand still wrapped in a matching pair of source quote delimiters.
///
/// Deliberately narrow: only a value that both opens and closes with the same
/// `'` or `"` delimiter is treated as undecoded. A filename that merely contains
/// a quote (`it's.pl`, `a"b.pm`) is a legitimate POSIX filename and stays valid.
fn undecoded_quote_delimiter(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if first != '\'' && first != '"' {
        return None;
    }
    let last = chars.next_back()?;
    (last == first).then_some(first)
}

/// Detect drive-qualified syntax such as `C:`, `C:foo`, or `C:\foo`.
///
/// A leading `X::` is a package separator, not a drive, so literal filenames
/// like `C::Bar` stay valid.
fn is_drive_qualified(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !first.is_ascii_alphabetic() {
        return false;
    }
    if chars.next() != Some(':') {
        return false;
    }

    chars.next() != Some(':')
}

#[cfg(test)]
mod tests {
    use super::{ModuleFilePath, ModuleFilePathError};

    #[test]
    fn relative_pm_path_is_validated() -> Result<(), ModuleFilePathError> {
        let path = ModuleFilePath::parse("Foo/Bar.pm")?;
        assert_eq!(path.literal(), "Foo/Bar.pm");
        Ok(())
    }

    #[test]
    fn literal_colon_name_stays_a_filename() -> Result<(), ModuleFilePathError> {
        let path = ModuleFilePath::parse("Foo::Bar")?;
        assert_eq!(
            path.literal(),
            "Foo::Bar",
            "a quoted require operand is a filename; `::` must not become `/`"
        );
        assert_eq!(path.with_forward_separators(), "Foo::Bar");
        Ok(())
    }

    #[test]
    fn backslash_separators_normalize_for_display_only() -> Result<(), ModuleFilePathError> {
        let path = ModuleFilePath::parse("Foo\\Bar.pm")?;
        assert_eq!(path.literal(), "Foo\\Bar.pm", "the literal spelling is preserved");
        assert_eq!(path.with_forward_separators(), "Foo/Bar.pm");
        Ok(())
    }

    #[test]
    fn rejections_are_classified_not_collapsed() {
        let cases = [
            ("", ModuleFilePathError::Empty),
            ("./", ModuleFilePathError::Empty),
            ("Foo\0.pm", ModuleFilePathError::InteriorNul),
            ("Foo\n.pm", ModuleFilePathError::ControlCharacter { character: '\n' }),
            ("/etc/passwd", ModuleFilePathError::Absolute),
            ("\\etc\\passwd", ModuleFilePathError::Absolute),
            ("\\\\server\\share\\Foo.pm", ModuleFilePathError::UncPrefix),
            ("//server/share/Foo.pm", ModuleFilePathError::UncPrefix),
            ("C:/Foo.pm", ModuleFilePathError::DriveQualified),
            ("C:Foo.pm", ModuleFilePathError::DriveQualified),
            ("../../etc/passwd", ModuleFilePathError::Traversal),
            ("Foo/../../etc/passwd", ModuleFilePathError::Traversal),
            ("Foo\\..\\..\\etc", ModuleFilePathError::Traversal),
            ("'Foo.pm'", ModuleFilePathError::UndecodedQuoting { delimiter: '\'' }),
        ];

        for (input, expected) in cases {
            assert_eq!(
                ModuleFilePath::parse(input),
                Err(expected),
                "`{input}` must carry its own classification"
            );
        }
    }

    #[test]
    fn an_undecoded_quoted_token_is_refused() {
        // The HIR require target can still carry its delimiters — `hir/model.rs`
        // strips them in `normalize_module_target` for exactly this reason. If the
        // caller forgets, the resolver would search for `'Foo/Bar.pm'`.
        for (input, delimiter) in
            [("'Foo/Bar.pm'", '\''), ("\"Foo::Bar\"", '"'), ("''", '\''), ("\"\"", '"')]
        {
            assert_eq!(
                ModuleFilePath::parse(input),
                Err(ModuleFilePathError::UndecodedQuoting { delimiter }),
                "`{input}` is a raw source token, not a decoded operand"
            );
        }
    }

    #[test]
    fn a_quote_inside_a_filename_stays_valid() -> Result<(), ModuleFilePathError> {
        // Only a matching wrapping pair is undecoded quoting; these are legitimate
        // POSIX filenames and must not be caught by the guard.
        for input in ["it's.pl", "a\"b.pm", "'", "\"", "don't/stop.pm", "a'b\"c.pm"] {
            let path = ModuleFilePath::parse(input)?;
            assert_eq!(path.literal(), input);
        }
        Ok(())
    }

    #[test]
    fn curdir_component_is_not_traversal() -> Result<(), ModuleFilePathError> {
        let path = ModuleFilePath::parse("./Foo/Bar.pm")?;
        assert_eq!(path.literal(), "./Foo/Bar.pm");
        Ok(())
    }

    #[test]
    fn every_rejection_has_a_stable_boundary_id() {
        for input in ["", "Foo\0", "/abs", "\\\\unc", "C:x", "../x", "'q'"] {
            let boundary_id = ModuleFilePath::parse(input).err().map(|error| error.boundary_id());
            assert!(
                boundary_id.is_some_and(|id| id.starts_with("module_file_path.")),
                "`{input}` must be rejected with a namespaced boundary id, got {boundary_id:?}"
            );
        }
    }
}
