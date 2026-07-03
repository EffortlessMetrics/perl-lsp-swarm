//! Repo-relative paths.
//!
//! Every file fact is keyed by a [`RepoRelativePath`] — a normalised,
//! forward-slash, repo-relative path. Host absolute paths must never enter the
//! substrate: they leak machine-specific state into facts, break determinism
//! across machines, and are a privacy hazard in shared artifacts. The
//! constructor rejects absolute paths and parent-directory traversal, and
//! normalises separators so the same logical file always yields the same key.

use serde::{Deserialize, Serialize};

/// Reasons a path cannot be admitted as a [`RepoRelativePath`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// The path was empty or contained only separators / `.` components.
    #[error("path is empty after normalisation")]
    Empty,
    /// The path is absolute (POSIX `/...`, Windows `C:\...`, or UNC `\\...`).
    #[error("absolute paths are not allowed in workspace facts: {0:?}")]
    Absolute(String),
    /// The path escaped the repo root via a `..` component.
    #[error("parent-directory traversal is not allowed: {0:?}")]
    Traversal(String),
}

/// A normalised, forward-slash, repo-relative path.
///
/// Invariants (established at construction, relied on everywhere else):
/// - never absolute (no leading `/`, no `C:\`, no `\\`);
/// - no `..` components (no traversal outside the repo root);
/// - `\` normalised to `/`; redundant `.` and empty components collapsed;
/// - non-empty.
///
/// Deserialization is routed through [`RepoRelativePath::new`] (via
/// `#[serde(try_from)]`) so the invariants hold on *every* construction path —
/// a serialized absolute or traversal path is rejected, not silently admitted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoRelativePath(String);

impl TryFrom<String> for RepoRelativePath {
    type Error = PathError;
    fn try_from(raw: String) -> Result<Self, PathError> {
        Self::new(&raw)
    }
}

impl From<RepoRelativePath> for String {
    fn from(path: RepoRelativePath) -> String {
        path.0
    }
}

impl RepoRelativePath {
    /// Normalise and validate `raw` into a repo-relative path.
    ///
    /// # Errors
    /// Returns [`PathError`] if `raw` is absolute, contains `..` traversal, or
    /// is empty after normalisation.
    pub fn new(raw: &str) -> Result<Self, PathError> {
        let unified = raw.replace('\\', "/");

        // Reject absolutes on the NORMALISED shape, so a path that is absolute
        // only after `\`->`/` conversion (`\foo`, UNC `\\srv\share`) is caught
        // rather than silently coerced to a relative key.
        if unified.starts_with('/') || is_windows_drive_absolute(raw) {
            return Err(PathError::Absolute(raw.to_string()));
        }

        let mut components: Vec<&str> = Vec::new();
        for component in unified.split('/') {
            match component {
                "" | "." => continue,
                ".." => return Err(PathError::Traversal(raw.to_string())),
                other => components.push(other),
            }
        }

        if components.is_empty() {
            return Err(PathError::Empty);
        }

        Ok(Self(components.join("/")))
    }

    /// The normalised path string (forward-slash, repo-relative).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The final path component (file name), or the whole path if it has no
    /// separator.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The lowercase file extension without the dot, if any. Case is folded so
    /// `.PL` and `.pl` are distinguishable by callers that care via
    /// [`RepoRelativePath::file_name`], while extension matching stays simple.
    #[must_use]
    pub fn extension(&self) -> Option<String> {
        let name = self.file_name();
        let dot = name.rfind('.')?;
        // A leading dot (dotfile) is not an extension.
        if dot == 0 {
            return None;
        }
        Some(name[dot + 1..].to_ascii_lowercase())
    }

    /// Path components, top-down.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl core::fmt::Display for RepoRelativePath {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Detect a Windows drive-qualified prefix (`C:`, `C:\`, `C:/`, or the
/// drive-relative `C:foo`). Any `<letter>:` two-byte prefix is non-portable and
/// must not become a repo-relative key — `C:foo` is relative to the current
/// directory *on drive C*, which still leaks host drive state.
fn is_windows_drive_absolute(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn accepts_and_normalises_relative() {
        let p = RepoRelativePath::new("lib/Foo/Bar.pm").expect("valid");
        assert_eq!(p.as_str(), "lib/Foo/Bar.pm");
        assert_eq!(p.file_name(), "Bar.pm");
        assert_eq!(p.extension().as_deref(), Some("pm"));
    }

    #[test]
    fn normalises_backslashes_and_dot_components() {
        let p = RepoRelativePath::new(".\\lib\\.\\Foo.pm").expect("valid");
        assert_eq!(p.as_str(), "lib/Foo.pm");
    }

    #[test]
    fn collapses_redundant_slashes() {
        let p = RepoRelativePath::new("lib//Foo///Bar.pm").expect("valid");
        assert_eq!(p.as_str(), "lib/Foo/Bar.pm");
    }

    #[test]
    fn rejects_posix_absolute() {
        assert_eq!(
            RepoRelativePath::new("/home/user/lib/Foo.pm"),
            Err(PathError::Absolute("/home/user/lib/Foo.pm".to_string()))
        );
    }

    #[test]
    fn rejects_windows_drive_absolute() {
        assert!(matches!(
            RepoRelativePath::new("C:\\Users\\me\\Foo.pm"),
            Err(PathError::Absolute(_))
        ));
        assert!(matches!(RepoRelativePath::new("C:/Users/me/Foo.pm"), Err(PathError::Absolute(_))));
    }

    #[test]
    fn rejects_unc_absolute() {
        assert!(matches!(
            RepoRelativePath::new("\\\\server\\share\\Foo.pm"),
            Err(PathError::Absolute(_))
        ));
    }

    #[test]
    fn rejects_backslash_root_absolute() {
        // Absolute only after `\`->`/` normalisation; must not become `foo`.
        assert!(matches!(RepoRelativePath::new("\\foo"), Err(PathError::Absolute(_))));
        assert!(matches!(RepoRelativePath::new("\\lib\\Foo.pm"), Err(PathError::Absolute(_))));
    }

    #[test]
    fn rejects_drive_relative_without_separator() {
        // `C:foo` is drive-qualified (relative to CWD on drive C) — non-portable
        // and leaks host drive state.
        assert!(matches!(RepoRelativePath::new("C:foo"), Err(PathError::Absolute(_))));
        assert!(matches!(RepoRelativePath::new("C:"), Err(PathError::Absolute(_))));
        assert!(matches!(RepoRelativePath::new("z:lib/Foo.pm"), Err(PathError::Absolute(_))));
    }

    #[test]
    fn deserialize_is_validated() {
        // Deserialization must route through `new` — no bypass of the invariants.
        assert!(serde_json::from_str::<RepoRelativePath>("\"/etc/passwd\"").is_err());
        assert!(serde_json::from_str::<RepoRelativePath>("\"../../secret\"").is_err());
        assert!(serde_json::from_str::<RepoRelativePath>("\"C:foo\"").is_err());
        let ok: RepoRelativePath =
            serde_json::from_str("\"lib/Foo.pm\"").expect("valid relative path");
        assert_eq!(ok.as_str(), "lib/Foo.pm");
    }

    #[test]
    fn serialize_roundtrips_through_validation() {
        let p = RepoRelativePath::new("lib/Foo/Bar.pm").expect("valid");
        let json = serde_json::to_string(&p).expect("serialize");
        assert_eq!(json, "\"lib/Foo/Bar.pm\"");
        let back: RepoRelativePath = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }

    #[test]
    fn rejects_traversal() {
        assert!(matches!(
            RepoRelativePath::new("lib/../../etc/passwd"),
            Err(PathError::Traversal(_))
        ));
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(RepoRelativePath::new(""), Err(PathError::Empty));
        assert_eq!(RepoRelativePath::new("./."), Err(PathError::Empty));
    }

    #[test]
    fn dotfile_has_no_extension() {
        let p = RepoRelativePath::new(".gitignore").expect("valid");
        assert_eq!(p.extension(), None);
        assert_eq!(p.file_name(), ".gitignore");
    }
}
