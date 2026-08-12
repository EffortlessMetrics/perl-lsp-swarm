use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

const SIDECAR_SUFFIX: &[u8] = b".meta.toml";

/// Portable root-relative identity of one sidecar/fixture pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarPairIdentity {
    /// Root-relative sidecar path.
    pub sidecar_path: PathBuf,
    /// Root-relative paired fixture path.
    pub fixture_path: PathBuf,
}

/// Runtime-resolved pair proven contained and regular under one bound root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSidecarPair {
    identity: SidecarPairIdentity,
    sidecar_path: PathBuf,
    fixture_path: PathBuf,
}

impl ValidatedSidecarPair {
    /// Portable pair identity.
    #[must_use]
    pub fn identity(&self) -> &SidecarPairIdentity {
        &self.identity
    }

    /// Validated runtime sidecar path.
    #[must_use]
    pub fn sidecar_path(&self) -> &Path {
        &self.sidecar_path
    }

    /// Validated runtime fixture path.
    #[must_use]
    pub fn fixture_path(&self) -> &Path {
        &self.fixture_path
    }
}

/// Root-bound filesystem authority for sidecar parsing and validation.
#[derive(Debug, Clone)]
pub struct SidecarValidationContext {
    root: PathBuf,
    selected_sidecars: Option<BTreeSet<PathBuf>>,
}

impl SidecarValidationContext {
    /// Bind validation to an existing non-symlink directory.
    pub fn bind(root: &Path) -> Result<Self> {
        let candidate = absolute_candidate(root)?;
        inspect_absolute_root(&candidate)?;
        let canonical = fs::canonicalize(&candidate)
            .with_context(|| format!("canonicalizing corpus root {}", candidate.display()))?;
        let metadata = fs::symlink_metadata(&canonical)
            .with_context(|| format!("inspecting corpus root {}", canonical.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("corpus root must be a non-symlink directory");
        }
        Ok(Self {
            root: canonical,
            selected_sidecars: None,
        })
    }

    /// Discover and bind the exact selected sidecar population.
    pub fn discover(root: &Path) -> Result<Self> {
        let mut context = Self::bind(root)?;
        let selected = context.discover_population()?;
        context.selected_sidecars = Some(selected);
        Ok(context)
    }

    /// Canonical runtime root. This path is execution context, not portable identity.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Deterministic selected sidecar identities, when this context came from discovery.
    pub fn sidecars(&self) -> impl Iterator<Item = &Path> {
        self.selected_sidecars
            .iter()
            .flat_map(|sidecars| sidecars.iter().map(PathBuf::as_path))
    }

    /// Resolve and validate one sidecar/fixture pair.
    pub fn resolve_pair(&self, sidecar_path: &Path) -> Result<ValidatedSidecarPair> {
        let sidecar_relative = self.relative_identity(sidecar_path)?;
        if let Some(selected) = &self.selected_sidecars
            && !selected.contains(&sidecar_relative)
        {
            bail!(
                "sidecar is not a member of the bound discovered population: {}",
                sidecar_relative.display()
            );
        }
        let fixture_relative = expected_fixture_relative(&sidecar_relative)?;
        let sidecar = self.resolve_regular_member(&sidecar_relative, "sidecar")?;
        let fixture = self.resolve_regular_member(&fixture_relative, "fixture")?;
        Ok(ValidatedSidecarPair {
            identity: SidecarPairIdentity {
                sidecar_path: sidecar_relative,
                fixture_path: fixture_relative,
            },
            sidecar_path: sidecar,
            fixture_path: fixture,
        })
    }

    /// Rebind a serialized portable identity and re-run every path check.
    pub fn rebind_pair(&self, identity: &SidecarPairIdentity) -> Result<ValidatedSidecarPair> {
        let pair = self.resolve_pair(&identity.sidecar_path)?;
        if pair.identity != *identity {
            bail!("sidecar pair identity does not match canonical sibling mapping");
        }
        Ok(pair)
    }

    fn discover_population(&self) -> Result<BTreeSet<PathBuf>> {
        let mut selected = BTreeSet::new();
        let mut stack = vec![PathBuf::new()];
        while let Some(relative_directory) = stack.pop() {
            let directory = self.root.join(&relative_directory);
            let entries = fs::read_dir(&directory)
                .with_context(|| format!("reading corpus directory {}", relative_directory.display()))?;
            for entry in entries {
                let entry = entry.with_context(|| {
                    format!("reading entry in corpus directory {}", relative_directory.display())
                })?;
                let file_name = entry.file_name();
                let relative = relative_directory.join(&file_name);
                let file_type = entry
                    .file_type()
                    .with_context(|| format!("inspecting corpus member {}", relative.display()))?;
                let selected_sidecar = has_sidecar_suffix(&file_name);

                if file_type.is_symlink() {
                    if selected_sidecar || path_has_extension(&relative, "pl") {
                        bail!("selected corpus member is a symlink: {}", relative.display());
                    }
                    if fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_dir()) {
                        bail!("corpus directory component is a symlink: {}", relative.display());
                    }
                    continue;
                }
                if file_type.is_dir() {
                    stack.push(relative);
                    continue;
                }
                if !selected_sidecar {
                    continue;
                }
                if !file_type.is_file() {
                    bail!("selected sidecar is not a regular file: {}", relative.display());
                }
                if file_name.to_str().is_none() {
                    bail!("selected sidecar filename is not valid UTF-8: {}", relative.display());
                }
                self.resolve_regular_member(&relative, "sidecar")?;
                let fixture = expected_fixture_relative(&relative)?;
                self.resolve_regular_member(&fixture, "fixture")?;
                selected.insert(relative);
            }
        }
        Ok(selected)
    }

    fn relative_identity(&self, path: &Path) -> Result<PathBuf> {
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root)
                .map_err(|_| anyhow::anyhow!("path is outside the bound corpus root"))?
        } else {
            path
        };
        normalize_relative(relative)
    }

    fn resolve_regular_member(&self, relative: &Path, role: &str) -> Result<PathBuf> {
        let relative = normalize_relative(relative)?;
        let components = relative.components().collect::<Vec<_>>();
        let mut current = self.root.clone();
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                bail!("{role} path is not a normalized relative identity");
            };
            current.push(name);
            let metadata = fs::symlink_metadata(&current)
                .with_context(|| format!("inspecting {role} member {}", relative.display()))?;
            if metadata.file_type().is_symlink() {
                bail!("{role} path crosses a symlink: {}", relative.display());
            }
            let final_component = index + 1 == components.len();
            if final_component {
                if !metadata.is_file() {
                    bail!("{role} member is not a regular file: {}", relative.display());
                }
            } else if !metadata.is_dir() {
                bail!("{role} parent component is not a directory: {}", relative.display());
            }
        }
        let canonical = fs::canonicalize(&current)
            .with_context(|| format!("canonicalizing {role} member {}", relative.display()))?;
        if !canonical.starts_with(&self.root) {
            bail!("{role} member escapes the bound corpus root");
        }
        Ok(canonical)
    }
}

fn expected_fixture_relative(sidecar: &Path) -> Result<PathBuf> {
    let file_name = sidecar
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("sidecar filename is not valid UTF-8"))?;
    let Some(stem) = file_name.strip_suffix(".meta.toml") else {
        bail!("sidecar filename must end with .meta.toml: {}", sidecar.display());
    };
    if stem.is_empty() {
        bail!("fixture stem must not be empty: {}", sidecar.display());
    }
    Ok(sidecar
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(format!("{stem}.pl")))
}

fn normalize_relative(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("path must be a nonempty relative corpus identity");
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => normalized.push(name),
            _ => bail!("path contains a prefix, root, current, or parent component"),
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("path must be a nonempty relative corpus identity");
    }
    Ok(normalized)
}

fn absolute_candidate(root: &Path) -> Result<PathBuf> {
    if root.is_absolute() {
        return Ok(root.to_path_buf());
    }
    let cwd = fs::canonicalize(std::env::current_dir().context("reading current directory")?)
        .context("canonicalizing current directory")?;
    let mut relative = PathBuf::new();
    for component in root.components() {
        match component {
            Component::Normal(name) => relative.push(name),
            Component::CurDir => {}
            Component::ParentDir => bail!("corpus root must not contain parent components"),
            Component::Prefix(_) | Component::RootDir => {
                bail!("relative corpus root contains an absolute prefix")
            }
        }
    }
    Ok(cwd.join(relative))
}

fn inspect_absolute_root(root: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in root.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(name) => {
                current.push(name);
                let metadata = fs::symlink_metadata(&current)
                    .with_context(|| format!("inspecting corpus root component {}", current.display()))?;
                if metadata.file_type().is_symlink() {
                    bail!("corpus root crosses a symlink component");
                }
            }
            Component::CurDir | Component::ParentDir => {
                bail!("corpus root must not contain current or parent components")
            }
        }
    }
    Ok(())
}

fn has_sidecar_suffix(file_name: &OsStr) -> bool {
    file_name.as_encoded_bytes().ends_with(SIDECAR_SUFFIX)
}

fn path_has_extension(path: &Path, extension: &str) -> bool {
    path.extension().and_then(OsStr::to_str) == Some(extension)
}
