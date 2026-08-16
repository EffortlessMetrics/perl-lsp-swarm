use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

pub const CANONICAL_EXPECTATION_SET_ID: &str = "perl-agent-client-v1";

pub const CANONICAL_EXPECTATION_IDS: &[&str] = &[
    "code_action_preview.syntax",
    "definition.widget_new",
    "diagnostic.syntax",
    "document_symbols.widget",
    "edit_requery.widget_greet",
    "hover.widget_name",
    "lifecycle.shutdown",
    "references.widget_greet",
    "rename_preview.greet",
    "unicode.utf16",
    "workspace.partial_not_ready",
    "workspace_symbols.widget",
];

pub fn canonical_expectation_set_digest() -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(CANONICAL_EXPECTATION_SET_ID.as_bytes());
    hasher.update([0]);
    update_expectation_ids(&mut hasher, CANONICAL_EXPECTATION_IDS)?;
    digest_identity(hasher)
}

fn update_expectation_ids(hasher: &mut Sha256, ids: &[&str]) -> Result<()> {
    ensure!(!ids.is_empty(), "expectation set must contain at least one id");
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ensure!(
        ids.windows(2).all(|pair| pair[0] != pair[1]),
        "expectation set contains duplicate ids"
    );

    for id in ids {
        ensure!(is_reason_token(id), "expectation id is not a stable reason token: {id}");
        hasher.update(id.as_bytes());
        hasher.update([0]);
    }
    Ok(())
}

pub fn fixture_digest(root: &Path) -> Result<String> {
    canonical_expectation_set_digest().context("validating canonical expectation set")?;
    ensure!(root.is_dir(), "fixture root is not a directory: {}", root.display());
    let mut files = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = entry.with_context(|| format!("walking fixture root {}", root.display()))?;
        if entry.file_type().is_symlink() {
            bail!("fixture must not contain symlink: {}", entry.path().display());
        }
        if entry.file_type().is_file() {
            let relative_path =
                entry.path().strip_prefix(root).with_context(|| "fixture path escaped root")?;
            let mut components = Vec::new();
            for component in relative_path.components() {
                let component = component.as_os_str().to_str().with_context(|| {
                    format!("fixture path is not valid UTF-8: {}", entry.path().display())
                })?;
                ensure!(
                    !component.contains('\\'),
                    "fixture path component must not contain a backslash"
                );
                components.push(component);
            }
            let relative = components.join("/");
            files.push((relative, entry.path().to_path_buf()));
        }
    }
    ensure!(!files.is_empty(), "fixture root contains no files: {}", root.display());
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    for (relative, path) in files {
        let bytes =
            fs::read(&path).with_context(|| format!("reading fixture file {}", path.display()))?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    digest_identity(hasher)
}

fn digest_identity(hasher: Sha256) -> Result<String> {
    let mut identity = String::with_capacity("sha256:".len() + 64);
    identity.push_str("sha256:");
    for byte in hasher.finalize() {
        write!(&mut identity, "{byte:02x}")?;
    }
    Ok(identity)
}

fn is_reason_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        })
        && value.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(ids: &[&str]) -> Result<String> {
        let mut hasher = Sha256::new();
        update_expectation_ids(&mut hasher, ids)?;
        digest_identity(hasher)
    }

    /// The canonical set is a fixed constant, so the contract tests only ever
    /// drive `update_expectation_ids` down its success path. These cases reach
    /// the rejection branches directly — without them the validation here is
    /// asserted rather than proven.
    #[test]
    fn expectation_set_rejects_empty_duplicate_and_untokenized_ids() -> Result<()> {
        ensure!(digest_of(&[]).is_err(), "empty expectation set was accepted");
        ensure!(
            digest_of(&["definition.widget_new", "definition.widget_new"]).is_err(),
            "duplicate expectation ids were accepted"
        );
        ensure!(digest_of(&["Definition.Widget"]).is_err(), "uppercase expectation id accepted");
        ensure!(digest_of(&["hover widget"]).is_err(), "expectation id with a space accepted");
        ensure!(digest_of(&["definition.widget_new"]).is_ok(), "a valid id was rejected");
        Ok(())
    }

    /// Ordering must not change identity, but membership must.
    #[test]
    fn expectation_set_digest_is_order_independent_and_membership_bound() -> Result<()> {
        ensure!(
            digest_of(&["a.one", "b.two"])? == digest_of(&["b.two", "a.one"])?,
            "expectation digest depended on input order"
        );
        ensure!(
            digest_of(&["a.one", "b.two"])? != digest_of(&["a.one", "b.three"])?,
            "expectation digest ignored a changed member"
        );
        Ok(())
    }

    #[test]
    fn canonical_expectation_set_is_stable_and_well_formed() -> Result<()> {
        let first = canonical_expectation_set_digest()?;
        ensure!(
            first == canonical_expectation_set_digest()?,
            "canonical expectation digest was not deterministic"
        );
        ensure!(
            first.starts_with("sha256:") && first.len() == "sha256:".len() + 64,
            "canonical expectation digest had the wrong identity shape"
        );
        for id in CANONICAL_EXPECTATION_IDS {
            ensure!(is_reason_token(id), "canonical expectation id is not a reason token: {id}");
        }
        Ok(())
    }

    #[test]
    fn reason_tokens_reject_leading_punctuation_and_non_ascii() -> Result<()> {
        for accepted in ["lifecycle.shutdown", "utf-16", "0abc"] {
            ensure!(is_reason_token(accepted), "reason token rejected: {accepted}");
        }
        for rejected in ["", "_leading", ".leading", "-leading", "Upper", "héllo"] {
            ensure!(!is_reason_token(rejected), "reason token accepted: {rejected}");
        }
        Ok(())
    }

    /// `fixture_digest`'s guards are error paths the contract tests never take,
    /// because they always point it at the real canonical fixture.
    #[test]
    fn fixture_digest_rejects_a_missing_or_empty_root() -> Result<()> {
        ensure!(
            fixture_digest(Path::new("this/path/does/not/exist")).is_err(),
            "non-directory fixture root was accepted"
        );
        let empty = tempfile::TempDir::new()?;
        ensure!(
            fixture_digest(empty.path()).is_err(),
            "fixture root containing no files was accepted"
        );
        Ok(())
    }

    #[test]
    fn fixture_digest_binds_relative_path_and_content() -> Result<()> {
        let root = tempfile::TempDir::new()?;
        fs::write(root.path().join("a.pl"), b"one")?;
        let before = fixture_digest(root.path())?;

        fs::write(root.path().join("a.pl"), b"two")?;
        ensure!(before != fixture_digest(root.path())?, "fixture digest ignored changed content");

        fs::write(root.path().join("a.pl"), b"one")?;
        fs::rename(root.path().join("a.pl"), root.path().join("b.pl"))?;
        ensure!(before != fixture_digest(root.path())?, "fixture digest ignored the relative path");
        Ok(())
    }
}
