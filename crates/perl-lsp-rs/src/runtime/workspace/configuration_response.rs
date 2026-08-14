use crate::runtime::workspace_folder::WorkspaceFolderState;
use perl_lsp_rs_core::config::WorkspaceConfigUpdateContext;
use serde_json::Value;

fn apply_workspace_config_layer(
    config: &mut perl_lsp_rs_core::config::WorkspaceConfig,
    settings: &Value,
    folder: &WorkspaceFolderState,
    apply_external_include_paths: bool,
) {
    let rejected = config.update_from_value_with_context(
        settings,
        WorkspaceConfigUpdateContext {
            workspace_root: folder.path.as_deref(),
            apply_external_include_paths,
        },
    );
    for entry in rejected {
        tracing::warn!(
            target: "perl_lsp::config",
            folder_uri = %folder.uri,
            entry = %entry.entry,
            reason = %entry.render(),
            "rejected client includePaths entry"
        );
    }
}

pub(super) fn apply_workspace_configuration_results(
    folders: &mut [WorkspaceFolderState],
    folder_uris: &[String],
    includes_global_item: bool,
    results: &[Value],
    request_id: i64,
    init_options_perl: Option<&Value>,
) {
    let global_settings = if includes_global_item { results.first() } else { None };
    let folder_results_start = usize::from(includes_global_item);

    if let Some(global_settings) = global_settings
        && let Ok(mut limits) = perl_lsp_rs_core::runtime::limits::LSP_LIMITS.write()
    {
        limits.update_from_value(global_settings);
    }

    for (idx, folder_uri) in folder_uris.iter().enumerate() {
        let Some(folder) = folders.iter_mut().find(|folder| &folder.uri == folder_uri) else {
            continue;
        };

        let mut effective_config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        if let Some(init_opts) = init_options_perl {
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
        if let Some(project_config) = &folder.project_config {
            // Re-applying an already-loaded project_config (loaded, validated, and
            // warned about once in lifecycle/workspace.rs). Discard the rejection
            // list here rather than re-warning on every reconfiguration.
            if let Some(folder_path) = folder.path.as_deref() {
                let _ =
                    project_config.apply_to_workspace_config(&mut effective_config, folder_path);
            }
            // else: folder.path is None. This is not "fail-closed" - it is a
            // silent no-op that skips the ENTIRE project config, dropping
            // discovery_extensions, perl5lib toggles and everything else, not
            // just include_paths validation, with no diagnostic.
            //
            // It is unreachable today because path and project_config are set
            // together in lifecycle/workspace.rs. If that invariant is ever
            // broken the symptom will be project settings silently not applying,
            // which is hard to trace back to here.
        }

        if let Some(global_settings) = global_settings {
            apply_workspace_config_layer(&mut effective_config, global_settings, folder, true);
        }

        if let Some(perl_settings) = results.get(folder_results_start + idx) {
            apply_workspace_config_layer(&mut effective_config, perl_settings, folder, false);
        } else {
            tracing::warn!(
                request_id,
                folder_uri = %folder_uri,
                "workspace/configuration response missing folder item; using TOML/default config for folder"
            );
        }

        folder.effective_workspace_config = effective_config;
        folder.refresh_workspace_metadata();
    }
}

#[cfg(test)]
mod tests {
    use super::apply_workspace_configuration_results;
    use crate::runtime::workspace_folder::WorkspaceFolderState;
    use serde_json::json;

    #[test]
    fn applies_global_and_folder_specific_configuration() {
        let mut folders = vec![
            WorkspaceFolderState::new("file:///workspace-a".to_string()),
            WorkspaceFolderState::new("file:///workspace-b".to_string()),
        ];
        let folder_uris =
            vec!["file:///workspace-a".to_string(), "file:///workspace-b".to_string()];
        let results = vec![
            json!({"workspace": {"useSystemInc": true}}),
            json!({"workspace": {"resolutionTimeout": 150}}),
            json!({"workspace": {"resolutionTimeout": 250}}),
        ];

        apply_workspace_configuration_results(&mut folders, &folder_uris, true, &results, 42, None);

        assert!(folders[0].effective_workspace_config.use_system_inc);
        assert_eq!(folders[0].effective_workspace_config.resolution_timeout_ms, 150);
        assert!(folders[1].effective_workspace_config.use_system_inc);
        assert_eq!(folders[1].effective_workspace_config.resolution_timeout_ms, 250);
    }

    #[test]
    fn leaves_unmatched_folder_untouched() {
        let mut folders = vec![
            WorkspaceFolderState::new("file:///workspace-a".to_string()),
            WorkspaceFolderState::new("file:///workspace-b".to_string()),
        ];
        let folder_uris = vec!["file:///workspace-a".to_string()];
        let results = vec![json!({"workspace": {"resolutionTimeout": 200}})];

        apply_workspace_configuration_results(
            &mut folders,
            &folder_uris,
            false,
            &results,
            43,
            None,
        );

        assert_eq!(folders[0].effective_workspace_config.resolution_timeout_ms, 200);
        assert_eq!(folders[1].effective_workspace_config.resolution_timeout_ms, 50);
    }

    #[test]
    fn refreshes_declared_dependencies_from_folder_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("cpanfile"), "requires 'JSON::PP', '4.16';\n")?;
        let folder_uri = url::Url::from_directory_path(temp.path())
            .map_err(|_| "failed to create folder URI")?
            .to_string();
        let mut folders = vec![
            WorkspaceFolderState::new(folder_uri.clone()).with_path(temp.path().to_path_buf()),
        ];
        let folder_uris = vec![folder_uri];
        let results = vec![json!({})];

        apply_workspace_configuration_results(
            &mut folders,
            &folder_uris,
            false,
            &results,
            44,
            None,
        );

        assert!(
            folders[0]
                .effective_workspace_config
                .declared_dependencies
                .iter()
                .any(|dependency| dependency.module == "JSON::PP"
                    && dependency.source.display_name() == "cpanfile"),
            "workspace configuration should cache declared dependencies from cpanfile"
        );
        Ok(())
    }
}
