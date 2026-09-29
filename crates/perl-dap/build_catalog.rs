use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct FeatureCatalog {
    feature: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
struct Feature {
    id: String,
    #[serde(default)]
    area: String,
    #[serde(default)]
    advertised: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSourceKind {
    Override,
    Workspace,
    Vendored,
}

#[derive(Debug)]
pub struct CatalogSource {
    pub path: PathBuf,
    pub kind: CatalogSourceKind,
}

impl CatalogSource {
    pub const fn kind_label(&self) -> &'static str {
        match self.kind {
            CatalogSourceKind::Override => "override",
            CatalogSourceKind::Workspace => "workspace",
            CatalogSourceKind::Vendored => "package-fallback",
        }
    }
}

pub fn resolve_catalog_source(manifest_dir: &Path) -> Result<CatalogSource, Box<dyn Error>> {
    resolve_catalog_source_with_override(
        manifest_dir,
        std::env::var("FEATURES_TOML_OVERRIDE").ok().map(PathBuf::from),
    )
}

pub fn resolve_catalog_source_with_override(
    manifest_dir: &Path,
    override_path: Option<PathBuf>,
) -> Result<CatalogSource, Box<dyn Error>> {
    resolve_catalog_source_with_mode(manifest_dir, override_path, false)
}

pub fn resolve_catalog_source_package_isolated(
    manifest_dir: &Path,
    override_path: Option<PathBuf>,
) -> Result<CatalogSource, Box<dyn Error>> {
    resolve_catalog_source_with_mode(manifest_dir, override_path, true)
}

fn resolve_catalog_source_with_mode(
    manifest_dir: &Path,
    override_path: Option<PathBuf>,
    package_isolated: bool,
) -> Result<CatalogSource, Box<dyn Error>> {
    if let Some(override_path) = override_path {
        if package_isolated {
            return Err(format!(
                "OVERRIDE_NOT_ALLOWED: package-isolated resolution refuses FEATURES_TOML_OVERRIDE ({})",
                override_path.display()
            )
            .into());
        }
        if override_path.exists() {
            return Ok(CatalogSource { path: override_path, kind: CatalogSourceKind::Override });
        }
        return Err(format!(
            "FEATURES_TOML_OVERRIDE path does not exist: {}",
            override_path.display()
        )
        .into());
    }

    if !package_isolated {
        let local = manifest_dir.join("features.toml");
        if local.exists() {
            return Ok(CatalogSource { path: local, kind: CatalogSourceKind::Workspace });
        }

        let workspace = manifest_dir
            .parent()
            .and_then(Path::parent)
            .map(|parent| parent.join("features.toml"))
            .filter(|path| path.exists());
        if let Some(path) = workspace {
            return Ok(CatalogSource { path, kind: CatalogSourceKind::Workspace });
        }
    }

    let vendored = manifest_dir.join("features_sot.toml");
    if vendored.exists() {
        return Ok(CatalogSource { path: vendored, kind: CatalogSourceKind::Vendored });
    }

    if package_isolated {
        return Err(format!(
            "MISSING_FALLBACK: package-isolated catalog requires {} (no workspace rediscovery)",
            vendored.display()
        )
        .into());
    }
    Err(format!("features catalog not found for manifest dir: {}", manifest_dir.display()).into())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("sha256:{hex}")
}

#[derive(Deserialize)]
struct PackageManifestFile {
    package: PackageManifestTable,
}

#[derive(Deserialize)]
struct PackageManifestTable {
    name: String,
}

fn read_package_name(manifest_dir: &Path) -> Result<String, Box<dyn Error>> {
    let path = manifest_dir.join("Cargo.toml");
    let text = fs::read_to_string(&path)?;
    let parsed: PackageManifestFile = toml::from_str(&text)?;
    Ok(parsed.package.name)
}

fn load_catalog_for_build(
    source: &CatalogSource,
) -> Result<(FeatureCatalog, String), Box<dyn Error>> {
    let bytes = fs::read(&source.path)?;
    let digest = sha256_hex(&bytes);
    let malformed = |error: &dyn std::fmt::Display| {
        if source.kind == CatalogSourceKind::Vendored {
            format!("MALFORMED_FALLBACK: {}: {error}", source.path.display())
        } else {
            format!("failed to parse features catalog {}: {error}", source.path.display())
        }
    };
    let text = std::str::from_utf8(&bytes).map_err(|error| malformed(&error))?;
    let catalog: FeatureCatalog = toml::from_str(text).map_err(|error| malformed(&error))?;
    Ok((catalog, digest))
}

fn advertised_debug_ids(catalog: &FeatureCatalog) -> Vec<String> {
    let mut source_features = catalog
        .feature
        .iter()
        .filter(|feature| feature.area == "debug" && feature.advertised)
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    source_features.sort_unstable();
    source_features.dedup();
    source_features
}

fn render_dap_feature_catalog_module(
    ids: &[String],
    kind_label: &str,
    digest: &str,
    package: Option<&str>,
) -> String {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut code = String::from("// @generated by build.rs; DO NOT EDIT.\n");
    code.push_str(&format!("// catalog-source-kind: {kind_label}\n"));
    code.push_str(&format!("// catalog-source-digest: {digest}\n"));
    code.push_str("// catalog-projection: FullCatalog\n");
    if let Some(package) = package {
        code.push_str(&format!("// catalog-package: {package}\n"));
    }
    code.push_str("\npub const ADVERTISED_DAP_FEATURES: &[&str] = &[\n");
    for id in &sorted {
        code.push_str(&format!("    {:?},\n", id));
    }
    code.push_str(
        "];\n\npub fn advertised_features() -> &'static [&'static str] { ADVERTISED_DAP_FEATURES }\n\npub fn has_feature(id: &str) -> bool { ADVERTISED_DAP_FEATURES.contains(&id) }\n",
    );
    code
}

pub fn generate_catalog_module_at(
    manifest_dir: &Path,
    out_dir: &Path,
    override_path: Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    generate_catalog_module_at_with_mode(manifest_dir, out_dir, override_path, false)
}

pub fn generate_catalog_module_package_isolated(
    manifest_dir: &Path,
    out_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    generate_catalog_module_at_with_mode(manifest_dir, out_dir, None, true)
}

fn generate_catalog_module_at_with_mode(
    manifest_dir: &Path,
    out_dir: &Path,
    override_path: Option<PathBuf>,
    package_isolated: bool,
) -> Result<(), Box<dyn Error>> {
    let source = resolve_catalog_source_with_mode(manifest_dir, override_path, package_isolated)?;
    let (catalog, digest) = load_catalog_for_build(&source)?;
    let source_features = advertised_debug_ids(&catalog);
    if source.kind != CatalogSourceKind::Override && source_features.is_empty() {
        return Err(format!(
            "EMPTY_FALLBACK: {} has no advertised debug catalog rows and cannot satisfy package proof",
            source.path.display()
        )
        .into());
    }
    let package = if source.kind == CatalogSourceKind::Vendored {
        let name = read_package_name(manifest_dir)?;
        if package_isolated && name != "perl-dap" {
            return Err(format!(
                "WRONG_PACKAGE: {} package name is {name}, expected perl-dap",
                manifest_dir.join("Cargo.toml").display()
            )
            .into());
        }
        Some(name)
    } else {
        None
    };
    let code = render_dap_feature_catalog_module(
        &source_features,
        source.kind_label(),
        &digest,
        package.as_deref(),
    );
    fs::write(out_dir.join("dap_feature_catalog.rs"), code)?;
    Ok(())
}
