//! Workspace management
//!
//! Handles workspace folders and root URI/path management.

#[cfg(test)]
use super::super::*;
use super::super::{LspServer, MessageType};
use perl_dap::platform::{PerlInterpreterResult, find_perl_interpreter};
use perl_lsp_rs_core::config::WorkspaceConfig;
use std::sync::Once;

/// Fires at most once per LSP session, when Perl is not found anywhere.
static PERL_NOT_FOUND_WARNED: Once = Once::new();

use crate::perl_remediation::PERL_REMEDIATION;

/// Message for "Perl was found, but only via an OS fallback path".
///
/// Kept separate from the emitting code so the wording is directly testable.
fn perl_fallback_message(path: &std::path::Path, label: &str) -> String {
    format!(
        "Perl LSP: Perl not found on PATH; using {label} at {}. \
         Add Perl to PATH to suppress this message.",
        path.display()
    )
}

/// Message for "no Perl interpreter could be used".
///
/// `configured` is the interpreter path the server was told to use, when one is
/// set — in that case the path itself is the thing to check, so the generic
/// install-Perl advice would be misleading.
fn perl_not_found_message(configured: Option<&str>) -> String {
    // An empty configured path is not a configuration the user can act on, so it
    // falls through to the generic install guidance rather than telling them to
    // check `` (#5034 review).
    match configured.filter(|path| !path.is_empty()) {
        Some(configured) => {
            format!(
                "Perl LSP: The configured Perl interpreter was not found at `{configured}`. \
                 Check that the path exists and is executable, then restart/reload server. \
                 (VS Code: Developer: Reload Window.)"
            )
        }
        None => format!("Perl LSP: Perl missing on PATH. {PERL_REMEDIATION}"),
    }
}

impl LspServer {
    /// Set the root path from the root URI during initialization
    ///
    /// Also performs one-time `.perltidyrc` discovery for the workspace so a
    /// project-local profile applies without explicit configuration. The
    /// discovered profile's supported scalar options are applied to the server
    /// config here — at initialize, **before** `.perl-lsp.toml` and
    /// `didChangeConfiguration` are applied — so they override the built-in
    /// defaults while explicit user configuration still wins. The discovered
    /// path is also cached for the external adapter (`--profile`). An explicitly
    /// configured `perltidy_profile` takes precedence when the formatter config
    /// is built.
    pub(crate) fn set_root_uri(&self, root_uri: &str) {
        let root_path = super::super::source_path_from_uri(root_uri);
        let discovered =
            root_path.as_deref().and_then(perl_lsp_rs_core::config::discover_perltidy_profile);
        let discovered_options =
            discovered.as_deref().and_then(super::super::read_perltidy_native_options);
        *self.root_path.lock() = root_path;
        *self.discovered_perltidy_profile.lock() = discovered;
        // Apply the discovered profile's options as a base layer over the
        // built-in defaults. This must happen before user config is applied;
        // a per-request `.or()` merge cannot work because the defaults are
        // `Some(..)` and would always short-circuit the profile value.
        if let Some(options) = discovered_options {
            self.config.lock().apply_perltidy_native_options(&options);
        }
    }

    /// Detect the Perl interpreter and surface an actionable message if not found.
    ///
    /// Called once during `handle_initialize`. Uses the configured interpreter path
    /// if one is present, then falls back to full OS-aware detection. Emits:
    ///
    /// - `window/logMessage` (Info) if Perl was found via an OS fallback path so the
    ///   user knows which interpreter will be used.
    /// - `window/showMessage` (Error) **once per session** if no Perl interpreter is found
    ///   anywhere, with actionable remediation text.
    /// - `window/showMessage` (Error) **once per session** if an interpreter path is
    ///   configured but does not exist.
    ///
    /// Does not alter any server state. Tracing fallback is preserved alongside user messages.
    pub(crate) fn check_perl_interpreter(&self) {
        let configured_path = self.workspace_config.lock().perl_path.clone();
        let result = find_perl_interpreter(configured_path.as_deref());

        match result {
            PerlInterpreterResult::ConfiguredPath(ref path) => {
                tracing::debug!(path = %path.display(), "Perl interpreter: using configured path");
            }
            PerlInterpreterResult::FoundOnPath(ref path) => {
                tracing::debug!(path = %path.display(), "Perl interpreter: found on PATH");
            }
            PerlInterpreterResult::FoundViaFallback { ref path, ref label } => {
                let msg = perl_fallback_message(path, label);
                tracing::info!(path = %path.display(), label = %label, "Perl interpreter found via fallback");
                if let Err(e) = self.log_message(MessageType::Info, &msg) {
                    tracing::warn!(error = %e, "Failed to send logMessage for perl fallback");
                }
            }
            PerlInterpreterResult::NotFound { ref searched } => {
                tracing::warn!(searched = ?searched, "Perl interpreter not found");
                PERL_NOT_FOUND_WARNED.call_once(|| {
                    let msg = perl_not_found_message(configured_path.as_deref());
                    if let Err(e) = self.show_message(MessageType::Error, &msg) {
                        tracing::warn!(error = %e, "Failed to send showMessage for perl not found");
                    }
                });
            }
        }
    }

    /// Load `.perl-lsp.toml` from each workspace folder and compute per-folder effective config.
    ///
    /// Called once during `handle_initialize`, after workspace folders are populated and
    /// before the server returns capabilities. Subsequent `didChangeConfiguration`
    /// notifications will override these values (LSP wins over TOML).
    ///
    /// On TOML parse error, emits a `window/showMessage` Warning so the user can fix the file.
    /// In single-file mode (no workspace folders), returns early without searching.
    ///
    /// Multi-root workspaces: each folder loads its own `.perl-lsp.toml` independently.
    ///
    /// The `[perl]` section is scoped per-folder through `effective_workspace_config`.
    /// The six server-global sections (`[diagnostics]`, `[critic]`, `[features]`,
    /// `[formatting]`, `[ai_completion]`, `[next_edit]`) target the single shared
    /// `ServerConfig`; they are merged with **first-folder-wins** semantics via
    /// [`perl_lsp_rs_core::config::merge_project_configs_for_server`] so a later
    /// folder can no longer silently overwrite an earlier folder's setting. When two
    /// or more folders set the same global key to different values, a single
    /// `window/showMessage` Warning is emitted naming the folders and keys, instead
    /// of silently discarding a folder's configuration.
    pub(crate) fn load_and_apply_project_config(&self) {
        let mut folders = self.workspace_folders.lock();

        if folders.is_empty() {
            // Single-file mode: try to discover .perl-lsp.toml from the
            // open document's directory. This is a common workflow — opening
            // a lone .pl file that has a .perl-lsp.toml next to it. (#UX15)
            if let Some(config) = self.discover_single_file_config() {
                let mut server_config = self.config.lock();
                config.apply_to_server_config(&mut server_config);
            }
            return;
        }

        // Collect (display_name, project_config) for folders that have a
        // .perl-lsp.toml, in workspace-folder iteration order, so the server-global
        // sections can be merged with first-folder-wins semantics after the loop.
        let mut global_configs: Vec<(String, perl_lsp_rs_core::config::ProjectConfig)> =
            Vec::with_capacity(folders.len());

        for folder in folders.iter_mut() {
            // Try to load .perl-lsp.toml from this folder
            if let Some(folder_path) = &folder.path {
                folder.project_config = None;

                // Start with initializationOptions.perl.* as the base layer, then
                // layer .perl-lsp.toml on top so project config wins.
                let mut effective_config = WorkspaceConfig::default();
                if let Some(init_opts) = self.initialization_options_perl_settings.lock().as_ref() {
                    let rejected = effective_config.update_from_value(init_opts);
                    for entry in rejected {
                        tracing::warn!(
                            target: "perl_lsp::config",
                            folder_uri = %folder.uri,
                            entry = %entry.entry,
                            reason = %entry.render(),
                            "rejected initializationOptions includePaths entry"
                        );
                    }
                }
                folder.effective_workspace_config = effective_config;

                match perl_lsp_rs_core::config::load_project_config(folder_path) {
                    Ok(None) => {
                        // No .perl-lsp.toml found — normal, no action needed
                    }
                    Ok(Some(project_config)) => {
                        tracing::debug!(path = %folder_path.display(), "Loaded .perl-lsp.toml for folder");

                        // Store project config in the folder state
                        folder.project_config = Some(project_config.clone());

                        // Layer project config on top of the init-options base
                        // already stored in folder.effective_workspace_config.
                        let rejected_include_paths = project_config.apply_to_workspace_config(
                            &mut folder.effective_workspace_config,
                            folder_path,
                        );
                        if !rejected_include_paths.is_empty() {
                            self.emit_rejected_include_paths_warning(
                                folder.display_name(),
                                &rejected_include_paths,
                            );
                        }

                        // Defer the server-global sections to the post-loop merge so a
                        // later folder cannot silently clobber an earlier folder's value.
                        global_configs.push((folder.display_name().to_string(), project_config));
                    }
                    Err(msg) => {
                        let user_msg = format!(
                            "Perl LSP: {msg} \
                             Fix the error in .perl-lsp.toml and reload the window \
                             (Ctrl+Shift+P \u{2192} Developer: Reload Window) to apply your settings.",
                        );
                        tracing::warn!(message = %user_msg, "Project config warning");
                        // Emit user-visible warning so devs can fix a broken .perl-lsp.toml
                        if let Err(e) = self.notify(
                            "window/showMessage",
                            serde_json::json!({
                                "type": 2, // Warning
                                "message": user_msg
                            }),
                        ) {
                            tracing::warn!(error = %e, "Failed to send showMessage warning");
                        }
                    }
                }
                folder.refresh_workspace_metadata();
            }
        }

        // Merge the server-global sections across all folders that have a config,
        // using first-folder-wins per field, then apply the merged result to the
        // shared ServerConfig exactly once. This replaces the former per-folder
        // `apply_to_server_config` loop, which silently let the last folder win.
        if !global_configs.is_empty() {
            let merge_inputs: Vec<(&str, &perl_lsp_rs_core::config::ProjectConfig)> =
                global_configs.iter().map(|(name, cfg)| (name.as_str(), cfg)).collect();
            let (merged, conflicts) =
                perl_lsp_rs_core::config::merge_project_configs_for_server(&merge_inputs);

            {
                let mut config = self.config.lock();
                merged.apply_to_server_config(&mut config);
            }

            if !conflicts.is_empty() {
                self.emit_multi_root_config_conflict_warning(&conflicts);
            }
        }

        // Client-scoped `workspace/configuration` is deliberately deferred to
        // the post-initialize lifecycle. During `initialize` only local project
        // and initialization-option state may be applied; server→client requests
        // are not legal until after InitializeResult has been returned (#7708).
        drop(folders);
    }

    /// In single-file mode, try to discover `.perl-lsp.toml` from the
    /// directory of the first open document. (#UX15)
    fn discover_single_file_config(&self) -> Option<perl_lsp_rs_core::config::ProjectConfig> {
        let documents = self.documents.lock();
        let uri = documents.keys().next()?.to_string();
        drop(documents);

        let path = super::super::source_path_from_uri(&uri)?;
        let dir = std::path::Path::new(&path).parent()?;
        perl_lsp_rs_core::config::load_project_config(dir).ok().flatten()
    }

    /// Emit a `window/showMessage` Warning describing the conflicting
    /// server-global `.perl-lsp.toml` keys across a multi-root workspace.
    ///
    /// The warning names each conflicting key and the per-folder values, and
    /// states the first-folder-wins resolution, so a silently overwritten
    /// folder setting becomes visible. See `docs/reference/CONFIG.md`.
    fn emit_multi_root_config_conflict_warning(
        &self,
        conflicts: &[perl_lsp_rs_core::config::MultiRootConfigConflict],
    ) {
        let rendered = conflicts
            .iter()
            .map(perl_lsp_rs_core::config::MultiRootConfigConflict::render)
            .collect::<Vec<_>>()
            .join("; ");
        let user_msg = format!(
            "Perl LSP: multi-root workspace has conflicting .perl-lsp.toml settings across \
             folders. The first folder wins for each key; others were ignored: {rendered}. \
             See docs/reference/CONFIG.md (Multi-root workspaces) for details."
        );
        tracing::warn!(conflicts = %rendered, "Multi-root config conflict; first folder wins");
        if let Err(e) = self.show_message(MessageType::Warning, &user_msg) {
            tracing::warn!(error = %e, "Failed to send showMessage for multi-root config conflict");
        }
    }

    /// Emit a `window/showMessage` Warning describing `.perl-lsp.toml`
    /// `include_paths` entries rejected during load — e.g. absolute paths or
    /// entries that escape the workspace root (see the `# Security` doc
    /// comment on [`perl_lsp_rs_core::config::ProjectConfig::apply_to_workspace_config`]).
    ///
    /// Only called from the initial-load path (this function) — reconfiguration
    /// re-application call sites re-apply an already-loaded, already-warned-about
    /// `project_config` and intentionally do not re-warn, to avoid spamming the
    /// user on every settings change.
    fn emit_rejected_include_paths_warning(
        &self,
        folder_name: &str,
        rejected: &[perl_lsp_rs_core::config::RejectedIncludePath],
    ) {
        let rendered = rejected
            .iter()
            .map(perl_lsp_rs_core::config::RejectedIncludePath::render)
            .collect::<Vec<_>>()
            .join("; ");
        let user_msg = format!(
            "Perl LSP: {folder_name}'s .perl-lsp.toml has include_paths entries that were \
             ignored: {rendered}"
        );
        tracing::warn!(folder = %folder_name, rejected = %rendered, "Rejected include_paths entries");
        if let Err(e) = self.show_message(MessageType::Warning, &user_msg) {
            tracing::warn!(error = %e, "Failed to send showMessage for rejected include_paths");
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    #![expect(
        clippy::expect_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]

    use super::*;

    #[test]
    fn check_perl_interpreter_does_not_panic() {
        let server = LspServer::new();
        // Should complete without panicking regardless of Perl install state
        server.check_perl_interpreter();
    }

    #[test]
    fn check_perl_interpreter_broken_config_path_does_not_panic() {
        let server = LspServer::new();
        // Set a broken perl path in the workspace config
        server.workspace_config.lock().perl_path =
            Some("/nonexistent/path/to/perl/that/does/not/exist".to_string());
        // Should not panic — message will fail to send silently (stdout in test mode)
        server.check_perl_interpreter();
    }

    #[test]
    fn check_perl_interpreter_valid_config_path_does_not_panic() {
        use std::fs;
        let tempdir = tempfile::tempdir().expect("tempdir");
        #[cfg(windows)]
        let fake_perl = tempdir.path().join("perl.exe");
        #[cfg(not(windows))]
        let fake_perl = tempdir.path().join("perl");
        fs::write(&fake_perl, "").expect("write fake perl");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&fake_perl).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake_perl, perms).expect("chmod");
        }
        let server = LspServer::new();
        server.workspace_config.lock().perl_path = Some(fake_perl.to_string_lossy().to_string());
        // Should not panic
        server.check_perl_interpreter();
    }

    // =====================================================================
    // Perl-not-found remediation must not name a setting that does not exist
    // (#5034). Mirrors the DAP-side guard in
    // crates/perl-dap/tests/dap_launch_error_remediation_tests.rs.
    // =====================================================================

    /// Every user-facing interpreter message, for the "must not mention" guards.
    fn all_perl_interpreter_messages() -> Vec<String> {
        vec![
            perl_fallback_message(std::path::Path::new("/usr/local/bin/perl"), "Homebrew"),
            perl_not_found_message(None),
            perl_not_found_message(Some("/opt/custom/perl")),
        ]
    }

    #[test]
    fn perl_messages_never_name_the_nonexistent_perl_path_setting() {
        // `perl-lsp.perl.path` is not in the extension's contributes.configuration,
        // `.perl-lsp.toml`'s [perl] section has no interpreter field, and
        // workspace-scoped perlPath is ignored for security (#3729). Advising it
        // sends the user somewhere they cannot act.
        // Matched on the bare `perl.path` substring, which also covers the
        // prefixed `perl-lsp.perl.path` spelling. The narrower prefixed match
        // was why #5376's two messages went uncaught: they said `perl.path`.
        for msg in all_perl_interpreter_messages() {
            assert!(
                !msg.contains("perl.path"),
                "message must not point at the nonexistent perl.path setting, got: {msg}"
            );
        }
    }

    #[test]
    fn perl_messages_never_name_dap_only_launch_json_keys() {
        // `perlPath` is a launch.json/DAP key and does not affect the language
        // server, so naming it here would be a different wrong answer.
        for msg in all_perl_interpreter_messages() {
            assert!(
                !msg.contains("launch.json"),
                "language-server message must not send users to launch.json, got: {msg}"
            );
        }
    }

    #[test]
    fn perl_not_found_message_gives_actionable_install_guidance() {
        let msg = perl_not_found_message(None);
        // Remediation the user can actually perform.
        assert!(msg.contains("PATH"), "must mention PATH, got: {msg}");
        assert!(msg.contains("Install Perl"), "must tell the user to install Perl, got: {msg}");
        assert!(msg.contains("Reload Window"), "must say how to re-trigger detection, got: {msg}");
        assert!(
            !msg.contains("/usr/bin") && !msg.contains("/bin"),
            "searched paths must remain out of the toast, got: {msg}"
        );
    }

    #[test]
    fn not_found_message_names_both_platform_install_routes() {
        // Guards against the remediation being collapsed to one canonical
        // sentence: a Windows user needs the Strawberry Perl link and a macOS
        // user needs the brew line, and neither is inferable from the other.
        let msg = perl_not_found_message(None);
        assert!(msg.contains("strawberryperl.com"), "must name the Windows route, got: {msg}");
        assert!(msg.contains("brew install perl"), "must name the macOS route, got: {msg}");
        assert!(msg.contains("package manager"), "must name the Linux route, got: {msg}");
    }

    #[test]
    fn configured_path_message_points_at_the_configured_path_not_at_installing_perl() {
        // When a path is configured, the path is the problem — generic
        // "install Perl" advice would be a misdiagnosis.
        let msg = perl_not_found_message(Some("/opt/custom/perl"));
        assert!(msg.contains("/opt/custom/perl"), "must name the configured path, got: {msg}");
        assert!(
            !msg.contains("Install Perl"),
            "a configured-but-missing path is not an install problem, got: {msg}"
        );
        assert!(
            msg.contains("restart/reload server"),
            "configured path must provide a client-neutral re-trigger action, got: {msg}"
        );
    }

    #[test]
    fn configured_path_message_prints_the_configured_path_once() {
        let configured = "/nonexistent/perl";
        let msg = perl_not_found_message(Some(configured));
        assert_eq!(
            msg.matches(configured).count(),
            1,
            "configured path should appear exactly once, got: {msg}"
        );
        assert!(!msg.contains("Also searched"), "search detail belongs in logs, got: {msg}");
    }

    #[test]
    fn messages_contain_no_doubled_whitespace() {
        // Rust's `\<newline>` continuation strips the newline and the following
        // line's indentation, so the multi-line literals render as single
        // spaces. Pinned because it is invisible in the source.
        for msg in all_perl_interpreter_messages() {
            assert!(!msg.contains("  "), "message has doubled whitespace: {msg:?}");
            assert!(!msg.contains('\n'), "message has a newline: {msg:?}");
        }
    }

    #[test]
    fn configured_path_message_keeps_search_detail_out_of_the_toast() {
        let msg = perl_not_found_message(Some("/nonexistent/perl"));
        assert!(!msg.contains("PATH"), "search detail should remain in tracing, got: {msg}");
    }

    #[test]
    fn interpreter_not_found_toasts_keep_the_actionable_text_compact() {
        for msg in all_perl_interpreter_messages() {
            assert!(
                msg.chars().count() <= 240,
                "toast is too long: {} chars: {msg}",
                msg.chars().count()
            );
        }
    }

    #[test]
    fn interpreter_messages_use_the_canonical_product_prefix() {
        for msg in all_perl_interpreter_messages() {
            assert!(msg.starts_with("Perl LSP: "), "unexpected product prefix: {msg}");
        }
    }

    #[test]
    fn empty_configured_path_falls_back_to_install_guidance() {
        // An empty string is not something the user can go check, so it must be
        // treated as "nothing configured" rather than producing "not found at ``".
        let msg = perl_not_found_message(Some(""));
        assert!(msg.contains("Install Perl"), "empty config must give install advice, got: {msg}");
        assert!(!msg.contains("not found at"), "must not render an empty path, got: {msg}");
    }

    #[test]
    fn fallback_message_names_the_interpreter_and_how_to_silence_it() {
        let msg = perl_fallback_message(std::path::Path::new("/opt/homebrew/bin/perl"), "Homebrew");
        assert!(msg.contains("/opt/homebrew/bin/perl"), "must name the interpreter, got: {msg}");
        assert!(msg.contains("Homebrew"), "must name the fallback source, got: {msg}");
        assert!(
            msg.contains("Add Perl to PATH"),
            "must give the one action that suppresses it, got: {msg}"
        );
    }

    #[test]
    fn load_and_apply_project_config_handles_empty_workspace_folders() {
        let server = LspServer::new();
        // Should not panic with empty workspace folders
        server.load_and_apply_project_config();
    }

    #[test]
    fn set_root_uri_ignores_non_file_scheme() {
        let server = LspServer::new();
        server.set_root_uri("untitled:Untitled-1");
        assert!(server.root_path.lock().is_none());
        // With no file-scheme root there is no workspace to search, so discovery
        // contributes nothing regardless of the ambient environment.
        assert!(server.discovered_perltidy_profile.lock().is_none());
    }

    #[test]
    fn set_root_uri_discovers_workspace_perltidyrc() -> std::io::Result<()> {
        let server = LspServer::new();
        let temp = tempfile::tempdir()?;
        let profile = temp.path().join(".perltidyrc");
        std::fs::write(&profile, "-l=100\n")?;

        server.set_root_uri(&format!("file://{}", temp.path().display()));

        // The workspace profile is searched first, so this assertion holds
        // regardless of any ambient $HOME/.perltidyrc or $PERLTIDY on the host.
        //
        // On Windows the URI-to-path round-trip lowercases the drive letter (the
        // `normalize_windows_path_to_key` helper in perl-uri lowercases it for URI
        // normalisation, so the stored path is e.g. `c:\…` while `profile.to_str()`
        // retains the OS-reported `C:\…`). Canonicalize both sides so that the
        // assertion tests path *equivalence* rather than byte equality, keeping the
        // check meaningful (wrong path → `canonicalize` succeeds on a different
        // location → paths still differ) without producing false failures on Windows.
        let discovered = server.discovered_perltidy_profile.lock().clone();
        let canon_discovered = discovered.as_deref().and_then(|s| std::fs::canonicalize(s).ok());
        let canon_expected = std::fs::canonicalize(&profile).ok();
        assert_eq!(
            canon_discovered.as_deref(),
            canon_expected.as_deref(),
            "workspace .perltidyrc should be discovered and cached at initialize"
        );
        Ok(())
    }

    #[test]
    fn set_root_uri_applies_discovered_perltidyrc_options_to_config() -> std::io::Result<()> {
        let server = LspServer::new();
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join(".perltidyrc"), "-l=120\n-i 3\n")?;

        server.set_root_uri(&format!("file://{}", temp.path().display()));

        // The discovered profile is applied at initialize and overrides the
        // built-in defaults (which are Some(80)/Some(4)), so the native
        // formatter actually honors the project profile. Workspace-first search
        // keeps this assertion independent of any ambient $HOME/$PERLTIDY profile.
        let config = server.config.lock();
        assert_eq!(
            config.perltidy_maximum_line_length,
            Some(120),
            "discovered profile's line width must override the built-in default (80)"
        );
        assert_eq!(config.perltidy_indent_columns, Some(3));
        Ok(())
    }

    #[test]
    fn load_and_apply_project_config_loads_per_folder_config() {
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder1 = temp.path().join("folder1");
        let folder2 = temp.path().join("folder2");
        std::fs::create_dir_all(&folder1).expect("failed to create folder1");
        std::fs::create_dir_all(&folder2).expect("failed to create folder2");

        // Create .perl-lsp.toml in folder1
        let config1 = folder1.join(".perl-lsp.toml");
        std::fs::write(
            &config1,
            r#"
[perl]
include_paths = ["custom_lib"]
"#,
        )
        .expect("failed to write config1");

        // Create .perl-lsp.toml in folder2
        let config2 = folder2.join(".perl-lsp.toml");
        std::fs::write(
            &config2,
            r#"
[perl]
include_paths = ["other_lib"]
"#,
        )
        .expect("failed to write config2");

        // Add workspace folders
        let uri1 =
            url::Url::from_directory_path(&folder1).expect("failed to create uri1").to_string();
        let uri2 =
            url::Url::from_directory_path(&folder2).expect("failed to create uri2").to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri1.clone())
                .with_path(folder1.clone()),
        );
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri2.clone())
                .with_path(folder2.clone()),
        );

        // Load configs
        server.load_and_apply_project_config();

        // Verify each folder has its own config
        let folders = server.workspace_folders.lock();
        assert_eq!(folders.len(), 2);

        let folder1_state = folders.iter().find(|f| f.uri == uri1).unwrap();
        assert!(folder1_state.project_config.is_some());
        assert!(
            folder1_state
                .effective_workspace_config
                .include_paths
                .contains(&"custom_lib".to_string())
        );

        let folder2_state = folders.iter().find(|f| f.uri == uri2).unwrap();
        assert!(folder2_state.project_config.is_some());
        assert!(
            folder2_state
                .effective_workspace_config
                .include_paths
                .contains(&"other_lib".to_string())
        );
    }

    #[test]
    fn load_and_apply_project_config_multi_root_first_folder_wins_on_conflict() {
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder1 = temp.path().join("folder1");
        let folder2 = temp.path().join("folder2");
        std::fs::create_dir_all(&folder1).expect("failed to create folder1");
        std::fs::create_dir_all(&folder2).expect("failed to create folder2");

        // folder1 enables perlcritic; folder2 disables it. Pre-fix, the loop
        // applied folder2 last and silently won server-wide. Post-fix, the first
        // folder wins and the conflict is surfaced instead of silently overwritten.
        std::fs::write(folder1.join(".perl-lsp.toml"), "[diagnostics]\nperlcritic = true\n")
            .expect("write config1");
        std::fs::write(folder2.join(".perl-lsp.toml"), "[diagnostics]\nperlcritic = false\n")
            .expect("write config2");

        let uri1 =
            url::Url::from_directory_path(&folder1).expect("failed to create uri1").to_string();
        let uri2 =
            url::Url::from_directory_path(&folder2).expect("failed to create uri2").to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri1.clone())
                .with_path(folder1.clone()),
        );
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri2.clone())
                .with_path(folder2.clone()),
        );

        server.load_and_apply_project_config();

        // First folder wins for the conflicting global key; folder2 does NOT
        // silently overwrite it.
        assert!(
            server.config.lock().perlcritic_enabled,
            "first folder's [diagnostics].perlcritic=true must win, not last folder's false"
        );

        // Both folders still have their own per-folder effective workspace config.
        let folders = server.workspace_folders.lock();
        assert_eq!(folders.len(), 2);
        assert!(folders.iter().find(|f| f.uri == uri1).expect("folder1").project_config.is_some());
        assert!(folders.iter().find(|f| f.uri == uri2).expect("folder2").project_config.is_some());
    }

    #[test]
    fn load_and_apply_project_config_multi_root_non_conflicting_fields_all_apply() {
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder1 = temp.path().join("folder1");
        let folder2 = temp.path().join("folder2");
        std::fs::create_dir_all(&folder1).expect("failed to create folder1");
        std::fs::create_dir_all(&folder2).expect("failed to create folder2");

        // folder1 sets [diagnostics].perlcritic; folder2 sets [features].inlay_hints.
        // Different keys -> both apply, no silent override.
        std::fs::write(folder1.join(".perl-lsp.toml"), "[diagnostics]\nperlcritic = true\n")
            .expect("write config1");
        std::fs::write(folder2.join(".perl-lsp.toml"), "[features]\ninlay_hints = false\n")
            .expect("write config2");

        let uri1 =
            url::Url::from_directory_path(&folder1).expect("failed to create uri1").to_string();
        let uri2 =
            url::Url::from_directory_path(&folder2).expect("failed to create uri2").to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri1.clone())
                .with_path(folder1.clone()),
        );
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri2.clone())
                .with_path(folder2.clone()),
        );

        server.load_and_apply_project_config();

        let config = server.config.lock();
        assert!(config.perlcritic_enabled, "folder1's perlcritic=true must apply");
        assert!(
            !config.inlay_hints_enabled,
            "folder2's inlay_hints=false must apply (default is true)"
        );
    }

    #[test]
    fn load_and_apply_project_config_multi_root_last_folder_no_longer_wins_silently() {
        // Regression guard for #4633: three folders, last one sets a value that
        // must NOT silently win over the first.
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder1 = temp.path().join("folder1");
        let folder2 = temp.path().join("folder2");
        let folder3 = temp.path().join("folder3");
        std::fs::create_dir_all(&folder1).expect("folder1");
        std::fs::create_dir_all(&folder2).expect("folder2");
        std::fs::create_dir_all(&folder3).expect("folder3");

        std::fs::write(folder1.join(".perl-lsp.toml"), "[diagnostics]\nperlcritic_severity = 3\n")
            .expect("write config1");
        std::fs::write(folder2.join(".perl-lsp.toml"), "[diagnostics]\nperlcritic_severity = 1\n")
            .expect("write config2");
        std::fs::write(folder3.join(".perl-lsp.toml"), "[diagnostics]\nperlcritic_severity = 5\n")
            .expect("write config3");

        for folder in [&folder1, &folder2, &folder3] {
            let uri = url::Url::from_directory_path(folder).expect("uri").to_string();
            server.workspace_folders.lock().push(
                crate::runtime::workspace_folder::WorkspaceFolderState::new(uri)
                    .with_path(folder.to_path_buf()),
            );
        }

        server.load_and_apply_project_config();

        // First folder (severity 3) wins; last folder (severity 5) does NOT win.
        assert_eq!(
            server.config.lock().perlcritic_severity,
            3,
            "first folder's severity=3 must win, not last folder's 5"
        );
    }

    #[test]
    fn load_and_apply_project_config_handles_missing_config() {
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder = temp.path().join("folder");
        std::fs::create_dir_all(&folder).expect("failed to create folder");

        // Add workspace folder without config
        let uri = url::Url::from_directory_path(&folder).expect("failed to create uri").to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri.clone())
                .with_path(folder.clone()),
        );

        // Load configs
        server.load_and_apply_project_config();

        // Verify folder has no project config but has default effective config
        let folders = server.workspace_folders.lock();
        assert_eq!(folders.len(), 1);

        let folder_state = folders.iter().find(|f| f.uri == uri).unwrap();
        assert!(folder_state.project_config.is_none());
        // Should have default include paths
        assert!(!folder_state.effective_workspace_config.include_paths.is_empty());
    }

    #[test]
    fn load_and_apply_project_config_clears_stale_folder_config() -> anyhow::Result<()> {
        let server = LspServer::new();
        let temp = tempfile::tempdir()?;
        let folder = temp.path().join("folder");
        std::fs::create_dir_all(&folder)?;

        let config = folder.join(".perl-lsp.toml");
        std::fs::write(
            &config,
            r#"
[perl]
include_paths = ["stale_lib"]
"#,
        )?;

        let uri = url::Url::from_directory_path(&folder)
            .map_err(|()| anyhow::anyhow!("failed to create folder URI"))?
            .to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri.clone())
                .with_path(folder.clone()),
        );

        server.load_and_apply_project_config();
        {
            let folders = server.workspace_folders.lock();
            let folder_state = folders
                .iter()
                .find(|f| f.uri == uri)
                .ok_or_else(|| anyhow::anyhow!("missing folder"))?;
            assert!(folder_state.project_config.is_some());
            assert!(
                folder_state
                    .effective_workspace_config
                    .include_paths
                    .contains(&"stale_lib".to_string())
            );
        }

        std::fs::remove_file(config)?;
        server.load_and_apply_project_config();

        let folders = server.workspace_folders.lock();
        let folder_state = folders
            .iter()
            .find(|f| f.uri == uri)
            .ok_or_else(|| anyhow::anyhow!("missing folder"))?;
        assert!(folder_state.project_config.is_none());
        assert!(
            !folder_state
                .effective_workspace_config
                .include_paths
                .contains(&"stale_lib".to_string())
        );
        Ok(())
    }

    #[test]
    fn handle_client_response_rejects_hostile_absolute_include_paths() {
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder = temp.path().join("folder");
        std::fs::create_dir_all(&folder).expect("failed to create folder");

        let uri = url::Url::from_directory_path(&folder).expect("failed to create uri").to_string();
        let absolute = if cfg!(windows) { "C:\\Windows" } else { "/etc" };

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri.clone())
                .with_path(folder.clone()),
        );
        server.pending_workspace_configuration_requests.lock().insert(
            ServerRequestId::for_test(12),
            crate::runtime::PendingWorkspaceConfigurationRequest {
                folder_uris: vec![uri.clone()],
                includes_global_item: true,
                created_at: std::time::Instant::now(),
            },
        );

        server.handle_client_response(Some(serde_json::json!({
            "id": 12,
            "result": [
                { "workspace": { "useSystemInc": false } },
                { "workspace": { "includePaths": [absolute, "lib"] } }
            ]
        })));

        let folders = server.workspace_folders.lock();
        let folder_state = folders.iter().find(|f| f.uri == uri).expect("missing folder");
        assert_eq!(folder_state.effective_workspace_config.include_paths, vec!["lib".to_string()]);
        assert!(folder_state.effective_workspace_config.external_include_paths.is_empty());
    }

    #[test]
    fn handle_client_response_applies_per_folder_workspace_config() {
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder1 = temp.path().join("folder1");
        let folder2 = temp.path().join("folder2");
        std::fs::create_dir_all(&folder1).expect("failed to create folder1");
        std::fs::create_dir_all(&folder2).expect("failed to create folder2");

        let uri1 =
            url::Url::from_directory_path(&folder1).expect("failed to create uri1").to_string();
        let uri2 =
            url::Url::from_directory_path(&folder2).expect("failed to create uri2").to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri1.clone())
                .with_path(folder1.clone()),
        );
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri2.clone())
                .with_path(folder2.clone()),
        );

        server.pending_workspace_configuration_requests.lock().insert(
            ServerRequestId::for_test(11),
            crate::runtime::PendingWorkspaceConfigurationRequest {
                folder_uris: vec![uri1.clone(), uri2.clone()],
                includes_global_item: true,
                created_at: std::time::Instant::now(),
            },
        );

        server.handle_client_response(Some(serde_json::json!({
            "id": 11,
            "result": [
                { "workspace": { "useSystemInc": true } },
                { "workspace": { "includePaths": ["api_lib"] } },
                { "workspace": { "includePaths": ["ui_lib"] } }
            ]
        })));

        let folders = server.workspace_folders.lock();
        let folder1_state = folders.iter().find(|f| f.uri == uri1).expect("missing folder1");
        let folder2_state = folders.iter().find(|f| f.uri == uri2).expect("missing folder2");

        assert!(
            folder1_state.effective_workspace_config.include_paths.contains(&"api_lib".to_string())
        );
        assert!(
            folder2_state.effective_workspace_config.include_paths.contains(&"ui_lib".to_string())
        );
        assert!(folder1_state.effective_workspace_config.use_system_inc);
        assert!(folder2_state.effective_workspace_config.use_system_inc);
    }

    #[test]
    fn handle_client_response_accepts_numeric_string_id() -> anyhow::Result<()> {
        let server = LspServer::new();
        let temp = tempfile::tempdir()?;
        let folder = temp.path().join("folder");
        std::fs::create_dir_all(&folder)?;
        let uri = url::Url::from_directory_path(&folder)
            .map_err(|()| anyhow::anyhow!("failed to create folder URI"))?
            .to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri.clone())
                .with_path(folder.clone()),
        );
        server.pending_workspace_configuration_requests.lock().insert(
            ServerRequestId::for_test(77),
            crate::runtime::PendingWorkspaceConfigurationRequest {
                folder_uris: vec![uri.clone()],
                includes_global_item: true,
                created_at: std::time::Instant::now(),
            },
        );

        server.handle_client_response(Some(serde_json::json!({
            "id": "77",
            "result": [
                {"workspace": {"useSystemInc": true}},
                {"workspace": {"includePaths": ["string_id_lib"]}}
            ]
        })));

        let folders = server.workspace_folders.lock();
        let folder_state = folders
            .iter()
            .find(|f| f.uri == uri)
            .ok_or_else(|| anyhow::anyhow!("missing folder"))?;
        assert!(
            folder_state
                .effective_workspace_config
                .include_paths
                .contains(&"string_id_lib".to_string())
        );
        assert!(folder_state.effective_workspace_config.use_system_inc);
        Ok(())
    }

    #[test]
    fn handle_client_response_ignores_non_array_result() {
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder = temp.path().join("folder");
        std::fs::create_dir_all(&folder).expect("failed to create folder");
        let uri = url::Url::from_directory_path(&folder).expect("failed to create uri").to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri.clone())
                .with_path(folder.clone()),
        );
        server.pending_workspace_configuration_requests.lock().insert(
            ServerRequestId::for_test(99),
            crate::runtime::PendingWorkspaceConfigurationRequest {
                folder_uris: vec![uri.clone()],
                includes_global_item: true,
                created_at: std::time::Instant::now(),
            },
        );

        server.handle_client_response(Some(serde_json::json!({
            "id": 99,
            "result": { "workspace": { "includePaths": ["oops"] } }
        })));

        let folders = server.workspace_folders.lock();
        let folder_state = folders.iter().find(|f| f.uri == uri).expect("missing folder");
        assert!(
            !folder_state.effective_workspace_config.include_paths.contains(&"oops".to_string())
        );
    }

    #[test]
    fn did_change_configuration_updates_folder_effective_configs() {
        let server = LspServer::new();
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder1 = temp.path().join("folder1");
        let folder2 = temp.path().join("folder2");
        std::fs::create_dir_all(&folder1).expect("failed to create folder1");
        std::fs::create_dir_all(&folder2).expect("failed to create folder2");

        let uri1 =
            url::Url::from_directory_path(&folder1).expect("failed to create uri1").to_string();
        let uri2 =
            url::Url::from_directory_path(&folder2).expect("failed to create uri2").to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri1)
                .with_path(folder1.clone()),
        );
        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri2)
                .with_path(folder2.clone()),
        );

        server.handle_did_change_configuration(Some(serde_json::json!({
            "settings": {
                "perl": {
                    "workspace": {
                        "includePaths": ["client_lib"],
                        "useSystemInc": true
                    }
                }
            }
        })));

        let folders = server.workspace_folders.lock();
        assert_eq!(folders.len(), 2);
        for folder in folders.iter() {
            assert!(
                folder.effective_workspace_config.include_paths.contains(&"client_lib".to_string()),
                "folder {} missing client_lib in effective include_paths",
                folder.uri
            );
            assert!(
                folder.effective_workspace_config.use_system_inc,
                "folder {} should have use_system_inc=true from didChangeConfiguration",
                folder.uri
            );
        }
    }
    #[test]
    fn request_workspace_configuration_supersedes_older_pending_requests() {
        let server = LspServer::new();
        server.client_capabilities.lock().workspace_configuration_support = true;
        // Supersession is a post-initialize concern; server->client requests are
        // rejected before initialization completes (#7708).
        server.initialized.store(true, Ordering::Release);

        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let folder = temp.path().join("folder");
        std::fs::create_dir_all(&folder).expect("failed to create folder");
        let uri = url::Url::from_directory_path(&folder).expect("failed to create uri").to_string();

        server.workspace_folders.lock().push(
            crate::runtime::workspace_folder::WorkspaceFolderState::new(uri)
                .with_path(folder.clone()),
        );
        let expected_uri = server.workspace_folders.lock()[0].uri.clone();

        server.pending_workspace_configuration_requests.lock().insert(
            ServerRequestId::for_test(1),
            crate::runtime::PendingWorkspaceConfigurationRequest {
                folder_uris: vec!["file:///stale".to_string()],
                includes_global_item: true,
                created_at: std::time::Instant::now(),
            },
        );

        server.request_workspace_configuration_for_folders();

        let pending = server.pending_workspace_configuration_requests.lock();
        assert_eq!(pending.len(), 1, "only latest request should remain pending");
        let pending_request = pending.values().next().expect("missing pending request");
        assert_eq!(pending_request.folder_uris.len(), 1);
        assert_eq!(pending_request.folder_uris[0], expected_uri);
    }

    #[test]
    fn did_change_workspace_folders_clears_pending_scoped_configuration_requests()
    -> anyhow::Result<()> {
        let server = LspServer::new();
        let temp = tempfile::tempdir()?;
        let folder = temp.path().join("folder");
        std::fs::create_dir_all(&folder)?;
        let uri = url::Url::from_directory_path(&folder)
            .map_err(|()| anyhow::anyhow!("failed to create folder URI"))?
            .to_string();

        server.pending_workspace_configuration_requests.lock().insert(
            ServerRequestId::for_test(700),
            crate::runtime::PendingWorkspaceConfigurationRequest {
                folder_uris: vec![uri.clone()],
                includes_global_item: true,
                created_at: std::time::Instant::now(),
            },
        );

        server.handle_did_change_workspace_folders(Some(serde_json::json!({
            "event": {
                "added": [{ "uri": uri, "name": "folder" }],
                "removed": []
            }
        })))?;

        assert!(
            server.pending_workspace_configuration_requests.lock().is_empty(),
            "workspace folder changes should invalidate stale scoped configuration requests",
        );
        Ok(())
    }

    #[test]
    fn did_change_configuration_accepts_unwrapped_perl_settings() {
        // Sublime Text's LSP package sends settings without the outer "perl" wrapper:
        //   {"settings": {"workspace": {"includePaths": [...], "useSystemInc": false}}}
        // rather than the standard:
        //   {"settings": {"perl": {"workspace": {"includePaths": [...], "useSystemInc": false}}}}
        // Both forms must be accepted and applied.
        let server = LspServer::new();

        server.handle_did_change_configuration(Some(serde_json::json!({
            "settings": {
                "workspace": {
                    "includePaths": ["vendor/lib"],
                    "useSystemInc": false
                }
            }
        })));

        let workspace_config = server.workspace_config.lock();
        assert!(
            workspace_config.include_paths.contains(&"vendor/lib".to_string()),
            "unwrapped Sublime-style settings must apply includePaths; got: {:?}",
            workspace_config.include_paths
        );
        assert!(
            !workspace_config.use_system_inc,
            "unwrapped Sublime-style settings must apply useSystemInc=false"
        );
    }

    #[test]
    fn did_change_configuration_wrapped_form_still_works() {
        // Ensure the standard wrapped form continues to work after the Sublime fix.
        let server = LspServer::new();

        server.handle_did_change_configuration(Some(serde_json::json!({
            "settings": {
                "perl": {
                    "workspace": {
                        "includePaths": ["lib/wrapped"],
                        "useSystemInc": false
                    }
                }
            }
        })));

        let workspace_config = server.workspace_config.lock();
        assert!(
            workspace_config.include_paths.contains(&"lib/wrapped".to_string()),
            "standard wrapped settings must still apply includePaths; got: {:?}",
            workspace_config.include_paths
        );
    }
}
