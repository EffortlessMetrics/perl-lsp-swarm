use anyhow::{Context, Result, bail};
use same_file::Handle;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use super::FIXTURE_EXPECTATION_SCHEMA;

const SIDECAR_SUFFIX: &[u8] = b".meta.toml";
const PAIR_IDENTITY_SCHEMA: &str = "fixture_expectation_pair.v1";
const TOPOLOGY_IDENTITY_SCHEMA: &str = "fixture_expectation_topology.v1";

/// Portable content-bound identity of one sidecar/fixture pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarPairIdentity {
    /// Pair identity schema.
    pub schema_version: String,
    /// Sidecar semantic schema.
    pub sidecar_schema: String,
    /// Optional exact discovered-population identity.
    pub topology_identity: Option<String>,
    /// Root-relative sidecar path.
    pub sidecar_path: PathBuf,
    /// SHA-256 of the exact retained sidecar bytes.
    pub sidecar_digest: String,
    /// Root-relative paired fixture path.
    pub fixture_path: PathBuf,
    /// SHA-256 of the exact retained fixture bytes.
    pub fixture_digest: String,
}

#[derive(Debug)]
struct OpenedMember {
    handle: Handle,
    bytes: Vec<u8>,
}

/// Runtime-resolved pair proven contained, regular, identity-stable, and read
/// from retained handles rather than reopening the path after validation.
#[derive(Debug)]
pub struct ValidatedSidecarPair {
    identity: SidecarPairIdentity,
    sidecar: OpenedMember,
    fixture: OpenedMember,
}

impl ValidatedSidecarPair {
    /// Portable pair identity.
    #[must_use]
    pub fn identity(&self) -> &SidecarPairIdentity {
        &self.identity
    }

    /// Exact retained sidecar bytes whose digest appears in the identity.
    #[must_use]
    pub fn sidecar_bytes(&self) -> &[u8] {
        &self.sidecar.bytes
    }

    /// Exact retained fixture bytes whose digest appears in the identity.
    #[must_use]
    pub fn fixture_bytes(&self) -> &[u8] {
        &self.fixture.bytes
    }

    /// Keep the sidecar handle alive and observable for post-open tests.
    #[must_use]
    pub fn sidecar_handle(&self) -> &std::fs::File {
        self.sidecar.handle.as_file()
    }

    /// Keep the fixture handle alive and observable for post-open tests.
    #[must_use]
    pub fn fixture_handle(&self) -> &std::fs::File {
        self.fixture.handle.as_file()
    }
}

/// Root-bound filesystem authority for sidecar parsing and validation.
#[derive(Debug, Clone)]
pub struct SidecarValidationContext {
    root: PathBuf,
    selected_pairs: Option<BTreeMap<PathBuf, SidecarPairIdentity>>,
    topology_identity: Option<String>,
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
            selected_pairs: None,
            topology_identity: None,
        })
    }

    /// Discover and bind the exact selected sidecar population and content.
    pub fn discover(root: &Path) -> Result<Self> {
        let mut context = Self::bind(root)?;
        let paths = context.discover_paths()?;
        let mut identities = BTreeMap::new();
        for path in paths {
            let pair = context.open_pair_unchecked(&path, None)?;
            identities.insert(path, pair.identity);
        }
        let topology_identity = topology_digest(&identities)?;
        for identity in identities.values_mut() {
            identity.topology_identity = Some(topology_identity.clone());
        }
        context.selected_pairs = Some(identities);
        context.topology_identity = Some(topology_identity);
        Ok(context)
    }

    /// Canonical runtime root. This path is execution context, not portable identity.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Exact discovered-population identity, when available.
    #[must_use]
    pub fn topology_identity(&self) -> Option<&str> {
        self.topology_identity.as_deref()
    }

    /// Deterministic selected sidecar identities, when this context came from discovery.
    pub fn sidecars(&self) -> impl Iterator<Item = &Path> {
        self.selected_pairs
            .iter()
            .flat_map(|pairs| pairs.keys().map(PathBuf::as_path))
    }

    /// Open, read, digest, and validate one sidecar/fixture pair without later
    /// reopening either path for parsing.
    pub fn resolve_pair(&self, sidecar_path: &Path) -> Result<ValidatedSidecarPair> {
        let sidecar_relative = self.relative_identity(sidecar_path)?;
        let expected = if let Some(selected) = &self.selected_pairs {
            Some(selected.get(&sidecar_relative).ok_or_else(|| {
                anyhow::anyhow!(
                    "sidecar is not a member of the bound discovered population: {}",
                    sidecar_relative.display()
                )
            })?)
        } else {
            None
        };
        let pair = self.open_pair_unchecked(&sidecar_relative, self.topology_identity.clone())?;
        if let Some(expected) = expected
            && pair.identity != *expected
        {
            bail!(
                "sidecar pair content or identity changed since discovery: {}",
                sidecar_relative.display()
            );
        }
        Ok(pair)
    }

    /// Rebind a serialized portable identity and re-run path, handle, content,
    /// digest, schema, and topology checks.
    pub fn rebind_pair(&self, identity: &SidecarPairIdentity) -> Result<ValidatedSidecarPair> {
        if identity.schema_version != PAIR_IDENTITY_SCHEMA
            || identity.sidecar_schema != FIXTURE_EXPECTATION_SCHEMA
            || identity.topology_identity != self.topology_identity
        {
            bail!("sidecar pair schema or topology identity does not match the bound context");
        }
        let pair = self.resolve_pair(&identity.sidecar_path)?;
        if pair.identity != *identity {
            bail!("sidecar pair content identity changed after rebinding");
        }
        Ok(pair)
    }

    fn open_pair_unchecked(
        &self,
        sidecar_relative: &Path,
        topology_identity: Option<String>,
    ) -> Result<ValidatedSidecarPair> {
        let fixture_relative = expected_fixture_relative(sidecar_relative)?;
        let sidecar = self.open_regular_member(sidecar_relative, "sidecar")?;
        let fixture = self.open_regular_member(&fixture_relative, "fixture")?;
        let identity = SidecarPairIdentity {
            schema_version: PAIR_IDENTITY_SCHEMA.into(),
            sidecar_schema: FIXTURE_EXPECTATION_SCHEMA.into(),
            topology_identity,
            sidecar_path: sidecar_relative.to_path_buf(),
            sidecar_digest: digest_bytes(&sidecar.bytes),
            fixture_path: fixture_relative,
            fixture_digest: digest_bytes(&fixture.bytes),
        };
        Ok(ValidatedSidecarPair {
            identity,
            sidecar,
            fixture,
        })
    }

    fn discover_paths(&self) -> Result<BTreeSet<PathBuf>> {
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
                    if selected_sidecar {
                        bail!("selected sidecar is a symlink: {}", relative.display());
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

    fn open_regular_member(&self, relative: &Path, role: &str) -> Result<OpenedMember> {
        let relative = normalize_relative(relative)?;
        let path = self.root.join(&relative);
        self.inspect_member_components(&relative, role)?;

        let file = OpenOptions::new()
            .read(true)
            .open(&path)
            .with_context(|| format!("opening {role} member {}", relative.display()))?;
        let mut handle = Handle::from_file(file)
            .with_context(|| format!("identifying opened {role} member {}", relative.display()))?;
        self.verify_open_identity(&relative, role, &handle)?;

        let expected_len = handle
            .as_file()
            .metadata()
            .with_context(|| format!("inspecting opened {role} member {}", relative.display()))?
            .len();
        let file = handle.as_file_mut();
        file.seek(SeekFrom::Start(0))
            .with_context(|| format!("rewinding {role} member {}", relative.display()))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .with_context(|| format!("reading {role} member {}", relative.display()))?;
        if bytes.len() as u64 != expected_len {
            bail!("{role} member changed while it was being read: {}", relative.display());
        }
        file.seek(SeekFrom::Start(0))
            .with_context(|| format!("rewinding {role} member {}", relative.display()))?;
        self.verify_open_identity(&relative, role, &handle)?;
        Ok(OpenedMember { handle, bytes })
    }

    fn verify_open_identity(&self, relative: &Path, role: &str, handle: &Handle) -> Result<()> {
        self.inspect_member_components(relative, role)?;
        let path = self.root.join(relative);
        let current = Handle::from_path(&path)
            .with_context(|| format!("reopening {role} identity {}", relative.display()))?;
        if handle != &current {
            bail!("{role} path changed during validation: {}", relative.display());
        }
        self.inspect_member_components(relative, role)?;
        Ok(())
    }

    fn inspect_member_components(&self, relative: &Path, role: &str) -> Result<()> {
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
        Ok(())
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

fn topology_digest(identities: &BTreeMap<PathBuf, SidecarPairIdentity>) -> Result<String> {
    let encoded = serde_json::to_vec(&(TOPOLOGY_IDENTITY_SCHEMA, identities))
        .context("serializing sidecar topology identity")?;
    Ok(digest_bytes(&encoded))
}

fn digest_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

fn has_sidecar_suffix(file_name: &OsStr) -> bool {
    file_name.as_encoded_bytes().ends_with(SIDECAR_SUFFIX)
}
