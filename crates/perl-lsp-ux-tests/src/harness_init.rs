use crate::{FakeWorkspace, ScenarioConfig, UxClient, resolve_binary};
use anyhow::{Context, Result};

pub(crate) fn prepare_workspace(config: &ScenarioConfig) -> Result<FakeWorkspace> {
    let workspace = FakeWorkspace::new()?;

    for (path, content) in &config.workspace_files {
        workspace.write(path, content)?;
    }

    for (path, _) in &config.workspace_folders {
        workspace.ensure_dir(path)?;
    }

    Ok(workspace)
}

pub(crate) fn spawn_client(workspace: &FakeWorkspace, config: &ScenarioConfig) -> Result<UxClient> {
    let binary_path = resolve_binary()?;

    UxClient::spawn(&binary_path, workspace, config).context("Failed to spawn LSP server")
}
