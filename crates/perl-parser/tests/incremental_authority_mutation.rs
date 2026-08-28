//! Independent mutation control for lower-tier incremental authority.
//!
//! This test deliberately does not reuse the detector in
//! `incremental_authority_contract.rs`: a regression in that detector must not be
//! able to prove itself complete. It reconciles every production reference to the
//! lower-tier `reparse` entry point against the authority ledger and covers method,
//! associated-function, and function-item syntax.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct AuthorityManifest {
    lower_tier: Vec<LowerTierSurface>,
}

#[derive(Debug, Deserialize)]
struct LowerTierSurface {
    package: String,
    allowed_consumers: Vec<AllowedConsumer>,
}

#[derive(Debug, Deserialize)]
struct AllowedConsumer {
    source_path: String,
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    crate_root()
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "perl-parser must live below the workspace crates directory".into())
}

fn compact_whitespace(source: &str) -> String {
    source.split_whitespace().collect()
}

fn rust_source_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && path.extension().is_some_and(|extension| extension == OsStr::new("rs"))
            {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

fn production_rust_sources(workspace: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let crates_root = workspace.join("crates");
    let mut crate_entries = fs::read_dir(&crates_root)?.collect::<Result<Vec<_>, _>>()?;
    crate_entries.sort_by_key(|entry| entry.file_name());

    let mut sources = Vec::new();
    for entry in crate_entries {
        if !entry.file_type()?.is_dir() || entry.file_name() == OsStr::new("perl-parser-core") {
            continue;
        }

        let src = entry.path().join("src");
        if src.is_dir() {
            sources.extend(rust_source_files(&src)?);
        }
    }
    sources.sort();
    Ok(sources)
}

fn uses_lower_tier_incremental(source: &str) -> bool {
    let compact = compact_whitespace(source);
    if compact.contains("perl_parser_core::incremental") {
        return true;
    }

    let mut cursor = 0;
    while let Some(offset) = compact[cursor..].find("perl_parser_core::{") {
        let statement = &compact[cursor + offset..];
        let imports = &statement[..statement.find(';').unwrap_or(statement.len())];
        if imports.contains("incremental::{")
            || imports.contains("incremental,")
            || imports.contains("incremental}")
            || imports.contains("incrementalas")
        {
            return true;
        }
        cursor += offset + "perl_parser_core::{".len();
    }

    compact.split(';').any(|statement| {
        ["useperl_parser_coreas", "externcrateperl_parser_coreas"].iter().any(|prefix| {
            statement.strip_prefix(prefix).is_some_and(|remainder| {
                let alias = remainder.split([':', '{', ',', '}']).next().unwrap_or_default();
                !alias.is_empty() && compact.contains(&format!("{alias}::incremental"))
            })
        })
    })
}

fn lower_tier_reparse_reference_count(source: &str) -> usize {
    let compact = compact_whitespace(source);
    compact.match_indices(".reparse(").count() + compact.match_indices("::reparse").count()
}

fn declared_reference_counts() -> Result<BTreeMap<String, usize>, Box<dyn std::error::Error>> {
    let manifest_source = fs::read_to_string(crate_root().join("incremental_authority.json"))?;
    let manifest: AuthorityManifest = serde_json::from_str(&manifest_source)?;
    let kernel = manifest
        .lower_tier
        .iter()
        .find(|surface| surface.package == "perl-parser-core")
        .ok_or("perl-parser-core is missing from the incremental authority ledger")?;

    let mut counts = BTreeMap::new();
    for consumer in &kernel.allowed_consumers {
        *counts.entry(consumer.source_path.clone()).or_insert(0) += 1;
    }
    Ok(counts)
}

fn discovered_reference_counts() -> Result<BTreeMap<String, usize>, Box<dyn std::error::Error>> {
    let workspace = workspace_root()?;
    let mut counts = BTreeMap::new();

    for path in production_rust_sources(&workspace)? {
        let source = fs::read_to_string(&path)?;
        if !uses_lower_tier_incremental(&source) {
            continue;
        }

        let count = lower_tier_reparse_reference_count(&source);
        if count > 0 {
            let relative = path.strip_prefix(&workspace)?.to_string_lossy().replace('\\', "/");
            counts.insert(relative, count);
        }
    }

    Ok(counts)
}

#[test]
fn detector_catches_method_associated_and_function_item_syntax() {
    assert_eq!(lower_tier_reparse_reference_count("state.reparse(source, &edit);"), 1);
    assert_eq!(
        lower_tier_reparse_reference_count("IncrementalState::reparse(&mut state, source, &edit);"),
        1
    );
    assert_eq!(
        lower_tier_reparse_reference_count("let reparse = State::reparse; reparse(&mut state);"),
        1
    );
}

#[test]
fn every_lower_tier_reparse_reference_is_ledger_owned() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        discovered_reference_counts()?,
        declared_reference_counts()?,
        "production references to perl-parser-core incremental reparse must match the authority ledger"
    );
    Ok(())
}
