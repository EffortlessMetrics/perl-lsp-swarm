// Integration test: assertion helpers (`expect`/`unwrap`/`panic!`) carry the
// failure message. The workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used, clippy::panic)]
use std::fs;
use std::path::{Path, PathBuf};

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
fn current_emacs_guide_does_not_route_emacs_28_to_current_lsp_mode() {
    let guide = read("docs/EDITORS/EMACS_SETUP.md");

    assert!(
        guide.contains("`lsp-mode` 10.0.1 requires Emacs 29.1"),
        "the active Emacs guide must pin the current tested lsp-mode/Emacs boundary"
    );
    assert!(
        guide.contains(
            "current `lsp-mode` 10.0.1 requires Emacs 29.1 and is not\na drop-in path for Emacs 28"
        ),
        "older-Emacs guidance must distinguish current lsp-mode from compatible historical releases"
    );
    assert!(
        !guide
            .contains("If you use Emacs 28 or older, install Eglot separately\nor use `lsp-mode`."),
        "the stale Emacs 28 -> unqualified lsp-mode guidance must not return"
    );
}

#[test]
fn active_emacs_guide_keeps_manual_and_stock_discovery_distinct() {
    let guide = read("docs/EDITORS/EMACS_SETUP.md");

    assert!(
        guide.contains(
            "Current stock Eglot does not yet\ndiscover `perllsp` automatically for Perl"
        ),
        "Eglot manual registration must not be rendered as stock discovery"
    );
    assert!(
        guide.contains("Current stock `lsp-mode` does\nnot yet ship a built-in `perllsp` client"),
        "lsp-mode manual registration must not be rendered as built-in discovery"
    );
    assert!(
        guide.contains(
            "Treat `:priority` as a default selection mechanism, not as a\nvalue to increase indefinitely."
        ),
        "wrong-server troubleshooting should identify client ownership instead of escalating priority forever"
    );
}
