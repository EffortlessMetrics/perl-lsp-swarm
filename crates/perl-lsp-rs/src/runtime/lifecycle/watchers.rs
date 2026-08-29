//! File watcher registration
//!
//! Handles registration of file watchers for workspace files.

use super::super::{LspServer, json};
use gen_lsp_types::{
    BaseUri, DidChangeWatchedFilesNotification, DidChangeWatchedFilesRegistrationOptions,
    FileSystemWatcher, GlobPattern, Notification, Registration, RegistrationParams,
    RelativePattern, Uri, WatchKind,
};

const PERL_WATCH_PATTERNS: &[&str] = &["**/*.pl", "**/*.pm", "**/*.t", "**/*.psgi"];

fn perl_watch_kind() -> WatchKind {
    WatchKind::Create | WatchKind::Change | WatchKind::Delete
}

/// Validate a workspace URI string and wrap it in the substrate's String-backed `Uri`.
fn validated_workspace_uri(uri: &str) -> Option<Uri> {
    match url::Url::parse(uri) {
        Ok(parsed) => Some(Uri(parsed.as_str().to_string())),
        Err(error) => {
            tracing::debug!(uri, %error, "Skipping invalid workspace URI for file watcher RelativePattern");
            None
        }
    }
}

fn string_file_watchers() -> Vec<FileSystemWatcher> {
    PERL_WATCH_PATTERNS
        .iter()
        .map(|pattern| FileSystemWatcher {
            glob_pattern: GlobPattern::Pattern((*pattern).to_string()),
            kind: Some(perl_watch_kind()),
        })
        .collect()
}

impl LspServer {
    /// Register file watchers for Perl files
    pub(crate) fn register_file_watchers_if_needed(&self) {
        if !self.runtime_tuning().file_watchers {
            tracing::debug!("Skipping file watcher registration; runtime tuning disabled watchers");
            return;
        }

        if !self.client_capabilities.lock().dynamic_registration_support {
            return;
        }

        if !self.advertised_features.lock().workspace_symbol {
            return;
        }

        let supports_relative_patterns =
            self.client_capabilities.lock().file_watcher_relative_pattern_support;
        let watchers = if supports_relative_patterns {
            self.relative_file_watchers().unwrap_or_else(string_file_watchers)
        } else {
            string_file_watchers()
        };

        let opts = DidChangeWatchedFilesRegistrationOptions { watchers };
        let register_options = match serde_json::to_value(opts) {
            Ok(val) => Some(val),
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize file watcher options");
                return;
            }
        };
        let reg = Registration {
            id: "perl-didChangeWatchedFiles".into(),
            method: <DidChangeWatchedFilesNotification as Notification>::METHOD.to_string(),
            register_options,
        };

        let params = RegistrationParams { registrations: vec![reg] };
        let params_value = match serde_json::to_value(&params) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize registration params");
                return;
            }
        };

        if let Err(error) = self.send_request("client/registerCapability", params_value) {
            tracing::error!(%error, "Failed to send file watcher registration request");
        }
    }

    fn relative_file_watchers(&self) -> Option<Vec<FileSystemWatcher>> {
        let workspace_uris = self.workspace_folder_uris();
        let base_uris = workspace_uris
            .iter()
            .filter_map(|uri| validated_workspace_uri(uri))
            .collect::<Vec<_>>();

        if base_uris.is_empty() {
            tracing::debug!(
                "Falling back to string file watcher globs; no valid workspace URI for RelativePattern"
            );
            return None;
        }

        let watchers = base_uris
            .into_iter()
            .flat_map(|base_uri| {
                PERL_WATCH_PATTERNS.iter().map(move |pattern| FileSystemWatcher {
                    glob_pattern: GlobPattern::RelativePattern(RelativePattern {
                        base_uri: BaseUri::Uri(base_uri.clone()),
                        pattern: (*pattern).to_string(),
                    }),
                    kind: Some(perl_watch_kind()),
                })
            })
            .collect::<Vec<_>>();

        Some(watchers)
    }

    pub(crate) fn register_inline_completion_if_needed(&self) {
        let should_register = {
            let caps = self.client_capabilities.lock();
            caps.inline_completion_dynamic_registration_support
                && self.advertised_features.lock().inline_completion
        };

        if !should_register {
            return;
        }

        let params = json!({
            "registrations": [{
                "id": "perl-inlineCompletion",
                "method": "textDocument/inlineCompletion",
                "registerOptions": {
                    "documentSelector": [
                        { "language": "perl" },
                        { "language": "perl5" }
                    ]
                }
            }]
        });

        if let Err(error) = self.send_request("client/registerCapability", params) {
            tracing::warn!(%error, "Failed to register inline completion capability");
        }
    }
}
