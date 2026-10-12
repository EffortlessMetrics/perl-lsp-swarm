//! Source evidence controls against clean-filter, dirty and ignored inputs.
#[path = "../../perllsp/src/build_source_identity.rs"]
mod identity;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    anyhow::ensure!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}
fn observed(root: &Path) -> Result<(String, String)> {
    identity::source_identity(root).map_err(|error| anyhow::anyhow!("{error}"))
}
#[test]
fn raw_worktree_changes_bind_identity_without_changing_real_index() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    git(root, &["init"])?;
    git(root, &["config", "user.name", "UX proof"])?;
    git(root, &["config", "user.email", "ux-proof@example.invalid"])?;
    std::fs::write(root.join("source.rs"), "fn answer() { return 1; }\n")?;
    std::fs::write(root.join(".gitattributes"), "*.rs filter=collapse\n")?;
    git(root, &["config", "filter.collapse.clean", "sed 's/return 2/return 1/g'"])?;
    git(root, &["add", "."])?;
    git(root, &["commit", "-m", "fixture"])?;
    let index_before = std::fs::read(root.join(".git/index"))?;
    let clean = observed(root)?;
    let canonical = git(root, &["ls-tree", "-r", "-z", "--full-tree", "HEAD"])?;
    let digest =
        Sha256::digest(canonical).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    assert_eq!(clean.1, digest, "clean release inventory compatibility");
    std::fs::write(root.join("source.rs"), "fn answer() { return 2; }\n")?;
    let dirty = observed(root)?;
    assert_eq!(dirty.0, clean.0, "HEAD did not move");
    assert_ne!(dirty.1, clean.1, "clean filter must not hide compiler source edits");
    std::fs::write(root.join("new.rs"), "fn new() {}\n")?;
    let added = observed(root)?;
    assert_ne!(added.1, dirty.1, "untracked source must be bound");
    std::fs::remove_file(root.join("source.rs"))?;
    assert_ne!(observed(root)?.1, added.1, "deleted source must be bound");
    assert_eq!(std::fs::read(root.join(".git/index"))?, index_before, "real index is unchanged");
    std::fs::write(root.join(".gitignore"), "temp_*.rs\n")?;
    std::fs::create_dir_all(root.join("crates/probe/src"))?;
    std::fs::write(root.join("crates/probe/src/temp_probe.rs"), "fn ignored() {}\n")?;
    assert!(observed(root).context("ignored source must refuse exact evidence").is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn external_source_symlink_cannot_count_as_exact_evidence() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let root = fixture.path();
    git(root, &["init"])?;
    git(root, &["config", "user.name", "UX proof"])?;
    git(root, &["config", "user.email", "ux-proof@example.invalid"])?;
    std::fs::write(outside.path().join("source.rs"), "fn external() {}\n")?;
    std::os::unix::fs::symlink(outside.path().join("source.rs"), root.join("source.rs"))?;
    git(root, &["add", "."])?;
    git(root, &["commit", "-m", "fixture"])?;
    let error = observed(root).err().context("external source must be refused")?;
    assert!(error.to_string().contains("external symlink"), "{error}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn leading_quote_path_cannot_silently_hash_a_different_source_file() -> Result<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    git(root, &["init"])?;
    git(root, &["config", "user.name", "UX proof"])?;
    git(root, &["config", "user.email", "ux-proof@example.invalid"])?;
    std::fs::write(root.join("plain.rs"), "fn answer() { return 1; }\n")?;
    std::fs::write(root.join("\"plain.rs\""), "fn answer() { return 2; }\n")?;
    git(root, &["add", "."])?;
    git(root, &["commit", "-m", "fixture"])?;
    let error = observed(root).err().context("ambiguous quoted source must refuse proof")?;
    assert!(error.to_string().contains("leading-quote source paths"), "{error}");
    Ok(())
}
