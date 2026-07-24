//! Workspace management
//!
//! Handles workspace folders and root URI/path management.

use super::super::*;
use perl_dap::platform::{PerlInterpreterResult, find_perl_interpreter};
use perl_lsp_rs_core::config::WorkspaceConfig;
use std::sync::Once;

/// Fires at most once per LSP session, when Perl is not found anywhere.
static PERL_NOT_FOUND_WARNED: Once = Once::new();

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
    /// Called once during `handle_initialize`. Reads `perl-lsp.perl.path` from the
    /// workspace config (if set), then falls back to full OS-aware detection. Emits:
    ///
    /// - `window/logMessage` (Info) if Perl was found via an OS fallback path so the
    ///   user knows which interpreter will be used.
    /// - `window/showMessage` (Error) **once per session** if no Perl interpreter is found
    ///   anywhere, with actionable remediation text.
    /// - `window/showMessage` (Error) **once per session** if `perl-lsp.perl.path` is set
    ///   but the configured path does not exist.
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
                let msg = format!(
                    "perl-lsp: Perl not found on PATH; using {label} at {}. \
                     Add Perl to PATH or set `perl-lsp.perl.path` to suppress this message.",
                    path.display()
                );
                tracing::info!(path = %path.display(), label = %label, "Perl interpreter found via fallback");
                if let Err(e) = self.log_message(MessageType::Info, &msg) {
                    tracing::warn!(error = %e, "Failed to send logMessage for perl fallback");
                }
            }
            PerlInterpreterResult::NotFound { ref searched } => {
                tracing::warn!(searched = ?searched, "Perl interpreter not found");
                PERL_NOT_FOUND_WARNED.call_once(|| {
                    let searched_str = searched.join(", ");
                    let msg = if configured_path.as_deref().is_some_and(|p| !p.is_empty()) {
                        format!(
                            "perl-lsp: The configured Perl interpreter was not found. \
                             Searched: {searched_str}. \
                             Check `perl-lsp.perl.path` in your settings and reload the window \
                             (Ctrl+Shift+P \u{2192} Developer: Reload Window)."
                        )
                    } else {
                        format!(
                            "perl-lsp: Perl interpreter not found on PATH. \
                             Searched: {searched_str}. \
                             Install Perl (https://strawberryperl.com on Windows, \
                             `brew install perl` on macOS, or your system package manager) \
                             and reload the window, or set `perl-lsp.perl.path` in settings."
                        )
                    };
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
            return; // Single-file mode; no workspace root to search
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
                    effective_config.update_from_value(init_opts);
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
                        project_config
                            .apply_to_workspace_config(&mut folder.effective_workspace_config);

                        // Defer the server-global sections to the post-loop merge so a
                        // later folder cannot silently clobber an earlier folder's value.
                        global_configs.push((folder.display_name().to_string(), project_config));
                    }
                    Err(msg) => {
                        let user_msg = format!(
                            "perl-lsp: {msg} \
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

        // Pull client-scoped workspace settings (if supported) and merge them
        // as the highest-precedence layer over TOML-derived folder config.
        drop(folders);
        self.request_workspace_configuration_for_folders();
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
            "perl-lsp: multi-root workspace has conflicting .perl-lsp.toml settings across \
             folders. The first folder wins for each key; others were ignored: {rendered}. \
             See docs/reference/CONFIG.md (Multi-root workspaces) for details."
        );
        tracing::warn!(conflicts = %rendered, "Multi-root config conflict; first folder wins");
        if let Err(e) = self.show_message(MessageType::Warning, &user_msg) {
            tracing::warn!(error = %e, "Failed to send showMessage for multi-root config conflict");
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
