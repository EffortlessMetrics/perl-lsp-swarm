//! check-version-sync task wrapper.
//!
//! Delegates to the workspace `perl-ci-hygiene` library, which is compiled into
//! this exact `xtask` binary. Keeping the call in-process avoids a nested Cargo
//! build on every CI gate invocation while retaining source-level freshness.

use crate::utils::project_root;
use color_eyre::eyre::Result;

pub fn run() -> Result<()> {
    let root = project_root()?;
    perl_ci_hygiene::version_sync::check(&root)
}
