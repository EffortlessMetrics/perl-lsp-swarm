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

    assert_eq!(
        guide.contains("`lsp-mode` 10.0.1 requires Emacs 29.1"),
        true,
        "the active Emacs guide must pin the current tested lsp-mode/Emacs boundary"
    );
    assert_eq!(
        guide.contains(
            "current `lsp-mode` 10.0.1 requires Emacs 29.1 and is not\na drop-in path for Emacs 28"
        ),
        true,
        "older-Emacs guidance must distinguish current lsp-mode from compatible historical releases"
    );
    assert_eq!(
        guide
            .contains("If you use Emacs 28 or older, install Eglot separately\nor use `lsp-mode`."),
        false,
        "the stale Emacs 28 -> unqualified lsp-mode guidance must not return"
    );
}

#[test]
fn active_emacs_guide_keeps_manual_and_stock_discovery_distinct() {
    let guide = read("docs/EDITORS/EMACS_SETUP.md");

    assert_eq!(
        guide.contains(
            "Current stock Eglot does not yet\ndiscover `perllsp` automatically for Perl"
        ),
        true,
        "Eglot manual registration must not be rendered as stock discovery"
    );
    assert_eq!(
        guide.contains("Current stock `lsp-mode` does\nnot yet ship a built-in `perllsp` client"),
        true,
        "lsp-mode manual registration must not be rendered as built-in discovery"
    );
    assert_eq!(
        guide.contains(
            "Treat `:priority` as a default selection mechanism, not as a\nvalue to increase indefinitely."
        ),
        true,
        "wrong-server troubleshooting should identify client ownership instead of escalating priority forever"
    );
}
