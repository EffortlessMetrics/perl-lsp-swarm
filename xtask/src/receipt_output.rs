//! Shared safety and durability rules for receipt files.
//!
//! A receipt writer must never overwrite the evidence it was asked to classify.
//! This module owns that rule once — path aliasing through symlinks and hard
//! links, and the atomic publish — so each receipt emitter states its subject
//! and inherits identical behavior instead of restating the guard.
//!
//! Lifted from `crate::publication_drift`, which remains a caller. The logic
//! and its resulting diagnostics are identical for that call site; the
//! signatures gained a `subject` label and a protected-source slice so a
//! second emitter could share them.

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tempfile::NamedTempFile;

/// Create the receipt's parent directory when it does not exist yet.
pub fn prepare_output_parent(subject: &str, path: &Path) -> Result<()> {
    let parent = parent_or_current(path);
    fs::create_dir_all(parent)
        .wrap_err_with(|| format!("creating {subject} output {}", parent.display()))
}

/// Refuse an output path that would destroy one of its own evidence inputs.
///
/// Rejects an output that resolves to a protected source, a symlink or
/// non-regular file, or a hard-link alias of a protected source. Sources that
/// do not exist cannot be aliased at this instant and are skipped; every other
/// inspection failure propagates and blocks the write.
pub fn ensure_safe_output(subject: &str, out: &Path, protected: &[&Path]) -> Result<()> {
    let output_identity = resolved_candidate_path(subject, out)?;

    for source in protected {
        // Raised in review: resolving a source canonicalizes its parent, so a
        // protected path under a directory that does not exist — `missing/
        // authority.json`, the shape `publication-drift` passes when no
        // authority was supplied — failed here and blocked the `not_proven`
        // receipt it was trying to write. An absent source cannot be aliased,
        // which is the rule this function already documents; it just was not
        // applied before resolution. Every other inspection error still blocks.
        match fs::symlink_metadata(source) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).wrap_err_with(|| {
                    format!("inspecting protected evidence source {}", source.display())
                });
            }
        }
        let source_identity = resolved_candidate_path(subject, source)?;
        if output_identity == source_identity {
            bail!(
                "{subject} output {} aliases protected evidence source {}",
                out.display(),
                source.display()
            );
        }
    }

    match fs::symlink_metadata(out) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                bail!(
                    "{subject} output {} must be a regular file and must not be a symlink",
                    out.display()
                );
            }
            for source in protected {
                // Deliberate proceed-with-evidence: a source that is absent right
                // now cannot be hard-link aliased at this instant, so only existing
                // sources need identity proof. Any other inspection error
                // propagates and blocks the run.
                match fs::symlink_metadata(source) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(error).wrap_err_with(|| {
                            format!("inspecting protected evidence source {}", source.display())
                        });
                    }
                }
                if same_file_identity(subject, out, source)? {
                    bail!(
                        "{subject} output {} is a hard-link alias of protected evidence source {}",
                        out.display(),
                        source.display()
                    );
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .wrap_err_with(|| format!("inspecting {subject} output {}", out.display()));
        }
    }
    Ok(())
}

/// Serialize and publish a receipt atomically, with a trailing newline.
pub fn write_receipt<T: Serialize>(subject: &str, path: &Path, receipt: &T) -> Result<()> {
    let parent = parent_or_current(path);
    let raw = serde_json::to_string_pretty(receipt)
        .wrap_err_with(|| format!("serializing {subject} receipt"))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .wrap_err_with(|| format!("creating atomic {subject} receipt in {}", parent.display()))?;
    temporary
        .write_all(format!("{raw}\n").as_bytes())
        .wrap_err_with(|| format!("writing temporary {subject} receipt for {}", path.display()))?;
    temporary
        .as_file_mut()
        .sync_all()
        .wrap_err_with(|| format!("syncing temporary {subject} receipt for {}", path.display()))?;
    temporary.persist(path).map_err(|error| {
        eyre!("atomically persisting {subject} receipt {}: {}", path.display(), error.error)
    })?;
    sync_directory(parent)?;
    Ok(())
}

/// Flush the rename itself, not just the bytes it points at.
///
/// `sync_all` on the temporary file durably stores its *contents*; the directory
/// entry created by the rename is a separate write. Without this, power loss can
/// leave a receipt whose data survived but whose name did not.
///
/// Raised in review: this was best-effort, which meant a caller was told the
/// receipt had been published durably when the durability step had failed. A
/// receipt is evidence, so reporting a success the filesystem did not confirm is
/// the one outcome worse than failing loudly. A failure here is therefore
/// propagated — but only where the platform actually offers the operation.
#[cfg(not(windows))]
fn sync_directory(parent: &Path) -> Result<()> {
    let handle = fs::File::open(parent)
        .wrap_err_with(|| format!("opening receipt output directory {}", parent.display()))?;
    handle
        .sync_all()
        .wrap_err_with(|| format!("syncing receipt output directory {}", parent.display()))?;
    Ok(())
}

/// Windows offers no directory flush to perform, so none is attempted.
///
/// This is the second correction to the same few lines, and the first one was
/// wrong. Making the sync fallible would have failed every receipt write on
/// Windows because `File::open` cannot return a directory handle; adding
/// `FILE_FLAG_BACKUP_SEMANTICS` fixed the open and left the failure one line
/// later, because `sync_all` issues `FlushFileBuffers`, which requires a handle
/// with write access — and a directory cannot be opened for writing. The net
/// effect was identical to the bug it claimed to fix.
///
/// Declining an operation the platform does not provide is not the silent
/// swallow that was removed here: nothing is attempted, so nothing is
/// discarded, and the difference is stated rather than hidden behind a
/// best-effort call. What backs durability on NTFS instead is metadata
/// journalling, which orders the rename against the already-`sync_all`ed file
/// contents. The guarantee is weaker than an explicit `fsync` and the platforms
/// genuinely differ here; that is a property of the platforms, not a claim this
/// module gets to smooth over.
#[cfg(windows)]
fn sync_directory(_parent: &Path) -> Result<()> {
    Ok(())
}

/// The directory a path lives in, reading a bare file name as the current one.
fn parent_or_current(path: &Path) -> &Path {
    path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."))
}

/// An absolute path suitable for comparison, whether or not the file exists.
///
/// An existing path is canonicalized outright. A receipt that has not been
/// written yet cannot be, so its parent is canonicalized and the file name
/// re-appended — otherwise the alias checks would have nothing to compare until
/// after the write they exist to prevent.
fn resolved_candidate_path(subject: &str, path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(eyre!("{subject} path must not be empty"));
    }
    if path.exists() {
        return fs::canonicalize(path)
            .wrap_err_with(|| format!("canonicalizing {subject} path {}", path.display()));
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| eyre!("{subject} path has no file name: {}", path.display()))?;
    let parent = parent_or_current(path);
    let canonical_parent = fs::canonicalize(parent)
        .wrap_err_with(|| format!("canonicalizing {subject} parent {}", parent.display()))?;
    normalize_lexically(subject, &canonical_parent.join(file_name))
}

/// Resolve `.` and `..` textually, without consulting the filesystem.
///
/// Only ever applied to a path whose parent is already canonical, so no symlink
/// can be hiding behind a `..` segment. A path that would climb above the
/// filesystem root is refused rather than silently clamped to it.
fn normalize_lexically(subject: &str, path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!("{subject} path escapes its filesystem root: {}", path.display());
                }
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    Ok(normalized)
}

/// Whether two paths name the same file, hard links included.
///
/// Canonical-path equality catches direct paths and symlinks but not a second
/// name for the same inode, which would let a receipt overwrite the very
/// observation it was classified from.
#[cfg(unix)]
fn same_file_identity(subject: &str, output: &Path, source: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;

    // Deliberate proceed-with-evidence: an absent source cannot be hard-link
    // aliased at this instant, so treating it as distinct is proven by direct
    // observation rather than assumed; every other metadata failure propagates
    // and blocks the run. The caller resolves absence before this comparison.
    let output = fs::metadata(output)
        .wrap_err_with(|| format!("reading {subject} output metadata {}", output.display()))?;
    let source = match fs::metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).wrap_err_with(|| {
                format!("reading protected evidence metadata {}", source.display())
            });
        }
    };
    Ok(output.dev() == source.dev() && output.ino() == source.ino())
}

/// Decide whether two existing paths denote the same underlying Windows file.
///
/// Fail closed: when either kernel identity cannot be established, the guard
/// refuses to classify the pair as distinct, because proceeding toward an
/// overwrite without identity proof could clobber protected evidence through an
/// unproven alias. Callers resolve absence first (a nonexistent source cannot
/// be aliased at that instant); this comparison only accepts proven answers.
#[cfg(windows)]
fn same_file_identity(subject: &str, output: &Path, source: &Path) -> Result<bool> {
    let output_identity = crate::file_identity::windows_file_identity(output)
        .wrap_err_with(|| format!("reading {subject} output identity {}", output.display()))?;
    let source_identity =
        crate::file_identity::windows_file_identity(source).wrap_err_with(|| {
            format!("reading protected evidence source identity {}", source.display())
        })?;
    let (Some(output_identity), Some(source_identity)) = (output_identity, source_identity) else {
        bail!(
            "{subject} cannot prove {} is distinct from protected evidence source {}; \
             Windows file identity is unavailable",
            output.display(),
            source.display()
        );
    };
    Ok(output_identity == source_identity)
}

/// Whether two paths name the same file, on a platform exposing no link identity.
///
/// The answer is the honest one available here: canonical-path equality has
/// already run in the caller, and nothing further can be proven, so this
/// reports no additional alias rather than guessing at one.
#[cfg(not(any(unix, windows)))]
fn same_file_identity(_subject: &str, _output: &Path, _source: &Path) -> Result<bool> {
    // Canonical path equality above covers direct paths and symlink aliases on every platform.
    // Stable hard-link identities are not exposed by the standard library on this platform.
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::ensure_safe_output;
    use color_eyre::eyre::{Result, bail};
    use std::fs;
    use tempfile::TempDir;

    const SUBJECT: &str = "publication drift";

    #[test]
    fn output_cannot_equal_the_observation() -> Result<()> {
        let temp = TempDir::new()?;
        let input = temp.path().join("observation.json");
        fs::write(&input, "{}")?;
        expect_rejection(&input, &[&input], "aliases protected evidence source")
    }

    #[cfg(windows)]
    #[test]
    fn dangling_protected_source_rejects_before_publication_write() -> Result<()> {
        use perl_tdd_support::try_create_file_symlink;

        // Typed skip when the Windows session lacks the symlink privilege
        // (os error 1314): the environment gap is not a product defect. With
        // the privilege present the test runs in full below.
        if perl_tdd_support::symlink_test_decision().skip_visibly() {
            return Ok(());
        }

        let temp = TempDir::new()?;
        let out = temp.path().join("receipt.json");
        let input = temp.path().join("observation.json");
        let dangling = temp.path().join("dangling-source.json");
        let missing_target = temp.path().join("missing-target.json");
        let original = b"existing receipt\n";
        fs::write(&out, original)?;
        fs::write(&input, "{}")?;
        if try_create_file_symlink(&missing_target, &dangling)?.is_none() {
            // Unprivileged Windows session: the typed skip is the honest
            // outcome; junction/copy fixtures cannot prove reparse rejection.
            return Ok(());
        }

        let error = ensure_safe_output(SUBJECT, &out, &[&input, &dangling])
            .expect_err("dangling protected source must fail closed");
        let message = format!("{error:#}");
        if !message.contains("protected evidence source")
            || !message.contains("dangling-source.json")
        {
            bail!("unexpected dangling-source error: {message}");
        }
        if fs::read(&out)? != original {
            bail!("publication output changed after rejecting dangling protected source");
        }

        Ok(())
    }

    #[test]
    fn absent_protected_source_does_not_block_a_regular_output() -> Result<()> {
        let temp = TempDir::new()?;
        let input = temp.path().join("observation.json");
        let authority = temp.path().join("authority.json");
        let out = temp.path().join("receipt.json");
        fs::write(&input, "{}")?;
        fs::write(&out, "{}")?;
        ensure_safe_output(SUBJECT, &out, &[&input, &authority])
    }

    #[test]
    fn a_protected_source_below_a_missing_directory_does_not_block_the_receipt() -> Result<()> {
        // Raised in review: resolving a protected source canonicalizes its
        // parent, so a path under a directory that does not exist errored here
        // rather than being skipped — and that is exactly the shape
        // `publication-drift` passes when no authority was supplied, so the run
        // could not write the `not_proven` receipt saying so. An absent source
        // cannot be aliased, which is the rule this function already documents.
        let temp = TempDir::new()?;
        let absent_authority = temp.path().join("missing/authority.json");
        let out = temp.path().join("receipt.json");

        ensure_safe_output(SUBJECT, &out, &[absent_authority.as_path()])?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn output_symlink_cannot_alias_the_observation() -> Result<()> {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new()?;
        let input = temp.path().join("observation.json");
        let out = temp.path().join("receipt.json");
        fs::write(&input, "{}")?;
        symlink(&input, &out)?;
        expect_rejection(&out, &[&input], "aliases protected evidence source")
    }

    #[cfg(unix)]
    #[test]
    fn output_hard_link_cannot_alias_the_authority() -> Result<()> {
        let temp = TempDir::new()?;
        let input = temp.path().join("observation.json");
        let authority = temp.path().join("authority.json");
        let out = temp.path().join("receipt.json");
        fs::write(&input, "{}")?;
        fs::write(&authority, "{}")?;
        fs::hard_link(&authority, &out)?;
        expect_rejection(&out, &[&input, &authority], "hard-link alias")
    }

    #[cfg(windows)]
    #[test]
    fn output_hard_link_cannot_alias_the_authority() -> Result<()> {
        let temp = TempDir::new()?;
        let input = temp.path().join("observation.json");
        let authority = temp.path().join("authority.json");
        let out = temp.path().join("receipt.json");
        fs::write(&input, "{}")?;
        fs::write(&authority, "{}")?;
        fs::hard_link(&authority, &out)?;
        expect_rejection(&out, &[&input, &authority], "hard-link alias")
    }

    fn expect_rejection(
        out: &std::path::Path,
        protected: &[&std::path::Path],
        expected: &str,
    ) -> Result<()> {
        let error = match ensure_safe_output(SUBJECT, out, protected) {
            Ok(()) => bail!("unsafe output alias should be rejected"),
            Err(error) => error,
        };
        if !format!("{error:#}").contains(expected) {
            bail!("unexpected output safety error: {error:#}");
        }
        Ok(())
    }
}
