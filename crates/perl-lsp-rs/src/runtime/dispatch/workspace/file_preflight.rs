//! Pure workspace file-operation preflight planning.
//!
//! LSP `will*Files` requests may calculate edits from current facts, but they
//! are not commit notifications. In particular, `willRenameFiles` must not
//! remove or re-index workspace state before the client later sends
//! `didRenameFiles` (which is not guaranteed to arrive).

use crate::protocol::JsonRpcError;
use crate::runtime::LspServer;
use serde_json::{Value, json};

#[cfg(feature = "workspace")]
use crate::runtime::workspace::{module_name_appears_in_text, path_to_module_name};
#[cfg(feature = "workspace")]
use perl_module::rename::{apply_module_rename_edits, plan_module_rename_edits};
#[cfg(feature = "workspace")]
use std::collections::{BTreeMap, HashSet};

impl LspServer {
    /// Calculate module/file rename edits without mutating retained server state.
    pub(super) fn handle_will_rename_files_pure(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        #[cfg(feature = "workspace")]
        {
            let Some(files) =
                params.as_ref().and_then(|value| value.get("files")).and_then(Value::as_array)
            else {
                return Ok(Some(empty_workspace_edit()));
            };

            let mut planned_workspace_texts: BTreeMap<String, (String, String)> = BTreeMap::new();

            for file in files {
                let Some(old_uri) = file.get("oldUri").and_then(Value::as_str) else {
                    continue;
                };
                let Some(new_uri) = file.get("newUri").and_then(Value::as_str) else {
                    continue;
                };

                tracing::debug!(old_uri, new_uri, "Planning file rename preflight");
                let old_module = path_to_module_name(old_uri);
                let new_module = path_to_module_name(new_uri);
                if old_module.is_empty() || new_module.is_empty() {
                    continue;
                }

                plan_uri_rewrite(
                    self,
                    &mut planned_workspace_texts,
                    old_uri,
                    &old_module,
                    &new_module,
                );

                let dependents = self
                    .coordinator()
                    .map(|coordinator| coordinator.index().find_dependents(&old_module))
                    .unwrap_or_default();
                for dependent_uri in dependents {
                    plan_uri_rewrite(
                        self,
                        &mut planned_workspace_texts,
                        &dependent_uri,
                        &old_module,
                        &new_module,
                    );
                }

                warn_unhandled_open_document_references(
                    self,
                    old_uri,
                    &old_module,
                    &planned_workspace_texts,
                );
            }

            let mut workspace_edit = empty_workspace_edit();
            for (uri, (original_text, current_text)) in planned_workspace_texts {
                append_workspace_edits(
                    &mut workspace_edit,
                    &uri,
                    build_module_rename_workspace_edits(&original_text, &current_text),
                );
            }
            return Ok(Some(workspace_edit));
        }

        #[cfg(not(feature = "workspace"))]
        {
            let _ = params;
            Ok(Some(empty_workspace_edit()))
        }
    }
}

fn empty_workspace_edit() -> Value {
    json!({ "changes": {} })
}

#[cfg(feature = "workspace")]
fn plan_uri_rewrite(
    server: &LspServer,
    planned_workspace_texts: &mut BTreeMap<String, (String, String)>,
    uri: &str,
    old_module: &str,
    new_module: &str,
) {
    if !planned_workspace_texts.contains_key(uri) {
        let Some(text) = read_workspace_text(server, uri) else {
            return;
        };
        planned_workspace_texts.insert(uri.to_string(), (text.clone(), text));
    }

    let Some((_, current_text)) = planned_workspace_texts.get_mut(uri) else {
        return;
    };
    let planned = plan_module_rename_edits(current_text, old_module, new_module);
    if !planned.is_empty() {
        *current_text = apply_module_rename_edits(current_text, &planned);
    }
}

#[cfg(feature = "workspace")]
fn read_workspace_text(server: &LspServer, uri: &str) -> Option<String> {
    let normalized_uri = server.normalize_uri_key(uri);

    {
        let documents = server.documents.lock();
        if let Some(document) = documents.get(uri).or_else(|| documents.get(&normalized_uri)) {
            return Some(document.text_arc.to_string());
        }
    }

    if let Some(coordinator) = server.coordinator() {
        let index = coordinator.index();
        let document_store = index.document_store();
        if let Some(document) =
            document_store.get(uri).or_else(|| document_store.get(&normalized_uri))
        {
            return Some(document.text().to_string());
        }
    }

    perl_uri::uri_to_fs_path(uri)
        .and_then(|path| crate::util::read_text_file_with_encoding(&path).ok())
}

#[cfg(feature = "workspace")]
fn warn_unhandled_open_document_references(
    server: &LspServer,
    renamed_uri: &str,
    old_module: &str,
    planned_workspace_texts: &BTreeMap<String, (String, String)>,
) {
    let updated_uris: HashSet<&str> = planned_workspace_texts.keys().map(String::as_str).collect();
    let documents = server.documents.lock();
    let unhandled = documents.iter().any(|(uri, document)| {
        uri.as_str() != renamed_uri
            && !updated_uris.contains(uri.as_str())
            && module_name_appears_in_text(&document.text, old_module)
    });
    drop(documents);
    if unhandled {
        let message = format!(
            "Some references to '{old_module}' may not have been updated. \
             String literals, comments, and dynamic method calls \
             are not automatically rewritten. \
             Use find-and-replace to update them manually."
        );
        server.show_message_or_log(crate::runtime::window::MessageType::Warning, &message);
    }
}

#[cfg(feature = "workspace")]
fn append_workspace_edits(workspace_edit: &mut Value, uri: &str, mut edits: Vec<Value>) {
    if edits.is_empty() {
        return;
    }
    if let Some(existing) = workspace_edit["changes"][uri].as_array_mut() {
        existing.append(&mut edits);
    } else {
        workspace_edit["changes"][uri] = Value::Array(edits);
    }
}

#[cfg(feature = "workspace")]
fn build_module_rename_workspace_edits(original: &str, updated: &str) -> Vec<Value> {
    let original_lines: Vec<&str> = original.split('\n').collect();
    let updated_lines: Vec<&str> = updated.split('\n').collect();

    debug_assert_eq!(
        original_lines.len(),
        updated_lines.len(),
        "module rename planning should not change line counts"
    );

    original_lines
        .iter()
        .zip(updated_lines.iter())
        .enumerate()
        .filter_map(|(line, (old_line, new_line))| {
            if old_line == new_line {
                return None;
            }
            Some(json!({
                "range": {
                    "start": { "line": line, "character": 0 },
                    "end": { "line": line, "character": old_line.len() }
                },
                "newText": new_line
            }))
        })
        .collect()
}

#[cfg(all(test, feature = "workspace"))]
mod tests {
    use super::*;
    use std::error::Error;
    use std::sync::atomic::Ordering;
    use url::Url;

    type TestResult<T = ()> = Result<T, Box<dyn Error>>;

    #[derive(Debug, PartialEq, Eq)]
    struct RetainedStateFingerprint {
        open_documents: usize,
        indexed_files: usize,
        indexed_symbols: usize,
        old_file_symbols: usize,
        new_file_symbols: usize,
        old_document_stored: bool,
        new_document_stored: bool,
        pending_index_tasks: usize,
        indexing_invocations: usize,
    }

    fn fingerprint(
        server: &LspServer,
        old_uri: &str,
        new_uri: &str,
    ) -> TestResult<RetainedStateFingerprint> {
        let coordinator = server.coordinator().ok_or("workspace coordinator unavailable")?;
        let index = coordinator.index();
        Ok(RetainedStateFingerprint {
            open_documents: server.documents.lock().len(),
            indexed_files: index.file_count(),
            indexed_symbols: index.symbol_count(),
            old_file_symbols: index.file_symbols(old_uri).len(),
            new_file_symbols: index.file_symbols(new_uri).len(),
            old_document_stored: index.document_store().get(old_uri).is_some(),
            new_document_stored: index.document_store().get(new_uri).is_some(),
            pending_index_tasks: server.pending_index_task_count.load(Ordering::SeqCst),
            indexing_invocations: server.workspace_indexing_invocation_count.load(Ordering::SeqCst),
        })
    }

    fn indexed_rename_fixture() -> TestResult<(LspServer, tempfile::TempDir, String, String, String)>
    {
        let server = LspServer::new();
        let directory = tempfile::tempdir()?;
        let old_path = directory.path().join("OldModule.pm");
        let new_path = directory.path().join("NewModule.pm");
        let old_uri = Url::from_file_path(&old_path).map_err(|_| "invalid old path")?.to_string();
        let new_uri = Url::from_file_path(&new_path).map_err(|_| "invalid new path")?.to_string();
        let old_module = path_to_module_name(&old_uri);
        let source = format!("package {old_module};\nsub value {{ 1 }}\n1;\n");
        std::fs::write(&old_path, &source)?;
        server
            .coordinator()
            .ok_or("workspace coordinator unavailable")?
            .index()
            .index_file(Url::parse(&old_uri)?, source.clone())?;
        Ok((server, directory, old_uri, new_uri, source))
    }

    #[test]
    fn routed_will_rename_is_read_only_and_returns_edits() -> TestResult {
        let (server, _directory, old_uri, new_uri, _source) = indexed_rename_fixture()?;
        let before = fingerprint(&server, &old_uri, &new_uri)?;

        let result = server.handle_will_rename_files_dispatch(Some(json!({
            "files": [{ "oldUri": old_uri.clone(), "newUri": new_uri.clone() }]
        })))?;

        let after = fingerprint(&server, &old_uri, &new_uri)?;
        assert_eq!(after, before, "willRenameFiles must not commit retained state");
        let changes = result
            .as_ref()
            .and_then(|value| value.get("changes"))
            .and_then(Value::as_object)
            .ok_or("preflight must return a WorkspaceEdit changes map")?;
        assert!(
            changes.get(&old_uri).is_some_and(Value::is_array),
            "rename preflight should still return the package edit"
        );
        Ok(())
    }

    #[test]
    fn did_rename_is_the_commit_boundary() -> TestResult {
        let (server, directory, old_uri, new_uri, source) = indexed_rename_fixture()?;
        let old_path = directory.path().join("OldModule.pm");
        let new_path = directory.path().join("NewModule.pm");

        let _ = server.handle_will_rename_files_dispatch(Some(json!({
            "files": [{ "oldUri": old_uri.clone(), "newUri": new_uri.clone() }]
        })))?;
        assert!(
            !server
                .coordinator()
                .ok_or("workspace coordinator unavailable")?
                .index()
                .file_symbols(&old_uri)
                .is_empty()
        );

        std::fs::rename(&old_path, &new_path)?;
        std::fs::write(&new_path, source)?;
        server.handle_did_rename_files(Some(json!({
            "files": [{ "oldUri": old_uri.clone(), "newUri": new_uri.clone() }]
        })))?;

        let committed = fingerprint(&server, &old_uri, &new_uri)?;
        assert_eq!(committed.old_file_symbols, 0);
        assert!(!committed.old_document_stored);
        assert!(committed.new_file_symbols > 0);
        assert!(committed.new_document_stored);
        Ok(())
    }

    #[test]
    fn will_create_and_will_delete_leave_retained_state_unchanged() -> TestResult {
        let (server, directory, old_uri, new_uri, _source) = indexed_rename_fixture()?;
        let before_create = fingerprint(&server, &old_uri, &new_uri)?;
        let create_uri = Url::from_file_path(directory.path().join("Created.pm"))
            .map_err(|_| "invalid create path")?
            .to_string();
        let _ = server.handle_will_create_files(Some(json!({
            "files": [{ "uri": create_uri }]
        })))?;
        assert_eq!(fingerprint(&server, &old_uri, &new_uri)?, before_create);

        let before_delete = fingerprint(&server, &old_uri, &new_uri)?;
        let _ = server.handle_will_delete_files(Some(json!({
            "files": [{ "uri": old_uri.clone() }]
        })))?;
        assert_eq!(fingerprint(&server, &old_uri, &new_uri)?, before_delete);
        Ok(())
    }

    #[test]
    fn routed_preflight_implementation_has_no_commit_operations() {
        let source = include_str!("file_preflight.rs");
        let implementation = source.split("#[cfg(all(test").next().unwrap_or(source);
        for forbidden in [
            ".notify_change(",
            ".notify_parse_complete(",
            ".remove_file(",
            ".clear_file(",
            ".index_file(",
            "documents.remove(",
            "documents.insert(",
            "refresh_all(",
        ] {
            assert!(
                !implementation.contains(forbidden),
                "will*Files preflight must not contain commit operation `{forbidden}`"
            );
        }

        let dispatch = include_str!("../workspace.rs");
        assert!(dispatch.contains("self.handle_will_rename_files_pure(params)"));
        assert!(!dispatch.contains("self.handle_will_rename_files(params)"));
    }
}
