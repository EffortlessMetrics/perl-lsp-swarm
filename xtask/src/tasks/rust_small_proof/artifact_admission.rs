//! Admission for one receipt destination in a governed checkout.
//!
//! Cooperating producers serialize on a stable sibling lock. This is not a
//! defense against another process changing filesystem names behind the owner.

use super::{RustSmallProofReceipt, staging_path, verify_receipt};
use color_eyre::eyre::{Result, WrapErr, bail};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub(super) struct Admission {
    path: PathBuf,
    lock: PathBuf,
}

impl Admission {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

pub(super) struct Artifact {
    admission: Admission,
    // Keep the OS handle live for the full proof and publication interval.
    file: Option<File>,
}

impl Artifact {
    pub(super) fn path(&self) -> &Path {
        self.admission.path()
    }
}

impl Drop for Artifact {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Err(error) = fs::remove_file(&self.admission.lock) {
            eprintln!(
                "[rust-small-proof] could not release owned receipt lock {}: {error}",
                self.admission.lock.display()
            );
        }
    }
}

pub(super) fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".lock");
    PathBuf::from(name)
}

/// Resolve the existing prefix without traversing links or silently cancelling
/// parent components. Missing directory suffixes are permitted but not created.
pub(super) fn canonical_destination(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("receipt path must be nonempty");
    }
    let absolute =
        if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    if !absolute.is_absolute() {
        bail!("receipt path must resolve absolutely; drive-relative paths are ambiguous");
    }
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        if component == Component::CurDir {
            continue;
        }
        if matches!(component, Component::Prefix(_)) {
            resolved.push(component.as_os_str());
            continue;
        }
        if component == Component::ParentDir {
            if !fs::symlink_metadata(&resolved).is_ok_and(|metadata| metadata.is_dir()) {
                bail!(
                    "parent traversal requires an existing unambiguous directory: {}",
                    resolved.display()
                );
            }
            resolved = fs::canonicalize(resolved.join(".."))?;
            continue;
        }
        resolved.push(component.as_os_str());
        match fs::symlink_metadata(&resolved) {
            Ok(metadata) => {
                if is_link(&metadata) {
                    bail!(
                        "receipt paths must not traverse links or reparse points: {}",
                        resolved.display()
                    );
                }
                resolved = fs::canonicalize(&resolved)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .wrap_err_with(|| format!("inspecting receipt path {}", resolved.display()));
            }
        }
    }
    if !resolved.is_absolute() {
        bail!("receipt path did not resolve to an absolute destination");
    }
    Ok(resolved)
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        bail!("receipt admission Git query failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(output.stdout)
}

fn reject_tracked(path: &Path, root: &Path) -> Result<()> {
    let Ok(relative) = path.strip_prefix(root) else {
        return Ok(());
    };
    let relative = relative
        .to_str()
        .ok_or_else(|| color_eyre::eyre::eyre!("receipt path is not UTF-8"))?
        .replace('\\', "/");
    let literal = format!(":(top,literal){relative}");
    for args in [
        vec!["ls-files", "--cached", "-z", "--", literal.as_str()],
        vec!["ls-tree", "-r", "--name-only", "-z", "HEAD", "--", literal.as_str()],
    ] {
        if !git_output(root, &args)?.is_empty() {
            bail!("receipt artifact is tracked in HEAD or the index: {}", path.display());
        }
    }
    Ok(())
}

fn require_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => bail!(
            "receipt sidecar already exists; refusing uncertain ownership: {}",
            path.display()
        ),
    }
}

fn validate(path: &Path, owned_lock: bool, repository: &Path) -> Result<Admission> {
    let path = canonical_destination(path)?;
    let lock = lock_path(&path);
    let stage = staging_path(&path);
    let root_bytes = git_output(repository, &["rev-parse", "--show-toplevel"])?;
    let root = fs::canonicalize(String::from_utf8(root_bytes)?.trim())?;
    for artifact in [&path, &lock, &stage] {
        canonical_destination(artifact)?;
        reject_tracked(artifact, &root)?;
    }
    if !owned_lock {
        require_absent(&lock)?;
    }
    require_absent(&stage)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                bail!("receipt destination is not a regular file: {}", path.display());
            }
            let prior: RustSmallProofReceipt = serde_json::from_slice(&fs::read(&path)?).wrap_err(
                "existing receipt destination is not a recognized receipt; preserving it",
            )?;
            verify_receipt(&prior, None)
                .wrap_err("existing destination is not a coherent prior receipt; preserving it")?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(Admission { path, lock })
}

pub(super) fn preflight(path: &Path) -> Result<Admission> {
    validate(path, false, &std::env::current_dir()?)
}

pub(super) fn acquire(path: &Path) -> Result<Artifact> {
    acquire_in(path, &std::env::current_dir()?)
}

fn acquire_in(path: &Path, repository: &Path) -> Result<Artifact> {
    let admission = validate(path, false, repository)?;
    let parent =
        admission.path.parent().ok_or_else(|| color_eyre::eyre::eyre!("receipt has no parent"))?;
    // Benign directories can remain after contention or refusal; never remove
    // directories whose ownership may be shared with another producer.
    fs::create_dir_all(parent)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&admission.lock).wrap_err(
        "acquiring exclusive receipt lock; existing locks require owner reconciliation",
    )?;
    // Construct the owner before fallible work so every normal exit releases it.
    let mut artifact = Artifact { admission, file: None };
    let written = writeln!(file, "rust-small-proof pid={}", std::process::id());
    artifact.file = Some(file);
    written?;
    validate(artifact.path(), true, repository)?;
    Ok(artifact)
}

#[cfg(test)]
mod tests {
    use super::super::{invalidate_prior_receipt, write_receipt};
    use super::*;

    struct Repository(PathBuf);

    impl Repository {
        fn new() -> Result<Self> {
            let nonce =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
            let path =
                std::env::temp_dir().join(format!("rsp-admission-{}-{nonce}", std::process::id()));
            fs::create_dir(&path)?;
            let repository = Self(path);
            git_output(&repository.0, &["init", "--quiet"])?;
            fs::write(repository.0.join("source.txt"), b"governed source\n")?;
            git_output(&repository.0, &["add", "source.txt"])?;
            git_output(
                &repository.0,
                &[
                    "-c",
                    "user.name=Receipt fixture",
                    "-c",
                    "user.email=receipt@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--quiet",
                    "-m",
                    "fixture",
                ],
            )?;
            Ok(repository)
        }
    }

    impl Drop for Repository {
        fn drop(&mut self) {
            // This unique directory was created by this fixture, never reused.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn seed(path: &Path) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(&super::super::tests::success_receipt())?;
        fs::write(path, &bytes)?;
        Ok(bytes)
    }

    fn refused(path: &Path, repository: &Path, reason: &str) -> Result<()> {
        match acquire_in(path, repository) {
            Ok(_) => bail!("unexpected admission of {}", path.display()),
            Err(error) if format!("{error:#}").contains(reason) => Ok(()),
            Err(error) => bail!("refused for wrong reason (expected {reason}): {error:#}"),
        }
    }

    #[test]
    fn fresh_parent_and_prior_receipt_lifecycle_is_exclusive() -> Result<()> {
        let repository = Repository::new()?;
        let path = repository.0.join("target/receipts/proof.json");
        let admitted = validate(&path, false, &repository.0)?;
        if path.parent().is_some_and(Path::exists) {
            bail!("preflight created directories");
        }
        let artifact = acquire_in(&path, &repository.0)?;
        if artifact.path() != admitted.path() {
            bail!("admission changed destination");
        }
        refused(&path, &repository.0, "sidecar already exists")?;
        write_receipt(artifact.path(), &super::super::tests::success_receipt())?;
        let bytes = fs::read(&path)?;
        if acquire_in(&path, &repository.0).is_ok() || fs::read(&path)? != bytes {
            bail!("contender replaced live receipt");
        }
        drop(artifact);
        let replacement = acquire_in(&path, &repository.0)?;
        invalidate_prior_receipt(replacement.path())?;
        if path.exists() {
            bail!("admitted prior was not invalidated");
        }
        drop(replacement);
        if lock_path(&path).exists() {
            bail!("normal return retained lock");
        }
        Ok(())
    }

    #[test]
    fn tracked_index_and_head_destinations_are_preserved() -> Result<()> {
        let repository = Repository::new()?;
        let path = repository.0.join("source.txt");
        let bytes = fs::read(&path)?;
        refused(&path, &repository.0, "tracked in HEAD or the index")?;
        if fs::read(&path)? != bytes {
            bail!("tracked destination changed");
        }
        git_output(&repository.0, &["rm", "--cached", "--quiet", "source.txt"])?;
        refused(&path, &repository.0, "tracked in HEAD or the index")?;
        let added = repository.0.join("added.json");
        let added_bytes = seed(&added)?;
        git_output(&repository.0, &["add", "added.json"])?;
        refused(&added, &repository.0, "tracked in HEAD or the index")?;
        if fs::read(&added)? != added_bytes {
            bail!("index-owned receipt changed");
        }
        Ok(())
    }

    #[test]
    fn unrelated_destination_and_uncertain_sidecars_are_preserved() -> Result<()> {
        let repository = Repository::new()?;
        let path = repository.0.join("custom.json");
        fs::write(&path, b"unrelated data")?;
        refused(&path, &repository.0, "not a recognized receipt")?;
        if fs::read(&path)? != b"unrelated data" {
            bail!("unrelated file changed");
        }
        fs::remove_file(&path)?;
        let prior = seed(&path)?;
        for sidecar in [lock_path(&path), staging_path(&path)] {
            fs::write(&sidecar, b"unknown owner")?;
            refused(&path, &repository.0, "sidecar already exists")?;
            if fs::read(&path)? != prior || fs::read(&sidecar)? != b"unknown owner" {
                bail!("uncertain sidecar modified");
            }
            fs::remove_file(sidecar)?;
        }
        Ok(())
    }

    #[test]
    fn existing_parent_traversal_resolves_but_missing_parent_traversal_fails() -> Result<()> {
        let repository = Repository::new()?;
        fs::create_dir(repository.0.join("nested"))?;
        let path = repository.0.join("nested/../proof.json");
        let artifact = acquire_in(&path, &repository.0)?;
        if artifact.path() != canonical_destination(&repository.0.join("proof.json"))? {
            bail!("ordinary parent-relative destination resolved incorrectly");
        }
        if canonical_destination(&repository.0.join("absent/../other.json")).is_ok() {
            bail!("missing parent traversal was admitted");
        }
        Ok(())
    }

    #[test]
    fn coherent_failed_older_candidate_is_replaceable() -> Result<()> {
        use super::super::{ProofResult, StepOutcome};
        let repository = Repository::new()?;
        let path = repository.0.join("failed.json");
        let mut receipt = super::super::tests::failure_receipt(
            1,
            StepOutcome::ProductFailure,
            ProofResult::ProductFailure,
        );
        receipt.subject.git_sha = "older-candidate".to_string();
        let bytes = serde_json::to_vec(&receipt)?;
        fs::write(&path, &bytes)?;
        let artifact = acquire_in(&path, &repository.0)?;
        if fs::read(&path)? != bytes {
            bail!("admission changed prior failure before invalidation");
        }
        invalidate_prior_receipt(artifact.path())?;
        write_receipt(artifact.path(), &super::super::tests::success_receipt())?;
        Ok(())
    }

    #[test]
    fn publication_preserves_unowned_stage_and_new_destination() -> Result<()> {
        let repository = Repository::new()?;
        let path = repository.0.join("proof.json");
        let artifact = acquire_in(&path, &repository.0)?;
        let stage = staging_path(artifact.path());
        fs::write(&stage, b"unowned stage")?;
        if write_receipt(artifact.path(), &super::super::tests::success_receipt()).is_ok()
            || fs::read(&stage)? != b"unowned stage"
        {
            bail!("publication changed unowned staging data");
        }
        fs::remove_file(&stage)?;
        fs::write(&path, b"new destination")?;
        if write_receipt(artifact.path(), &super::super::tests::success_receipt()).is_ok()
            || fs::read(&path)? != b"new destination"
            || stage.exists()
        {
            bail!("publication replaced destination or left owned staging data");
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn linked_destination_and_directory_are_refused() -> Result<()> {
        use std::os::unix::fs::symlink;
        let repository = Repository::new()?;
        let source = repository.0.join("source.txt");
        let linked = repository.0.join("linked.json");
        symlink(&source, &linked)?;
        if acquire_in(&linked, &repository.0).is_ok() {
            bail!("linked destination was admitted");
        }
        let alias = repository.0.join("alias");
        symlink(&repository.0, &alias)?;
        if acquire_in(&alias.join("proof.json"), &repository.0).is_ok() {
            bail!("linked parent was admitted");
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn junction_parent_is_refused_and_drive_relative_paths_are_ambiguous() -> Result<()> {
        let repository = Repository::new()?;
        let alias = repository.0.join("alias");
        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&alias)
            .arg(&repository.0)
            .output()?;
        if !output.status.success() {
            bail!("junction fixture creation failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        let result = refused(&alias.join("proof.json"), &repository.0, "links or reparse points");
        fs::remove_dir(&alias)?;
        result?;
        if canonical_destination(Path::new("Z:proof.json")).is_ok() {
            bail!("unresolved drive-relative destination accepted");
        }
        Ok(())
    }
}
