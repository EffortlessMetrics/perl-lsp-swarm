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
    /// A token given to [`ModuleFilePath::from_quoted_token`] carried no
    /// matching pair of `'` or `"` delimiters to strip.
    UnquotedToken,
    /// A token's content needs escape or interpolation decoding that this crate
    /// does not perform, so stripping delimiters would not yield the filename
    /// Perl actually looks up.
    UndecodableToken,
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
            Self::UnquotedToken => {
                f.write_str("source token is not wrapped in a matching pair of quote delimiters")
            }
            Self::UndecodableToken => {
                f.write_str("source token contains escape or interpolation syntax; decode it first")
            }
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
            Self::UnquotedToken => "module_file_path.unquoted_token",
            Self::UndecodableToken => "module_file_path.undecodable_token",
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
/// # Decoded value versus raw token
///
/// Whether a string is a decoded operand or a still-quoted source token is not
/// recoverable from the string itself: Perl permits quote characters in a
/// filename, so `'Foo.pm'` is both a plausible raw token *and* a legitimate
/// decoded filename. Guessing would reject valid requests.
///
/// The caller therefore states which it holds, by choosing a constructor:
///
/// - [`Self::parse`] takes the **decoded** operand — the value Perl looks up.
/// - [`Self::from_quoted_token`] takes the **raw** source token, delimiters
///   included, and strips exactly one matching outer pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleFilePath {
    literal: String,
}

impl ModuleFilePath {
    /// Validate an already-decoded literal relative file request.
    ///
    /// `text` is the value Perl looks up, so quote characters in it are filename
    /// bytes and are preserved. Pass a still-quoted source token to
    /// [`Self::from_quoted_token`] instead.
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

    /// Decode a raw source token and validate the operand inside it.
    ///
    /// Strips exactly one matching outer pair of `\'` or `"` delimiters, then
    /// applies [`Self::parse`] to the remainder. Use this for a token that still
    /// carries its delimiters — a `perl_parser_core::hir` require target keeps
    /// the string node's value verbatim, and `hir::model::normalize_module_target`
    /// exists to strip exactly this.
    ///
    /// # Only when stripping *is* the whole decode
    ///
    /// Removing delimiters yields the runtime filename only when the content
    /// carries no Perl quoting syntax. This constructor therefore fails closed
    /// on anything it cannot decode faithfully:
    ///
    /// - a single-quoted token containing a `\\\\` or `\\'` escape sequence;
    /// - a double-quoted token containing `\\`, `$`, or `@` (escapes and
    ///   interpolation).
    ///
    /// Single quotes escape only those two sequences, so a lone backslash there
    /// is literal and `'Foo\\Bar.pm'` decodes to `Foo\\Bar.pm` by stripping alone —
    /// verified against `perl`, which prints `Foo\\Bar.pm` for `'Foo\\Bar.pm'` and
    /// `a\\b` for `'a\\\\b'`.
    ///
    /// An interpolated operand is not a literal filename at all. Its producer
    /// holds the AST and should classify it with
    /// [`ModuleRequest::partially_static`] or [`ModuleRequest::dynamic`], which
    /// carry the recovered evidence — this constructor will not invent it.
    ///
    /// The double-quote rule is deliberately coarse: a token containing `\`,
    /// `$`, or `@` is refused even when the sequence would not actually
    /// interpolate. `perl -we 'print "Foo@.pm\n"'` prints `Foo@.pm`, so
    /// `"Foo@.pm"` is a token this constructor could in principle have decoded.
    /// Separating those cases means implementing Perl's interpolation grammar
    /// (`@name`, `@{...}`, `@$ref`, `@{[ ... ]}`, `@Foo::bar`, and the follower
    /// rules that make `@.` literal), which `perl-parser-core` owns rather than
    /// this crate. A partial second copy of that grammar would stop failing
    /// closed and start minting filenames Perl never opens, so the coarse rule
    /// refuses instead of guessing.
    ///
    /// Nothing becomes unreachable: the refusal applies only to the raw-token
    /// shortcut. `@` is an ordinary filename byte, so a caller that decodes with
    /// the lexer can always hand the operand to [`ModuleFilePath::parse`], which
    /// places no restriction on it.
    ///
    /// [`ModuleRequest::partially_static`]: super::ModuleRequest::partially_static
    /// [`ModuleRequest::dynamic`]: super::ModuleRequest::dynamic
    ///
    /// # Errors
    ///
    /// [`ModuleFilePathError::UnquotedToken`] when `token` is not wrapped in a
    /// matching delimiter pair, [`ModuleFilePathError::UndecodableToken`] when
    /// its content needs real decoding, otherwise the classified error for the
    /// decoded operand.
    pub fn from_quoted_token(token: &str) -> Result<Self, ModuleFilePathError> {
        let mut chars = token.chars();
        let Some(delimiter) = chars.next().filter(|first| *first == '\'' || *first == '"') else {
            return Err(ModuleFilePathError::UnquotedToken);
        };
        if chars.next_back() != Some(delimiter) {
            return Err(ModuleFilePathError::UnquotedToken);
        }

        let inner = &token[delimiter.len_utf8()..token.len() - delimiter.len_utf8()];
        let needs_decoding = if delimiter == '\'' {
            single_quoted_needs_decoding(inner)
        } else {
            inner.contains(['\\', '$', '@'])
        };
        if needs_decoding {
            return Err(ModuleFilePathError::UndecodableToken);
        }

        Self::parse(inner)
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

/// Whether a single-quoted token's body is anything other than its own literal value.
///
/// Single quotes recognize only `\\` and `\'`; every other backslash is literal.
/// A byte scan is exact here because both delimiters are ASCII and a UTF-8
/// continuation byte can never be mistaken for one. A trailing lone backslash is
/// also refused, because it can only arise from a truncated token.
fn single_quoted_needs_decoding(inner: &str) -> bool {
    // A body ending in a lone backslash cannot come from a well-formed token:
    // `'Foo\'` does not terminate in Perl ("Can't find string terminator"),
    // because `\'` escapes the quote. Such input is truncated, so refuse it
    // rather than invent an exact filename from it.
    if inner.ends_with('\\') {
        return true;
    }

    inner
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'\\' && (pair[1] == b'\\' || pair[1] == b'\''))
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
    fn a_raw_token_is_decoded_by_its_own_constructor() -> Result<(), ModuleFilePathError> {
        // The HIR require target keeps the string node's value verbatim, which is
        // why `hir::model::normalize_module_target` strips delimiters. This is the
        // typed way to hand such a token over.
        for (token, decoded) in [
            ("'Foo/Bar.pm'", "Foo/Bar.pm"),
            ("\"Foo/Bar.pm\"", "Foo/Bar.pm"),
            ("'Foo::Bar'", "Foo::Bar"),
        ] {
            let path = ModuleFilePath::from_quoted_token(token)?;
            assert_eq!(path.literal(), decoded, "`{token}` must decode to `{decoded}`");
        }
        Ok(())
    }

    #[test]
    fn a_quoted_filename_is_a_valid_decoded_operand() -> Result<(), ModuleFilePathError> {
        // Perl permits quote characters in a filename, so a decoded operand that
        // happens to be wrapped in them is legitimate and must not be second-guessed.
        // `parse` never infers encoded state from the value.
        for input in ["'Foo.pm'", "\"Foo.pm\"", "it's.pl", "a\"b.pm", "don't/stop.pm", "'", "\""] {
            let path = ModuleFilePath::parse(input)?;
            assert_eq!(path.literal(), input, "`{input}` is a decoded filename, not a token");
        }
        Ok(())
    }

    #[test]
    fn a_token_needing_real_decoding_is_refused() {
        // Stripping delimiters is the whole decode only when the content carries
        // no Perl quoting syntax. Anything else would hand back a filename Perl
        // never looks up, marked as exact.
        for token in [
            "'Foo\\\\Bar.pm'",  // single-quoted escape: Perl yields `Foo\Bar.pm`
            "'Foo\\'Bar.pm'",   // escaped delimiter
            "\"Foo\\tBar.pm\"", // double-quoted escape
            "\"$class.pm\"",    // scalar interpolation
            "\"@list.pm\"",     // array interpolation
            "\"Foo${leaf}.pm\"",
        ] {
            assert_eq!(
                ModuleFilePath::from_quoted_token(token),
                Err(ModuleFilePathError::UndecodableToken),
                "`{token}` needs decoding this crate does not perform"
            );
        }
    }

    #[test]
    fn a_truncated_single_quoted_token_is_refused() {
        // A body ending in a lone backslash cannot come from well-formed source:
        // `perl` rejects `'Foo\'` with "Can't find string terminator", because
        // `\'` escapes the quote. Accepting it would mint an exact filename out
        // of a token the caller truncated.
        for token in ["'Foo\\'", "'Foo\\\\Bar\\'", "'\\'"] {
            assert_eq!(
                ModuleFilePath::from_quoted_token(token),
                Err(ModuleFilePathError::UndecodableToken),
                "`{token}` is truncated, not an exact filename"
            );
        }
    }

    #[test]
    fn a_literal_backslash_in_single_quotes_decodes_by_stripping() -> Result<(), ModuleFilePathError>
    {
        // Perl leaves a lone backslash literal inside single quotes, so these are
        // valid relative filenames rather than tokens needing a decode.
        for (token, decoded) in [
            ("'Foo\\Bar.pm'", "Foo\\Bar.pm"),
            ("'a\\b'", "a\\b"),
            ("'lib\\Foo\\Bar.pm'", "lib\\Foo\\Bar.pm"),
        ] {
            let path = ModuleFilePath::from_quoted_token(token)?;
            assert_eq!(path.literal(), decoded, "`{token}` needs no decoding");
        }
        Ok(())
    }

    #[test]
    fn a_single_quoted_sigil_is_not_interpolation() -> Result<(), ModuleFilePathError> {
        // `$` and `@` are literal inside single quotes, so these decode by
        // stripping alone and must not be refused.
        for (token, decoded) in [("'$literal.pm'", "$literal.pm"), ("'@literal.pm'", "@literal.pm")]
        {
            let path = ModuleFilePath::from_quoted_token(token)?;
            assert_eq!(path.literal(), decoded);
        }
        Ok(())
    }

    #[test]
    fn an_unquoted_token_is_refused_by_the_token_constructor() {
        for token in ["Foo.pm", "'Foo", "Foo'", "\"Foo", "'Foo\"", "", "'"] {
            assert_eq!(
                ModuleFilePath::from_quoted_token(token),
                Err(ModuleFilePathError::UnquotedToken),
                "`{token}` carries no matching delimiter pair to strip"
            );
        }
    }

    #[test]
    fn a_decoded_token_still_faces_every_validation_rule() {
        assert_eq!(
            ModuleFilePath::from_quoted_token("'../../etc/passwd'"),
            Err(ModuleFilePathError::Traversal),
            "stripping delimiters must not skip traversal validation"
        );
        assert_eq!(
            ModuleFilePath::from_quoted_token("'/etc/passwd'"),
            Err(ModuleFilePathError::Absolute)
        );
        assert_eq!(ModuleFilePath::from_quoted_token("''"), Err(ModuleFilePathError::Empty));
    }

    #[test]
    fn curdir_component_is_not_traversal() -> Result<(), ModuleFilePathError> {
        let path = ModuleFilePath::parse("./Foo/Bar.pm")?;
        assert_eq!(path.literal(), "./Foo/Bar.pm");
        Ok(())
    }

    #[test]
    fn every_rejection_has_a_stable_boundary_id() {
        for input in ["", "Foo\0", "/abs", "\\\\unc", "C:x", "../x"] {
            let boundary_id = ModuleFilePath::parse(input).err().map(|error| error.boundary_id());
            assert!(
                boundary_id.is_some_and(|id| id.starts_with("module_file_path.")),
                "`{input}` must be rejected with a namespaced boundary id, got {boundary_id:?}"
            );
        }
    }
}
