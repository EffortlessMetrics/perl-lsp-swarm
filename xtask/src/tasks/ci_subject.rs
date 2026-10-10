//! Compatibility front door for the shared product-free ci subject owner.

pub use perl_ci_hygiene::ci_subject::*;

/// Preserve xtask's repository-root default before delegating to the shared owner.
pub fn run(mut config: CiSubjectConfig) -> color_eyre::eyre::Result<()> {
    if config.root.is_none() {
        config.root = Some(crate::utils::project_root()?);
    }
    perl_ci_hygiene::ci_subject::run(config)
}
