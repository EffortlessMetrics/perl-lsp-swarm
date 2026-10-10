//! Compatibility adapter for the shared change-set resolver.

use color_eyre::eyre::Result;

pub use perl_ci_hygiene::change_set::{
    ArtifactIdentity, ChangeSet, ChangeSetConfig, DiffMode, resolve_change_set,
    resolve_change_set_with_mode,
};

/// Preserve xtask's compile-time workspace-root default for its legacy CLI.
pub fn run(mut config: ChangeSetConfig) -> Result<()> {
    if config.root.is_none() {
        config.root = Some(crate::utils::project_root()?);
    }
    perl_ci_hygiene::change_set::run(config)
}
