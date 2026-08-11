//! check-version-sync task wrapper.
//!
//! Delegates to the workspace `perl-ci-hygiene` library, which is compiled into
//! this exact `xtask` binary. Keeping the call in-process avoids a nested Cargo
//! build on every CI gate invocation while retaining source-level freshness.
//!
//! Product/package identity is checked in the same gate because a coherent
//! semantic version is not sufficient when the product, executable, Cargo
//! package, extension, or debug-adapter identities drift. The path-authority
//! check rejects symlink aliases before the workspace-binding check proves the
//! selected Cargo manifests are the active members Cargo resolves.

use crate::utils::project_root;
use color_eyre::eyre::Result;

#[path = "product_identity_path_authority.rs"]
mod product_identity_path_authority;

pub fn run() -> Result<()> {
    let root = project_root()?;
    perl_ci_hygiene::version_sync::check(&root)?;
    super::product_identity::check(&root)?;
    product_identity_path_authority::check(&root)?;
    super::product_identity_workspace::check(&root)
}
