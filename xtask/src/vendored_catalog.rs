//! Declared crate-local feature-catalog fallbacks (#9198 / #9199).
//!
//! Root `features.toml` is the sole human-edited authority. The landed #9198
//! generator projects that file as **byte-identical** crate-local
//! `features_sot.toml` copies (`ProjectionClass::FullCatalog`). This module is
//! the one inventory plus regenerate/check surface; it does not invent a second
//! generation authority or a resolver.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use perl_lsp_rs_core::feature_catalog::Catalog;

/// Repository-relative path of the human-edited catalog authority.
pub const AUTHORITY_RELATIVE: &str = "features.toml";

/// Header marker that crate-local copies are generated projections, not a
/// rival source of truth.
pub const GENERATED_MARKER: &str = "GENERATED PROJECTIONS";

/// Landed #9198 projection class. Subset/filtered projections are not a
/// current declared class; an undeclared subset is a checker failure (#9199).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionClass {
    /// Crate-local bytes are identical to root `features.toml`.
    FullCatalog,
}

/// One retained package fallback declared for generation and drift checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredProjection {
    /// Cargo package name that consumes the fallback.
    pub package: &'static str,
    /// Repository-relative path of the generated `features_sot.toml`.
    pub relative_path: &'static str,
    /// Projection class assigned to this package.
    pub class: ProjectionClass,
}

/// Every retained package-local feature-catalog fallback.
///
/// Adding a crate-local `features_sot.toml` without a row here is
/// `UNDECLARED_PROJECTION`. Omitting a declared row is `MISSING_PROJECTION`.
pub const DECLARED_PROJECTIONS: &[DeclaredProjection] = &[
    DeclaredProjection {
        package: "perl-lsp-rs",
        relative_path: "crates/perl-lsp-rs/features_sot.toml",
        class: ProjectionClass::FullCatalog,
    },
    DeclaredProjection {
        package: "perl-lsp-rs-core",
        relative_path: "crates/perl-lsp-rs-core/features_sot.toml",
        class: ProjectionClass::FullCatalog,
    },
    DeclaredProjection {
        package: "perl-parser",
        relative_path: "crates/perl-parser/features_sot.toml",
        class: ProjectionClass::FullCatalog,
    },
    DeclaredProjection {
        package: "perl-dap",
        relative_path: "crates/perl-dap/features_sot.toml",
        class: ProjectionClass::FullCatalog,
    },
];

/// Absorbed/deleted crate prefixes that must not appear as proof references in
/// generated fallbacks (or the authority they copy).
const ABSORBED_PROOF_PREFIXES: &[&str] = &[
    "crates/perl-feature-catalog/",
    "crates/perl-lsp-feature-contracts/",
    "crates/perl-lsp-config/",
    "crates/perl-content-length-framing/",
];

/// Bridge-era feature IDs that must not re-enter generated catalog bytes
/// after #6991 (#9199 DAP ordering note).
const BRIDGE_ERA_FEATURE_IDS: &[&str] = &["legacy-pls-bridge", "dap-phase1"];

/// Outcome of regenerating one declared projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegenFileOutcome {
    /// Repository-relative projection path.
    pub relative_path: String,
    /// Whether bytes were written because they differed from the authority.
    pub updated: bool,
}

/// Report from a regenerate pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegenReport {
    /// Per-declared-projection outcomes, in inventory order.
    pub files: Vec<RegenFileOutcome>,
}

impl RegenReport {
    /// True when any declared projection needed a write.
    pub fn any_updated(&self) -> bool {
        self.files.iter().any(|file| file.updated)
    }
}

/// Stable checker codes for fixture tests and `features invariants`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationCode {
    /// Authority or a declared projection could not be read.
    DriftRead,
    /// Projection bytes differ from the authority and are not a classified subset.
    VendoredDrift,
    /// Projection parses as a proper feature-id subset of the authority.
    UndeclaredSubset,
    /// A declared projection path is missing.
    MissingProjection,
    /// A crate-local `features_sot.toml` exists outside the declared inventory.
    UndeclaredProjection,
    /// Catalog bytes cite an absorbed crate path or bridge-era feature id.
    StaleProofPath,
    /// Root `features.toml` is missing.
    MissingAuthority,
    /// Authority/projection is missing the generated-projection marker.
    MissingGeneratedMarker,
    /// A declared projection's Cargo package manifest is missing.
    MissingPackage,
}

/// One deterministic checker finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionViolation {
    /// Machine-stable code.
    pub code: ViolationCode,
    /// Repository-relative path, or `features.toml` for authority findings.
    pub path: String,
    /// Extra detail for operators and tests.
    pub detail: String,
}

impl ProjectionViolation {
    /// Render the historical `features invariants` line for this finding.
    pub fn display_line(&self) -> String {
        match self.code {
            ViolationCode::VendoredDrift => format!(
                "VENDORED_DRIFT: {} differs from root features.toml (#7029); \
                 run `cargo xtask features regen-vendored`",
                self.path
            ),
            ViolationCode::DriftRead => {
                format!("DRIFT_READ: cannot read {}: {}", self.path, self.detail)
            }
            ViolationCode::MissingAuthority => {
                format!("DRIFT_READ: cannot read root features.toml: {}", self.detail)
            }
            ViolationCode::UndeclaredSubset => format!(
                "UNDECLARED_SUBSET: {} is a feature-id subset of root authority, \
                 not the declared FullCatalog projection (#9199)",
                self.path
            ),
            ViolationCode::MissingProjection => {
                format!("MISSING_PROJECTION: {} is declared but missing (#9199)", self.path)
            }
            ViolationCode::UndeclaredProjection => format!(
                "UNDECLARED_PROJECTION: {} exists but is not in the declared \
                 #9198 projection inventory (#9199)",
                self.path
            ),
            ViolationCode::StaleProofPath => {
                format!("STALE_PROOF_PATH: {}: {} (#9199)", self.path, self.detail)
            }
            ViolationCode::MissingGeneratedMarker => format!(
                "MISSING_GENERATED_MARKER: {} does not declare {GENERATED_MARKER} (#9199)",
                self.path
            ),
            ViolationCode::MissingPackage => format!(
                "MISSING_PACKAGE: {} is declared but its Cargo.toml is missing (#9199)",
                self.path
            ),
        }
    }
}

/// Byte-copy every declared projection from root `features.toml`.
///
/// This is the landed #9198 generator: same authority bytes in, same bytes out,
/// no timestamp, no per-package rewrite.
pub fn regenerate(root: &Path) -> Result<RegenReport, String> {
    let authority_path = root.join(AUTHORITY_RELATIVE);
    let authority = fs::read(&authority_path)
        .map_err(|error| format!("reading root {AUTHORITY_RELATIVE}: {error}"))?;
    let mut files = Vec::new();
    for projection in DECLARED_PROJECTIONS {
        match projection.class {
            ProjectionClass::FullCatalog => {}
        }
        let manifest = package_manifest_path(root, projection)?;
        if !manifest.is_file() {
            return Err(format!(
                "MISSING_PACKAGE: {} has no Cargo.toml; refusing to create a phantom \
                 projection (#9199)",
                projection.relative_path
            ));
        }
        let path = root.join(projection.relative_path);
        let previous = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(format!(
                    "reading current projection {}: {error}",
                    projection.relative_path
                ));
            }
        };
        let updated = previous != authority;
        if updated {
            fs::write(&path, &authority).map_err(|error| {
                format!("writing projection {}: {error}", projection.relative_path)
            })?;
        }
        files.push(RegenFileOutcome {
            relative_path: projection.relative_path.to_string(),
            updated,
        });
    }
    Ok(RegenReport { files })
}

/// Check declared projections, undeclared extras, stale proof refs, and markers.
pub fn check(root: &Path) -> Vec<ProjectionViolation> {
    let mut violations = Vec::new();
    let authority_path = root.join(AUTHORITY_RELATIVE);
    let authority = match fs::read(&authority_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            violations.push(ProjectionViolation {
                code: ViolationCode::MissingAuthority,
                path: AUTHORITY_RELATIVE.to_string(),
                detail: error.to_string(),
            });
            return violations;
        }
    };

    if !bytes_contain_marker(&authority) {
        violations.push(ProjectionViolation {
            code: ViolationCode::MissingGeneratedMarker,
            path: AUTHORITY_RELATIVE.to_string(),
            detail: GENERATED_MARKER.to_string(),
        });
    }
    push_stale_proof_paths(AUTHORITY_RELATIVE, &authority, &mut violations);

    for projection in DECLARED_PROJECTIONS {
        match package_manifest_path(root, projection) {
            Ok(manifest) if manifest.is_file() => {}
            Ok(_) | Err(_) => {
                violations.push(ProjectionViolation {
                    code: ViolationCode::MissingPackage,
                    path: projection.relative_path.to_string(),
                    detail: format!("package {} has no Cargo.toml", projection.package),
                });
                continue;
            }
        }
        match fs::read(root.join(projection.relative_path)) {
            Ok(bytes) if bytes == authority => {}
            Ok(bytes) => {
                classify_mismatch(projection.relative_path, &authority, &bytes, &mut violations);
                push_stale_proof_paths(projection.relative_path, &bytes, &mut violations);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                violations.push(ProjectionViolation {
                    code: ViolationCode::MissingProjection,
                    path: projection.relative_path.to_string(),
                    detail: String::new(),
                });
            }
            Err(error) => violations.push(ProjectionViolation {
                code: ViolationCode::DriftRead,
                path: projection.relative_path.to_string(),
                detail: error.to_string(),
            }),
        }
    }

    match discover_crate_local_fallbacks(root) {
        Ok(found) => {
            let declared: BTreeSet<&str> =
                DECLARED_PROJECTIONS.iter().map(|projection| projection.relative_path).collect();
            for extra in found {
                if !declared.contains(extra.as_str()) {
                    violations.push(ProjectionViolation {
                        code: ViolationCode::UndeclaredProjection,
                        path: extra,
                        detail: String::new(),
                    });
                }
            }
        }
        Err(error) => violations.push(ProjectionViolation {
            code: ViolationCode::DriftRead,
            path: "crates".to_string(),
            detail: error,
        }),
    }

    violations
}

fn classify_mismatch(
    relative: &str,
    authority: &[u8],
    projection: &[u8],
    violations: &mut Vec<ProjectionViolation>,
) {
    if is_undeclared_subset(authority, projection) {
        violations.push(ProjectionViolation {
            code: ViolationCode::UndeclaredSubset,
            path: relative.to_string(),
            detail: String::new(),
        });
        return;
    }
    violations.push(ProjectionViolation {
        code: ViolationCode::VendoredDrift,
        path: relative.to_string(),
        detail: String::new(),
    });
}

fn is_undeclared_subset(authority: &[u8], projection: &[u8]) -> bool {
    let Some(authority_ids) = parse_feature_ids(authority) else {
        return false;
    };
    let Some(projection_ids) = parse_feature_ids(projection) else {
        return false;
    };
    !projection_ids.is_empty()
        && projection_ids.len() < authority_ids.len()
        && projection_ids.is_subset(&authority_ids)
}

fn parse_feature_ids(bytes: &[u8]) -> Option<BTreeSet<String>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let catalog: Catalog = toml::from_str(text).ok()?;
    Some(catalog.feature.into_iter().map(|feature| feature.id).collect())
}

fn bytes_contain_marker(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|text| text.contains(GENERATED_MARKER))
}

fn push_stale_proof_paths(relative: &str, bytes: &[u8], violations: &mut Vec<ProjectionViolation>) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    for prefix in ABSORBED_PROOF_PREFIXES {
        if text.contains(prefix) {
            violations.push(ProjectionViolation {
                code: ViolationCode::StaleProofPath,
                path: relative.to_string(),
                detail: format!("cites absorbed/deleted proof path {prefix}"),
            });
        }
    }
    for feature_id in BRIDGE_ERA_FEATURE_IDS {
        if text.contains(feature_id) {
            violations.push(ProjectionViolation {
                code: ViolationCode::StaleProofPath,
                path: relative.to_string(),
                detail: format!("contains bridge-era feature id {feature_id}"),
            });
        }
    }
}

fn package_manifest_path(root: &Path, projection: &DeclaredProjection) -> Result<PathBuf, String> {
    let relative = Path::new(projection.relative_path);
    let Some(parent) = relative.parent() else {
        return Err(format!("{} has no parent directory", projection.relative_path));
    };
    Ok(root.join(parent).join("Cargo.toml"))
}

fn discover_crate_local_fallbacks(root: &Path) -> Result<BTreeSet<String>, String> {
    let crates_dir = root.join("crates");
    if !crates_dir.exists() {
        return Ok(BTreeSet::new());
    }
    let mut found = BTreeSet::new();
    let entries = fs::read_dir(&crates_dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(format!(
                "crate directory name is not UTF-8: {}",
                entry.file_name().to_string_lossy()
            ));
        };
        let relative = format!("crates/{name}/features_sot.toml");
        if root.join(&relative).is_file() {
            found.insert(relative);
        }
    }
    Ok(found)
}
