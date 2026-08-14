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
    let uri_key = server.normalize_uri_key(uri);
    if !planned_workspace_texts.contains_key(&uri_key) {
        let Some(text) = read_workspace_text(server, uri) else {
            return;
        };
        planned_workspace_texts.insert(uri_key.clone(), (text.clone(), text));
    }

    let Some((_, current_text)) = planned_workspace_texts.get_mut(&uri_key) else {
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
    let renamed_key = server.normalize_uri_key(renamed_uri);
    let updated_uris: HashSet<String> = planned_workspace_texts.keys().cloned().collect();
    let documents = server.documents.lock();
    let unhandled = documents.iter().any(|(uri, document)| {
        let uri_key = server.normalize_uri_key(uri);
        uri_key != renamed_key
            && !updated_uris.contains(&uri_key)
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
        open_document_identities: Vec<(String, i32, u32, usize)>,
        indexed_document_identities: Vec<(String, i32, usize)>,
        indexed_files: usize,
        indexed_symbols: usize,
        semantic_facts: Vec<String>,
        dependency_facts: Vec<(String, Vec<String>)>,
        old_file_symbols: usize,
        new_file_symbols: usize,
        old_document_stored: bool,
        new_document_stored: bool,
        index_state: String,
        readiness: String,
        memory: crate::runtime::MemoryStateSnapshot,
        pressure: crate::runtime::RuntimePressureSnapshot,
        semantic_tokens_cache_entries: usize,
        provider_decision_trace_entries: usize,
        progress_tokens: Vec<String>,
        progress_requests: Vec<String>,
        cancelled_requests: usize,
        pending_request_ids: usize,
        parse_cancel_uris: Vec<String>,
        indexing_invocations: usize,
        indexing_in_progress: bool,
    }

    fn fingerprint(
        server: &LspServer,
        old_uri: &str,
        new_uri: &str,
    ) -> TestResult<RetainedStateFingerprint> {
        let coordinator = server.coordinator().ok_or("workspace coordinator unavailable")?;
        let index = coordinator.index();

        let mut open_document_identities = server
            .documents
            .lock()
            .iter()
            .map(|(uri, document)| {
                (
                    uri.clone(),
                    document.version,
                    document.generation.load(Ordering::SeqCst),
                    document.text.len(),
                )
            })
            .collect::<Vec<_>>();
        open_document_identities.sort();

        let mut indexed_document_identities = index
            .document_store()
            .all_documents()
            .into_iter()
            .map(|document| {
                let uri = document.uri.clone();
                let version = document.version;
                let text_len = document.text().len();
                (uri, version, text_len)
            })
            .collect::<Vec<_>>();
        indexed_document_identities.sort();

        let mut semantic_facts = index
            .all_symbols()
            .into_iter()
            .map(|symbol| serde_json::to_string(&symbol))
            .collect::<Result<Vec<_>, _>>()?;
        semantic_facts.sort();

        let mut dependency_facts = indexed_document_identities
            .iter()
            .map(|(uri, _, _)| {
                let mut dependencies = index.file_dependencies(uri).into_iter().collect::<Vec<_>>();
                dependencies.sort();
                (uri.clone(), dependencies)
            })
            .collect::<Vec<_>>();
        let mut dependents = index.find_dependents(&path_to_module_name(old_uri));
        dependents.sort();
        dependency_facts.push(("<dependents>".to_string(), dependents));
        dependency_facts.sort();

        let mut progress_tokens = server.progress_tokens.lock().iter().cloned().collect::<Vec<_>>();
        progress_tokens.sort();
        let mut progress_requests =
            server.progress_token_to_request.lock().keys().cloned().collect::<Vec<_>>();
        progress_requests.sort();
        let mut parse_cancel_uris =
            server.parse_cancel_flags.lock().keys().cloned().collect::<Vec<_>>();
        parse_cancel_uris.sort();

        let index_state = {
            let state = coordinator.state();
            format!(
                "kind={:?};phase={:?};files={};symbols={}",
                state.kind(),
                state.phase(),
                index.file_count(),
                index.symbol_count()
            )
        };
        let readiness =
            serde_json::to_string(&server.workspace_readiness_receipt.lock().summary_json())?;
        let memory = server.memory_state_snapshot();
        let pressure = server.runtime_pressure_snapshot();

        Ok(RetainedStateFingerprint {
            open_documents: server.documents.lock().len(),
            open_document_identities,
            indexed_document_identities,
            indexed_files: index.file_count(),
            indexed_symbols: index.symbol_count(),
            semantic_facts,
            dependency_facts,
            old_file_symbols: index.file_symbols(old_uri).len(),
            new_file_symbols: index.file_symbols(new_uri).len(),
            old_document_stored: index.document_store().get(old_uri).is_some(),
            new_document_stored: index.document_store().get(new_uri).is_some(),
            index_state,
            readiness,
            memory,
            pressure,
            semantic_tokens_cache_entries: server.semantic_tokens_cache.lock().len(),
            provider_decision_trace_entries: server.provider_decision_traces.lock().len(),
            progress_tokens,
            progress_requests,
            cancelled_requests: server.cancelled.lock().len(),
            pending_request_ids: server.pending_request_ids.lock().len(),
            parse_cancel_uris,
            indexing_invocations: server.workspace_indexing_invocation_count.load(Ordering::SeqCst),
            indexing_in_progress: server.indexing_in_progress.load(Ordering::SeqCst),
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
    fn missing_files_params_return_empty_workspace_edit_call_observation() -> TestResult {
        let (server, _directory, old_uri, new_uri, _source) = indexed_rename_fixture()?;
        let before = fingerprint(&server, &old_uri, &new_uri)?;

        for params in [None, Some(json!({})), Some(json!({ "files": null }))] {
            let outcome = server.handle_will_rename_files_dispatch(params);
            assert!(
                matches!(outcome, Ok(Some(_))),
                "missing/invalid files must succeed with a WorkspaceEdit, got {outcome:?}"
            );
            let edit =
                outcome.map_err(|error| format!("missing-files preflight failed: {error}"))?;
            let changes = edit
                .as_ref()
                .and_then(|value| value.get("changes"))
                .and_then(Value::as_object)
                .ok_or("missing-files preflight must return a WorkspaceEdit changes map")?;
            assert!(
                changes.is_empty(),
                "missing/invalid files must return an empty changes map, got {changes:?}"
            );
        }

        assert_eq!(
            fingerprint(&server, &old_uri, &new_uri)?,
            before,
            "missing-files preflight must not commit retained state"
        );
        Ok(())
    }

    #[test]
    fn routed_will_rename_is_read_only_and_returns_edits() -> TestResult {
        let (server, _directory, old_uri, new_uri, _source) = indexed_rename_fixture()?;
        let before = fingerprint(&server, &old_uri, &new_uri)?;

        let outcome = server.handle_will_rename_files_dispatch(Some(json!({
            "files": [{ "oldUri": old_uri.clone(), "newUri": new_uri.clone() }]
        })));
        assert!(
            matches!(outcome, Ok(Some(_))),
            "willRenameFiles preflight must succeed with a WorkspaceEdit, got {outcome:?}"
        );
        let result =
            outcome.map_err(|error| format!("willRenameFiles preflight failed: {error}"))?;

        let after = fingerprint(&server, &old_uri, &new_uri)?;
        assert_eq!(after, before, "willRenameFiles must not commit retained state");
        let changes = result
            .as_ref()
            .and_then(|value| value.get("changes"))
            .and_then(Value::as_object)
            .ok_or("preflight must return a WorkspaceEdit changes map")?;
        assert!(
            changes.get(&server.normalize_uri_key(&old_uri)).is_some_and(Value::is_array),
            "rename preflight should still return the package edit under the normalized URI key"
        );
        Ok(())
    }

    #[test]
    fn planned_workspace_texts_collapse_uri_aliases() -> TestResult {
        let (server, _directory, old_uri, new_uri, _source) = indexed_rename_fixture()?;
        let aliased_old = if let Some(rest) = old_uri.strip_prefix("file:///") {
            if rest.len() > 1 && rest.as_bytes()[1] == b':' {
                let mut chars = rest.chars();
                let drive = chars.next().ok_or("missing drive letter")?;
                let flipped = if drive.is_ascii_uppercase() {
                    drive.to_ascii_lowercase()
                } else {
                    drive.to_ascii_uppercase()
                };
                format!("file:///{flipped}{}", chars.as_str())
            } else {
                old_uri.clone()
            }
        } else {
            old_uri.clone()
        };

        let mut planned = BTreeMap::new();
        let old_module = path_to_module_name(&old_uri);
        let new_module = path_to_module_name(&new_uri);
        plan_uri_rewrite(&server, &mut planned, &old_uri, &old_module, &new_module);
        plan_uri_rewrite(&server, &mut planned, &aliased_old, &old_module, &new_module);

        assert_eq!(
            planned.len(),
            1,
            "raw and normalized URI spellings must share one planned edit entry"
        );
        assert_eq!(
            server.normalize_uri_key(&old_uri),
            server.normalize_uri_key(&aliased_old),
            "fixture aliases must normalize to the same URI key"
        );
        Ok(())
    }

    #[test]
    fn did_rename_is_the_commit_boundary() -> TestResult {
        let (server, directory, old_uri, new_uri, source) = indexed_rename_fixture()?;
        let old_path = directory.path().join("OldModule.pm");
        let new_path = directory.path().join("NewModule.pm");

        let will_outcome = server.handle_will_rename_files_dispatch(Some(json!({
            "files": [{ "oldUri": old_uri.clone(), "newUri": new_uri.clone() }]
        })));
        assert!(
            matches!(will_outcome, Ok(Some(_))),
            "willRenameFiles before didRename must succeed, got {will_outcome:?}"
        );
        let _ =
            will_outcome.map_err(|error| format!("willRenameFiles preflight failed: {error}"))?;
        assert!(
            !server
                .coordinator()
                .ok_or("workspace coordinator unavailable")?
                .index()
                .file_symbols(&old_uri)
                .is_empty(),
            "preflight must retain the old file symbols until didRenameFiles"
        );

        std::fs::rename(&old_path, &new_path)?;
        std::fs::write(&new_path, source)?;
        let did_outcome = server.handle_did_rename_files(Some(json!({
            "files": [{ "oldUri": old_uri.clone(), "newUri": new_uri.clone() }]
        })));
        assert!(did_outcome.is_ok(), "didRenameFiles commit must succeed, got {did_outcome:?}");
        did_outcome.map_err(|error| format!("didRenameFiles commit failed: {error}"))?;

        let committed = fingerprint(&server, &old_uri, &new_uri)?;
        assert_eq!(committed.old_file_symbols, 0, "didRenameFiles must drop old file symbols");
        assert!(!committed.old_document_stored, "didRenameFiles must drop the old document");
        assert!(committed.new_file_symbols > 0, "didRenameFiles must index the new file");
        assert!(committed.new_document_stored, "didRenameFiles must store the new document");
        Ok(())
    }

    #[test]
    fn will_create_and_will_delete_leave_retained_state_unchanged() -> TestResult {
        let (server, directory, old_uri, new_uri, _source) = indexed_rename_fixture()?;
        let before_create = fingerprint(&server, &old_uri, &new_uri)?;
        let create_uri = Url::from_file_path(directory.path().join("Created.pm"))
            .map_err(|_| "invalid create path")?
            .to_string();
        let create_outcome = server.handle_will_create_files_dispatch(Some(json!({
            "files": [{ "uri": create_uri }]
        })));
        assert!(
            matches!(create_outcome, Ok(Some(_)) | Ok(None)),
            "willCreateFiles must succeed without committing state, got {create_outcome:?}"
        );
        let _ =
            create_outcome.map_err(|error| format!("willCreateFiles preflight failed: {error}"))?;
        assert_eq!(
            fingerprint(&server, &old_uri, &new_uri)?,
            before_create,
            "willCreateFiles must leave retained state unchanged"
        );

        let before_delete = fingerprint(&server, &old_uri, &new_uri)?;
        let delete_outcome = server.handle_will_delete_files_dispatch(Some(json!({
            "files": [{ "uri": old_uri.clone() }]
        })));
        assert!(
            matches!(delete_outcome, Ok(Some(_)) | Ok(None)),
            "willDeleteFiles must succeed without committing state, got {delete_outcome:?}"
        );
        let _ =
            delete_outcome.map_err(|error| format!("willDeleteFiles preflight failed: {error}"))?;
        assert_eq!(
            fingerprint(&server, &old_uri, &new_uri)?,
            before_delete,
            "willDeleteFiles must leave retained state unchanged"
        );
        Ok(())
    }

    #[test]
    fn routed_preflight_implementation_has_no_commit_operations() -> TestResult {
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
            "refresh_controller",
            "progress_tokens",
            "progress_token_to_request",
            "pending_index_task_count",
            "workspace_readiness_receipt",
            "ast_cache",
            "pod_cache",
            "semantic_tokens_cache",
            "module_scan_cache",
            "use_lib_hir_cache",
            "provider_decision_traces",
        ] {
            assert!(
                !implementation.contains(forbidden),
                "will*Files preflight must not contain commit operation `{forbidden}`"
            );
        }

        let dispatch = include_str!("../workspace.rs");
        for (handler, target) in [
            (
                "pub(super) fn handle_will_rename_files_dispatch(",
                "self.handle_will_rename_files_pure(params)",
            ),
            (
                "pub(super) fn handle_will_delete_files_dispatch(",
                "self.handle_will_delete_files(params)",
            ),
            (
                "pub(super) fn handle_will_create_files_dispatch(",
                "self.handle_will_create_files(params)",
            ),
        ] {
            let route = source_function(dispatch, handler).ok_or("missing will*Files route")?;
            assert!(route.contains(target), "route `{handler}` must call `{target}`");
        }

        let workspace = include_str!("../../workspace.rs");
        for handler in
            ["pub(super) fn handle_will_delete_files(", "pub(super) fn handle_will_create_files("]
        {
            let implementation =
                source_function(workspace, handler).ok_or("missing direct handler")?;
            for forbidden in [
                ".notify_change(",
                ".notify_parse_complete(",
                ".remove_file(",
                ".clear_file(",
                ".index_file(",
                "documents.remove(",
                "documents.insert(",
                "refresh_all(",
                "pending_index_task_count.fetch_",
                "progress_tokens.lock().",
                "workspace_readiness_receipt.lock().",
                "ast_cache",
                "pod_cache",
                "semantic_tokens_cache",
                "module_scan_cache",
                "use_lib_hir_cache",
                "provider_decision_traces",
            ] {
                assert!(
                    !implementation.contains(forbidden),
                    "routed will*Files handler `{handler}` must not contain commit operation `{forbidden}`"
                );
            }
        }
        Ok(())
    }

    fn source_function<'a>(source: &'a str, marker: &str) -> Option<&'a str> {
        let start = source.find(marker)?;
        let remainder = &source[start..];
        let next_function = remainder[marker.len()..]
            .find("\n    pub(super) fn ")
            .map(|offset| marker.len() + offset);
        Some(&remainder[..next_function.unwrap_or(remainder.len())])
    }
}
