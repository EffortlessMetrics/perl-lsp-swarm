//! Feature-catalog source check surface (#9201).
//!
//! This is the inspectable resolver/check CLI. Generation remains
//! `vendored_catalog`; this module does not invent a second authority.

use std::fmt::Write as _;
use std::path::PathBuf;

use color_eyre::eyre::{Result, bail, eyre};
use perl_lsp_rs_core::feature_catalog::{
    CatalogResolution, CatalogResolveMode, CatalogResolveRequest, CatalogSourceKind,
    resolve_catalog,
};

use crate::vendored_catalog::DECLARED_PROJECTIONS;

/// Selection mode for `cargo xtask features check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMode {
    /// Workspace authority from `features.toml`.
    Workspace,
    /// Crate-local generated fallback without walking to a workspace checkout.
    Package,
    /// Explicit test/tooling override path.
    Override,
}

/// Inputs for one check invocation.
#[derive(Debug, Clone)]
pub struct CheckRequest {
    /// Workspace root used as default authority and crate prefix.
    pub root: PathBuf,
    /// When `None`, check workspace plus every declared package.
    pub mode: Option<CheckMode>,
    /// Required for [`CheckMode::Package`].
    pub package: Option<String>,
    /// Package manifest directory; defaults to `root/crates/<package>`.
    pub manifest_dir: Option<PathBuf>,
    /// Required for [`CheckMode::Override`].
    pub override_path: Option<PathBuf>,
    /// Explicit authority for stale/wrong-projection comparison.
    pub authority_path: Option<PathBuf>,
}

/// One successful resolution line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOk {
    /// `workspace`, `package-fallback`, or `override`.
    pub kind: String,
    /// Source digest.
    pub digest: String,
    /// Declared projection class (`FullCatalog` on current main).
    pub projection: String,
    /// Selected path.
    pub path: PathBuf,
    /// Package identity when recorded.
    pub package: Option<String>,
}

/// Render a compact identity receipt.
pub fn format_ok(ok: &CheckOk) -> String {
    let mut line = format!("OK {} {}", ok.kind, ok.digest);
    let _ = write!(line, " projection={}", ok.projection);
    if let Some(package) = &ok.package {
        let _ = write!(line, " package={package}");
    }
    let _ = write!(line, " path={}", ok.path.display());
    line
}

/// Run `features check` for the requested mode(s).
pub fn check(request: &CheckRequest) -> Result<Vec<CheckOk>> {
    match request.mode {
        None => {
            let mut results = Vec::new();
            results.push(check_workspace(request)?);
            for projection in DECLARED_PROJECTIONS {
                results.push(check_package(request, projection.package)?);
            }
            Ok(results)
        }
        Some(CheckMode::Workspace) => Ok(vec![check_workspace(request)?]),
        Some(CheckMode::Package) => {
            let Some(package) = request.package.as_deref() else {
                bail!("--package is required for --mode package");
            };
            Ok(vec![check_package(request, package)?])
        }
        Some(CheckMode::Override) => Ok(vec![check_override(request)?]),
    }
}

fn check_workspace(request: &CheckRequest) -> Result<CheckOk> {
    let resolution = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &request.root,
        mode: CatalogResolveMode::Workspace,
        override_path: None,
        package: None,
        authority_path: None,
    })
    .map_err(|error| eyre!("{error}"))?;
    Ok(ok_from_resolution(&resolution))
}

fn check_package(request: &CheckRequest, package: &str) -> Result<CheckOk> {
    if !DECLARED_PROJECTIONS.iter().any(|projection| projection.package == package) {
        bail!("package {package} is not a declared feature-catalog fallback consumer");
    }
    let manifest_dir =
        request.manifest_dir.clone().unwrap_or_else(|| request.root.join("crates").join(package));
    let authority =
        request.authority_path.clone().unwrap_or_else(|| request.root.join("features.toml"));
    let resolution = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some(package),
        authority_path: Some(authority.as_path()),
    })
    .map_err(|error| eyre!("{error}"))?;
    if resolution.source.kind != CatalogSourceKind::Vendored {
        bail!(
            "{}: package-isolated check selected {}, not package-fallback",
            package,
            resolution.source.kind_label()
        );
    }
    Ok(ok_from_resolution(&resolution))
}

fn check_override(request: &CheckRequest) -> Result<CheckOk> {
    let Some(override_path) = request.override_path.clone() else {
        bail!("--override is required for --mode override");
    };
    let resolution = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &request.root,
        mode: CatalogResolveMode::Auto,
        override_path: Some(override_path),
        package: None,
        authority_path: None,
    })
    .map_err(|error| eyre!("{error}"))?;
    Ok(ok_from_resolution(&resolution))
}

fn ok_from_resolution(resolution: &CatalogResolution) -> CheckOk {
    CheckOk {
        kind: resolution.source.kind_label().to_string(),
        digest: resolution.identity.source_digest.clone(),
        projection: resolution.identity.projection_class.to_string(),
        path: resolution.source.path.clone(),
        package: resolution.identity.package.clone(),
    }
}
