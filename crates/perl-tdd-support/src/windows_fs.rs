//! Windows symlink capability helpers for tests that exercise symlink and
//! reparse-point semantics.
//!
//! Creating file symlinks on Windows requires `SeCreateSymbolicLinkPrivilege`,
//! which unprivileged sessions hold only when Developer Mode is enabled.
//! Tests whose subject is symlink *rejection* (reparse-point admission
//! guards, dangling-source handling) cannot honestly substitute a directory
//! junction or a file copy; when the privilege is missing the honest outcome
//! is a visible typed skip, not an environment-shaped red X or a weakened
//! fixture ([#12567]).
//!
//! The helpers in this module wrap [`std::os::windows::fs::symlink_file`] and
//! [`std::os::windows::fs::symlink_dir`], mapping exactly one error — os
//! error 1314 ("A required privilege is not held by the client") — to a
//! skipped result. Every other error is returned unchanged so real defects
//! keep failing loudly.
//!
//! # Conventions for branch authors
//!
//! ```ignore
//! #[cfg(windows)]
//! #[test]
//! fn rejects_reparse_points() -> Result<(), Box<dyn std::error::Error>> {
//!     use perl_tdd_support::try_create_file_symlink;
//!     // ...
//!     if try_create_file_symlink(&target, &link)?.is_none() {
//!         return Ok(()); // visible skip note already printed
//!     }
//!     // assertions against reparse-point behavior
//! }
//! ```
//!
//! Enabling Windows Developer Mode (or running elevated) opts the machine
//! out of these skips entirely; it is opt-in, never a requirement.

use std::io;
use std::path::Path;

const PRIVILEGE_NOT_HELD: i32 = 1314;

/// Reports whether the raw OS error is the Windows "privilege not held"
/// condition for symbolic-link creation.
#[cfg(windows)]
fn is_privilege_not_held(error: &io::Error) -> bool {
    error.raw_os_error() == Some(PRIVILEGE_NOT_HELD)
}

/// Creates a file symlink on Windows, or skips with a visible note.
///
/// Returns `Ok(Some(()))` when the link was created and `Ok(None)` when the
/// session lacks `SeCreateSymbolicLinkPrivilege` (os error 1314); callers
/// perform their typed skip. Any other error is a real failure.
///
/// On non-Windows targets this helper does not exist; Unix tests use
/// `std::os::unix::fs::symlink` under `#[cfg(unix)]`.
#[cfg(windows)]
pub fn try_create_file_symlink(original: &Path, link: &Path) -> io::Result<Option<()>> {
    match std::os::windows::fs::symlink_file(original, link) {
        Ok(()) => Ok(Some(())),
        Err(error) if is_privilege_not_held(&error) => {
            eprintln!(
                "skipping: Windows session lacks SeCreateSymbolicLinkPrivilege \
                 (os error {PRIVILEGE_NOT_HELD}); enable Developer Mode or run \
                 elevated to execute this test"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Creates a directory symlink on Windows, or skips with a visible note.
///
/// Behaves like [`try_create_file_symlink`] but for
/// [`std::os::windows::fs::symlink_dir`].
#[cfg(windows)]
pub fn try_create_dir_symlink(original: &Path, link: &Path) -> io::Result<Option<()>> {
    match std::os::windows::fs::symlink_dir(original, link) {
        Ok(()) => Ok(Some(())),
        Err(error) if is_privilege_not_held(&error) => {
            eprintln!(
                "skipping: Windows session lacks SeCreateSymbolicLinkPrivilege \
                 (os error {PRIVILEGE_NOT_HELD}); enable Developer Mode or run \
                 elevated to execute this test"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The helper either creates the link (privileged session) or performs a
    /// typed 1314 skip; both outcomes are acceptable. Any other OS error is a
    /// hard failure of this test.
    #[test]
    fn file_symlink_helper_creates_or_typed_skips() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        std::fs::write(&target, b"data")?;
        let created = try_create_file_symlink(&target, &link)?;
        if created.is_some() {
            let metadata = std::fs::symlink_metadata(&link)?;
            assert!(
                metadata.file_type().is_symlink(),
                "expected {} to be a symlink",
                link.display()
            );
        }
        Ok(())
    }

    /// Non-1314 errors stay real failures: creating into a nonexistent parent
    /// directory must return the system error, never a skip.
    #[test]
    fn other_errors_are_not_swallowed_as_skips() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        let target = temp.path().join("target.txt");
        let link = temp.path().join("missing-dir").join("link.txt");
        std::fs::write(&target, b"data")?;
        let error = try_create_file_symlink(&target, &link)
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected a real failure"))?;
        assert_ne!(
            error.raw_os_error(),
            Some(PRIVILEGE_NOT_HELD),
            "unexpected 1314; the probe path should fail with the system error instead"
        );
        Ok(())
    }
}
