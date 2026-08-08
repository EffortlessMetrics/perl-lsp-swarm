//! check-version-sync task wrapper.
//!
//! Calls the authoritative `perl-ci-hygiene` library directly so release
//! verification does not spawn a nested Cargo build.

use crate::utils::project_root;
use color_eyre::eyre::Result;

pub fn run() -> Result<()> {
    let root = project_root()?;
    perl_ci_hygiene::version_sync::check(&root)
}
