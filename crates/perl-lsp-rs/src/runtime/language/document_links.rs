//! Document link handlers.
//!
//! Keeps document-link feature logic isolated from other language handlers.

use super::super::document_access::UserAnswerTextLookup;
use super::super::{INVALID_PARAMS, INVALID_REQUEST, JsonRpcError, LspServer, Value, json};
use crate::documentation_targets::PerlDocumentationTarget;
use crate::protocol::req_uri;
use std::borrow::Cow;
use std::path::Path;

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

fn resolve_file_link_target(base_uri: &str, file_path: &str) -> Option<String> {
    if file_path.starts_with("//") {
        return url::Url::parse(&format!("file:{file_path}")).ok().map(|url| url.to_string());
    }

    if is_windows_drive_path(file_path) {
        return url::Url::parse(&format!("file:///{file_path}")).ok().map(|url| url.to_string());
    }

    if Path::new(file_path).is_absolute()
        && let Ok(target_url) = url::Url::from_file_path(file_path)
    {
        return Some(target_url.to_string());
    }

    let base_url = url::Url::parse(base_uri).ok()?;
    if let Ok(target_url) = base_url.join(file_path) {
        return Some(target_url.to_string());
    }

    if let Ok(base_path) = base_url.to_file_path()
        && let Some(parent) = base_path.parent()
    {
        let resolved = parent.join(file_path);
        if let Ok(target_url) = url::Url::from_file_path(&resolved) {
            return Some(target_url.to_string());
        }
    }

    None
}

fn normalize_document_link_file_path(file_path: &str) -> Cow<'_, str> {
    if !file_path.contains(['\\', '/']) {
        return Cow::Borrowed(file_path);
    }

    let preserve_unc_prefix = file_path.starts_with("\\\\") || file_path.starts_with("//");
    let mut normalized = String::with_capacity(file_path.len());
    let mut chars = file_path.chars().peekable();

    if preserve_unc_prefix {
        normalized.push_str("//");
        while matches!(chars.peek(), Some('\\' | '/')) {
            chars.next();
        }
    }

    let mut saw_separator = false;

    for ch in chars {
        let is_separator = ch == '\\' || ch == '/';
        if is_separator {
            if !saw_separator {
                normalized.push('/');
            }
        } else {
            normalized.push(ch);
        }
        saw_separator = is_separator;
    }

    Cow::Owned(normalized)
}

fn is_valid_pod_section_fragment(section: &str) -> bool {
    !section.is_empty()
        && section.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ' '))
}

impl LspServer {
    /// Handle textDocument/documentLink request
    pub(crate) fn handle_document_links(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(p) = params {
            let uri = p["textDocument"]["uri"].as_str().ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "Missing textDocument.uri".into(),
                data: None,
            })?;
            // Snapshot usable user-answer text under lock, then release before
            // workspace_roots() / compute_links. Predecessor text stays stored
            // as evidence; it must not be copied into a current-answer result.
            let lookup = self.lookup_user_answer_text(uri);
            let snapshot = match lookup {
                UserAnswerTextLookup::NotOpen => {
                    return Err(JsonRpcError {
                        code: INVALID_REQUEST,
                        message: format!("Document not open: {}", uri),
                        data: None,
                    });
                }
                UserAnswerTextLookup::Unavailable => {
                    return Ok(Some(json!([])));
                }
                UserAnswerTextLookup::Current(snapshot) => snapshot,
            };
            // documents lock released here

            let roots = self.workspace_roots();
            let links = crate::document_links::compute_links(uri, &snapshot.text, &roots);
            Ok(Some(self.publish_user_answer_value(
                uri,
                snapshot.generation,
                json!(links),
                json!([]),
            )))
        } else {
            Ok(Some(json!([])))
        }
    }

    /// Handle documentLink/resolve request
    ///
    /// Resolves a document link by filling in the target URI based on the data field.
    /// This allows the initial documentLink response to defer expensive resolution
    /// until the user actually hovers over or clicks the link.
    pub(crate) fn handle_document_link_resolve(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(mut link) = params {
            // Extract data field to determine link type
            let data = link.get("data").cloned();

            // If link already has a target, return as-is (already resolved)
            if link.get("target").and_then(|t| t.as_str()).is_some() {
                return Ok(Some(link));
            }

            // Resolve based on data field
            if let Some(data_obj) = data {
                let link_type = data_obj.get("type").and_then(|t| t.as_str());

                match link_type {
                    Some("module") => {
                        // Module reference - resolve to file path or MetaCPAN
                        let module_name = data_obj
                            .get("module")
                            .and_then(|m| m.as_str())
                            .ok_or_else(|| JsonRpcError {
                                code: INVALID_PARAMS,
                                message: "Missing module name in data".into(),
                                data: None,
                            })?;

                        let documentation_target = PerlDocumentationTarget::new(module_name)
                            .ok_or_else(|| JsonRpcError {
                                code: INVALID_PARAMS,
                                message: "Invalid module name in data".into(),
                                data: Some(json!({"module": module_name})),
                            })?;

                        // Try to resolve module to local file
                        if let Some(target) = self.resolve_module_to_path(module_name) {
                            link["target"] = json!(target);
                        } else {
                            // Fallback to MetaCPAN
                            link["target"] = json!(documentation_target.metacpan_pod_uri());
                        }
                    }
                    Some("file") => {
                        // File reference - resolve to absolute path
                        let file_path =
                            data_obj.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
                                JsonRpcError {
                                    code: INVALID_PARAMS,
                                    message: "Missing file path in data".into(),
                                    data: None,
                                }
                            })?;
                        let normalized_file_path = normalize_document_link_file_path(file_path);

                        let base_uri = data_obj
                            .get("baseUri")
                            .and_then(|u| u.as_str())
                            .ok_or_else(|| JsonRpcError {
                                code: INVALID_PARAMS,
                                message: "Missing base URI in data".into(),
                                data: None,
                            })?;

                        if let Some(target) =
                            resolve_file_link_target(base_uri, normalized_file_path.as_ref())
                        {
                            link["target"] = json!(target);
                        }
                    }
                    Some("url") => {
                        // URL reference - already resolved, just copy from data
                        if let Some(url) = data_obj.get("url").and_then(|u| u.as_str()) {
                            link["target"] = json!(url);
                        }
                    }
                    Some("pod_section") => {
                        let section =
                            data_obj.get("section").and_then(|s| s.as_str()).ok_or_else(|| {
                                JsonRpcError {
                                    code: INVALID_PARAMS,
                                    message: "Missing POD section in data".into(),
                                    data: None,
                                }
                            })?;
                        if !is_valid_pod_section_fragment(section) {
                            return Err(JsonRpcError {
                                code: INVALID_PARAMS,
                                message: "Invalid POD section in data".into(),
                                data: Some(json!({"section": section})),
                            });
                        }

                        let base_uri = data_obj
                            .get("baseUri")
                            .and_then(|u| u.as_str())
                            .ok_or_else(|| JsonRpcError {
                                code: INVALID_PARAMS,
                                message: "Missing base URI in data".into(),
                                data: None,
                            })?;
                        let mut target_url =
                            url::Url::parse(base_uri).map_err(|_| JsonRpcError {
                                code: INVALID_PARAMS,
                                message: "Invalid base URI in data".into(),
                                data: Some(json!({"baseUri": base_uri})),
                            })?;
                        target_url.set_fragment(Some(section));
                        link["target"] = json!(target_url.to_string());
                    }
                    _ => {
                        // Unknown link type - return error
                        return Err(JsonRpcError {
                            code: INVALID_PARAMS,
                            message: "Unknown link type in data field".into(),
                            data: Some(json!({"linkType": link_type})),
                        });
                    }
                }
            }

            Ok(Some(link))
        } else {
            Err(JsonRpcError {
                code: INVALID_PARAMS,
                message: "Missing parameters for documentLink/resolve".into(),
                data: None,
            })
        }
    }

    /// Handle documentLink request (alternative)
    #[allow(dead_code)] // Alternative implementation
    pub(crate) fn handle_document_link(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params {
            let uri = req_uri(&params)?;

            let documents = self.documents_guard();
            if let Some(doc) = self.get_document(&documents, uri) {
                let Some(text) = doc.text_for_user_answers() else {
                    return Ok(Some(json!([])));
                };
                let uri_parsed = url::Url::parse(uri).map_err(|_| JsonRpcError {
                    code: -32602,
                    message: "Invalid URI".to_string(),
                    data: None,
                })?;
                match crate::lsp_document_link::collect_document_links(text, &uri_parsed) {
                    Ok(links) => Ok(Some(serde_json::to_value(links).map_err(|e| {
                        crate::protocol::internal_error(&format!(
                            "Failed to serialize document links: {}",
                            e
                        ))
                    })?)),
                    Err(_) => Ok(Some(Value::Null)),
                }
            } else {
                Ok(Some(Value::Null))
            }
        } else {
            Ok(Some(Value::Null))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::LspServer;
    use super::{
        is_valid_pod_section_fragment, normalize_document_link_file_path, resolve_file_link_target,
    };
    use serde_json::{Value, json};

    #[test]
    fn normalize_document_link_file_path_collapses_windows_separators() {
        assert_eq!(normalize_document_link_file_path(r"lib\\Thing.pm"), "lib/Thing.pm");
        assert_eq!(normalize_document_link_file_path(r"lib\Thing.pm"), "lib/Thing.pm");
        assert_eq!(normalize_document_link_file_path("lib/Thing.pm"), "lib/Thing.pm");
    }

    #[test]
    fn normalize_document_link_file_path_preserves_unc_prefix() {
        assert_eq!(
            normalize_document_link_file_path(r"\\server\share\Thing.pm"),
            "//server/share/Thing.pm"
        );
    }

    #[test]
    fn resolve_file_link_target_handles_windows_absolute_paths() {
        let resolved =
            resolve_file_link_target("file:///workspace/project/file.pl", "C:/Users/me/Thing.pm");
        assert_eq!(resolved, Some("file:///C:/Users/me/Thing.pm".to_string()));
    }

    #[test]
    fn resolve_file_link_target_handles_unc_paths() {
        let resolved = resolve_file_link_target(
            "file:///workspace/project/file.pl",
            "//server/share/lib/Thing.pm",
        );
        assert_eq!(resolved, Some("file://server/share/lib/Thing.pm".to_string()));
    }

    #[test]
    fn pod_section_fragments_reject_path_like_targets() {
        assert!(is_valid_pod_section_fragment("method_name"));
        assert!(is_valid_pod_section_fragment("method name"));
        assert!(!is_valid_pod_section_fragment(""));
        assert!(!is_valid_pod_section_fragment("Other/section"));
        assert!(!is_valid_pod_section_fragment("Other::section"));
    }

    fn document_link_modules(value: &Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let links = value.as_array().ok_or("documentLink must return an array")?;
        Ok(links
            .iter()
            .filter_map(|link| link.pointer("/data/module").and_then(Value::as_str))
            .map(str::to_string)
            .collect())
    }

    fn ranged_violation(uri: &str, version: i32) -> Value {
        json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 1 }
                },
                "text": "x"
            }]
        })
    }

    #[test]
    fn document_links_fail_closed_across_full_sync_violation_and_recover()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/desync_document_links.pl";
        let predecessor = "use PredLinkMod;\n";
        let recovered = "use RecoveredLinkMod;\n";

        server.test_apply_did_open(uri, predecessor, 1)?;
        let live = server
            .handle_document_links(Some(json!({ "textDocument": { "uri": uri } })))?
            .ok_or("live documentLink must return a result")?;
        let live_modules = document_link_modules(&live)?;
        assert!(
            live_modules.iter().any(|module| module == "PredLinkMod"),
            "live documentLink must return the current predecessor module: {live}"
        );

        server.handle_did_change(Some(ranged_violation(uri, 2)))?;
        let desync = server
            .handle_document_links(Some(json!({ "textDocument": { "uri": uri } })))?
            .ok_or("desync documentLink must return a result")?;
        let desync_modules = document_link_modules(&desync)?;
        assert!(
            desync_modules.is_empty(),
            "predecessor documentLink ranges must not publish as current: {desync}"
        );

        server.test_apply_did_change(uri, recovered, 3)?;
        let restored = server
            .handle_document_links(Some(json!({ "textDocument": { "uri": uri } })))?
            .ok_or("recovered documentLink must return a result")?;
        let restored_modules = document_link_modules(&restored)?;
        assert!(
            restored_modules.iter().any(|module| module == "RecoveredLinkMod"),
            "accepted full replacement must restore current documentLink: {restored}"
        );
        assert!(
            !restored_modules.iter().any(|module| module == "PredLinkMod"),
            "recovered documentLink must not keep predecessor module: {restored}"
        );
        Ok(())
    }

    #[test]
    fn document_links_do_not_publish_in_flight_predecessor_after_violation()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let uri = "file:///workspace/inflight_document_links.pl";
        let predecessor = "use PredLinkMod;\n";

        server.test_apply_did_open(uri, predecessor, 1)?;
        let snapshot = server
            .snapshot_user_answer_text(uri)
            .ok_or("open document must have a usable user-answer snapshot")?;
        let computed = crate::document_links::compute_links(uri, &snapshot.text, &[]);
        assert!(
            computed
                .iter()
                .any(|link| link.pointer("/data/module").and_then(Value::as_str)
                    == Some("PredLinkMod")),
            "in-flight compute must see the predecessor module: {computed:?}"
        );

        server.handle_did_change(Some(ranged_violation(uri, 2)))?;
        assert!(
            !server.user_answer_text_is_current(uri, snapshot.generation),
            "ranged violation must invalidate the captured user-answer generation"
        );
        let published =
            server.publish_user_answer_value(uri, snapshot.generation, json!(computed), json!([]));
        let published_modules = document_link_modules(&published)?;
        assert!(
            published_modules.is_empty(),
            "in-flight predecessor links must not publish after invalidation: {published}"
        );
        Ok(())
    }
}
