//! Validated root-relative logical source paths.
//!
//! [`LogicalSourceId`](crate::LogicalSourceId) has always documented a hard
//! requirement on its path material: forward-slash separated, relative to the
//! workspace root, no leading separator, no `..` segments. Until this module
//! that requirement was prose only, so any caller could mint a durable-looking
//! identity from a host absolute path, a traversal sequence, or an empty
//! string, and two callers normalizing the same file differently both produced
//! valid-looking — and different — IDs.
//!
//! [`RootRelativeLogicalPath`] makes the requirement a type. Its only
//! constructor is fallible, so a value of this type *is* the proof that the
//! invariant holds, and [`LogicalSourceId::from_root_and_logical_path`] cannot
//! be handed anything else.
//!
//! # What this module deliberately does not do
//!
//! It validates; it never repairs. There is no separator folding, no traversal
//! resolution, no case folding, and no Unicode normalization — see
//! [`RootRelativeLogicalPath`] for the recorded policy and why each silent fix
//! would be the more dangerous choice.
//!
//! It also performs no I/O and consults no ambient state: no filesystem, no
//! symlink resolution, no current directory, no platform probing. Deciding
//! which physical path a logical source resolves to, and under whose authority,
//! belongs to the path-mechanics owners above this crate (#7621, #8185, #8198),
//! not here.
//!
//! [`LogicalSourceId::from_root_and_logical_path`]: crate::LogicalSourceId::from_root_and_logical_path

use std::fmt;

/// Why a candidate string is not a canonical root-relative logical path.
///
/// # Privacy
///
/// No variant carries the rejected path, and [`fmt::Display`] never echoes it.
/// Rejected material is exactly the material most likely to be a host absolute
/// path — the thing that must not reach logs, wire receipts, or error strings.
/// Callers that genuinely need the offending text already hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum LogicalPathError {
    /// The candidate was empty. A logical source always names something.
    Empty,
    /// The candidate began with `/`, so it is absolute rather than
    /// root-relative. Stripping the separator is not a valid repair: it turns a
    /// host path into a plausible-looking relative path and silently binds the
    /// host layout into durable identity.
    LeadingSeparator,
    /// The candidate contained a `\`.
    ///
    /// Rejected rather than folded to `/`, because at this layer the two
    /// readings — an unconverted Windows separator, or a literal backslash in a
    /// legal POSIX filename — are indistinguishable without platform authority
    /// this crate does not have. Folding would alias `we\ird.pm` onto the
    /// genuinely different `we/ird.pm`.
    BackslashSeparator,
    /// The candidate began with a Windows drive prefix such as `C:` or `c:/`.
    /// Drive-qualified paths are not root-relative and cannot be made so
    /// without the root authority that owns the mapping.
    ///
    /// Matched only at the start of the string, so a colon elsewhere stays an
    /// ordinary filename character. The known cost is that a POSIX file named
    /// `a:b` sitting directly in the root is refused; a drive-qualified path
    /// reaching a durable identity is the worse outcome, and such a file is
    /// still reachable at any depth (`lib/a:b`).
    WindowsDrivePrefix,
    /// The candidate contained a `.` segment. Dropping it is a normalization
    /// decision, and normalization here would mean two spellings of one file
    /// both validate while producing different identities.
    CurrentDirectorySegment,
    /// The candidate contained a `..` segment. Resolving it lexically can
    /// escape the root, and resolving it truthfully needs the filesystem.
    ParentDirectorySegment,
    /// The candidate contained an empty segment: a `//` run, or a trailing `/`.
    /// Collapsing these would make several spellings validate to one file while
    /// hashing differently.
    EmptySegment,
    /// The candidate contained a C0/C1 control character, including NUL.
    /// Such bytes are not representable in the identity-bearing forms this
    /// substrate feeds and frequently indicate a truncated or injected value.
    ControlCharacter,
}

impl fmt::Display for LogicalPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::Empty => "logical path is empty",
            Self::LeadingSeparator => {
                "logical path is absolute; a root-relative path must not begin with `/`"
            }
            Self::BackslashSeparator => {
                "logical path contains `\\`; supply forward-slash separated material"
            }
            Self::WindowsDrivePrefix => {
                "logical path begins with a Windows drive prefix and is not root-relative"
            }
            Self::CurrentDirectorySegment => "logical path contains a `.` segment",
            Self::ParentDirectorySegment => "logical path contains a `..` segment",
            Self::EmptySegment => {
                "logical path contains an empty segment (`//` run or trailing `/`)"
            }
            Self::ControlCharacter => "logical path contains a control character",
        };
        f.write_str(reason)
    }
}

impl std::error::Error for LogicalPathError {}

/// A path proven to satisfy [`LogicalSourceId`](crate::LogicalSourceId)'s
/// canonical root-relative contract.
///
/// Construct with [`RootRelativeLogicalPath::parse`]. Holding a value of this
/// type means every rule in [`LogicalPathError`] was checked and passed.
///
/// # Recorded policy: no folding of any kind
///
/// Validation never rewrites. In particular:
///
/// - **Case is preserved.** `lib/App.pm` and `lib/app.pm` are different logical
///   sources. Whether a given filesystem can hold both is a property of that
///   filesystem, not of this identity; modelling case-insensitive contexts
///   requires declared filesystem authority and is #7655's business.
/// - **Unicode is preserved byte for byte.** No NFC/NFD normalization is
///   applied, so the composed and decomposed spellings of a name are distinct
///   logical sources. Normalizing here would make identity depend on this
///   crate's Unicode tables rather than on what the producer actually named,
///   and would silently merge two files a case-sensitive filesystem keeps
///   apart.
///
/// Both choices are deliberate and testable. The alternative — quietly
/// canonicalizing — makes an identity that no longer corresponds to a file
/// anyone can name.
///
/// # Not checked here
///
/// A validated path is well-formed, not *true*. It is not known to exist, to be
/// contained by its root, or to be free of symlink escapes; percent-encoded
/// material (`My%20File.pm`) is well-formed and passes, so decoding remains the
/// caller's job. Those are physical-location questions owned above this crate.
///
/// [`LogicalPathError::ControlCharacter`] rejects the `Cc` category (C0, C1 and
/// DEL) and nothing more. Unicode *format* characters — a zero-width space, a
/// bidi override — are not control characters and are accepted, so a path can
/// still be spelled to look like a different path in a log or an editor. That is
/// a presentation concern for whatever renders the path, not an identity one:
/// such spellings are distinct sources here and are treated as such. Refusing
/// them would be a folding policy, which this type deliberately does not own.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RootRelativeLogicalPath(String);

impl RootRelativeLogicalPath {
    /// Validate `candidate` as a canonical root-relative logical path.
    ///
    /// # Errors
    ///
    /// Returns the first applicable [`LogicalPathError`]. Whole-string
    /// conditions are checked before per-segment ones, so an absolute path
    /// containing `..` reports [`LogicalPathError::LeadingSeparator`]: the
    /// caller's first problem is that it supplied a path from the wrong domain.
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_source_identity::{LogicalPathError, RootRelativeLogicalPath};
    ///
    /// let ok = RootRelativeLogicalPath::parse("lib/Widget.pm")?;
    /// assert_eq!(ok.as_str(), "lib/Widget.pm");
    ///
    /// assert_eq!(
    ///     RootRelativeLogicalPath::parse("/home/alice/proj/lib/Widget.pm"),
    ///     Err(LogicalPathError::LeadingSeparator),
    /// );
    /// assert_eq!(
    ///     RootRelativeLogicalPath::parse("lib/../secret.pm"),
    ///     Err(LogicalPathError::ParentDirectorySegment),
    /// );
    /// # Ok::<(), LogicalPathError>(())
    /// ```
    pub fn parse(candidate: &str) -> Result<Self, LogicalPathError> {
        if candidate.is_empty() {
            return Err(LogicalPathError::Empty);
        }
        if candidate.starts_with('/') {
            return Err(LogicalPathError::LeadingSeparator);
        }
        if candidate.contains('\\') {
            return Err(LogicalPathError::BackslashSeparator);
        }
        if candidate.chars().any(|c| c.is_control()) {
            return Err(LogicalPathError::ControlCharacter);
        }
        if has_windows_drive_prefix(candidate) {
            return Err(LogicalPathError::WindowsDrivePrefix);
        }

        for segment in candidate.split('/') {
            match segment {
                "" => return Err(LogicalPathError::EmptySegment),
                "." => return Err(LogicalPathError::CurrentDirectorySegment),
                ".." => return Err(LogicalPathError::ParentDirectorySegment),
                _ => {}
            }
        }

        Ok(Self(candidate.to_owned()))
    }

    /// The validated path, exactly as supplied.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Does `candidate` begin with a Windows drive qualifier (`C:`, `c:/`, …)?
///
/// Matched only at the start: a colon later in the string is an ordinary
/// character in a POSIX filename and is not this crate's business.
fn has_windows_drive_prefix(candidate: &str) -> bool {
    let mut chars = candidate.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    // ── Accepted canonical forms ──────────────────────────────────────────────

    #[test]
    fn accepts_ordinary_root_relative_paths() {
        for path in ["lib/Widget.pm", "script.pl", "t/unit/basic.t", "a/b/c/d/e.pm"] {
            let parsed = RootRelativeLogicalPath::parse(path);
            assert!(parsed.is_ok(), "{path:?} must validate, got {parsed:?}");
            let parsed = parsed.expect("asserted ok above");
            assert_eq!(parsed.as_str(), path, "validation must not rewrite the path");
        }
    }

    /// A dot inside a segment is an ordinary filename character; only a whole
    /// `.` segment is a normalization request.
    #[test]
    fn accepts_dots_inside_segments() {
        for path in ["lib/My.Module.pm", ".hidden/config.pm", "lib/..weird.pm", "a/b..c/d.pm"] {
            assert!(RootRelativeLogicalPath::parse(path).is_ok(), "{path:?} must validate");
        }
    }

    /// Percent-encoded and colon-bearing names are well-formed logical paths.
    /// Decoding is the caller's job; this type does not pretend to do it.
    #[test]
    fn accepts_well_formed_names_this_layer_does_not_interpret() {
        for path in ["lib/My%20File.pm", "lib/a:b.pm", "lib/space name.pm", "lib/naïve.pm"] {
            assert!(RootRelativeLogicalPath::parse(path).is_ok(), "{path:?} must validate");
        }
    }

    // ── Rejections: the invariant `from_root_and_path` only documented ────────

    #[test]
    fn rejects_empty() {
        assert_eq!(RootRelativeLogicalPath::parse(""), Err(LogicalPathError::Empty));
    }

    /// The exact shape the pull-diagnostics composer produced before #15555:
    /// a host absolute path with the leading slash stripped is *not* the fix,
    /// and a host absolute path itself must not validate.
    #[test]
    fn rejects_absolute_paths() {
        for path in ["/lib/Widget.pm", "/home/alice/proj/lib/App.pm", "/", "//server/share"] {
            assert_eq!(
                RootRelativeLogicalPath::parse(path),
                Err(LogicalPathError::LeadingSeparator),
                "{path:?} must be rejected as absolute"
            );
        }
    }

    #[test]
    fn rejects_parent_segments() {
        for path in ["../../etc/passwd", "lib/../secret.pm", "..", "a/b/.."] {
            assert_eq!(
                RootRelativeLogicalPath::parse(path),
                Err(LogicalPathError::ParentDirectorySegment),
                "{path:?} must be rejected for traversal"
            );
        }
    }

    #[test]
    fn rejects_current_directory_segments() {
        for path in [".", "./lib/App.pm", "lib/./App.pm", "lib/."] {
            assert_eq!(
                RootRelativeLogicalPath::parse(path),
                Err(LogicalPathError::CurrentDirectorySegment),
                "{path:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_empty_segments() {
        for path in ["lib//App.pm", "lib/App.pm/", "lib/", "a//b//c"] {
            assert_eq!(
                RootRelativeLogicalPath::parse(path),
                Err(LogicalPathError::EmptySegment),
                "{path:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_backslashes_rather_than_folding_them() {
        for path in ["lib\\App.pm", "we\\ird.pm", "a\\b/c"] {
            assert_eq!(
                RootRelativeLogicalPath::parse(path),
                Err(LogicalPathError::BackslashSeparator),
                "{path:?} must be rejected, not folded"
            );
        }
    }

    #[test]
    fn rejects_windows_drive_prefixes() {
        for path in ["C:/proj/lib/App.pm", "c:lib/App.pm", "Z:"] {
            assert_eq!(
                RootRelativeLogicalPath::parse(path),
                Err(LogicalPathError::WindowsDrivePrefix),
                "{path:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_control_characters_including_nul() {
        for path in ["lib/App\0.pm", "lib/\u{1}App.pm", "lib/App.pm\n", "lib/\u{7f}x.pm"] {
            assert_eq!(
                RootRelativeLogicalPath::parse(path),
                Err(LogicalPathError::ControlCharacter),
                "{path:?} must be rejected"
            );
        }
    }

    // ── Negative control: rejection is not incidental ─────────────────────────

    /// Guards against a validator that rejects everything. Each rejected input
    /// above has a near neighbour that must still pass, so the tests above
    /// cannot be satisfied by a blanket refusal.
    #[test]
    fn near_neighbours_of_rejected_inputs_still_validate() {
        for path in ["lib/App.pm", "a/b/c", "lib/..weird.pm", "lib/a:b.pm", "hidden/.config"] {
            assert!(
                RootRelativeLogicalPath::parse(path).is_ok(),
                "{path:?} must still validate — the validator must discriminate, not refuse"
            );
        }
    }

    // ── Declared policy: no folding ───────────────────────────────────────────

    #[test]
    fn case_is_preserved_and_distinguishing() {
        let lower = RootRelativeLogicalPath::parse("lib/app.pm").expect("valid");
        let upper = RootRelativeLogicalPath::parse("lib/App.pm").expect("valid");
        assert_ne!(lower, upper, "case must not be folded");
        assert_eq!(upper.as_str(), "lib/App.pm", "case must be preserved verbatim");
    }

    #[test]
    fn unicode_spellings_are_not_normalized() {
        // U+00E9 (composed) versus "e" + U+0301 (decomposed).
        let composed = RootRelativeLogicalPath::parse("lib/caf\u{e9}.pm").expect("valid");
        let decomposed = RootRelativeLogicalPath::parse("lib/cafe\u{301}.pm").expect("valid");
        assert_ne!(
            composed, decomposed,
            "NFC and NFD spellings must remain distinct logical paths"
        );
        assert_eq!(composed.as_str(), "lib/caf\u{e9}.pm", "bytes must be preserved");
    }

    // ── Every refusal is renderable, distinct, and path-free ──────────────────

    /// Exhaustive over the error enum: adding a variant without naming it here is
    /// a compile error, so no refusal can ship without a rendered message.
    fn variant_label(error: LogicalPathError) -> &'static str {
        match error {
            LogicalPathError::Empty => "Empty",
            LogicalPathError::LeadingSeparator => "LeadingSeparator",
            LogicalPathError::BackslashSeparator => "BackslashSeparator",
            LogicalPathError::WindowsDrivePrefix => "WindowsDrivePrefix",
            LogicalPathError::CurrentDirectorySegment => "CurrentDirectorySegment",
            LogicalPathError::ParentDirectorySegment => "ParentDirectorySegment",
            LogicalPathError::EmptySegment => "EmptySegment",
            LogicalPathError::ControlCharacter => "ControlCharacter",
        }
    }

    /// A typed refusal nobody can read is not much better than an untyped one.
    #[test]
    fn every_error_variant_renders_a_distinct_message() {
        let all = [
            LogicalPathError::Empty,
            LogicalPathError::LeadingSeparator,
            LogicalPathError::BackslashSeparator,
            LogicalPathError::WindowsDrivePrefix,
            LogicalPathError::CurrentDirectorySegment,
            LogicalPathError::ParentDirectorySegment,
            LogicalPathError::EmptySegment,
            LogicalPathError::ControlCharacter,
        ];

        let mut rendered = Vec::new();
        for error in all {
            let text = format!("{error}");
            assert!(!text.is_empty(), "{} must render a message", variant_label(error));
            assert!(
                text.contains("logical path"),
                "{} must name its subject, got {text:?}",
                variant_label(error)
            );
            rendered.push(text);
        }

        let mut unique = rendered.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            rendered.len(),
            "each refusal must be distinguishable from the others: {rendered:?}"
        );
    }

    // ── Privacy ───────────────────────────────────────────────────────────────

    /// Rejected material is exactly the material most likely to be a host path.
    /// Neither `Display` nor `Debug` on the error may carry it.
    #[test]
    fn error_text_never_echoes_the_rejected_path() {
        let secret = "/home/alice/private/credentials.pm";
        let error = RootRelativeLogicalPath::parse(secret).expect_err("must reject");
        let rendered = format!("{error} / {error:?}");
        assert!(!rendered.contains("alice"), "error must not leak host path: {rendered}");
        assert!(!rendered.contains("credentials"), "error must not leak file name: {rendered}");
        assert!(!rendered.contains(secret), "error must not leak the path: {rendered}");
    }

    // ── Precedence ────────────────────────────────────────────────────────────

    #[test]
    fn whole_string_conditions_are_reported_before_segment_conditions() {
        assert_eq!(
            RootRelativeLogicalPath::parse("/lib/../App.pm"),
            Err(LogicalPathError::LeadingSeparator),
            "the caller's first problem is supplying an absolute path"
        );
        assert_eq!(
            RootRelativeLogicalPath::parse("lib\\..\\App.pm"),
            Err(LogicalPathError::BackslashSeparator),
        );
    }
}
