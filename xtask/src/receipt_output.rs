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
    Ok(())
}

fn parent_or_current(path: &Path) -> &Path {
    path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."))
}

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
