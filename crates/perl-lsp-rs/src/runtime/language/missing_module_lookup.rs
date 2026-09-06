//! Missing-module lookup explanation command.
//!
//! This command exposes the existing module-resolution state as a user-facing
//! receipt. It does not perform a new workspace scan or change PL701 behavior.
//!
//! The explanation also projects the stored startup-`@INC` acquisition state
//! (#13589) so a user can tell an exact not-found from a lookup that omitted
//! interpreter roots because their acquisition was disabled, pending, timed
//! out, or failed. Reading the explanation never launches or retries Perl.

use super::super::{JsonRpcError, LspServer, Value, json, md5};
use crate::protocol::invalid_params;
use perl_lsp_rs_core::config::{
    SystemIncLookupImpact, SystemIncProbeOutcomeKind, SystemIncProbeSnapshot,
};
use perl_module::is_lookup_safe_module_name;
use perl_module::module_name_to_path;
use perl_module::{
    IncRoot, IncRootKind, ModuleUriResolution, resolve_module_uri_with_effective_inc,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MISSING_MODULE_LOOKUP_SCHEMA_VERSION: &str = "missing_module_lookup_explanation.v1";
const EXPLAIN_MISSING_MODULE_LOOKUP_COMMAND: &str = "perl.explainMissingModuleLookup";

impl LspServer {
    pub(crate) fn explain_missing_module_lookup(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let params =
            params.ok_or_else(|| invalid_params("Missing missing-module lookup argument"))?;
        let request = MissingModuleLookupRequest::from_value(&params)?;

        let (doc_text, doc_offset, document_open) =
            self.missing_module_document_context(request.doc_uri.as_deref(), request.position);
        // Explanation reads stored startup-@INC state; it must never launch
        // or retry Perl on the user's behalf (#13589).
        let context = match self.effective_inc_context_for_doc_without_probe(
            request.doc_uri.as_deref(),
            doc_text.as_deref(),
            doc_offset,
        ) {
            Some(context) => context,
            None => {
                let payload = missing_module_root_missing_payload(&request);
                return Ok(Some(payload));
            }
        };

        let workspace_folders = self.workspace_folders.lock().clone();
        let workspace_folder_uris: Vec<String> =
            workspace_folders.iter().map(|folder| folder.uri.clone()).collect();
        let workspace_folder_paths = workspace_folder_paths(&workspace_folder_uris, &context.root);

        let timeout = Duration::from_millis(context.resolution_timeout_ms);
        let open_document_uris: Vec<String> = {
            let documents = self.documents.lock();
            documents
                .keys()
                .filter(|uri| doc_offset.is_none() || context.symbol_uri_reachable(uri))
                .cloned()
                .collect()
        };

        let resolution = resolve_module_uri_with_effective_inc(
            &request.module,
            &open_document_uris,
            &workspace_folder_uris,
            &context.effective_roots,
            timeout,
        );
        let expected_relative_path = module_name_to_path(&request.module);
        let searched_inc_paths = searched_inc_paths(
            &context.effective_roots,
            &workspace_folder_paths,
            &expected_relative_path,
        );
        let startup_inc = &context.system_inc_state;
        let lookup_impact = startup_inc.lookup_impact();
        let result = module_lookup_result(
            &request.module,
            &resolution,
            context.resolution_timeout_ms,
            &open_document_uris,
            lookup_impact,
        );
        let user_message = missing_module_user_message(
            &request.module,
            &resolution,
            context.resolution_timeout_ms,
            startup_inc,
        );
        let interpreter_startup_inc =
            interpreter_startup_inc_payload(startup_inc, context.folder_uri.as_deref());
        let workspace_roots = workspace_folder_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        Ok(Some(json!({
            "schema_version": MISSING_MODULE_LOOKUP_SCHEMA_VERSION,
            "command": EXPLAIN_MISSING_MODULE_LOOKUP_COMMAND,
            "requested_module": request.module,
            "expected_relative_path": expected_relative_path,
            "text_document_uri": request.doc_uri,
            "request_position": request.position.map(|(line, character)| json!({
                "line": line,
                "character": character,
            })),
            "document_open": document_open,
            "workspace": {
                "root": context.root.display().to_string(),
                "folder_uri": context.folder_uri,
                "workspace_root_count": workspace_folder_paths.len(),
            },
            "module_resolution": {
                "result": result,
                "effective_include_paths": searched_inc_paths,
                "open_document_uri_count": open_document_uris.len(),
                "use_system_inc": context.use_system_inc,
                "use_perl5lib": context.use_perl5lib,
                "perl5lib_policy": perl5lib_policy(context.use_perl5lib),
                "resolution_timeout_ms": context.resolution_timeout_ms,
                "dot_or_workspace_root_caveat": dot_or_workspace_root_caveat(
                    &context.effective_roots,
                    &context.root,
                ),
                "interpreter_startup_inc": interpreter_startup_inc,
            },
            "user_message": user_message,
            "claim_boundary": "explains one missing-module lookup using existing runtime @INC state only; no workspace scan, diagnostic suppression change, resolver behavior change, Perl launch or probe retry, or support-tier promotion",
            "copyable_payload": {
                "schema_version": MISSING_MODULE_LOOKUP_SCHEMA_VERSION,
                "perl_lsp_version": env!("CARGO_PKG_VERSION"),
                "provider": "module_resolution",
                "command": EXPLAIN_MISSING_MODULE_LOOKUP_COMMAND,
                "requested_module": request.module,
                "expected_relative_path": expected_relative_path,
                "result": copyable_resolution_result(&resolution),
                "search_complete": search_complete(&resolution, lookup_impact),
                "workspace_root_class": workspace_root_class(&workspace_roots),
                "workspace_root_hash": workspace_root_hash(&workspace_roots),
                "effective_include_path_count": context.effective_roots.len(),
                "use_system_inc": context.use_system_inc,
                "use_perl5lib": context.use_perl5lib,
                "perl5lib_policy": perl5lib_policy(context.use_perl5lib),
                "startup_inc_outcome": startup_inc.outcome.code(),
                "startup_inc_explanation_class": startup_inc_explanation_class(startup_inc),
                "startup_inc_lookup_impact": lookup_impact.code(),
                "startup_inc_retry_state": startup_inc_retry_state(startup_inc),
                "startup_inc_remediation": startup_inc_remediation(startup_inc),
                "support_tier_link": "docs/project/status/SUPPORT_TIERS.md#claim-rows",
                "request_position": request.position.map(|(line, character)| json!({
                    "line": line,
                    "character": character,
                })),
            },
        })))
    }

    fn missing_module_document_context(
        &self,
        doc_uri: Option<&str>,
        position: Option<(u32, u32)>,
    ) -> (Option<String>, Option<usize>, bool) {
        let Some(uri) = doc_uri else {
            return (None, None, false);
        };
        let documents = self.documents.lock();
        let Some(doc) = self.get_document(&documents, uri) else {
            return (None, None, false);
        };

        let offset = position.map(|(line, character)| self.pos16_to_offset(doc, line, character));
        (Some(doc.text_str().to_string()), offset, true)
    }
}

struct MissingModuleLookupRequest {
    module: String,
    doc_uri: Option<String>,
    position: Option<(u32, u32)>,
}

impl MissingModuleLookupRequest {
    fn from_value(value: &Value) -> Result<Self, JsonRpcError> {
        if let Some(module) = value.as_str() {
            if !is_lookup_safe_module_name(module) {
                return Err(invalid_params("Invalid module name for missing-module lookup"));
            }
            return Ok(Self { module: module.to_string(), doc_uri: None, position: None });
        }

        let module = request_module(value)
            .ok_or_else(|| invalid_params("Missing module for missing-module lookup"))?;
        if !is_lookup_safe_module_name(&module) {
            return Err(invalid_params("Invalid module name for missing-module lookup"));
        }
        let doc_uri = request_doc_uri(value);
        let position = request_position(value);

        Ok(Self { module, doc_uri, position })
    }
}

fn request_module(value: &Value) -> Option<String> {
    value
        .get("module")
        .and_then(Value::as_str)
        .or_else(|| value.get("moduleName").and_then(Value::as_str))
        .or_else(|| value.pointer("/diagnostic/data/module").and_then(Value::as_str))
        .map(str::to_string)
        .or_else(|| {
            value
                .pointer("/diagnostic/message")
                .and_then(Value::as_str)
                .and_then(extract_module_from_pl701_message)
        })
}

fn request_doc_uri(value: &Value) -> Option<String> {
    value
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .or_else(|| value.get("uri").and_then(Value::as_str))
        .or_else(|| value.pointer("/request_position/uri").and_then(Value::as_str))
        .map(str::to_string)
}

fn request_position(value: &Value) -> Option<(u32, u32)> {
    value.get("position").or_else(|| value.pointer("/diagnostic/range/start")).and_then(
        |position| {
            let line = position.get("line").and_then(Value::as_u64)?;
            let character = position.get("character").and_then(Value::as_u64)?;
            let line = u32::try_from(line).ok()?;
            let character = u32::try_from(character).ok()?;
            Some((line, character))
        },
    )
}

fn extract_module_from_pl701_message(message: &str) -> Option<String> {
    let start = message.find("Module '")? + "Module '".len();
    let rest = &message[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn searched_inc_paths(
    roots: &[IncRoot],
    workspace_folder_paths: &[PathBuf],
    relative_path: &str,
) -> Vec<Value> {
    let canonical_workspace_roots = canonical_workspace_roots(workspace_folder_paths);
    let mut ordered_roots = roots.to_vec();
    ordered_roots.sort_by_key(|root| root.precedence);
    ordered_roots
        .iter()
        .map(|root| {
            let candidates = candidate_paths_for_root(root, workspace_folder_paths, relative_path)
                .into_iter()
                .map(|path| {
                    let inside_workspace =
                        path_is_under_workspace_roots(&path, &canonical_workspace_roots);
                    let exists = if inside_workspace { json!(path.is_file()) } else { Value::Null };
                    json!({
                        "path": path.display().to_string(),
                        "exists": exists,
                        "inside_workspace": inside_workspace,
                        "probed": inside_workspace,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "path": root.path.display().to_string(),
                "source": root_source_label(&root.source),
                "kind": inc_root_kind_label(root.kind),
                "precedence": root.precedence,
                "candidate_paths": candidates,
            })
        })
        .collect()
}

fn candidate_paths_for_root(
    root: &IncRoot,
    workspace_folder_paths: &[PathBuf],
    relative_path: &str,
) -> Vec<PathBuf> {
    match root.kind {
        IncRootKind::FileLocalLexical | IncRootKind::WorkspaceRelative => {
            if root.path.is_absolute() {
                vec![root.path.join(relative_path)]
            } else {
                workspace_folder_paths
                    .iter()
                    .map(|workspace| {
                        if root.path == Path::new(".") {
                            workspace.join(relative_path)
                        } else {
                            workspace.join(&root.path).join(relative_path)
                        }
                    })
                    .collect()
            }
        }
        IncRootKind::ExternalAbsolute
        | IncRootKind::Perl5LibEnv
        | IncRootKind::InterpreterStartup
        | IncRootKind::RuntimeDerived => vec![root.path.join(relative_path)],
    }
}

fn module_lookup_result(
    module: &str,
    resolution: &ModuleUriResolution,
    timeout_ms: u64,
    open_document_uris: &[String],
    lookup_impact: SystemIncLookupImpact,
) -> Value {
    let search_complete = search_complete(resolution, lookup_impact);
    match resolution {
        ModuleUriResolution::Resolved(uri) => {
            let source = if open_document_uris.iter().any(|open_uri| open_uri == uri) {
                "open_document"
            } else {
                "effective_inc"
            };
            json!({
                "status": "resolved",
                "resolved": true,
                "resolved_uri": uri,
                "source": source,
                "search_complete": search_complete,
                "why": "A source-backed module file matched the requested module name.",
            })
        }
        ModuleUriResolution::TimedOut => json!({
            "status": "timed_out",
            "resolved": false,
            "resolved_uri": null,
            "source": "timeout",
            "search_complete": search_complete,
            "why": format!(
                "Lookup for module {module} exceeded the configured {timeout_ms}ms resolution timeout."
            ),
        }),
        ModuleUriResolution::NotFound => json!({
            "status": "not_found",
            "resolved": false,
            "resolved_uri": null,
            "source": "effective_inc",
            "search_complete": search_complete,
            "why": not_found_why(lookup_impact),
        }),
    }
}

/// Whether the lookup searched every root family the configuration enables.
///
/// A resolved module is complete by construction. A `NotFound` is exact only
/// when interpreter startup roots either participated or were configured off;
/// a pending, transient, or terminal omission leaves the search incomplete,
/// so the lookup must not be called an exact not-found (#13589 falsifier 6).
fn search_complete(resolution: &ModuleUriResolution, lookup_impact: SystemIncLookupImpact) -> bool {
    match resolution {
        ModuleUriResolution::Resolved(_) => true,
        ModuleUriResolution::TimedOut => false,
        ModuleUriResolution::NotFound => matches!(
            lookup_impact,
            SystemIncLookupImpact::Participated | SystemIncLookupImpact::Disabled
        ),
    }
}

fn not_found_why(lookup_impact: SystemIncLookupImpact) -> &'static str {
    match lookup_impact {
        SystemIncLookupImpact::Participated | SystemIncLookupImpact::Disabled => {
            "No open document or searched @INC candidate matched the expected relative module path."
        }
        SystemIncLookupImpact::NotObserved => {
            "No open document or searched @INC candidate matched the expected relative module path, but interpreter startup @INC roots have not been acquired yet, so this is not an exact not-found."
        }
        SystemIncLookupImpact::OmittedTransient => {
            "No open document or searched @INC candidate matched the expected relative module path, but interpreter startup @INC roots were omitted after a transient probe timeout with a retry remaining, so this is not an exact not-found."
        }
        SystemIncLookupImpact::OmittedTerminal => {
            "No open document or searched @INC candidate matched the expected relative module path, but interpreter startup @INC roots were omitted because their acquisition failed or exhausted its retry budget, so this is not an exact not-found."
        }
    }
}

/// Explanation class per the #13589 outcome law.
fn startup_inc_explanation_class(snapshot: &SystemIncProbeSnapshot) -> &'static str {
    match snapshot.outcome {
        SystemIncProbeOutcomeKind::Disabled => "configured_off",
        SystemIncProbeOutcomeKind::NotObserved => "not_observed",
        SystemIncProbeOutcomeKind::TimedOut if snapshot.retry_eligible() => "transient_degraded",
        SystemIncProbeOutcomeKind::TimedOut => "terminal_degraded",
        SystemIncProbeOutcomeKind::Unavailable => "terminal_unavailable",
        SystemIncProbeOutcomeKind::IoFailed | SystemIncProbeOutcomeKind::NonZeroExit => {
            "terminal_failed"
        }
        SystemIncProbeOutcomeKind::SuccessfulEmpty => "legitimate_empty",
        SystemIncProbeOutcomeKind::Paths => "exact_current",
    }
}

/// Retry disposition per the #13589 outcome law.
fn startup_inc_retry_state(snapshot: &SystemIncProbeSnapshot) -> &'static str {
    match snapshot.outcome {
        SystemIncProbeOutcomeKind::Disabled => "terminal_for_current_config",
        SystemIncProbeOutcomeKind::NotObserved => "eligible",
        SystemIncProbeOutcomeKind::TimedOut if snapshot.retry_eligible() => "one_retry_remains",
        SystemIncProbeOutcomeKind::TimedOut => "exhausted",
        _ => "settled",
    }
}

/// Redacted remediation code; never a path, command line, or environment value.
///
/// Codes name only actions a user can actually perform. There is no
/// user-facing channel that writes `WorkspaceConfig::perl_path` (see
/// `perl_remediation::PERL_REMEDIATION`, #5376), so an unavailable
/// interpreter routes to install/PATH/restart and a persistently slow one to
/// PATH order, never to an interpreter-path setting.
fn startup_inc_remediation(snapshot: &SystemIncProbeSnapshot) -> &'static str {
    match snapshot.outcome {
        SystemIncProbeOutcomeKind::Disabled => "enable_perl_workspace_use_system_inc",
        SystemIncProbeOutcomeKind::NotObserved => "await_first_live_lookup",
        SystemIncProbeOutcomeKind::TimedOut if snapshot.retry_eligible() => "retry_lookup",
        SystemIncProbeOutcomeKind::TimedOut => {
            "toggle_use_system_inc_or_put_faster_perl_first_on_path"
        }
        SystemIncProbeOutcomeKind::Unavailable => "install_perl_add_to_path_and_restart",
        SystemIncProbeOutcomeKind::IoFailed | SystemIncProbeOutcomeKind::NonZeroExit => {
            "check_perl_interpreter"
        }
        SystemIncProbeOutcomeKind::SuccessfulEmpty | SystemIncProbeOutcomeKind::Paths => "none",
    }
}

fn startup_inc_limitations(snapshot: &SystemIncProbeSnapshot) -> Vec<&'static str> {
    let mut limitations = vec![
        "bound to the stored folder/global configuration epoch; no typed ProjectEnvironment generation exists yet, so currentness is the configuration epoch rather than an interpreter/environment generation",
        "root paths are redacted to a count in this projection; the effective_include_paths listing above remains the only path-bearing surface",
    ];
    if snapshot.outcome == SystemIncProbeOutcomeKind::IoFailed {
        limitations.push(
            "io_failed does not distinguish a spawn failure from a later process I/O failure",
        );
    }
    limitations
}

/// Typed, non-probing projection of the stored startup-`@INC` acquisition
/// state that the lookup consumed (#13589).
fn interpreter_startup_inc_payload(
    snapshot: &SystemIncProbeSnapshot,
    folder_uri: Option<&str>,
) -> Value {
    let owner_scope = if folder_uri.is_some() { "workspace_folder" } else { "global" };
    json!({
        "outcome_code": snapshot.outcome.code(),
        "explanation_class": startup_inc_explanation_class(snapshot),
        "attempts_consumed": snapshot.attempts_consumed,
        "max_attempts": snapshot.max_attempts,
        "retry_eligible": snapshot.retry_eligible(),
        "terminal": snapshot.terminal(),
        "retry_state": startup_inc_retry_state(snapshot),
        "use_system_inc": snapshot.use_system_inc,
        "use_perl5lib": snapshot.use_perl5lib,
        "system_root_count": snapshot.system_root_count,
        "lookup_impact": snapshot.lookup_impact().code(),
        "remediation_code": startup_inc_remediation(snapshot),
        "owner": {
            "scope": owner_scope,
            "folder_uri": folder_uri,
        },
        "currentness": {
            "basis": "stored_configuration_epoch",
            "generation_ceiling": "configuration_epoch",
        },
        "limitations": startup_inc_limitations(snapshot),
        "claim_boundary": "projects the stored startup @INC probe state the live resolver used; reading it never launches or retries Perl",
    })
}

fn copyable_resolution_result(resolution: &ModuleUriResolution) -> &'static str {
    match resolution {
        ModuleUriResolution::Resolved(_) => "resolved",
        ModuleUriResolution::TimedOut => "timed_out",
        ModuleUriResolution::NotFound => "not_found",
    }
}

fn missing_module_user_message(
    module: &str,
    resolution: &ModuleUriResolution,
    timeout_ms: u64,
    startup_inc: &SystemIncProbeSnapshot,
) -> String {
    match resolution {
        ModuleUriResolution::Resolved(_) => {
            format!(
                "Module {module} resolved through the current effective @INC state. This explanation does not change diagnostics."
            )
        }
        ModuleUriResolution::TimedOut => {
            format!(
                "Module {module} lookup timed out after {timeout_ms}ms. Consider increasing `perl.workspace.resolutionTimeout` for slow filesystems."
            )
        }
        ModuleUriResolution::NotFound => match startup_inc.lookup_impact() {
            SystemIncLookupImpact::Participated | SystemIncLookupImpact::Disabled => format!(
                "Module {module} was not found in the current effective @INC state. Check `perl.workspace.includePaths`, PERL5LIB policy, or install the module."
            ),
            SystemIncLookupImpact::NotObserved => format!(
                "Module {module} was not found, but interpreter startup @INC roots have not been acquired yet, so this is not an exact not-found. The next module lookup will attempt the startup @INC probe."
            ),
            SystemIncLookupImpact::OmittedTransient => format!(
                "Module {module} was not found, but interpreter startup @INC roots were omitted after a transient probe timeout ({}/{} attempts used), so this is not an exact not-found. A later lookup may recover them.",
                startup_inc.attempts_consumed, startup_inc.max_attempts
            ),
            SystemIncLookupImpact::OmittedTerminal => format!(
                "Module {module} was not found, but interpreter startup @INC roots were omitted because their acquisition {}, so this is not an exact not-found. They stay omitted until `perl.workspace.useSystemInc` or `perl.workspace.usePerl5lib` changes.",
                terminal_omission_reason(startup_inc)
            ),
        },
    }
}

fn terminal_omission_reason(startup_inc: &SystemIncProbeSnapshot) -> &'static str {
    match startup_inc.outcome {
        SystemIncProbeOutcomeKind::TimedOut => "timed out on every bounded attempt",
        SystemIncProbeOutcomeKind::Unavailable => "had no admitted Perl interpreter",
        SystemIncProbeOutcomeKind::IoFailed => "failed at the process or I/O boundary",
        SystemIncProbeOutcomeKind::NonZeroExit => "ran Perl but the probe exited non-zero",
        _ => "failed",
    }
}

fn missing_module_root_missing_payload(request: &MissingModuleLookupRequest) -> Value {
    json!({
        "schema_version": MISSING_MODULE_LOOKUP_SCHEMA_VERSION,
        "command": EXPLAIN_MISSING_MODULE_LOOKUP_COMMAND,
        "requested_module": request.module,
        "expected_relative_path": module_name_to_path(&request.module),
        "text_document_uri": request.doc_uri,
        "request_position": request.position.map(|(line, character)| json!({
            "line": line,
            "character": character,
        })),
        "module_resolution": {
            "result": {
                "status": "workspace_root_missing",
                "resolved": false,
                "resolved_uri": null,
                "source": "workspace",
                "why": "No workspace root is available, so perl-lsp cannot assemble effective @INC roots.",
            },
            "effective_include_paths": [],
            "open_document_uri_count": 0,
            "use_system_inc": false,
            "use_perl5lib": false,
            "perl5lib_policy": "workspace_root_missing",
            "resolution_timeout_ms": null,
            "dot_or_workspace_root_caveat": false,
            "interpreter_startup_inc": null,
        },
        "user_message": format!(
            "Module {} could not be checked because no workspace root is available. Open a project folder before using module lookup explanations.",
            request.module
        ),
        "claim_boundary": "explains one missing-module lookup using existing runtime @INC state only; no workspace scan, diagnostic suppression change, resolver behavior change, Perl launch or probe retry, or support-tier promotion",
        "copyable_payload": {
            "schema_version": MISSING_MODULE_LOOKUP_SCHEMA_VERSION,
            "perl_lsp_version": env!("CARGO_PKG_VERSION"),
            "provider": "module_resolution",
            "command": EXPLAIN_MISSING_MODULE_LOOKUP_COMMAND,
            "requested_module": request.module,
            "expected_relative_path": module_name_to_path(&request.module),
            "result": "workspace_root_missing",
            "workspace_root_class": "none",
            "workspace_root_hash": null,
            "effective_include_path_count": 0,
            "use_system_inc": false,
            "use_perl5lib": false,
            "perl5lib_policy": "workspace_root_missing",
            "support_tier_link": "docs/project/status/SUPPORT_TIERS.md#claim-rows",
        },
    })
}

fn path_is_under_workspace_roots(path: &Path, workspace_roots: &[PathBuf]) -> bool {
    let Some(canonical_path) = canonicalize_nearest_existing_ancestor(path) else {
        return false;
    };
    workspace_roots.iter().any(|root| canonical_path.starts_with(root))
}

fn canonicalize_nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut candidate = path;
    loop {
        if let Ok(canonical_path) = candidate.canonicalize() {
            return Some(canonical_path);
        }
        candidate = candidate.parent()?;
    }
}

fn canonical_workspace_roots(workspace_roots: &[PathBuf]) -> Vec<PathBuf> {
    workspace_roots.iter().filter_map(|root| root.canonicalize().ok()).collect()
}

fn workspace_folder_paths(workspace_folder_uris: &[String], fallback_root: &Path) -> Vec<PathBuf> {
    let mut paths = workspace_folder_uris
        .iter()
        .filter_map(|uri| super::super::source_path_from_uri(uri))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        paths.push(fallback_root.to_path_buf());
    }
    paths
}

fn root_source_label(source: &str) -> &'static str {
    match source {
        "use-lib-lexical" => "use lib",
        "workspace-include-paths" => "workspace includePaths",
        "perl5lib-env" => "PERL5LIB",
        "interpreter-startup-inc" => "interpreter startup @INC",
        _ => "unknown @INC source",
    }
}

fn inc_root_kind_label(kind: IncRootKind) -> &'static str {
    match kind {
        IncRootKind::FileLocalLexical => "file_local_lexical",
        IncRootKind::WorkspaceRelative => "workspace_relative",
        IncRootKind::ExternalAbsolute => "external_absolute",
        IncRootKind::Perl5LibEnv => "perl5lib_env",
        IncRootKind::InterpreterStartup => "interpreter_startup",
        IncRootKind::RuntimeDerived => "runtime_derived",
    }
}

fn perl5lib_policy(use_perl5lib: bool) -> &'static str {
    if !use_perl5lib {
        return "disabled_by_workspace_config";
    }

    if std::env::var_os("PERL5LIB").is_some() {
        "enabled_from_environment"
    } else {
        "enabled_but_environment_empty"
    }
}

fn dot_or_workspace_root_caveat(roots: &[IncRoot], workspace_root: &Path) -> bool {
    roots.iter().any(|root| {
        if root.path == Path::new(".") {
            return true;
        }
        let absolute = if root.path.is_absolute() {
            root.path.clone()
        } else {
            workspace_root.join(&root.path)
        };
        absolute == workspace_root
    })
}

fn workspace_root_class(workspace_roots: &[String]) -> &'static str {
    match workspace_roots.len() {
        0 => "none",
        1 => "single_root",
        _ => "multi_root",
    }
}

fn workspace_root_hash(workspace_roots: &[String]) -> Option<String> {
    if workspace_roots.is_empty() {
        return None;
    }

    let mut roots = workspace_roots.iter().map(|root| root.replace('\\', "/")).collect::<Vec<_>>();
    roots.sort();
    Some(format!("{:x}", md5::compute(roots.join("\n"))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::workspace_folder::WorkspaceFolderState;
    use perl_lsp_rs_core::config::WorkspaceConfig;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn file_uri(path: &Path) -> Result<String, String> {
        url::Url::from_file_path(path)
            .map(|url| url.to_string())
            .map_err(|()| format!("failed to create URI for {}", path.display()))
    }

    fn snapshot(outcome: SystemIncProbeOutcomeKind, attempts: u32) -> SystemIncProbeSnapshot {
        SystemIncProbeSnapshot {
            use_system_inc: outcome != SystemIncProbeOutcomeKind::Disabled,
            use_perl5lib: true,
            outcome,
            attempts_consumed: attempts,
            max_attempts: 2,
            system_root_count: match outcome {
                SystemIncProbeOutcomeKind::Paths => Some(3),
                SystemIncProbeOutcomeKind::SuccessfulEmpty => Some(0),
                _ => None,
            },
        }
    }

    fn str_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, String> {
        value.pointer(pointer).and_then(Value::as_str).ok_or_else(|| format!("missing {pointer}"))
    }

    fn explain(server: &LspServer, module: &str, doc_uri: &str) -> Result<Value, String> {
        server
            .explain_missing_module_lookup(Some(json!({
                "module": module,
                "textDocument": { "uri": doc_uri },
            })))
            .map_err(|error| error.message)?
            .ok_or_else(|| "explanation returned no payload".to_string())
    }

    /// A folder whose startup-`@INC` probe would fail at spawn: the path
    /// does not exist, so the live path records exactly one `IoFailed`
    /// attempt without ever launching a real interpreter.
    fn unspawnable_system_inc_config() -> WorkspaceConfig {
        let mut config = WorkspaceConfig::default();
        config.use_system_inc = true;
        config.use_perl5lib = false;
        config.perl_path = Some(
            std::env::temp_dir()
                .join("perl-lsp-13589-missing-interpreter")
                .join("perl")
                .display()
                .to_string(),
        );
        config
    }

    /// The outcome law from #13589, row by row, over the pure projection.
    #[test]
    fn projection_follows_the_outcome_law() -> TestResult {
        use SystemIncProbeOutcomeKind as K;
        // (outcome, attempts, class, retry_state, impact, remediation, retry_eligible, terminal)
        let rows: [(K, u32, &str, &str, &str, &str, bool, bool); 9] = [
            (
                K::Disabled,
                0,
                "configured_off",
                "terminal_for_current_config",
                "disabled",
                "enable_perl_workspace_use_system_inc",
                false,
                true,
            ),
            (
                K::NotObserved,
                0,
                "not_observed",
                "eligible",
                "not_observed",
                "await_first_live_lookup",
                true,
                false,
            ),
            (
                K::TimedOut,
                1,
                "transient_degraded",
                "one_retry_remains",
                "omitted_transient",
                "retry_lookup",
                true,
                false,
            ),
            (
                K::TimedOut,
                2,
                "terminal_degraded",
                "exhausted",
                "omitted_terminal",
                "toggle_use_system_inc_or_put_faster_perl_first_on_path",
                false,
                true,
            ),
            (
                K::Unavailable,
                1,
                "terminal_unavailable",
                "settled",
                "omitted_terminal",
                "install_perl_add_to_path_and_restart",
                false,
                true,
            ),
            (
                K::IoFailed,
                1,
                "terminal_failed",
                "settled",
                "omitted_terminal",
                "check_perl_interpreter",
                false,
                true,
            ),
            (
                K::NonZeroExit,
                1,
                "terminal_failed",
                "settled",
                "omitted_terminal",
                "check_perl_interpreter",
                false,
                true,
            ),
            (
                K::SuccessfulEmpty,
                1,
                "legitimate_empty",
                "settled",
                "participated",
                "none",
                false,
                true,
            ),
            (K::Paths, 1, "exact_current", "settled", "participated", "none", false, true),
        ];
        for (outcome, attempts, class, retry_state, impact, remediation, eligible, terminal) in rows
        {
            let payload = interpreter_startup_inc_payload(&snapshot(outcome, attempts), None);
            let label = format!("{outcome:?}/{attempts}");
            assert_eq!(str_at(&payload, "/outcome_code")?, outcome.code(), "{label}");
            assert_eq!(str_at(&payload, "/explanation_class")?, class, "{label}");
            assert_eq!(str_at(&payload, "/retry_state")?, retry_state, "{label}");
            assert_eq!(str_at(&payload, "/lookup_impact")?, impact, "{label}");
            assert_eq!(str_at(&payload, "/remediation_code")?, remediation, "{label}");
            assert_eq!(payload.pointer("/retry_eligible"), Some(&json!(eligible)), "{label}");
            assert_eq!(payload.pointer("/terminal"), Some(&json!(terminal)), "{label}");
            assert_eq!(payload.pointer("/attempts_consumed"), Some(&json!(attempts)), "{label}");
            assert_eq!(payload.pointer("/max_attempts"), Some(&json!(2)), "{label}");
            assert_eq!(str_at(&payload, "/owner/scope")?, "global", "{label}");
        }

        // A first timeout with a retry remaining must never be terminal, and
        // a second timeout must never be retryable (falsifiers 2 and 3).
        let first = interpreter_startup_inc_payload(&snapshot(K::TimedOut, 1), None);
        assert_ne!(str_at(&first, "/explanation_class")?, "terminal_degraded");
        let second = interpreter_startup_inc_payload(&snapshot(K::TimedOut, 2), None);
        assert_ne!(str_at(&second, "/retry_state")?, "one_retry_remains");
        Ok(())
    }

    /// `SuccessfulEmpty` is a legitimate empty result and `Disabled` is a
    /// configuration choice; neither may be flattened into a failure class
    /// (falsifiers 4 and 5), and only those two plus `Paths` make a
    /// not-found exact (falsifier 6).
    #[test]
    fn not_found_is_exact_only_when_startup_roots_participated_or_were_configured_off() {
        use SystemIncProbeOutcomeKind as K;
        for (outcome, attempts, expect_complete) in [
            (K::Paths, 1, true),
            (K::SuccessfulEmpty, 1, true),
            (K::Disabled, 0, true),
            (K::NotObserved, 0, false),
            (K::TimedOut, 1, false),
            (K::TimedOut, 2, false),
            (K::Unavailable, 1, false),
            (K::IoFailed, 1, false),
            (K::NonZeroExit, 1, false),
        ] {
            let snap = snapshot(outcome, attempts);
            let impact = snap.lookup_impact();
            assert_eq!(
                search_complete(&ModuleUriResolution::NotFound, impact),
                expect_complete,
                "{outcome:?}/{attempts}"
            );
            let message =
                missing_module_user_message("Foo::Bar", &ModuleUriResolution::NotFound, 50, &snap);
            assert_eq!(
                message.contains("not an exact not-found"),
                !expect_complete,
                "{outcome:?}/{attempts}: {message}"
            );
            let result =
                module_lookup_result("Foo::Bar", &ModuleUriResolution::NotFound, 50, &[], impact);
            assert_eq!(result.pointer("/status").and_then(Value::as_str), Some("not_found"));
            assert_eq!(result.pointer("/search_complete"), Some(&json!(expect_complete)));
        }
        // A resolved module is complete even when startup roots were omitted.
        assert!(search_complete(
            &ModuleUriResolution::Resolved("file:///x/Foo/Bar.pm".to_string()),
            SystemIncLookupImpact::OmittedTerminal
        ));
        assert!(!search_complete(
            &ModuleUriResolution::TimedOut,
            SystemIncLookupImpact::Participated
        ));
    }

    /// Remediation codes must name only actions a user can perform; no
    /// user-facing channel writes the interpreter path (#5376 rule).
    #[test]
    fn remediation_codes_never_name_an_unsettable_interpreter_setting() {
        use SystemIncProbeOutcomeKind as K;
        for (outcome, attempts) in [
            (K::Disabled, 0),
            (K::NotObserved, 0),
            (K::TimedOut, 1),
            (K::TimedOut, 2),
            (K::Unavailable, 1),
            (K::IoFailed, 1),
            (K::NonZeroExit, 1),
            (K::SuccessfulEmpty, 1),
            (K::Paths, 1),
        ] {
            let code = startup_inc_remediation(&snapshot(outcome, attempts));
            for forbidden in ["perl_path", "perlpath", "perl.path", "pin"] {
                assert!(
                    !code.to_ascii_lowercase().contains(forbidden),
                    "{outcome:?}/{attempts}: remediation {code:?} names an unsettable route ({forbidden})"
                );
            }
        }
    }

    /// The projection carries counts and codes only: no root path, home
    /// path, command line, or environment value (falsifier 10).
    #[test]
    fn projection_is_redacted_to_codes_and_counts() -> TestResult {
        let payload = interpreter_startup_inc_payload(
            &snapshot(SystemIncProbeOutcomeKind::Paths, 1),
            Some("file:///home/someone/project/"),
        );
        assert_eq!(payload.pointer("/system_root_count"), Some(&json!(3)));
        assert_eq!(str_at(&payload, "/owner/scope")?, "workspace_folder");
        for key in ["paths", "system_paths", "perl_path", "command", "stderr", "env"] {
            assert!(payload.get(key).is_none(), "projection must not carry {key}");
        }
        Ok(())
    }

    /// Reading the explanation never launches or retries Perl, and after the
    /// live resolver acquires the state the explanation reports exactly what
    /// that stored subject holds (falsifiers 1 and 7).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn explanation_never_probes_and_reflects_the_live_stored_state() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let script = workspace.join("script.pl");
        std::fs::create_dir_all(&workspace)?;
        let source = "use Missing::Payload;\n";
        std::fs::write(&script, source)?;
        let workspace_uri = file_uri(&workspace)?;
        let script_uri = file_uri(&script)?;

        let server = LspServer::new();
        *server.workspace_folders.lock() = vec![
            WorkspaceFolderState::new(workspace_uri.clone())
                .with_path(workspace.clone())
                .with_effective_workspace_config(unspawnable_system_inc_config()),
        ];
        *server.root_path.lock() = Some(workspace);

        let stored_state = || -> Result<SystemIncProbeSnapshot, String> {
            server
                .workspace_folders
                .lock()
                .iter()
                .find(|folder| folder.uri == workspace_uri)
                .map(|folder| folder.effective_workspace_config.peek_system_inc_probe())
                .ok_or_else(|| "stored folder config missing".to_string())
        };

        // Two explanations before any live lookup: nothing attempted.
        for _ in 0..2 {
            let payload = explain(&server, "Missing::Payload", &script_uri)?;
            let startup = payload
                .pointer("/module_resolution/interpreter_startup_inc")
                .ok_or("missing interpreter_startup_inc")?;
            assert_eq!(str_at(startup, "/outcome_code")?, "not_observed");
            assert_eq!(str_at(startup, "/lookup_impact")?, "not_observed");
            assert_eq!(startup.pointer("/attempts_consumed"), Some(&json!(0)));
            assert_eq!(str_at(startup, "/owner/scope")?, "workspace_folder");
            assert_eq!(str_at(startup, "/owner/folder_uri")?, workspace_uri.as_str());
            assert_eq!(str_at(&payload, "/module_resolution/result/status")?, "not_found");
            assert_eq!(
                payload.pointer("/module_resolution/result/search_complete"),
                Some(&json!(false))
            );
            assert_eq!(str_at(&payload, "/copyable_payload/startup_inc_outcome")?, "not_observed");
            assert!(str_at(&payload, "/user_message")?.contains("not an exact not-found"));
        }
        assert_eq!(
            stored_state()?.attempts_consumed,
            0,
            "explaining must not spend a probe attempt on the stored config"
        );

        // The live resolver path acquires the state: one attempt, settled.
        let live = server
            .effective_inc_context_for_doc(Some(&script_uri), Some(source), Some(0))
            .ok_or("expected live context")?;
        assert_eq!(live.system_inc_state.outcome, SystemIncProbeOutcomeKind::IoFailed);
        assert_eq!(stored_state()?.attempts_consumed, 1);

        // The explanation now reports the stored subject's settled outcome
        // and still spends nothing.
        let payload = explain(&server, "Missing::Payload", &script_uri)?;
        let startup = payload
            .pointer("/module_resolution/interpreter_startup_inc")
            .ok_or("missing interpreter_startup_inc")?;
        assert_eq!(str_at(startup, "/outcome_code")?, "io_failed");
        assert_eq!(str_at(startup, "/explanation_class")?, "terminal_failed");
        assert_eq!(str_at(startup, "/lookup_impact")?, "omitted_terminal");
        assert_eq!(startup.pointer("/attempts_consumed"), Some(&json!(1)));
        assert_eq!(startup.pointer("/terminal"), Some(&json!(true)));
        assert_eq!(
            str_at(&payload, "/copyable_payload/startup_inc_lookup_impact")?,
            "omitted_terminal"
        );
        assert!(str_at(&payload, "/user_message")?.contains("process or I/O boundary"));
        assert_eq!(stored_state()?.attempts_consumed, 1);
        Ok(())
    }

    /// Folder A's probe state cannot attach to folder B's lookup, and a
    /// configuration change makes the old outcome unavailable rather than
    /// current (falsifiers 8 and 9).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn explanation_binds_to_the_owning_folder_and_drops_stale_outcomes() -> TestResult {
        let temp = tempfile::tempdir()?;
        let folder_a = temp.path().join("a");
        let folder_b = temp.path().join("b");
        let script_a = folder_a.join("run.pl");
        let script_b = folder_b.join("run.pl");
        std::fs::create_dir_all(&folder_a)?;
        std::fs::create_dir_all(&folder_b)?;
        std::fs::write(&script_a, "use Missing::A;\n")?;
        std::fs::write(&script_b, "use Missing::B;\n")?;
        let uri_a = file_uri(&folder_a)?;
        let uri_b = file_uri(&folder_b)?;
        let script_uri_a = file_uri(&script_a)?;
        let script_uri_b = file_uri(&script_b)?;

        let mut config_b = WorkspaceConfig::default();
        config_b.use_system_inc = false;
        config_b.use_perl5lib = false;

        let server = LspServer::new();
        *server.workspace_folders.lock() = vec![
            WorkspaceFolderState::new(uri_a.clone())
                .with_path(folder_a.clone())
                .with_effective_workspace_config(unspawnable_system_inc_config()),
            WorkspaceFolderState::new(uri_b.clone())
                .with_path(folder_b)
                .with_effective_workspace_config(config_b),
        ];
        *server.root_path.lock() = Some(folder_a);

        // Advance folder A through the live path so it holds a settled outcome.
        server
            .effective_inc_context_for_doc(Some(&script_uri_a), Some("use Missing::A;\n"), Some(0))
            .ok_or("expected live context for folder A")?;

        let payload_b = explain(&server, "Missing::B", &script_uri_b)?;
        let startup_b = payload_b
            .pointer("/module_resolution/interpreter_startup_inc")
            .ok_or("missing interpreter_startup_inc for B")?;
        assert_eq!(str_at(startup_b, "/owner/folder_uri")?, uri_b.as_str());
        assert_eq!(str_at(startup_b, "/outcome_code")?, "disabled");
        assert_eq!(str_at(startup_b, "/lookup_impact")?, "disabled");
        assert_eq!(startup_b.pointer("/attempts_consumed"), Some(&json!(0)));
        assert_eq!(
            payload_b.pointer("/module_resolution/result/search_complete"),
            Some(&json!(true)),
            "a configured-off folder's not-found is exact"
        );

        let payload_a = explain(&server, "Missing::A", &script_uri_a)?;
        let startup_a = payload_a
            .pointer("/module_resolution/interpreter_startup_inc")
            .ok_or("missing interpreter_startup_inc for A")?;
        assert_eq!(str_at(startup_a, "/owner/folder_uri")?, uri_a.as_str());
        assert_eq!(str_at(startup_a, "/outcome_code")?, "io_failed");

        // Invalidate folder A's configuration: the settled outcome must not
        // be published as current afterwards.
        {
            let mut folders = server.workspace_folders.lock();
            let folder =
                folders.iter_mut().find(|folder| folder.uri == uri_a).ok_or("folder A missing")?;
            folder
                .effective_workspace_config
                .update_from_value(&json!({ "workspace": { "usePerl5lib": true } }));
        }
        let payload_after = explain(&server, "Missing::A", &script_uri_a)?;
        let startup_after = payload_after
            .pointer("/module_resolution/interpreter_startup_inc")
            .ok_or("missing interpreter_startup_inc after invalidation")?;
        assert_eq!(str_at(startup_after, "/outcome_code")?, "not_observed");
        assert_eq!(startup_after.pointer("/attempts_consumed"), Some(&json!(0)));
        assert_eq!(startup_after.pointer("/use_perl5lib"), Some(&json!(true)));
        Ok(())
    }
}
