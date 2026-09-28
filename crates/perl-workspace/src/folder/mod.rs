//! Workspace folder URI/path parsing.
//!
//! Converts workspace folder entries into local filesystem paths with
//! deterministic behavior for both plain paths and `file://` URIs.
//! Relative and non-filesystem `workspaceFolders` values are rejected
//! rather than manufactured into `file:///` URIs.

mod admission;

use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use perl_uri::uri_to_fs_path;
use serde_json::Value;

pub use admission::{
    WorkspaceFolderAdmission, WorkspaceFolderRejection, WorkspaceFolderRejectionKind,
    admit_workspace_folder_uris, classify_workspace_folder_entry,
};

/// URI lists extracted from an LSP workspace folder change event.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceFolderChange {
    /// Added workspace folder URIs.
    pub added: Vec<String>,
    /// Removed workspace folder URIs.
    pub removed: Vec<String>,
}

/// Parse a workspace folder declaration into a filesystem path.
///
/// Workspace folders can be passed as absolute paths or `file://` URIs. For
/// `file://` URIs this attempts to resolve through `perl_uri::uri_to_fs_path`.
/// If URI resolution fails for a local file URI, the scheme prefix is trimmed
/// and the remainder is interpreted as a path fallback. Remote file URI hosts
/// are preserved as raw input instead of being converted into filesystem paths.
#[must_use]
pub fn workspace_folder_to_path(workspace_folder: &str) -> PathBuf {
    let workspace_folder = workspace_folder.trim();

    if has_file_uri_scheme(workspace_folder) {
        // A URI with a non-local host (e.g. `file://evil.example.com/path`)
        // must not be passed to platform URI conversion first: on Windows that
        // can produce a UNC path like `\\evil.example.com\path`.
        if file_uri_has_remote_host(workspace_folder) {
            return PathBuf::from(workspace_folder);
        }

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = uri_to_fs_path(workspace_folder) {
            return path;
        }

        if let Some(path) = parse_file_uri_fallback(workspace_folder) {
            return path;
        }

        return PathBuf::from(trim_file_uri_prefix(workspace_folder));
    }

    PathBuf::from(workspace_folder)
}

pub(super) fn has_file_uri_scheme(value: &str) -> bool {
    value.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
}

fn trim_file_uri_prefix(value: &str) -> &str {
    let suffix = &value[5..];
    suffix.strip_prefix("//").unwrap_or(suffix)
}

/// Returns `true` when `value` is a `file://` URI whose authority component
/// names a non-local host (i.e. something other than empty or `"localhost"`).
///
/// Used to block the `trim_file_uri_prefix` last-resort path in
/// [`workspace_folder_to_path`] so that remote hostnames cannot leak into the
/// returned `PathBuf`.
pub(super) fn file_uri_has_remote_host(value: &str) -> bool {
    url::Url::parse(value)
        .ok()
        .filter(|u| u.scheme() == "file")
        .and_then(|u| u.host_str().map(|h| !is_local_file_host(h)))
        .unwrap_or(false)
}

fn is_local_file_host(host: &str) -> bool {
    let normalized = host
        .trim_end_matches('.')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    matches!(normalized.as_str(), "" | "localhost" | "127.0.0.1" | "::1")
}

fn parse_file_uri_fallback(workspace_folder: &str) -> Option<PathBuf> {
    let parsed = url::Url::parse(workspace_folder).ok()?;
    if parsed.scheme() != "file" {
        return None;
    }

    if let Ok(path) = parsed.to_file_path() {
        return Some(path);
    }

    let path = parsed.path();
    if path.is_empty() {
        return None;
    }

    match parsed.host_str() {
        None => Some(PathBuf::from(path)),
        Some(host) if is_local_file_host(host) => Some(PathBuf::from(path)),
        Some(_) => None,
    }
}

/// Extract admitted workspace folder URIs from an LSP `workspaceFolders` array.
///
/// Non-entries (wrong JSON type, name-only objects) are ignored. Relative
/// paths and non-filesystem URIs are omitted rather than manufactured into
/// `file:///` roots. Request handlers that can return `-32602` should use
/// [`admit_workspace_folder_uris`] so those rejections are typed.
#[must_use]
pub fn extract_workspace_folder_uris(workspace_folders: &[Value]) -> Vec<String> {
    workspace_folders
        .iter()
        .filter_map(|folder| match classify_workspace_folder_entry(folder) {
            WorkspaceFolderAdmission::Admitted(uri) => Some(uri),
            WorkspaceFolderAdmission::Ignored | WorkspaceFolderAdmission::Rejected(_) => None,
        })
        .collect()
}

/// Extract URI changes from an LSP `workspace/didChangeWorkspaceFolders` event payload.
///
/// Missing/invalid sections are treated as empty.
#[must_use]
pub fn extract_workspace_folder_change(event: &Value) -> WorkspaceFolderChange {
    let added = event
        .get("added")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |entries| extract_workspace_folder_uris(entries));

    let removed = event
        .get("removed")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |entries| extract_workspace_folder_uris(entries));

    WorkspaceFolderChange { added, removed }
}

/// Convert a legacy LSP `rootPath` string to a `file://` URI.
///
/// Absolute POSIX and Windows-style paths convert honestly. Relative
/// `rootPath` values keep the historical conversion used by deprecated
/// initialize `rootPath` callers. `workspaceFolders` admission never uses
/// this function for relative paths.
#[must_use]
pub fn root_path_to_file_uri(root_path: &str) -> String {
    if has_file_uri_scheme(root_path) {
        return root_path.to_string();
    }

    admission::try_absolute_path_to_file_uri(root_path).unwrap_or_else(|| {
        let normalized = root_path.replace('\\', "/");
        let pseudo_absolute = format!("/{normalized}");
        url::Url::from_file_path(std::path::Path::new(&pseudo_absolute))
            .map_or_else(|_| format!("file:///{normalized}"), |uri| uri.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::{
        WorkspaceFolderAdmission, WorkspaceFolderRejectionKind, admit_workspace_folder_uris,
        classify_workspace_folder_entry, extract_workspace_folder_change,
        extract_workspace_folder_uris, root_path_to_file_uri, workspace_folder_to_path,
    };
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn parses_plain_folder_path() {
        assert_eq!(workspace_folder_to_path("/tmp/project"), PathBuf::from("/tmp/project"));
    }

    #[test]
    fn trims_surrounding_whitespace_for_plain_paths() {
        assert_eq!(workspace_folder_to_path("  /tmp/project  "), PathBuf::from("/tmp/project"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parses_file_uri_when_possible() {
        let parsed = workspace_folder_to_path("file:///tmp/project");
        assert!(parsed.to_string_lossy().contains("tmp"));
        assert!(parsed.to_string_lossy().contains("project"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parses_uppercase_file_uri_when_possible() {
        let parsed = workspace_folder_to_path("FILE:///tmp/project");
        assert!(parsed.to_string_lossy().contains("tmp"));
        assert!(parsed.to_string_lossy().contains("project"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parses_single_slash_file_uri_when_possible() {
        let parsed = workspace_folder_to_path("file:/tmp/project");
        assert_eq!(parsed, PathBuf::from("/tmp/project"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parses_uppercase_single_slash_file_uri_when_possible() {
        let parsed = workspace_folder_to_path("FILE:/tmp/project");
        assert_eq!(parsed, PathBuf::from("/tmp/project"));
    }

    #[test]
    fn parses_localhost_file_uri_without_leaking_host_component() {
        let parsed = workspace_folder_to_path("file://localhost/tmp/project");
        let path = parsed.to_string_lossy();
        assert!(path.contains("tmp"));
        assert!(path.contains("project"));
        assert!(!path.contains("localhost/tmp"));
    }

    #[test]
    fn root_path_preserves_single_slash_file_uri() {
        assert_eq!(root_path_to_file_uri("file:/tmp/project"), "file:/tmp/project");
    }

    #[test]
    fn trims_surrounding_whitespace_for_file_uri_inputs() {
        let parsed = workspace_folder_to_path("  file:///tmp/project  ");
        assert_eq!(parsed, PathBuf::from("/tmp/project"));
    }

    #[test]
    fn parses_percent_encoded_file_uri_path_segment() {
        let parsed = workspace_folder_to_path("file:///tmp/my%20project");
        assert_eq!(parsed, PathBuf::from("/tmp/my project"));
    }

    #[test]
    fn extracts_workspace_uris() {
        let entries = vec![
            json!({"uri": "file:///one"}),
            json!({"uri": "file:///two"}),
            json!({"path": "/three"}),
            json!("file:///four"),
            json!({"name": "invalid"}),
        ];
        let uris = extract_workspace_folder_uris(&entries);
        assert_eq!(uris, vec!["file:///one", "file:///two", "file:///three", "file:///four"]);
    }

    #[test]
    fn relative_path_is_not_manufactured_into_a_file_uri() {
        let entries = vec![json!({"path": "relative/rel2", "name": "rel2"})];
        let extracted = extract_workspace_folder_uris(&entries);
        assert!(
            extracted.iter().all(|uri| !uri.starts_with("file:///relative")),
            "relative path must not become file:///relative/rel2, got {extracted:?}"
        );
        assert!(extracted.is_empty(), "relative path must not be registered, got {extracted:?}");

        let rejection = admit_workspace_folder_uris(&entries).expect_err("relative path");
        assert_eq!(rejection.kind, WorkspaceFolderRejectionKind::RelativePath);
        assert_eq!(rejection.input, "relative/rel2");
        assert!(
            !rejection.message().contains("file:///relative/rel2"),
            "rejection must not advertise a manufactured URI: {}",
            rejection.message()
        );

        // The deprecated rootPath helper may still convert relative strings;
        // workspaceFolders admission must not consult that branch.
        let legacy = root_path_to_file_uri("relative/rel2");
        assert!(
            legacy.starts_with("file:"),
            "legacy rootPath helper remains available for initialize.rootPath, got {legacy}"
        );
        assert!(extract_workspace_folder_uris(&entries).is_empty());
        assert!(admit_workspace_folder_uris(&entries).is_err());
    }

    #[test]
    fn relative_uri_string_is_not_passed_through_as_a_root() {
        let entries = vec![json!({"uri": "just/a/relative/uri", "name": "rel3"})];
        assert!(extract_workspace_folder_uris(&entries).is_empty());
        let rejection = admit_workspace_folder_uris(&entries).expect_err("relative uri");
        assert_eq!(rejection.kind, WorkspaceFolderRejectionKind::RelativeUri);
    }

    #[test]
    fn non_filesystem_schemes_are_rejected_instead_of_registered() {
        for (uri, kind) in [
            ("untitled:Untitled-1", WorkspaceFolderRejectionKind::NonFilesystemScheme),
            ("git:///repo", WorkspaceFolderRejectionKind::NonFilesystemScheme),
            (
                "vscode-remote://ssh-remote+host/workspace",
                WorkspaceFolderRejectionKind::NonFilesystemScheme,
            ),
            ("file://evil.example.com/share/project", WorkspaceFolderRejectionKind::RemoteFileHost),
        ] {
            let entries = vec![json!({"uri": uri, "name": "virtual"})];
            assert!(
                extract_workspace_folder_uris(&entries).is_empty(),
                "{uri} must not be extracted as a filesystem root"
            );
            let rejection =
                admit_workspace_folder_uris(&entries).expect_err("non-filesystem folder");
            assert_eq!(rejection.kind, kind, "uri={uri}");
            assert_eq!(rejection.input, uri);
        }
    }

    #[test]
    fn mixed_valid_and_relative_folders_fail_admission_without_keeping_the_valid_root() {
        let entries = vec![
            json!({"uri": "file:///tmp/ws", "name": "ws"}),
            json!({"path": "relative/rel2", "name": "rel2"}),
        ];
        let extracted = extract_workspace_folder_uris(&entries);
        assert_eq!(extracted, vec!["file:///tmp/ws"]);

        let rejection = admit_workspace_folder_uris(&entries).expect_err("mixed folders");
        assert_eq!(rejection.kind, WorkspaceFolderRejectionKind::RelativePath);
    }

    #[test]
    fn name_only_and_non_entry_shapes_stay_ignored_not_invalid_params() {
        let entries = vec![json!({"name": "no-uri-here"}), json!(42), json!(null)];
        assert!(extract_workspace_folder_uris(&entries).is_empty());
        let admitted = admit_workspace_folder_uris(&entries).expect("ignored non-entries");
        assert!(admitted.is_empty());
        assert!(matches!(
            classify_workspace_folder_entry(&json!({"name": "no-uri-here"})),
            WorkspaceFolderAdmission::Ignored
        ));
    }

    #[test]
    fn dotted_and_parent_relative_paths_are_rejected() {
        for path in [".", "..", "./lib", "../outside"] {
            let rejection = admit_workspace_folder_uris(&[json!({"path": path})])
                .expect_err("dotted relative path");
            assert_eq!(rejection.kind, WorkspaceFolderRejectionKind::RelativePath);
            assert_eq!(rejection.input, path);
        }
    }

    #[test]
    fn empty_uri_is_rejected() {
        let rejection =
            admit_workspace_folder_uris(&[json!({"uri": "   "})]).expect_err("empty uri");
        assert_eq!(rejection.kind, WorkspaceFolderRejectionKind::Empty);
    }

    #[test]
    fn relative_file_scheme_without_absolute_path_is_rejected() {
        let rejection = admit_workspace_folder_uris(&[json!({"uri": "file:relative/rel2"})])
            .expect_err("relative file URI");
        assert_eq!(rejection.kind, WorkspaceFolderRejectionKind::RelativeUri);
        assert!(extract_workspace_folder_uris(&[json!("file:relative/rel2")]).is_empty());
    }

    #[test]
    fn absolute_file_uri_that_happens_to_look_like_a_relative_name_is_still_admitted() {
        // A client that actually sent file:///relative/rel2 named an absolute
        // path /relative/rel2. That is not the manufacturing bug.
        let admitted = admit_workspace_folder_uris(&[json!({"uri": "file:///relative/rel2"})])
            .expect("absolute file URI");
        assert_eq!(admitted, vec!["file:///relative/rel2"]);
    }

    #[test]
    fn relative_root_path_helper_still_manufactures_for_legacy_initialize() {
        let uri = root_path_to_file_uri("relative/rel2");
        assert!(uri.starts_with("file:"), "legacy rootPath conversion, got {uri}");
        assert_ne!(uri, "relative/rel2");
    }

    #[test]
    fn unc_style_path_is_rejected_as_a_remote_file_host() {
        // `Url::from_file_path` can rewrite `//host/share` into
        // `file://host/share`. Admission must reject that as remote, not
        // manufacture a filesystem root.
        for path in ["//evil.example.com/share/project", r"\\evil.example.com\share\project"] {
            let entries = vec![json!({"path": path, "name": "unc"})];
            assert!(
                extract_workspace_folder_uris(&entries).is_empty(),
                "{path} must not become a workspace root"
            );
            let rejection = admit_workspace_folder_uris(&entries).expect_err("UNC path");
            assert_eq!(
                rejection.kind,
                WorkspaceFolderRejectionKind::RemoteFileHost,
                "UNC-style path {path} must be classified as a remote host"
            );
            assert_eq!(rejection.input, path);
            assert!(
                !rejection.message().contains("file://evil.example.com"),
                "rejection must not advertise a manufactured URI: {}",
                rejection.message()
            );
        }
    }

    #[test]
    fn string_form_uri_passes_through_without_normalization() {
        // Value::String arm passes the string through as-is, matching the behavior
        // of the Value::Object{"uri": ...} arm which also does not normalize.
        let entries = vec![json!("file:///a/b/c"), json!("file:///C:/Users/foo")];
        let uris = extract_workspace_folder_uris(&entries);
        assert_eq!(uris, vec!["file:///a/b/c", "file:///C:/Users/foo"]);
    }

    #[test]
    fn non_file_and_non_object_entries_are_dropped() {
        // Null, arrays, booleans, and numbers should all be silently skipped.
        let entries = vec![json!(null), json!(42), json!(true), json!([])];
        let uris = extract_workspace_folder_uris(&entries);
        assert!(uris.is_empty(), "expected empty result, got {uris:?}");
    }

    #[test]
    fn object_uri_key_takes_precedence_over_path_key() {
        // When an object contains both "uri" and "path", "uri" wins.
        let entries = vec![json!({"uri": "file:///from-uri", "path": "/from-path"})];
        let uris = extract_workspace_folder_uris(&entries);
        assert_eq!(uris, vec!["file:///from-uri"]);
    }

    #[test]
    fn extracts_workspace_change_entries() {
        let change = extract_workspace_folder_change(&json!({
            "added": [{"uri": "file:///add"}],
            "removed": [{"uri": "file:///remove"}],
        }));

        assert_eq!(change.added, vec!["file:///add"]);
        assert_eq!(change.removed, vec!["file:///remove"]);
    }

    #[test]
    fn converts_legacy_root_path_to_file_uri() {
        let uri = root_path_to_file_uri("/legacy/workspace");
        assert_eq!(uri, "file:///legacy/workspace");
    }

    #[test]
    fn preserves_file_uri_root_path_input() {
        let uri = root_path_to_file_uri("file:///already/uri");
        assert_eq!(uri, "file:///already/uri");
    }

    #[test]
    fn encodes_spaces_in_windows_style_root_path() {
        let uri = root_path_to_file_uri(r"C:\Users\me\My Project");
        assert_eq!(uri, "file:///C:/Users/me/My%20Project");
    }

    #[test]
    fn preserves_uppercase_file_uri_root_path_input() {
        let uri = root_path_to_file_uri("FILE:///already/uri");
        assert_eq!(uri, "FILE:///already/uri");
    }

    #[test]
    fn parses_file_uri_with_localhost_authority() {
        let parsed = workspace_folder_to_path("file://localhost/tmp/project");
        assert!(parsed.to_string_lossy().contains("tmp"));
        assert!(parsed.to_string_lossy().contains("project"));
    }

    #[test]
    fn parses_file_uri_with_localhost_variants() {
        for uri in [
            "file://LOCALHOST/tmp/project",
            "file://localhost./tmp/project",
            "file://127.0.0.1/tmp/project",
            "file://[::1]/tmp/project",
        ] {
            let parsed = workspace_folder_to_path(uri);
            assert!(!parsed.to_string_lossy().contains("file://"), "uri leaked: {uri}");
        }
    }

    #[test]
    fn does_not_generate_unc_path_for_non_local_file_uri_host() {
        let parsed = workspace_folder_to_path("file://evil.example.com/share/project");
        let path = parsed.to_string_lossy();
        // Must not contain the remote hostname in any form — neither as a UNC-style
        // `//evil.example.com/...` prefix nor as a plain leading component
        // `evil.example.com/...` (which `trim_file_uri_prefix` would previously
        // produce after stripping the `//`).
        assert!(
            !path.starts_with("//evil.example.com") && !path.starts_with("evil.example.com"),
            "remote hostname leaked into path: {path}"
        );
    }

    #[test]
    fn does_not_resolve_remote_host_with_path_component() {
        // Ensure the trim_file_uri_prefix last-resort path is also blocked for
        // URIs that url::Url cannot convert to a file path (remote host present).
        for uri in &[
            "file://attacker.example.org/sensitive/data",
            "file://192.0.2.1/share",
            "file://[::1]/ipv6-local",
        ] {
            let parsed = workspace_folder_to_path(uri);
            let path = parsed.to_string_lossy();
            // The raw URI itself should be the fallback — the remote hostname
            // must not appear as a bare leading path component.
            assert!(
                !path.starts_with("attacker.example.org")
                    && !path.starts_with("192.0.2.1")
                    && !path.starts_with("[::1]")
                    && !path.starts_with("::1"),
                "remote hostname leaked into path for {uri}: {path}"
            );
        }
    }
}
