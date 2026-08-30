//! Governed Rust file inventory for the tautology checker.
//!
//! Walks the same production roots as `ci_policy` (`crates`, `xtask`,
//! `examples`, `tests`) and skips generated/vendored/archive trees. Missing
//! search roots fall back to walking `root` itself so fixture trees work.

use color_eyre::eyre::{Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const SEARCH_ROOTS: &[&str] = &["crates", "xtask", "examples", "tests"];
const SKIP_DIR_NAMES: &[&str] = &["target", "generated", "vendor", ".git"];
const SKIP_PREFIXES: &[&str] =
    &["archive/", "fuzz/", "tree-sitter-perl/", "test_corpus/", "vendor/"];

pub fn collect_rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut found_search_root = false;
    for relative in SEARCH_ROOTS {
        let search_root = root.join(relative);
        if search_root.is_dir() {
            found_search_root = true;
            collect_under(&search_root, root, &mut files)?;
        }
    }
    if !found_search_root {
        collect_under(root, root, &mut files)?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_under(dir: &Path, root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|entry| !(entry.file_type().is_dir() && skip_dir(entry.path())))
    {
        let entry = entry.with_context(|| format!("failed to walk {}", dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
        if skip_relative(relative) {
            continue;
        }
        files.push(entry.path().to_path_buf());
    }
    Ok(())
}

fn skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| SKIP_DIR_NAMES.contains(&name))
}

fn skip_relative(path: &Path) -> bool {
    let rendered = path.to_string_lossy().replace('\\', "/");
    SKIP_PREFIXES
        .iter()
        .any(|prefix| rendered.starts_with(prefix) || rendered.contains(&format!("/{prefix}")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{collect_rust_files, skip_relative};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn inventory_skips_generated_and_collects_governed_rust() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();
        fs::create_dir_all(root.join("crates/demo/src")).expect("crates dir");
        fs::create_dir_all(root.join("crates/demo/generated")).expect("generated dir");
        fs::create_dir_all(root.join("archive/old")).expect("archive dir");
        fs::write(root.join("crates/demo/src/lib.rs"), "fn f() {}").expect("lib");
        fs::write(root.join("crates/demo/generated/out.rs"), "fn g() {}").expect("generated");
        fs::write(root.join("archive/old/legacy.rs"), "fn h() {}").expect("archive");
        fs::write(root.join("README.md"), "no").expect("readme");

        let files = collect_rust_files(root).expect("inventory");
        let relative: Vec<_> = files
            .iter()
            .map(|path| {
                path.strip_prefix(root).expect("prefix").to_string_lossy().replace('\\', "/")
            })
            .collect();
        assert_eq!(relative, vec!["crates/demo/src/lib.rs".to_string()]);
    }

    #[test]
    fn skip_relative_covers_workspace_exclusions() {
        assert!(skip_relative(Path::new("archive/foo.rs")));
        assert!(skip_relative(Path::new("fuzz/target.rs")));
        assert!(!skip_relative(Path::new("crates/perl-parser/src/lib.rs")));
    }
}
