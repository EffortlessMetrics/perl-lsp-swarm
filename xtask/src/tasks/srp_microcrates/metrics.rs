use crate::tasks::srp_microcrates::metadata::Metadata;
use color_eyre::eyre::{Result, WrapErr};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct CrateMetrics {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) total_deps: usize,
    pub(super) internal_deps: usize,
    pub(super) rust_files: usize,
    pub(super) loc: usize,
}

pub(super) fn collect(metadata: Metadata) -> Result<(PathBuf, Vec<CrateMetrics>)> {
    let members: HashSet<_> = metadata.workspace_members.into_iter().collect();
    let workspace_root = PathBuf::from(&metadata.workspace_root);
    let mut metrics = Vec::new();

    for pkg in metadata.packages {
        if !members.contains(&pkg.id) {
            continue;
        }
        let manifest = PathBuf::from(&pkg.manifest_path);
        let Some(crate_dir) = manifest.parent() else {
            continue;
        };

        let relative_crate_dir = match crate_dir.strip_prefix(&workspace_root) {
            Ok(path) => path,
            Err(_) => continue,
        };

        if !relative_crate_dir.starts_with("crates") {
            continue;
        }

        let src_dir = crate_dir.join("src");
        let (rust_files, loc) = count_rust_code(&src_dir)?;
        let total_deps = pkg.dependencies.len();
        let internal_deps = pkg
            .dependencies
            .iter()
            .filter(|dep| dep.name.starts_with("perl-") || dep.name.starts_with("tree-sitter-perl"))
            .count();

        metrics.push(CrateMetrics {
            name: pkg.name,
            path: relative_crate_dir.display().to_string(),
            total_deps,
            internal_deps,
            rust_files,
            loc,
        });
    }

    metrics.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((workspace_root, metrics))
}

fn count_rust_code(src_dir: &Path) -> Result<(usize, usize)> {
    if !src_dir.exists() {
        return Ok((0, 0));
    }

    let mut files = 0;
    let mut loc = 0;
    for entry in walkdir::WalkDir::new(src_dir) {
        let entry = entry.wrap_err("failed to walk src dir")?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "rs") {
            files += 1;
            let contents = fs::read_to_string(entry.path()).wrap_err_with(|| {
                format!("failed to read source file {}", entry.path().display())
            })?;
            loc += contents.lines().count();
        }
    }

    Ok((files, loc))
}
