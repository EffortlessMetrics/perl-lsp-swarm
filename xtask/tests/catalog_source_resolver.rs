//! Discriminating resolver fixtures for workspace vs package-fallback selection (#9201).
//!
//! These prove fail-closed source selection without running the unpacked-package
//! matrix (#9202). `perl-dap` is required in the package-isolated matrix.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use perl_lsp_rs_core::feature_catalog::{
    CatalogRejectCode, CatalogResolveMode, CatalogResolveRequest, CatalogSourceKind,
    resolve_catalog,
};
use tempfile::TempDir;
use xtask::catalog_source::{CheckMode, CheckRequest, check, format_ok};
use xtask::vendored_catalog::GENERATED_MARKER;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const AUTHORITY: &str = r#"# LSP/DAP Feature & Evidence Catalog - AUTHORITY
# Crate-local `features_sot.toml` files are DETERMINISTIC GENERATED PROJECTIONS of this file.

[meta]
version = "0.0.0-test"
lsp_version = "3.18"

[[feature]]
id = "lsp.completion"
maturity = "preview"
advertised = true
area = "text_document"
description = "completion"

[[feature]]
id = "dap.core"
maturity = "preview"
advertised = true
area = "debug"
description = "dap core"
"#;

const MINIMAL_EMPTY: &str = r#"# DETERMINISTIC GENERATED PROJECTIONS
[meta]
version = "0.0.0-test"
lsp_version = "3.18"
[[feature]]
id = "lsp.planned"
maturity = "planned"
advertised = false
area = "text_document"
description = "planned only"
"#;

fn sha256_of(bytes: &[u8]) -> String {
    perl_lsp_rs_core::hashing::sha256_hex(bytes)
}

fn write_package(root: &Path, package: &str, fallback: &str) -> TestResult<PathBuf> {
    let manifest_dir = root.join("crates").join(package);
    fs::create_dir_all(&manifest_dir)?;
    fs::write(
        manifest_dir.join("Cargo.toml"),
        format!("[package]\nname = \"{package}\"\nversion = \"0.0.0\"\n"),
    )?;
    fs::write(manifest_dir.join("features_sot.toml"), fallback)?;
    Ok(manifest_dir)
}

fn reject_code(
    error: perl_lsp_rs_core::feature_catalog::CatalogError,
) -> TestResult<CatalogRejectCode> {
    match error {
        perl_lsp_rs_core::feature_catalog::CatalogError::Rejected { code, .. } => Ok(code),
        other => Err(format!("expected Rejected, got {other}").into()),
    }
}

#[test]
fn workspace_selection_is_deterministic_and_digest_observable() -> TestResult {
    let root = TempDir::new()?;
    fs::write(root.path().join("features.toml"), AUTHORITY)?;
    let resolution = resolve_catalog(CatalogResolveRequest {
        manifest_dir: root.path(),
        mode: CatalogResolveMode::Workspace,
        override_path: None,
        package: None,
        authority_path: None,
    })?;
    assert_eq!(resolution.source.kind, CatalogSourceKind::Workspace);
    assert_eq!(resolution.identity.source_digest, sha256_of(AUTHORITY.as_bytes()));
    assert_eq!(resolution.identity.projection_class, "FullCatalog");
    let header = resolution.generated_header();
    assert!(header.contains("catalog-source-kind: workspace"));
    assert!(
        header.contains(&format!("catalog-source-digest: {}", resolution.identity.source_digest))
    );
    Ok(())
}

#[test]
fn package_isolated_perl_dap_accepts_exact_generated_fallback() -> TestResult {
    let root = TempDir::new()?;
    fs::write(root.path().join("features.toml"), AUTHORITY)?;
    let manifest_dir = write_package(root.path(), "perl-dap", AUTHORITY)?;
    let resolution = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some("perl-dap"),
        authority_path: Some(&root.path().join("features.toml")),
    })?;
    assert_eq!(resolution.source.kind, CatalogSourceKind::Vendored);
    assert_eq!(resolution.source.kind_label(), "package-fallback");
    assert_eq!(resolution.identity.package.as_deref(), Some("perl-dap"));
    assert_eq!(resolution.identity.source_digest, sha256_of(AUTHORITY.as_bytes()));
    assert!(resolution.generated_header().contains("catalog-package: perl-dap"));
    Ok(())
}

#[test]
fn package_isolated_does_not_rediscover_workspace_authority() -> TestResult {
    let root = TempDir::new()?;
    fs::write(root.path().join("features.toml"), AUTHORITY)?;
    let manifest_dir = write_package(root.path(), "perl-dap", AUTHORITY)?;
    fs::remove_file(manifest_dir.join("features_sot.toml"))?;
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some("perl-dap"),
        authority_path: Some(&root.path().join("features.toml")),
    })
    .expect_err("missing fallback must not walk to workspace features.toml");
    assert_eq!(reject_code(error)?, CatalogRejectCode::MissingFallback);
    Ok(())
}

#[test]
fn missing_package_fallback_fails_closed() -> TestResult {
    let root = TempDir::new()?;
    let manifest_dir = root.path().join("crates/perl-dap");
    fs::create_dir_all(&manifest_dir)?;
    fs::write(
        manifest_dir.join("Cargo.toml"),
        "[package]\nname = \"perl-dap\"\nversion = \"0.0.0\"\n",
    )?;
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some("perl-dap"),
        authority_path: None,
    })
    .expect_err("missing fallback");
    assert_eq!(reject_code(error)?, CatalogRejectCode::MissingFallback);
    Ok(())
}

#[test]
fn stale_root_digest_fails_closed() -> TestResult {
    let root = TempDir::new()?;
    fs::write(root.path().join("features.toml"), AUTHORITY)?;
    let stale = AUTHORITY.replace("0.0.0-test", "0.0.0-stale");
    let manifest_dir = write_package(root.path(), "perl-dap", &stale)?;
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some("perl-dap"),
        authority_path: Some(&root.path().join("features.toml")),
    })
    .expect_err("stale digest");
    assert_eq!(reject_code(error)?, CatalogRejectCode::StaleDigest);
    Ok(())
}

#[test]
fn malformed_package_fallback_fails_closed() -> TestResult {
    let root = TempDir::new()?;
    let manifest_dir = write_package(root.path(), "perl-dap", "this is not toml [[[")?;
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some("perl-dap"),
        authority_path: None,
    })
    .expect_err("malformed fallback");
    assert_eq!(reject_code(error)?, CatalogRejectCode::MalformedFallback);
    Ok(())
}

#[test]
fn wrong_projection_subset_fails_closed() -> TestResult {
    let root = TempDir::new()?;
    fs::write(root.path().join("features.toml"), AUTHORITY)?;
    let subset = r#"# DETERMINISTIC GENERATED PROJECTIONS
[meta]
version = "0.0.0-test"
lsp_version = "3.18"

[[feature]]
id = "dap.core"
maturity = "preview"
advertised = true
area = "debug"
description = "dap core"
"#;
    let manifest_dir = write_package(root.path(), "perl-dap", subset)?;
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some("perl-dap"),
        authority_path: Some(&root.path().join("features.toml")),
    })
    .expect_err("subset is wrong projection");
    assert_eq!(reject_code(error)?, CatalogRejectCode::WrongProjection);
    Ok(())
}

#[test]
fn silent_empty_fallback_cannot_satisfy_package_proof() -> TestResult {
    let root = TempDir::new()?;
    let manifest_dir = write_package(root.path(), "perl-dap", MINIMAL_EMPTY)?;
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some("perl-dap"),
        authority_path: None,
    })
    .expect_err("empty fallback");
    assert_eq!(reject_code(error)?, CatalogRejectCode::EmptyFallback);
    Ok(())
}

#[test]
fn wrong_package_identity_fails_closed() -> TestResult {
    let root = TempDir::new()?;
    let manifest_dir = write_package(root.path(), "perl-lsp-rs-core", AUTHORITY)?;
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: None,
        package: Some("perl-dap"),
        authority_path: None,
    })
    .expect_err("wrong package");
    assert_eq!(reject_code(error)?, CatalogRejectCode::WrongPackage);
    Ok(())
}

#[test]
fn explicit_override_is_identity_recorded_and_missing_is_terminal() -> TestResult {
    let root = TempDir::new()?;
    fs::write(root.path().join("features.toml"), AUTHORITY)?;
    let override_path = root.path().join("override.toml");
    fs::write(&override_path, AUTHORITY)?;
    let resolution = resolve_catalog(CatalogResolveRequest {
        manifest_dir: root.path(),
        mode: CatalogResolveMode::Auto,
        override_path: Some(override_path.clone()),
        package: None,
        authority_path: None,
    })?;
    assert_eq!(resolution.source.kind, CatalogSourceKind::Override);
    assert!(resolution.generated_header().contains("catalog-source-kind: override"));
    assert_eq!(resolution.identity.source_digest, sha256_of(AUTHORITY.as_bytes()));

    let missing = root.path().join("missing.toml");
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: root.path(),
        mode: CatalogResolveMode::Auto,
        override_path: Some(missing),
        package: None,
        authority_path: None,
    })
    .expect_err("missing override");
    assert!(matches!(error, perl_lsp_rs_core::feature_catalog::CatalogError::MissingOverride(_)));
    Ok(())
}

#[test]
fn package_isolated_refuses_arbitrary_override() -> TestResult {
    let root = TempDir::new()?;
    let manifest_dir = write_package(root.path(), "perl-dap", AUTHORITY)?;
    let override_path = root.path().join("override.toml");
    fs::write(&override_path, AUTHORITY)?;
    let error = resolve_catalog(CatalogResolveRequest {
        manifest_dir: &manifest_dir,
        mode: CatalogResolveMode::PackageIsolated,
        override_path: Some(override_path),
        package: Some("perl-dap"),
        authority_path: None,
    })
    .expect_err("override is not a package production path");
    assert_eq!(reject_code(error)?, CatalogRejectCode::OverrideNotAllowed);
    Ok(())
}

#[test]
fn features_check_covers_workspace_package_and_override() -> TestResult {
    let root = TempDir::new()?;
    fs::write(root.path().join("features.toml"), AUTHORITY)?;
    write_package(root.path(), "perl-dap", AUTHORITY)?;
    let workspace = check(&CheckRequest {
        root: root.path().to_path_buf(),
        mode: Some(CheckMode::Workspace),
        package: None,
        manifest_dir: None,
        override_path: None,
        authority_path: None,
    })?;
    assert_eq!(workspace.len(), 1);
    let workspace_line = format_ok(&workspace[0]);
    assert!(workspace_line.starts_with("OK workspace sha256:"));
    assert!(workspace_line.contains("projection=FullCatalog"));

    let package = check(&CheckRequest {
        root: root.path().to_path_buf(),
        mode: Some(CheckMode::Package),
        package: Some("perl-dap".into()),
        manifest_dir: None,
        override_path: None,
        authority_path: None,
    })?;
    assert_eq!(package.len(), 1);
    let line = format_ok(&package[0]);
    assert!(line.contains("OK package-fallback"));
    assert!(line.contains("projection=FullCatalog"));
    assert!(line.contains("package=perl-dap"));

    let override_path = root.path().join("override.toml");
    fs::write(&override_path, AUTHORITY)?;
    let over = check(&CheckRequest {
        root: root.path().to_path_buf(),
        mode: Some(CheckMode::Override),
        package: None,
        manifest_dir: None,
        override_path: Some(override_path),
        authority_path: None,
    })?;
    assert_eq!(over.len(), 1);
    assert!(format_ok(&over[0]).starts_with("OK override sha256:"));
    Ok(())
}

#[test]
fn generated_marker_is_present_on_full_catalog_authority_copy() {
    assert!(AUTHORITY.contains(GENERATED_MARKER));
}
