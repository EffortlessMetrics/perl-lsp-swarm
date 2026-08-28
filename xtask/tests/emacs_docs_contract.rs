// Integration test: assertion helpers (`expect`/`unwrap`/`panic!`) carry the
// failure message. The workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used, clippy::panic)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const EMACS_SUBJECT_MANIFEST: &str = ".ci/editor-clients/emacs-subjects.v1.json";
// The manifest pins the exact upstream-source bytes and version. The pinned
// source header's Package-Requires field supplies this audited minimum until
// the subject schema carries source dependency metadata directly.
const PINNED_SOURCE_MINIMUM_EMACS: &str = "29.1";

#[derive(Debug)]
struct LspModeSubject {
    version: String,
    minimum_emacs: Option<String>,
}

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

fn normalize_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn lsp_mode_subject(source_state: &str) -> LspModeSubject {
    let manifest: Value = serde_json::from_str(&read(EMACS_SUBJECT_MANIFEST))
        .expect("the checked Emacs subject manifest must be valid JSON");
    let subjects = manifest
        .get("subjects")
        .and_then(Value::as_array)
        .expect("the checked Emacs subject manifest must contain subjects[]");
    let matching = subjects
        .iter()
        .filter(|subject| {
            subject.get("client_kind").and_then(Value::as_str) == Some("lsp_mode")
                && subject.get("source_state").and_then(Value::as_str) == Some(source_state)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        matching.len(),
        1,
        "the checked manifest must contain exactly one lsp-mode {source_state} subject"
    );

    let subject = matching[0];
    LspModeSubject {
        version: subject
            .get("client_version_hint")
            .and_then(Value::as_str)
            .expect("the lsp-mode subject must declare client_version_hint")
            .to_owned(),
        minimum_emacs: subject
            .get("external_package")
            .and_then(|package| package.get("minimum_emacs"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

#[test]
fn active_emacs_guide_separates_released_and_source_lsp_mode_subjects() {
    let guide = normalize_whitespace(&read("docs/EDITORS/EMACS_SETUP.md"));
    let released = lsp_mode_subject("released");
    let source = lsp_mode_subject("upstream_source");
    let released_minimum = released
        .minimum_emacs
        .as_deref()
        .expect("the released lsp-mode subject must declare minimum_emacs");

    let released_sentence = format!(
        "Released MELPA Stable `lsp-mode` {} declares Emacs {} or later.",
        released.version, released_minimum
    );
    let source_sentence = format!(
        "The pinned upstream-source subject reports `lsp-mode` {} and declares Emacs {} or later.",
        source.version, PINNED_SOURCE_MINIMUM_EMACS
    );
    let source_is_not_release = format!(
        "The source header is not a released `lsp-mode` {} package.",
        source.version
    );
    let emacs_28_release_boundary = format!(
        "For package metadata only, Emacs 28.1 and 28.2 fall within the released {} line's declared range;",
        released.version
    );
    let stale_source_as_package = format!(
        "current `lsp-mode` {} requires Emacs {}",
        source.version, PINNED_SOURCE_MINIMUM_EMACS
    );
    let stale_tested_package_line = format!(
        "For the currently tested package line, `lsp-mode` {} requires Emacs {}.",
        source.version, PINNED_SOURCE_MINIMUM_EMACS
    );

    assert!(
        guide.contains(&released_sentence),
        "the active guide must project the released lsp-mode version and minimum from the checked subject manifest"
    );
    assert!(
        guide.contains(&source_sentence),
        "the active guide must identify the pinned upstream-source subject separately"
    );
    assert!(
        guide.contains(&source_is_not_release),
        "a source header version must not be rendered as a released package identity"
    );
    assert!(
        guide.contains(&emacs_28_release_boundary),
        "the source-head minimum must not erase the released Emacs 28.1/28.2 package boundary"
    );
    assert!(
        guide.contains(
            "These package metadata bounds do not by themselves prove the complete `perllsp` client journey."
        ),
        "package compatibility must remain separate from actual-client support evidence"
    );
    assert!(
        !guide.contains(&stale_source_as_package),
        "the upstream-source row must not return as the unqualified current package line"
    );
    assert!(
        !guide.contains(&stale_tested_package_line),
        "the pinned source subject must not be relabeled as the currently tested package line"
    );
    assert!(
        !guide.contains(
            "If you use Emacs 28 or older, install Eglot separately or use `lsp-mode`."
        ),
        "Emacs 27 and Emacs 28.1/28.2 have different current package boundaries"
    );
}

#[test]
fn active_emacs_guide_keeps_manual_and_stock_discovery_distinct() {
    let guide = normalize_whitespace(&read("docs/EDITORS/EMACS_SETUP.md"));

    assert!(
        guide.contains(
            "Current stock Eglot does not yet discover `perllsp` automatically for Perl"
        ),
        "Eglot manual registration must not be rendered as stock discovery"
    );
    assert!(
        guide.contains("Current stock `lsp-mode` does not yet ship a built-in `perllsp` client"),
        "lsp-mode manual registration must not be rendered as built-in discovery"
    );
    assert!(
        guide.contains(
            "Treat `:priority` as a default selection mechanism, not as a value to increase indefinitely."
        ),
        "wrong-server troubleshooting should identify client ownership instead of escalating priority forever"
    );
}
