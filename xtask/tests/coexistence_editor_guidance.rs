// Integration test: assertion helpers carry failure messages.
#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

const COEXISTENCE_DOC: &str = "docs/EDITORS/COEXISTENCE.md";

const NON_VSCODE_EDITOR_GUIDES: &[&str] = &[
    "docs/EDITORS/NEOVIM_SETUP.md",
    "docs/EDITORS/EMACS_SETUP.md",
    "docs/EDITORS/INTELLIJ_IDEA_SETUP.md",
    "docs/EDITORS/ZED_SETUP.md",
    "docs/EDITORS/HELIX_SETUP.md",
    "docs/EDITORS/SUBLIME_SETUP.md",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live directly under the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

#[test]
fn coexistence_doc_states_non_vscode_boundary_and_vs_code_command() {
    let doc = read(COEXISTENCE_DOC);
    for needle in [
        "run the VS Code coexistence detector",
        "Show Coexistence Status",
        "perllsp --health",
        "PATH presence of `perlcritic`",
        "has no selectable external Perl::Critic engine",
    ] {
        assert!(doc.contains(needle), "COEXISTENCE.md must contain {needle:?}");
    }
}

#[test]
fn non_vscode_editor_guides_link_coexistence_guidance() {
    for guide in NON_VSCODE_EDITOR_GUIDES {
        let body = read(guide);
        assert!(
            body.contains("COEXISTENCE.md"),
            "{guide} must link to COEXISTENCE.md for honest coexistence boundaries (#7214)"
        );
    }
}
