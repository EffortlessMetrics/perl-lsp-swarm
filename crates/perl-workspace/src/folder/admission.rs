//! Workspace-folder admission: only absolute local filesystem roots.
//!
//! `workspaceFolders` entries are client-declared index roots. Relative paths
//! and non-filesystem URIs must not be rewritten into `file:///` locations
//! that the server would then treat as filesystem truth.

use std::path::Path;

use serde_json::Value;

use super::{file_uri_has_remote_host, has_file_uri_scheme};

/// Outcome of classifying one `workspaceFolders` JSON entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceFolderAdmission {
    /// Absolute local filesystem URI, or an absolute path converted to one.
    Admitted(String),
    /// Not a folder entry (wrong JSON type, name-only object, non-string uri).
    Ignored,
    /// Relative or non-filesystem input that must not become a workspace root.
    Rejected(WorkspaceFolderRejection),
}

/// Why a workspace folder entry cannot be admitted as a filesystem root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkspaceFolderRejectionKind {
    /// Empty or whitespace-only uri/path.
    Empty,
    /// Relative `path` value (would previously be manufactured into `file:///`).
    RelativePath,
    /// Relative or non-URI `uri` value.
    RelativeUri,
    /// Scheme the server cannot index as a local filesystem tree.
    NonFilesystemScheme,
    /// `file://` URI whose authority is a remote host.
    RemoteFileHost,
}

/// A rejected workspace folder entry and the client-supplied input that caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFolderRejection {
    /// Classification of the rejection.
    pub kind: WorkspaceFolderRejectionKind,
    /// Trimmed client-supplied uri or path.
    pub input: String,
}

impl WorkspaceFolderRejection {
    fn new(kind: WorkspaceFolderRejectionKind, input: impl Into<String>) -> Self {
        Self { kind, input: input.into() }
    }

    /// Client-visible `-32602` message. Does not mention a manufactured URI.
    #[must_use]
    pub fn message(&self) -> String {
        match self.kind {
            WorkspaceFolderRejectionKind::Empty => "Workspace folder URI is empty".to_string(),
            WorkspaceFolderRejectionKind::RelativePath => {
                format!(
                    "Relative workspace folder path cannot be manufactured into a file URI: {}",
                    self.input
                )
            }
            WorkspaceFolderRejectionKind::RelativeUri => {
                format!("Workspace folder uri is not an absolute file URI: {}", self.input)
            }
            WorkspaceFolderRejectionKind::NonFilesystemScheme => {
                format!("Workspace folder URI is not a filesystem root: {}", self.input)
            }
            WorkspaceFolderRejectionKind::RemoteFileHost => {
                format!(
                    "Workspace folder file URI has a non-local host and is not a filesystem root: {}",
                    self.input
                )
            }
        }
    }
}

/// Classify one JSON value from an LSP `workspaceFolders` array.
#[must_use]
pub fn classify_workspace_folder_entry(folder: &Value) -> WorkspaceFolderAdmission {
    match folder {
        Value::String(uri) => classify_raw(uri, AsPath::Uri),
        Value::Object(_) => {
            if let Some(uri) = folder.get("uri").and_then(Value::as_str) {
                classify_raw(uri, AsPath::Uri)
            } else if let Some(path) = folder.get("path").and_then(Value::as_str) {
                classify_raw(path, AsPath::Path)
            } else {
                WorkspaceFolderAdmission::Ignored
            }
        }
        _ => WorkspaceFolderAdmission::Ignored,
    }
}

/// Admit every folder entry, failing on the first relative or non-filesystem value.
///
/// Ignored entries (wrong JSON type, name-only objects) are skipped, matching
/// the historical extract policy for non-entries. Rejected entries do not
/// become `file:///` URIs.
pub fn admit_workspace_folder_uris(
    folders: &[Value],
) -> Result<Vec<String>, WorkspaceFolderRejection> {
    let mut admitted = Vec::new();
    for folder in folders {
        match classify_workspace_folder_entry(folder) {
            WorkspaceFolderAdmission::Admitted(uri) => admitted.push(uri),
            WorkspaceFolderAdmission::Ignored => {}
            WorkspaceFolderAdmission::Rejected(rejection) => return Err(rejection),
        }
    }
    Ok(admitted)
}

#[derive(Clone, Copy)]
enum AsPath {
    Path,
    Uri,
}

fn classify_raw(input: &str, origin: AsPath) -> WorkspaceFolderAdmission {
    let input = input.trim();
    if input.is_empty() {
        return WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
            WorkspaceFolderRejectionKind::Empty,
            input,
        ));
    }

    if has_file_uri_scheme(input) {
        // Includes four-slash `file:////host/share` (UNC lives in the path,
        // authority is empty). Path-origin UNC below is not reachable here.
        return classify_file_uri(input);
    }

    // UNC-style paths (`\\server\share` or `//server/share`) convert to
    // `file://server/share` on some platforms. That is a remote-host file
    // URI, not a local filesystem root, so reject before conversion.
    if is_unc_style_path(input) {
        return WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
            WorkspaceFolderRejectionKind::RemoteFileHost,
            input,
        ));
    }

    if is_absolute_workspace_path(input) {
        return match try_absolute_path_to_file_uri(input) {
            Some(uri) if file_uri_has_remote_host(&uri) => WorkspaceFolderAdmission::Rejected(
                WorkspaceFolderRejection::new(WorkspaceFolderRejectionKind::RemoteFileHost, input),
            ),
            Some(uri) => WorkspaceFolderAdmission::Admitted(uri),
            None => WorkspaceFolderAdmission::Rejected(relative_rejection(origin, input)),
        };
    }

    if has_non_file_uri_scheme(input) {
        return WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
            WorkspaceFolderRejectionKind::NonFilesystemScheme,
            input,
        ));
    }

    WorkspaceFolderAdmission::Rejected(relative_rejection(origin, input))
}

fn relative_rejection(origin: AsPath, input: &str) -> WorkspaceFolderRejection {
    let kind = match origin {
        AsPath::Path => WorkspaceFolderRejectionKind::RelativePath,
        AsPath::Uri => WorkspaceFolderRejectionKind::RelativeUri,
    };
    WorkspaceFolderRejection::new(kind, input)
}

fn classify_file_uri(input: &str) -> WorkspaceFolderAdmission {
    // `file:relative/rel2` is not a hierarchical file URI. The `url` crate
    // still parses it with path `/relative/rel2`, which would reintroduce
    // the manufacturing bug if we admitted it.
    if input.get(5..).is_none_or(|suffix| !suffix.starts_with('/')) {
        return WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
            WorkspaceFolderRejectionKind::RelativeUri,
            input,
        ));
    }

    if file_uri_has_remote_host(input) {
        return WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
            WorkspaceFolderRejectionKind::RemoteFileHost,
            input,
        ));
    }

    match url::Url::parse(input) {
        Ok(parsed) if parsed.scheme().eq_ignore_ascii_case("file") => {
            let path = parsed.path();
            if path.is_empty() {
                return WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
                    WorkspaceFolderRejectionKind::Empty,
                    input,
                ));
            }
            // Four-slash `file:////host/share` has an empty authority, so
            // `file_uri_has_remote_host` does not fire. The host lives in the
            // path as `//host/share` — the same UNC root path-origin already
            // rejects before conversion.
            if is_unc_style_path(path) {
                return WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
                    WorkspaceFolderRejectionKind::RemoteFileHost,
                    input,
                ));
            }
            if path.starts_with('/') || is_windows_drive_absolute(path) {
                WorkspaceFolderAdmission::Admitted(input.to_string())
            } else {
                WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
                    WorkspaceFolderRejectionKind::RelativeUri,
                    input,
                ))
            }
        }
        _ => WorkspaceFolderAdmission::Rejected(WorkspaceFolderRejection::new(
            WorkspaceFolderRejectionKind::RelativeUri,
            input,
        )),
    }
}

fn has_non_file_uri_scheme(value: &str) -> bool {
    if perl_uri::is_special_scheme(value) {
        return true;
    }

    let Some(colon) = value.find(':') else {
        return false;
    };
    if colon == 0 {
        return false;
    }
    let scheme = &value[..colon];
    let valid_scheme = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .bytes()
            .skip(1)
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
    if !valid_scheme {
        return false;
    }
    // A single-letter scheme is a Windows drive, not a URI scheme.
    scheme.len() != 1
}

pub(super) fn try_absolute_path_to_file_uri(root_path: &str) -> Option<String> {
    if !is_absolute_workspace_path(root_path) {
        return None;
    }

    let path = Path::new(root_path);
    if let Ok(uri) = url::Url::from_file_path(path) {
        return Some(uri.to_string());
    }

    if root_path.starts_with('/') {
        return Some(format!("file://{root_path}"));
    }

    if is_windows_drive_absolute(root_path) {
        let normalized = root_path.replace('\\', "/");
        return Some(
            url::Url::from_file_path(Path::new(&format!("/{normalized}")))
                .map(|uri| uri.to_string())
                .unwrap_or_else(|_| format!("file:///{normalized}")),
        );
    }

    None
}

fn is_unc_style_path(value: &str) -> bool {
    // Windows UNC `\\server\share`, or POSIX-looking `//server/share`.
    // Do not treat extra-slash POSIX paths (`///tmp`) as UNC.
    value.starts_with(r"\\") || (value.starts_with("//") && !value.starts_with("///"))
}

fn is_absolute_workspace_path(value: &str) -> bool {
    Path::new(value).is_absolute()
        || is_windows_drive_absolute(value)
        // POSIX absolute paths (and the historical `/root` form) stay
        // absolute even when this code is compiled for Windows.
        || value.starts_with('/')
}

fn is_windows_drive_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}
