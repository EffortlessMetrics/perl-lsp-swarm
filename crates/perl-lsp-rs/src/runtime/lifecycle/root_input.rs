//! Typed classification of initialization root inputs (#8161).
//!
//! One explicit disposition per `initialize` request distinguishes the
//! provenance of the workspace root input, so downstream behavior never
//! conflates "the client declared folders", "the client declared no folders",
//! "the client declared nothing", and "the client used a legacy root".
//!
//! The classification is a pure parse: it performs no indexing, watcher
//! installation, configuration fetch, or provider work. #8945 owns what an
//! explicit empty/rootless session does after initialization; #8995 owns
//! later `workspace/didChangeWorkspaceFolders` transitions.

use serde_json::Value;

/// Where the initialize request's workspace root input came from.
///
/// The variants preserve input provenance as required by #8161: a present
/// non-empty `workspaceFolders` array is authoritative; present empty/null
/// arrays are explicit no-active-folder states; legacy `rootUri`/`rootPath`
/// apply only when `workspaceFolders` was genuinely omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InitialRootInput {
    /// `workspaceFolders` present with at least one entry (authoritative).
    /// Malformed entries are rejected by the canonical URI policy inside this
    /// disposition; they never move the request into another mode.
    ExplicitWorkspaceFolders,
    /// `workspaceFolders` present as `[]`: explicit no-active-folder state.
    /// Never falls through to legacy root authority or the process CWD.
    ExplicitEmptyWorkspaceFolders,
    /// `workspaceFolders` present as `null`: explicit no-active-folder state.
    /// Never falls through to legacy root authority or the process CWD.
    ExplicitNullWorkspaceFolders,
    /// `workspaceFolders` present but not an array or null (malformed shape).
    /// Rootless for the same reason as the explicit empty/null states: the
    /// field was declared, so the legacy fallback does not apply.
    MalformedWorkspaceFoldersShape,
    /// `workspaceFolders` omitted, legacy `rootUri` supplied.
    LegacyRootUri,
    /// `workspaceFolders` omitted, `rootUri` absent or null, legacy
    /// `rootPath` supplied.
    LegacyRootPath,
    /// No usable root input of any kind was declared by the client.
    NoWorkspaceRoot,
}

impl InitialRootInput {
    /// Stable receipt name for tests and diagnostics.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::ExplicitWorkspaceFolders => "explicit_workspace_folders",
            Self::ExplicitEmptyWorkspaceFolders => "explicit_empty_workspace_folders",
            Self::ExplicitNullWorkspaceFolders => "explicit_null_workspace_folders",
            Self::MalformedWorkspaceFoldersShape => "malformed_workspace_folders_shape",
            Self::LegacyRootUri => "legacy_root_uri",
            Self::LegacyRootPath => "legacy_root_path",
            Self::NoWorkspaceRoot => "no_workspace_root",
        }
    }

    /// Whether the client explicitly declared a no-active-folder session
    /// (empty array, null, or a malformed shape). Rootless-session runtime
    /// behavior belongs to #8945; this predicate only exposes the input fact.
    pub(crate) fn is_explicit_rootless(&self) -> bool {
        matches!(
            self,
            Self::ExplicitEmptyWorkspaceFolders
                | Self::ExplicitNullWorkspaceFolders
                | Self::MalformedWorkspaceFoldersShape
        )
    }
}

/// Classify the initialize request's root inputs once, per #8161.
///
/// Rules:
/// - a present non-empty `workspaceFolders` array is authoritative;
/// - a present empty array is explicit no-active-folder state and does not
///   fall through to `rootUri`/`rootPath`;
/// - a present null value is explicit no-active-folder state and does not
///   silently manufacture a legacy root;
/// - only a genuinely omitted `workspaceFolders` field may use the reviewed
///   legacy `rootUri`, then `rootPath`, fallback;
/// - the classification itself performs no provider work.
pub(crate) fn classify_initial_root_input(params: &Value) -> InitialRootInput {
    match params.get("workspaceFolders") {
        // Field present with a JSON array: presence decides the mode. Entry
        // validation stays with the canonical URI policy
        // (`perl_workspace::folder::extract_workspace_folder_uris`).
        Some(Value::Array(folders)) => {
            if folders.is_empty() {
                InitialRootInput::ExplicitEmptyWorkspaceFolders
            } else {
                InitialRootInput::ExplicitWorkspaceFolders
            }
        }
        // Field present but null: explicit no-active-folder input.
        Some(Value::Null) => InitialRootInput::ExplicitNullWorkspaceFolders,
        // Field present with any other shape: declared, but not a folder list.
        // This is malformed input, not permission to mine a legacy root.
        Some(_) => InitialRootInput::MalformedWorkspaceFoldersShape,
        // Field genuinely omitted: reviewed legacy fallback applies.
        None => {
            if params.get("rootUri").and_then(Value::as_str).is_some() {
                InitialRootInput::LegacyRootUri
            } else if params.get("rootPath").and_then(Value::as_str).is_some() {
                InitialRootInput::LegacyRootPath
            } else {
                InitialRootInput::NoWorkspaceRoot
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InitialRootInput, classify_initial_root_input};
    use serde_json::json;

    #[test]
    fn present_non_empty_workspace_folders_is_authoritative() {
        let params = json!({
            "workspaceFolders": [
                { "uri": "file:///a", "name": "a" },
                { "uri": "file:///b", "name": "b" }
            ],
            "rootUri": "file:///legacy"
        });

        let classified = classify_initial_root_input(&params);

        assert_eq!(classified, InitialRootInput::ExplicitWorkspaceFolders);
        assert!(!classified.is_explicit_rootless());
    }

    #[test]
    fn present_empty_array_is_explicit_rootless_even_with_a_legacy_root() {
        let params = json!({ "workspaceFolders": [], "rootUri": "file:///legacy" });

        let classified = classify_initial_root_input(&params);

        assert_eq!(classified, InitialRootInput::ExplicitEmptyWorkspaceFolders);
        assert!(classified.is_explicit_rootless());
    }

    #[test]
    fn present_null_is_explicit_rootless_even_with_a_legacy_root() {
        let params = json!({ "workspaceFolders": null, "rootUri": "file:///legacy" });

        let classified = classify_initial_root_input(&params);

        assert_eq!(classified, InitialRootInput::ExplicitNullWorkspaceFolders);
        assert!(classified.is_explicit_rootless());
    }

    #[test]
    fn present_malformed_shape_is_rootless_and_not_a_legacy_fallback() {
        // #8161 negative control 9: malformed folder input must not convert
        // into a successful unrelated fallback. The JSON type space has five
        // non-array, non-null shapes and a client bug can produce any of them
        // — a string was the only one covered, so an implementation that
        // matched on `Value::String` alone and let the rest fall through to
        // `rootUri` would have passed.
        for shape in [
            json!("file:///not-an-array"),
            json!(7),
            json!(true),
            json!({ "uri": "file:///a", "name": "a" }),
        ] {
            let classified = classify_initial_root_input(
                &json!({ "workspaceFolders": shape, "rootUri": "file:///legacy" }),
            );

            assert_eq!(
                classified,
                InitialRootInput::MalformedWorkspaceFoldersShape,
                "a declared-but-malformed folder field must not mine a legacy root: {shape}"
            );
            assert!(classified.is_explicit_rootless());
        }
    }

    #[test]
    fn a_legacy_root_is_refused_when_it_is_not_a_string() {
        // The fallback reads `rootUri`/`rootPath` through `as_str`, so a
        // non-string value is no root at all. Without this an implementation
        // that tested only for key presence would report `LegacyRootUri` and
        // then register nothing, leaving a receipt that names a root the
        // session does not have.
        let classified = classify_initial_root_input(&json!({ "rootUri": 42, "rootPath": [] }));

        assert_eq!(classified, InitialRootInput::NoWorkspaceRoot);
        assert!(!classified.is_explicit_rootless());
    }

    #[test]
    fn omitted_field_falls_back_to_root_uri_then_root_path() {
        let root_uri = classify_initial_root_input(&json!({ "rootUri": "file:///legacy" }));
        assert_eq!(root_uri, InitialRootInput::LegacyRootUri);
        assert!(!root_uri.is_explicit_rootless());

        let root_path =
            classify_initial_root_input(&json!({ "rootUri": null, "rootPath": "/legacy" }));
        assert_eq!(root_path, InitialRootInput::LegacyRootPath);

        let null_root_path = classify_initial_root_input(&json!({ "rootPath": "/legacy" }));
        assert_eq!(null_root_path, InitialRootInput::LegacyRootPath);
    }

    #[test]
    fn absent_inputs_are_no_workspace_root() {
        let classified = classify_initial_root_input(&json!({ "capabilities": {} }));

        assert_eq!(classified, InitialRootInput::NoWorkspaceRoot);
        assert!(!classified.is_explicit_rootless());
        assert_eq!(
            classified.as_str(),
            "no_workspace_root",
            "the receipt must not name a client-declared root the client never sent"
        );
    }

    #[test]
    fn dispositions_have_distinct_receipt_names() {
        let names = [
            InitialRootInput::ExplicitWorkspaceFolders,
            InitialRootInput::ExplicitEmptyWorkspaceFolders,
            InitialRootInput::ExplicitNullWorkspaceFolders,
            InitialRootInput::MalformedWorkspaceFoldersShape,
            InitialRootInput::LegacyRootUri,
            InitialRootInput::LegacyRootPath,
            InitialRootInput::NoWorkspaceRoot,
        ]
        .map(|disposition| disposition.as_str());

        for (index, name) in names.iter().enumerate() {
            for other in names.iter().skip(index + 1) {
                assert_ne!(name, other, "dispositions must keep distinct provenance names");
            }
        }
    }
}
