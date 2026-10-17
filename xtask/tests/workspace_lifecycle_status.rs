//! Architecture fence for `workspace_lifecycle_status.v1` (#16895).
//!
//! `cargo xtask check-architecture` is not a current CLI target. This test is
//! the current equivalent: the contract lives in `perl-lsp-rs-core`, production
//! code does not read live workspace state, and sibling CLI/protocol/editor/
//! support surfaces do not define a second status vocabulary.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(rel: &str) -> Result<String> {
    let path = workspace_root().join(rel);
    Ok(fs::read_to_string(&path)?)
}

fn walk_source_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "target" || name == "node_modules" {
                continue;
            }
            walk_source_files(&path, out)?;
        } else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs" | "ts" | "js"))
        {
            out.push(path);
        }
    }
    Ok(())
}

#[test]
fn workspace_lifecycle_status_contract_is_owned_by_perl_lsp_rs_core() -> Result<()> {
    let lib = read("crates/perl-lsp-rs-core/src/lib.rs")?;
    ensure!(
        lib.contains("pub mod workspace_lifecycle_status"),
        "perl-lsp-rs-core must export workspace_lifecycle_status"
    );
    ensure!(
        workspace_root()
            .join("crates/perl-lsp-rs-core/src/workspace_lifecycle_status/mod.rs")
            .is_file(),
        "contract module must exist"
    );
    Ok(())
}

#[test]
fn workspace_lifecycle_status_production_source_does_not_read_live_state() -> Result<()> {
    let dir = workspace_root().join("crates/perl-lsp-rs-core/src/workspace_lifecycle_status");
    let mut files = Vec::new();
    walk_source_files(&dir, &mut files)?;
    ensure!(!files.is_empty(), "contract sources must exist");
    for path in files {
        if path.file_name().and_then(|name| name.to_str()) == Some("tests.rs") {
            continue;
        }
        let source = fs::read_to_string(&path)?;
        for forbidden in [
            "use perl_workspace",
            "use crate::providers",
            "use crate::protocol",
            "use crate::runtime",
            "use crate::config::",
            "std::fs::",
            "std::env::",
            "crate::workspace::",
        ] {
            ensure!(
                !source.contains(forbidden),
                "{} must not read live workspace/CLI/protocol state: found `{forbidden}`",
                path.display()
            );
        }
    }
    Ok(())
}

#[test]
fn sibling_surfaces_do_not_define_workspace_lifecycle_status_vocabulary() -> Result<()> {
    let mut files = Vec::new();
    for rel in [
        "crates/perl-lsp-rs/src",
        "crates/perl-workspace/src",
        "crates/perllsp/src",
        "vscode-extension/src",
    ] {
        walk_source_files(&workspace_root().join(rel), &mut files)?;
    }
    for path in files {
        let source = fs::read_to_string(&path)?;
        ensure!(
            !source.contains("workspace_lifecycle_status"),
            "{} must not define a sibling workspace_lifecycle_status vocabulary",
            path.display()
        );
        ensure!(
            !source.contains("WorkspaceLifecycleStatusV1"),
            "{} must not define a sibling WorkspaceLifecycleStatusV1 type",
            path.display()
        );
    }
    Ok(())
}
