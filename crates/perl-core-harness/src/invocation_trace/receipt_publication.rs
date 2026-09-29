//! Failure-safe publication of one capture's receipt files.
//!
//! Staging precedes installation. Returned installation failures roll back only
//! files installed by this attempt. This is not a crash-atomic multi-file commit.

use color_eyre::eyre::{Context, Result, bail};
use std::fs;
use std::io::Write;
use std::path::Path;

pub(super) fn publish(receipts: &[(&Path, Vec<u8>)]) -> Result<()> {
    let mut staged = Vec::new();
    for (path, bytes) in receipts {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
        let mut file = tempfile::NamedTempFile::new_in(parent)
            .with_context(|| format!("staging receipt {}", path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("writing staged receipt {}", path.display()))?;
        staged.push((*path, file));
    }
    let mut installed: Vec<&Path> = Vec::new();
    for (path, file) in staged {
        if let Err(error) = file.persist_noclobber(path) {
            let mut cleanup_errors = Vec::new();
            for owned in installed {
                if let Err(cleanup) = fs::remove_file(owned) {
                    cleanup_errors.push(format!("{}: {cleanup}", owned.display()));
                }
            }
            if !cleanup_errors.is_empty() {
                bail!(
                    "publishing receipt {}: {error}; rollback failures: {}",
                    path.display(),
                    cleanup_errors.join("; ")
                );
            }
            return Err(error).with_context(|| format!("publishing receipt {}", path.display()));
        }
        installed.push(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::ensure;

    #[test]
    fn staging_failure_publishes_nothing() -> Result<()> {
        let root = tempfile::tempdir()?;
        let first = root.path().join("first.json");
        let barrier = root.path().join("not-a-directory");
        fs::write(&barrier, b"preserve")?;
        let last = barrier.join("last.json");
        ensure!(
            publish(&[(&first, b"first".to_vec()), (&last, b"last".to_vec())]).is_err(),
            "invalid later staging directory must fail"
        );
        ensure!(!first.exists(), "earlier receipt must remain unpublished");
        ensure!(fs::read(&barrier)? == b"preserve", "unrelated barrier must survive");
        Ok(())
    }

    #[test]
    fn install_failure_rolls_back_only_this_attempt() -> Result<()> {
        let root = tempfile::tempdir()?;
        let first = root.path().join("first.json");
        let occupied = root.path().join("occupied.json");
        fs::write(&occupied, b"preserve")?;
        ensure!(
            publish(&[(&first, b"first".to_vec()), (&occupied, b"replace".to_vec())]).is_err(),
            "publication must not overwrite another writer's file"
        );
        ensure!(!first.exists(), "earlier installed receipt must be rolled back");
        ensure!(fs::read(&occupied)? == b"preserve", "existing destination must survive");
        fs::remove_file(&occupied)?;
        publish(&[(&first, b"first".to_vec()), (&occupied, b"second".to_vec())])?;
        ensure!(
            fs::read(&first)? == b"first" && fs::read(&occupied)? == b"second",
            "successful publication must retain the complete set"
        );
        Ok(())
    }
}
