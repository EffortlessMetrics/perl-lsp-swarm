//! Workspace folder state representation.
//!
//! This module provides explicit workspace folder state to replace the current
//! "folder list + singleton config/root" assumptions as part of multi-root
//! workspace support.

#![warn(missing_docs)]

use std::path::PathBuf;

use perl_lsp_rs_core::config::{ProjectConfig, WorkspaceConfig};

/// State for a single workspace folder.
///
/// This struct represents a workspace folder with its metadata and configuration.
/// It will eventually support per-folder effective settings, but for now it provides
/// the foundation for multi-root workspace support.
#[derive(Debug, Clone)]
pub struct WorkspaceFolderState {
    /// The URI of the workspace folder (e.g., "file:///path/to/folder")
    pub uri: String,
    /// The filesystem path of the workspace folder (if resolvable)
    pub path: Option<PathBuf>,
    /// The name of the workspace folder (optional, from LSP client)
    pub name: Option<String>,
    /// Project configuration loaded from `.perl-lsp.toml` in this folder
    pub project_config: Option<ProjectConfig>,
    /// Accepted project-configuration generation for this folder.
    pub project_config_generation: u64,
    /// Effective workspace configuration for this folder
    ///
    /// This will eventually be computed by merging:
    /// 1. Default workspace config
    /// 2. Project config from `.perl-lsp.toml`
    /// 3. LSP client settings
    pub effective_workspace_config: WorkspaceConfig,
}

impl WorkspaceFolderState {
    /// Create a new workspace folder state from a URI.
    #[must_use]
    pub fn new(uri: String) -> Self {
        Self {
            uri,
            path: None,
            name: None,
            project_config: None,
            project_config_generation: 0,
            effective_workspace_config: WorkspaceConfig::default(),
        }
    }

    /// Set the filesystem path for this workspace folder.
    #[must_use]
    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    /// Set the name for this workspace folder.
    #[must_use]
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Set the project configuration for this workspace folder.
    #[must_use]
    pub fn with_project_config(mut self, config: ProjectConfig) -> Self {
        self.project_config = Some(config);
        self
    }

    /// Set the effective workspace configuration for this workspace folder.
    #[must_use]
    pub fn with_effective_workspace_config(mut self, config: WorkspaceConfig) -> Self {
        self.effective_workspace_config = config;
        self
    }

    /// Refresh metadata-derived facts for this folder's effective workspace config.
    pub fn refresh_workspace_metadata(&mut self) {
        if let Some(path) = self.path.as_deref() {
            self.effective_workspace_config.refresh_declared_dependencies(path);
            self.effective_workspace_config.refresh_dependency_include_paths(path);
        }
    }

    /// Refresh metadata-derived facts from per-source captured reads (#13640).
    ///
    /// Declared dependencies come from `reads`, so an open metadata buffer's
    /// staged text is authoritative and an unreadable source keeps only its own
    /// previous entries. Dependency-manager include roots are reconciled from
    /// the filesystem regardless: those markers are existence probes, so an
    /// unreadable declaration file must not stop a deleted `carton.lock` from
    /// retiring its root.
    pub fn refresh_workspace_metadata_from_reads(
        &mut self,
        reads: &[(
            perl_lsp_rs_core::config::DeclaredDependencySource,
            perl_lsp_rs_core::config::MetadataSourceRead,
        )],
    ) {
        self.effective_workspace_config.apply_declared_dependency_reads(reads);
        if let Some(path) = self.path.as_deref() {
            self.effective_workspace_config.refresh_dependency_include_paths(path);
        }
    }

    /// Get the URI as a string reference.
    #[must_use]
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Get the name, or derive it from the URI if not set.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_new_folder_state() {
        let folder = WorkspaceFolderState::new("file:///test/path".to_string());
        assert_eq!(folder.uri, "file:///test/path");
        assert!(folder.path.is_none());
        assert!(folder.name.is_none());
        assert!(folder.project_config.is_none());
    }

    #[test]
    fn builds_with_path() {
        let folder = WorkspaceFolderState::new("file:///test/path".to_string())
            .with_path(PathBuf::from("/test/path"));
        assert_eq!(folder.path, Some(PathBuf::from("/test/path")));
    }

    #[test]
    fn builds_with_name() {
        let folder = WorkspaceFolderState::new("file:///test/path".to_string())
            .with_name("My Project".to_string());
        assert_eq!(folder.name, Some("My Project".to_string()));
    }

    #[test]
    fn display_name_uses_name_when_set() {
        let folder = WorkspaceFolderState::new("file:///test/path".to_string())
            .with_name("My Project".to_string());
        assert_eq!(folder.display_name(), "My Project");
    }

    #[test]
    fn display_name_falls_back_to_uri() {
        let folder = WorkspaceFolderState::new("file:///test/path".to_string());
        assert_eq!(folder.display_name(), "file:///test/path");
    }

    #[test]
    fn builds_with_project_config() {
        let project_config = ProjectConfig::default();
        let folder = WorkspaceFolderState::new("file:///test/path".to_string())
            .with_project_config(project_config.clone());
        assert!(folder.project_config.is_some());
    }

    #[test]
    fn builds_with_effective_workspace_config() {
        let workspace_config = WorkspaceConfig::default();
        let folder = WorkspaceFolderState::new("file:///test/path".to_string())
            .with_effective_workspace_config(workspace_config.clone());
        assert_eq!(folder.effective_workspace_config.include_paths, workspace_config.include_paths);
    }

    #[test]
    fn effective_workspace_config_has_defaults() {
        let folder = WorkspaceFolderState::new("file:///test/path".to_string());
        let config = &folder.effective_workspace_config;
        assert!(!config.include_paths.is_empty());
        assert_eq!(config.resolution_timeout_ms, 50);
        assert!(!config.use_system_inc);
    }

    #[test]
    fn refresh_workspace_metadata_adds_carton_include_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(temp.path().join("carton.lock"), "snapshot\n")?;
        let mut folder = WorkspaceFolderState::new("file:///workspace".to_string())
            .with_path(temp.path().to_path_buf());

        folder.refresh_workspace_metadata();

        assert_eq!(
            folder.effective_workspace_config.include_paths,
            vec!["lib", ".", "local/lib/perl5"]
        );
        Ok(())
    }

    /// Invalidation must be reversible (#13640): a root this detector
    /// contributed is retired once its markers are gone, so deleting the lock
    /// file does not leave a stale include root behind.
    #[test]
    fn refresh_workspace_metadata_retires_a_detected_include_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(temp.path().join("carton.lock"), "snapshot\n")?;
        let mut config = WorkspaceConfig::default();
        config.include_paths = vec!["lib".to_string(), ".".to_string()];
        let mut folder = WorkspaceFolderState::new("file:///workspace".to_string())
            .with_path(temp.path().to_path_buf())
            .with_effective_workspace_config(config);

        folder.refresh_workspace_metadata();
        assert_eq!(
            folder.effective_workspace_config.include_paths,
            vec!["lib", ".", "local/lib/perl5"],
            "the detected root is contributed while its marker exists"
        );

        std::fs::remove_file(temp.path().join("carton.lock"))?;
        folder.refresh_workspace_metadata();

        assert_eq!(
            folder.effective_workspace_config.include_paths,
            vec!["lib", "."],
            "a detected root must be retired once its marker is gone"
        );
        Ok(())
    }

    /// A path the user configured is never removed, even when the detector
    /// also reports it and its marker later disappears.
    #[test]
    fn refresh_workspace_metadata_never_retires_a_configured_include_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(temp.path().join("carton.lock"), "snapshot\n")?;
        let mut folder = WorkspaceFolderState::new("file:///workspace".to_string())
            .with_path(temp.path().to_path_buf());

        // The default config already configures `local/lib/perl5`.
        folder.refresh_workspace_metadata();
        std::fs::remove_file(temp.path().join("carton.lock"))?;
        folder.refresh_workspace_metadata();

        assert_eq!(
            folder.effective_workspace_config.include_paths,
            vec!["lib", ".", "local/lib/perl5"],
            "a user-configured include path is never claimed or retired by detection"
        );
        Ok(())
    }

    /// Migrated Carmel detection (#13642 §2/§3): a rolled-out Carmel project
    /// (discriminated by the `local/.carmel` sentinel) contributes the shared
    /// install-base root `local/lib/perl5`. The previous `vendor/lib/perl5`
    /// marker had no basis in Carmel source and is retired.
    #[test]
    fn refresh_workspace_metadata_adds_carmel_include_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let mut config = WorkspaceConfig::default();
        config.include_paths = vec!["lib".to_string(), ".".to_string()];
        let mut folder = WorkspaceFolderState::new("file:///workspace".to_string())
            .with_path(temp.path().to_path_buf())
            .with_effective_workspace_config(config);

        folder.refresh_workspace_metadata();

        assert_eq!(folder.effective_workspace_config.include_paths, vec!["lib", "."]);

        std::fs::write(temp.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::create_dir_all(temp.path().join("local"))?;
        std::fs::write(temp.path().join("local/.carmel"), "")?;
        std::fs::create_dir_all(temp.path().join("local/lib/perl5"))?;
        folder.refresh_workspace_metadata();

        assert_eq!(
            folder.effective_workspace_config.include_paths,
            vec!["lib", ".", "local/lib/perl5"]
        );
        Ok(())
    }
}
