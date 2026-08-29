// Vim + vim-lsp Git-root marker contract (#7762).
//
// The cross-editor activation contract stores the semantic marker name `.git`.
// The pinned vim-lsp helper, however, treats a marker as a directory only when
// its spelling ends in `/` or `\`. The Vim adapter must therefore expand the
// one semantic marker into both client spellings: `.git/` for an ordinary Git
// directory and `.git` for linked worktrees/submodules where `.git` is a file.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(1).unwrap_or(Path::new(".")).to_path_buf()
}

fn source(path: &str) -> Result<String> {
    let full = repo_root().join(path);
    fs::read_to_string(&full).with_context(|| format!("reading {}", full.display()))
}

#[test]
fn canonical_adapter_expands_the_plain_git_marker_for_vim_lsp() -> Result<()> {
    let manifest_bytes = source(".ci/editor-clients/vim-vim-lsp-activation-root.v1.json")?;
    let manifest: Value = serde_json::from_str(&manifest_bytes)?;
    let markers = manifest
        .get("root")
        .and_then(|root| root.get("markers"))
        .and_then(Value::as_array)
        .context("activation-root manifest has no root.markers array")?;
    ensure!(
        markers.iter().any(|marker| marker.as_str() == Some(".git")),
        "the cross-editor activation contract must retain the semantic `.git` marker"
    );
    ensure!(
        !markers.iter().any(|marker| marker.as_str() == Some(".git/")),
        "vim-lsp syntax must not leak into the cross-editor activation contract"
    );

    let adapter = source("scripts/test/vim-clients/vim-lsp-adapter.vim")?;
    ensure!(
        adapter.contains("function! VimLspHostClientRootMarkers() abort"),
        "the canonical adapter must expose its client-specific marker projection"
    );
    ensure!(
        adapter.contains("call extend(l:markers, ['.git/', '.git'])"),
        "the canonical adapter must cover both Git-directory and Git-file roots"
    );
    ensure!(
        adapter.contains("\\ expand('%:p'), VimLspHostClientRootMarkers())"),
        "root selection must consume the adapted marker list"
    );
    Ok(())
}

#[test]
fn executable_vim_lsp_root_proof_covers_both_git_root_shapes() -> Result<()> {
    let host_driver = source("scripts/test/vim-host-driver.vim")?;
    for required in [
        "filereadable(s:marker_path) || isdirectory(s:marker_path)",
        "function! s:ProbeGitRootShapes() abort",
        "VimLspHostClientRootMarkers()",
        "s:SamePath(s:root_path, s:fixture_root)",
        "git_directory_root_mismatch",
        "git_file_root_mismatch",
    ] {
        ensure!(
            host_driver.contains(required),
            "the canonical hermetic host proof is missing the load-bearing `{required}` discriminator"
        );
    }

    let deep_rail = source("scripts/ux/vim_vim_lsp_driver.vim")?;
    ensure!(
        deep_rail.contains("VimLspHostRegister()"),
        "the deep actual-client rail must use adapter-owned registration"
    );

    let smoke = source("scripts/ux/vim_activation_root_smoke.sh")?;
    for required in [
        "roots/git-only/.git",
        "roots/git-file/.git",
        "VimLspHostClientRootMarkers",
        "vim_lsp_helper_roots",
        "git_directory",
        "git_file",
        "accepted an unmarked tree",
    ] {
        ensure!(
            smoke.contains(required),
            "actual-host root smoke is missing the load-bearing `{required}` discriminator"
        );
    }
    Ok(())
}

#[test]
fn documented_vim_lsp_recipe_covers_git_directories_and_git_files() -> Result<()> {
    let guide = source("docs/EDITORS/VIM_SETUP.md")?;
    ensure!(
        guide.contains("'dist.ini', '.git/', '.git'"),
        "the documented vim-lsp root helper must cover both Git root shapes"
    );
    ensure!(
        guide.contains("linked worktree") && guide.contains("submodule"),
        "the guide must explain why both Git marker spellings are required"
    );
    Ok(())
}
