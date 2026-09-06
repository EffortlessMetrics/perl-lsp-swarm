//! Resolve Cargo-configured rustc wrappers into a durable subject digest (#14687).
//!
//! `ux_case_inventory.v1`'s `subject_digest` must pin the compiler wrapper that
//! actually sat between Cargo and rustc. Reading only `$RUSTC_WRAPPER` /
//! `$RUSTC_WORKSPACE_WRAPPER` misses `build.rustc-wrapper` declared through
//! Cargo configuration. Reading only the workspace `.cargo/config.toml` misses
//! `$CARGO_HOME/config.toml` — the usual global `sccache` site — while making
//! the subject look complete.
//!
//! This module reproduces stable Cargo's documented resolution for those two
//! keys, evaluated at the workspace root `compile_test_targets` uses:
//!
//! - hierarchical files: each ancestor's `.cargo/config` (legacy name, preferred
//!   when both exist) or `.cargo/config.toml`, then `$CARGO_HOME/config.toml`
//!   (same legacy-name rule), with deeper directories winning;
//! - environment over configuration, including the empty-string clear;
//! - dedicated vars (`RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`) over the
//!   generic `CARGO_BUILD_*` mappings, including when the dedicated var is
//!   empty;
//! - no automatic `.cargo/config.local.toml` (not a Cargo-native file);
//! - unstable `include` is not followed on stable Cargo; its presence keeps
//!   `cargo_config_wrapper_not_resolved` rather than silently applying or
//!   silently ignoring included wrappers;
//! - relative wrapper values with a directory component are resolved from the
//!   parent of the directory that holds the config file (project `.cargo/` or
//!   `$CARGO_HOME`), matching stable Cargo; environment values use the
//!   workspace root as cwd; bare names stay PATH lookups;
//! - empty string (`""`) clears a wrapper; whitespace-only values are present
//!   programs, as Cargo does not trim.
//!
//! Absolute paths never enter the durable projection: in-workspace paths become
//! workspace-relative, and external paths become `{basename}@{sha256:...}` of
//! the slash-normalized absolute path. Unresolvable layers raise a named
//! limitation and never serialize as "no wrapper".

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use perl_lsp_rs_core::hashing::sha256_hex;

/// Standing limitation name from #14687. Present only when a wrapper slot
/// could not be established; retired when both slots resolve (including to
/// "no wrapper").
pub const CARGO_CONFIG_WRAPPER_NOT_RESOLVED: &str = "cargo_config_wrapper_not_resolved";

const WRAPPER_ENV: &str = "RUSTC_WRAPPER";
const WRAPPER_CARGO_ENV: &str = "CARGO_BUILD_RUSTC_WRAPPER";
const WORKSPACE_WRAPPER_ENV: &str = "RUSTC_WORKSPACE_WRAPPER";
const WORKSPACE_WRAPPER_CARGO_ENV: &str = "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER";
const CARGO_HOME_ENV: &str = "CARGO_HOME";
const HOME_ENV: &str = "HOME";
const USERPROFILE_ENV: &str = "USERPROFILE";

const ENV_KEYS: &[&str] = &[
    WRAPPER_ENV,
    WRAPPER_CARGO_ENV,
    WORKSPACE_WRAPPER_ENV,
    WORKSPACE_WRAPPER_CARGO_ENV,
    CARGO_HOME_ENV,
    HOME_ENV,
    USERPROFILE_ENV,
];

/// Process-visible environment for wrapper resolution.
///
/// Only the keys Cargo uses for these two settings plus home discovery are
/// captured, so tests can construct an isolated snapshot without locking the
/// whole process environment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvSnapshot {
    vars: BTreeMap<String, String>,
}

impl EnvSnapshot {
    /// Capture the Cargo wrapper and home variables from the current process.
    #[must_use]
    pub fn from_process() -> Self {
        let mut vars = BTreeMap::new();
        for key in ENV_KEYS {
            if let Ok(value) = std::env::var(key) {
                vars.insert((*key).to_string(), value);
            }
        }
        Self { vars }
    }

    /// Insert or replace a captured variable. Used by tests and by callers that
    /// already isolated an environment.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }
}

/// One resolved wrapper slot. `Unresolved` must not hash as `Absent`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum WrapperSlot {
    Absent,
    Present { durable: String, local: String },
    Unresolved,
}

/// Effective `build.rustc-wrapper` / `build.rustc-workspace-wrapper` plus the
/// durable subject contribution they produce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCompilerWrappers {
    rustc_wrapper: Option<String>,
    rustc_workspace_wrapper: Option<String>,
    rustc_wrapper_local: Option<String>,
    rustc_workspace_wrapper_local: Option<String>,
    rustc_wrapper_unresolved: bool,
    rustc_workspace_wrapper_unresolved: bool,
    limitations: Vec<String>,
    subject_digest: String,
}

impl ResolvedCompilerWrappers {
    /// Durable wrapper value, if the slot resolved to a configured program.
    #[must_use]
    pub fn rustc_wrapper(&self) -> Option<&str> {
        self.rustc_wrapper.as_deref()
    }

    /// Durable workspace-wrapper value, if resolved to a configured program.
    #[must_use]
    pub fn rustc_workspace_wrapper(&self) -> Option<&str> {
        self.rustc_workspace_wrapper.as_deref()
    }

    /// Machine-local wrapper path or name; excluded from [`Self::subject_digest`].
    #[must_use]
    pub fn rustc_wrapper_local(&self) -> Option<&str> {
        self.rustc_wrapper_local.as_deref()
    }

    /// Machine-local workspace-wrapper path or name.
    #[must_use]
    pub fn rustc_workspace_wrapper_local(&self) -> Option<&str> {
        self.rustc_workspace_wrapper_local.as_deref()
    }

    /// Sorted, deduplicated limitation names. Empty when both slots resolved.
    #[must_use]
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }

    /// Digest of the durable wrapper projection. Two resolved wrapper settings
    /// that Cargo would treat as different environments produce different
    /// digests; an unresolved hierarchy does not hash as "no wrapper".
    #[must_use]
    pub fn subject_digest(&self) -> &str {
        &self.subject_digest
    }

    /// True when both wrapper keys were established (including "no wrapper").
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.rustc_wrapper_unresolved
            && !self.rustc_workspace_wrapper_unresolved
            && self.limitations.is_empty()
    }

    /// Suffix for a rustc-identity subject field, matching the env-only form
    /// `RUSTC_WRAPPER: name | RUSTC_WORKSPACE_WRAPPER: name`.
    #[must_use]
    pub fn durable_toolchain_suffix(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(value) = &self.rustc_wrapper {
            parts.push(format!("{WRAPPER_ENV}: {value}"));
        }
        if let Some(value) = &self.rustc_workspace_wrapper {
            parts.push(format!("{WORKSPACE_WRAPPER_ENV}: {value}"));
        }
        (!parts.is_empty()).then(|| parts.join(" | "))
    }
}

/// Resolve wrappers the way stable Cargo does at `workspace_root`.
///
/// Walks to the filesystem root and reads the real `$CARGO_HOME`. Tests pass a
/// search ceiling so a temp fixture cannot inherit a machine parent config.
#[must_use]
pub fn resolve_compiler_wrappers(
    workspace_root: &Path,
    env: &EnvSnapshot,
) -> ResolvedCompilerWrappers {
    resolve_compiler_wrappers_in(workspace_root, env, None, &RealFs)
}

/// Resolve wrappers, stopping directory probing at `search_ceiling` when set.
///
/// `search_ceiling` is a test seam so a temp fixture does not inherit a
/// machine `/tmp/.cargo/config.toml`. Production discovery passes `None`.
#[must_use]
pub(crate) fn resolve_compiler_wrappers_in(
    workspace_root: &Path,
    env: &EnvSnapshot,
    search_ceiling: Option<&Path>,
    fs: &dyn ConfigFs,
) -> ResolvedCompilerWrappers {
    let mut layers = ConfigLayers::default();
    match cargo_home_dir(env) {
        CargoHome::Path(path) => {
            if let Err(limitation) = load_home_config(&path, workspace_root, fs, &mut layers) {
                layers.fail(limitation);
            }
        }
        CargoHome::Unknown => layers.fail(CARGO_CONFIG_WRAPPER_NOT_RESOLVED),
    }

    for dir in search_dirs(workspace_root, search_ceiling).into_iter().rev() {
        if let Err(limitation) = load_directory_config(&dir, workspace_root, fs, &mut layers) {
            layers.fail(limitation);
        }
    }

    let rustc_wrapper = resolve_slot(
        env,
        WRAPPER_ENV,
        WRAPPER_CARGO_ENV,
        layers.rustc_wrapper.as_ref(),
        layers.complete,
        workspace_root,
    );
    let rustc_workspace_wrapper = resolve_slot(
        env,
        WORKSPACE_WRAPPER_ENV,
        WORKSPACE_WRAPPER_CARGO_ENV,
        layers.rustc_workspace_wrapper.as_ref(),
        layers.complete,
        workspace_root,
    );

    finish(rustc_wrapper, rustc_workspace_wrapper, layers.limitations)
}

/// Filesystem view used while probing Cargo config paths.
pub(crate) trait ConfigFs {
    /// Whether `path` is a regular file.
    fn is_file(&self, path: &Path) -> bool;
    /// Read `path` as UTF-8 text. Missing files should return `NotFound`.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
}

/// Real filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RealFs;

impl ConfigFs for RealFs {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// Raw config value plus the directory Cargo joins relative wrappers against.
struct ConfiguredValue {
    raw: String,
    origin_dir: PathBuf,
}

struct ConfigLayers {
    rustc_wrapper: Option<ConfiguredValue>,
    rustc_workspace_wrapper: Option<ConfiguredValue>,
    complete: bool,
    limitations: BTreeSet<String>,
}

impl ConfigLayers {
    fn fail(&mut self, limitation: &str) {
        self.complete = false;
        self.limitations.insert(limitation.to_string());
    }

    fn apply_file(&mut self, parsed: ParsedConfig, origin_dir: PathBuf) {
        if parsed.include_present {
            self.fail(CARGO_CONFIG_WRAPPER_NOT_RESOLVED);
        }
        if let Some(value) = parsed.rustc_wrapper {
            self.rustc_wrapper =
                Some(ConfiguredValue { raw: value, origin_dir: origin_dir.clone() });
        }
        if let Some(value) = parsed.rustc_workspace_wrapper {
            self.rustc_workspace_wrapper = Some(ConfiguredValue { raw: value, origin_dir });
        }
    }
}

impl Default for ConfigLayers {
    fn default() -> Self {
        Self {
            rustc_wrapper: None,
            rustc_workspace_wrapper: None,
            complete: true,
            limitations: BTreeSet::new(),
        }
    }
}

enum CargoHome {
    Path(PathBuf),
    Unknown,
}

fn cargo_home_dir(env: &EnvSnapshot) -> CargoHome {
    match env.get(CARGO_HOME_ENV) {
        Some(value) if value.trim().is_empty() => return CargoHome::Unknown,
        Some(value) => return CargoHome::Path(PathBuf::from(value)),
        None => {}
    }
    let home = env.get(HOME_ENV).or_else(|| env.get(USERPROFILE_ENV));
    match home {
        Some(value) if !value.trim().is_empty() => {
            CargoHome::Path(PathBuf::from(value).join(".cargo"))
        }
        _ => CargoHome::Unknown,
    }
}

fn search_dirs(workspace_root: &Path, ceiling: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = Some(workspace_root);
    while let Some(dir) = current {
        dirs.push(dir.to_path_buf());
        if ceiling.is_some_and(|limit| dir == limit) {
            break;
        }
        let parent = dir.parent();
        if parent == Some(dir) {
            break;
        }
        current = parent;
    }
    dirs
}

fn load_home_config(
    cargo_home: &Path,
    workspace_root: &Path,
    fs: &dyn ConfigFs,
    layers: &mut ConfigLayers,
) -> Result<(), &'static str> {
    load_named_config(cargo_home, workspace_root, fs, layers)
}

fn load_directory_config(
    dir: &Path,
    workspace_root: &Path,
    fs: &dyn ConfigFs,
    layers: &mut ConfigLayers,
) -> Result<(), &'static str> {
    load_named_config(&dir.join(".cargo"), workspace_root, fs, layers)
}

fn load_named_config(
    dir: &Path,
    workspace_root: &Path,
    fs: &dyn ConfigFs,
    layers: &mut ConfigLayers,
) -> Result<(), &'static str> {
    let legacy = dir.join("config");
    let modern = dir.join("config.toml");
    let path = if fs.is_file(&legacy) {
        legacy
    } else if fs.is_file(&modern) {
        modern
    } else {
        return Ok(());
    };
    let body = match fs.read_to_string(&path) {
        Ok(body) => body,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(CARGO_CONFIG_WRAPPER_NOT_RESOLVED),
    };
    let parsed = parse_config(&body)?;
    layers.apply_file(parsed, origin_dir_for_config_path(&path, workspace_root));
    Ok(())
}

/// Parent of the directory that holds the config file.
///
/// `{workspace}/.cargo/config.toml` → `{workspace}`. `$CARGO_HOME/config.toml`
/// → parent of `$CARGO_HOME`. Relative wrappers with a directory component are
/// joined against this origin, matching stable Cargo.
fn origin_dir_for_config_path(config_path: &Path, workspace_root: &Path) -> PathBuf {
    config_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| workspace_root.to_path_buf())
}

struct ParsedConfig {
    rustc_wrapper: Option<String>,
    rustc_workspace_wrapper: Option<String>,
    include_present: bool,
}

fn parse_config(body: &str) -> Result<ParsedConfig, &'static str> {
    let value: toml::Value = toml::from_str(body).map_err(|_| CARGO_CONFIG_WRAPPER_NOT_RESOLVED)?;
    Ok(ParsedConfig {
        rustc_wrapper: config_string(&value, "build", "rustc-wrapper")?,
        rustc_workspace_wrapper: config_string(&value, "build", "rustc-workspace-wrapper")?,
        include_present: value.get("include").is_some(),
    })
}

fn config_string(
    value: &toml::Value,
    table: &str,
    key: &str,
) -> Result<Option<String>, &'static str> {
    // Only `[build] rustc-wrapper` (or unquoted dotted keys that TOML nests
    // into that table). A quoted root key `"build.rustc-wrapper"` is a
    // different string key; stable Cargo ignores it.
    match value.get(table).and_then(|nested| nested.get(key)) {
        Some(entry) => string_entry(entry),
        None => Ok(None),
    }
}

fn string_entry(value: &toml::Value) -> Result<Option<String>, &'static str> {
    match value.as_str() {
        Some(text) => Ok(Some(text.to_string())),
        None => Err(CARGO_CONFIG_WRAPPER_NOT_RESOLVED),
    }
}

fn resolve_slot(
    env: &EnvSnapshot,
    dedicated: &str,
    cargo_mapped: &str,
    from_config: Option<&ConfiguredValue>,
    config_complete: bool,
    workspace_root: &Path,
) -> WrapperSlot {
    if let Some(value) = env.get(dedicated) {
        return slot_from_env_value(value, workspace_root, workspace_root);
    }
    if let Some(value) = env.get(cargo_mapped) {
        return slot_from_env_value(value, workspace_root, workspace_root);
    }
    if !config_complete {
        return WrapperSlot::Unresolved;
    }
    match from_config {
        Some(configured) if !configured.raw.is_empty() => {
            present_from_configured(&configured.raw, &configured.origin_dir, workspace_root)
        }
        _ => WrapperSlot::Absent,
    }
}

fn slot_from_env_value(value: &str, origin_dir: &Path, workspace_root: &Path) -> WrapperSlot {
    if value.is_empty() {
        WrapperSlot::Absent
    } else {
        present_from_configured(value, origin_dir, workspace_root)
    }
}

fn present_from_configured(value: &str, origin_dir: &Path, workspace_root: &Path) -> WrapperSlot {
    let resolved = resolve_wrapper_path(value, origin_dir);
    let durable = durable_wrapper_identity(&resolved, workspace_root);
    let local = resolved.to_string_lossy().replace('\\', "/");
    WrapperSlot::Present { durable, local }
}

fn wrapper_value_has_directory(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

fn resolve_wrapper_path(value: &str, origin_dir: &Path) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if wrapper_value_has_directory(value) {
        return origin_dir.join(path);
    }
    PathBuf::from(value)
}

fn durable_wrapper_identity(resolved: &Path, workspace_root: &Path) -> String {
    let display = resolved.to_string_lossy();
    if !resolved.is_absolute() && !wrapper_value_has_directory(&display) {
        return display.replace('\\', "/");
    }
    let abs =
        if resolved.is_absolute() { resolved.to_path_buf() } else { workspace_root.join(resolved) };
    if let Ok(relative) = abs.strip_prefix(workspace_root) {
        let rendered = relative.to_string_lossy().replace('\\', "/");
        if !rendered.is_empty() {
            return rendered;
        }
    }
    let basename = abs
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "wrapper".to_string());
    let normalized = abs.to_string_lossy().replace('\\', "/");
    format!("{basename}@{}", sha256_hex(normalized.as_bytes()))
}

fn finish(
    rustc_wrapper: WrapperSlot,
    rustc_workspace_wrapper: WrapperSlot,
    mut limitations: BTreeSet<String>,
) -> ResolvedCompilerWrappers {
    let rustc_wrapper_unresolved = matches!(rustc_wrapper, WrapperSlot::Unresolved);
    let rustc_workspace_wrapper_unresolved =
        matches!(rustc_workspace_wrapper, WrapperSlot::Unresolved);
    if rustc_wrapper_unresolved || rustc_workspace_wrapper_unresolved {
        limitations.insert(CARGO_CONFIG_WRAPPER_NOT_RESOLVED.to_string());
    } else {
        // Environment fully answers both keys: a missing CARGO_HOME or an
        // unreadable config file cannot change the effective wrappers.
        limitations.remove(CARGO_CONFIG_WRAPPER_NOT_RESOLVED);
    }
    let rustc_wrapper_local = local_of(&rustc_wrapper);
    let rustc_workspace_wrapper_local = local_of(&rustc_workspace_wrapper);
    let rustc_wrapper = durable_of(&rustc_wrapper);
    let rustc_workspace_wrapper = durable_of(&rustc_workspace_wrapper);
    let encoded = encode_digest_payload(
        rustc_wrapper.as_deref(),
        rustc_workspace_wrapper.as_deref(),
        rustc_wrapper_unresolved,
        rustc_workspace_wrapper_unresolved,
    );
    ResolvedCompilerWrappers {
        rustc_wrapper,
        rustc_workspace_wrapper,
        rustc_wrapper_local,
        rustc_workspace_wrapper_local,
        rustc_wrapper_unresolved,
        rustc_workspace_wrapper_unresolved,
        limitations: limitations.into_iter().collect(),
        subject_digest: sha256_hex(&encoded),
    }
}

/// Length-prefixed encoding so embedded newlines or field-like text in a
/// wrapper value cannot collide with another slot assignment.
fn encode_digest_payload(
    rustc_wrapper: Option<&str>,
    rustc_workspace_wrapper: Option<&str>,
    rustc_wrapper_unresolved: bool,
    rustc_workspace_wrapper_unresolved: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    encode_digest_slot(&mut out, b"rustc_wrapper", rustc_wrapper, rustc_wrapper_unresolved);
    encode_digest_slot(
        &mut out,
        b"rustc_workspace_wrapper",
        rustc_workspace_wrapper,
        rustc_workspace_wrapper_unresolved,
    );
    out
}

fn encode_digest_slot(out: &mut Vec<u8>, name: &[u8], value: Option<&str>, unresolved: bool) {
    out.extend_from_slice(name);
    out.push(0);
    if unresolved {
        out.extend_from_slice(b"unresolved");
        out.push(0);
        return;
    }
    match value {
        None => {
            out.extend_from_slice(b"absent");
            out.push(0);
        }
        Some(text) => {
            out.extend_from_slice(b"present");
            out.push(0);
            out.extend_from_slice(text.len().to_string().as_bytes());
            out.push(0);
            out.extend_from_slice(text.as_bytes());
            out.push(0);
        }
    }
}

fn durable_of(slot: &WrapperSlot) -> Option<String> {
    match slot {
        WrapperSlot::Present { durable, .. } => Some(durable.clone()),
        WrapperSlot::Absent | WrapperSlot::Unresolved => None,
    }
}

fn local_of(slot: &WrapperSlot) -> Option<String> {
    match slot {
        WrapperSlot::Present { local, .. } => Some(local.clone()),
        WrapperSlot::Absent | WrapperSlot::Unresolved => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;

    #[derive(Default)]
    struct OverlayFs {
        unreadable: BTreeSet<PathBuf>,
    }

    impl OverlayFs {
        fn unreadable(path: impl Into<PathBuf>) -> Self {
            let mut fs = Self::default();
            fs.unreadable.insert(path.into());
            fs
        }
    }

    impl ConfigFs for OverlayFs {
        fn is_file(&self, path: &Path) -> bool {
            self.unreadable.contains(path) || path.is_file()
        }

        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            if self.unreadable.contains(path) {
                return Err(io::Error::new(
                    ErrorKind::PermissionDenied,
                    "config file is unreadable",
                ));
            }
            std::fs::read_to_string(path)
        }
    }

    fn write_config(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create config parent");
        }
        fs::write(path, body).expect("write config");
    }

    fn isolated_env(cargo_home: &Path) -> EnvSnapshot {
        let mut env = EnvSnapshot::default();
        env.insert(CARGO_HOME_ENV, cargo_home.to_string_lossy());
        env
    }

    fn resolve_tree(
        workspace: &Path,
        ceiling: &Path,
        cargo_home: &Path,
    ) -> ResolvedCompilerWrappers {
        resolve_compiler_wrappers_in(workspace, &isolated_env(cargo_home), Some(ceiling), &RealFs)
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let cargo_home = root.path().join("cargo-home");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&cargo_home).expect("cargo home");
        fs::create_dir_all(workspace.join(".cargo")).expect("workspace cargo dir");
        (root, cargo_home, workspace.clone(), workspace)
    }

    #[test]
    fn quoted_dotted_build_key_at_the_root_table_is_ignored() {
        let (root, home, ws, _) = fixture();
        write_config(
            &ws.join(".cargo/config.toml"),
            "\"build.rustc-wrapper\" = \"quoted-dotted\"\n",
        );
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert!(resolved.rustc_wrapper().is_none());
        assert!(resolved.is_complete());
    }

    #[test]
    fn unquoted_dotted_build_key_nests_into_the_build_table() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "build.rustc-wrapper = \"nested-dotted\"\n");
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert_eq!(resolved.rustc_wrapper(), Some("nested-dotted"));
        assert!(resolved.is_complete());
    }

    #[test]
    fn two_workspace_wrappers_produce_different_subject_digests() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = \"sccache\"\n");
        let first = resolve_tree(&ws, root.path(), &home);
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = \"cachepot\"\n");
        let second = resolve_tree(&ws, root.path(), &home);
        assert_ne!(first.subject_digest(), second.subject_digest());
        assert!(first.is_complete());
        assert!(second.is_complete());
        assert_eq!(first.rustc_wrapper(), Some("sccache"));
        assert_eq!(second.rustc_wrapper(), Some("cachepot"));
        assert!(first.limitations().is_empty());
    }

    #[test]
    fn cargo_home_wrapper_moves_the_subject_digest() {
        let (root, home, ws, _) = fixture();
        let without = resolve_tree(&ws, root.path(), &home);
        write_config(&home.join("config.toml"), "[build]\nrustc-wrapper = \"sccache\"\n");
        let with = resolve_tree(&ws, root.path(), &home);
        assert_ne!(without.subject_digest(), with.subject_digest());
        assert_eq!(with.rustc_wrapper(), Some("sccache"));
        assert!(with.is_complete());
    }

    #[test]
    fn environment_wins_when_it_disagrees_with_config() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = \"from-config\"\n");
        let mut env = isolated_env(&home);
        env.insert(WRAPPER_ENV, "from-env");
        let resolved = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        assert_eq!(resolved.rustc_wrapper(), Some("from-env"));
        assert!(resolved.is_complete());
    }

    #[test]
    fn empty_env_clears_a_config_wrapper() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = \"from-config\"\n");
        let configured = resolve_tree(&ws, root.path(), &home);
        let mut env = isolated_env(&home);
        env.insert(WRAPPER_ENV, "");
        let cleared = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        write_config(&ws.join(".cargo/config.toml"), "[build]\njobs = 1\n");
        let none = resolve_tree(&ws, root.path(), &home);
        assert_ne!(configured.subject_digest(), cleared.subject_digest());
        assert_eq!(cleared.subject_digest(), none.subject_digest());
        assert!(cleared.rustc_wrapper().is_none());
        assert!(cleared.is_complete());
    }

    #[test]
    fn dedicated_env_wins_over_cargo_mapped_env_even_when_empty() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = \"from-config\"\n");
        let mut env = isolated_env(&home);
        env.insert(WRAPPER_ENV, "");
        env.insert(WRAPPER_CARGO_ENV, "from-cargo-env");
        let resolved = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        assert!(resolved.rustc_wrapper().is_none());
        assert!(resolved.is_complete());
    }

    #[test]
    fn rustc_wrapper_env_wins_over_cargo_build_env() {
        let (root, home, ws, _) = fixture();
        let mut env = isolated_env(&home);
        env.insert(WRAPPER_ENV, "from-rustc");
        env.insert(WRAPPER_CARGO_ENV, "from-cargo");
        let resolved = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        assert_eq!(resolved.rustc_wrapper(), Some("from-rustc"));
    }

    #[test]
    fn cargo_mapped_env_is_used_when_the_dedicated_var_is_unset() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = \"from-config\"\n");
        let mut env = isolated_env(&home);
        env.insert(WRAPPER_CARGO_ENV, "from-cargo-env");
        let resolved = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        assert_eq!(resolved.rustc_wrapper(), Some("from-cargo-env"));
    }

    #[test]
    fn parent_directory_config_applies_when_workspace_does_not_set_the_key() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("cargo-home");
        let parent = root.path().join("parent");
        let ws = parent.join("workspace");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(ws.join(".cargo")).unwrap();
        write_config(
            &parent.join(".cargo/config.toml"),
            "[build]\nrustc-wrapper = \"from-parent\"\n",
        );
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert_eq!(resolved.rustc_wrapper(), Some("from-parent"));
        assert!(resolved.is_complete());
    }

    #[test]
    fn workspace_config_overrides_parent_and_cargo_home() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("cargo-home");
        let parent = root.path().join("parent");
        let ws = parent.join("workspace");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(ws.join(".cargo")).unwrap();
        write_config(&home.join("config.toml"), "[build]\nrustc-wrapper = \"from-home\"\n");
        write_config(
            &parent.join(".cargo/config.toml"),
            "[build]\nrustc-wrapper = \"from-parent\"\n",
        );
        write_config(
            &ws.join(".cargo/config.toml"),
            "[build]\nrustc-wrapper = \"from-workspace\"\n",
        );
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert_eq!(resolved.rustc_wrapper(), Some("from-workspace"));
    }

    #[test]
    fn workspace_and_global_wrapper_keys_are_independent() {
        let (root, home, ws, _) = fixture();
        write_config(
            &ws.join(".cargo/config.toml"),
            "[build]\nrustc-wrapper = \"outer\"\nrustc-workspace-wrapper = \"inner\"\n",
        );
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert_eq!(resolved.rustc_wrapper(), Some("outer"));
        assert_eq!(resolved.rustc_workspace_wrapper(), Some("inner"));
        assert_eq!(
            resolved.durable_toolchain_suffix().as_deref(),
            Some("RUSTC_WRAPPER: outer | RUSTC_WORKSPACE_WRAPPER: inner")
        );
    }

    #[test]
    fn workspace_wrapper_env_over_config() {
        let (root, home, ws, _) = fixture();
        write_config(
            &ws.join(".cargo/config.toml"),
            "[build]\nrustc-workspace-wrapper = \"from-config\"\n",
        );
        let mut env = isolated_env(&home);
        env.insert(WORKSPACE_WRAPPER_ENV, "from-env");
        let resolved = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        assert_eq!(resolved.rustc_workspace_wrapper(), Some("from-env"));
    }

    #[test]
    fn legacy_config_name_wins_over_config_toml() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config"), "[build]\nrustc-wrapper = \"legacy\"\n");
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = \"modern\"\n");
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert_eq!(resolved.rustc_wrapper(), Some("legacy"));
    }

    #[test]
    fn cargo_home_legacy_config_name_is_read() {
        let (root, home, ws, _) = fixture();
        write_config(&home.join("config"), "[build]\nrustc-wrapper = \"home-legacy\"\n");
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert_eq!(resolved.rustc_wrapper(), Some("home-legacy"));
    }

    #[test]
    fn config_local_toml_is_not_auto_read() {
        let (root, home, ws, _) = fixture();
        write_config(
            &ws.join(".cargo/config.local.toml"),
            "[build]\nrustc-wrapper = \"local-only\"\n",
        );
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert!(resolved.rustc_wrapper().is_none());
        assert!(resolved.is_complete(), "stable Cargo ignores config.local.toml");
        let none = resolve_tree(&ws, root.path(), &home);
        assert_eq!(resolved.subject_digest(), none.subject_digest());
    }

    #[test]
    fn include_key_does_not_apply_the_included_wrapper_and_is_not_silent() {
        let (root, home, ws, _) = fixture();
        write_config(
            &ws.join(".cargo/config.local.toml"),
            "[build]\nrustc-wrapper = \"from-include\"\n",
        );
        write_config(&ws.join(".cargo/config.toml"), "include = [\"config.local.toml\"]\n");
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert!(resolved.rustc_wrapper().is_none());
        assert!(!resolved.is_complete());
        assert_eq!(resolved.limitations(), &[CARGO_CONFIG_WRAPPER_NOT_RESOLVED.to_string()]);
        write_config(&ws.join(".cargo/config.toml"), "[build]\njobs = 1\n");
        let none = resolve_tree(&ws, root.path(), &home);
        assert_ne!(
            resolved.subject_digest(),
            none.subject_digest(),
            "unresolved include must not hash as no wrapper"
        );
    }

    #[test]
    fn malformed_toml_is_a_named_limitation_not_an_empty_wrapper() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "this is not toml = {{\n");
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert!(resolved.rustc_wrapper().is_none());
        assert!(!resolved.is_complete());
        assert!(
            resolved.limitations().iter().any(|item| item == CARGO_CONFIG_WRAPPER_NOT_RESOLVED)
        );
        fs::remove_file(ws.join(".cargo/config.toml")).unwrap();
        let none = resolve_tree(&ws, root.path(), &home);
        assert_ne!(resolved.subject_digest(), none.subject_digest());
    }

    #[test]
    fn unreadable_config_is_a_named_limitation_not_an_empty_wrapper() {
        let (root, home, ws, _) = fixture();
        let path = ws.join(".cargo/config.toml");
        write_config(&path, "[build]\nrustc-wrapper = \"hidden\"\n");
        let fs = OverlayFs::unreadable(&path);
        let resolved =
            resolve_compiler_wrappers_in(&ws, &isolated_env(&home), Some(root.path()), &fs);
        assert!(resolved.rustc_wrapper().is_none());
        assert!(!resolved.is_complete());
        let readable = resolve_tree(&ws, root.path(), &home);
        assert_ne!(resolved.subject_digest(), readable.subject_digest());
        fs::remove_file(&path).unwrap();
        let none = resolve_tree(&ws, root.path(), &home);
        assert_ne!(resolved.subject_digest(), none.subject_digest());
    }

    #[test]
    fn unknown_cargo_home_is_a_named_limitation_unless_env_resolves_both_keys() {
        let root = tempfile::tempdir().expect("tempdir");
        let ws = root.path().join("workspace");
        fs::create_dir_all(ws.join(".cargo")).unwrap();
        let env = EnvSnapshot::default();
        let unresolved = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        assert!(!unresolved.is_complete());

        let mut env = EnvSnapshot::default();
        env.insert(WRAPPER_ENV, "");
        env.insert(WORKSPACE_WRAPPER_ENV, "");
        let from_env = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        assert!(from_env.is_complete());
        assert_ne!(unresolved.subject_digest(), from_env.subject_digest());
    }

    #[test]
    fn absolute_wrapper_paths_do_not_enter_the_durable_projection() {
        let (root, home, ws, _) = fixture();
        let abs = ws.join("tools").join("sccache");
        write_config(
            &ws.join(".cargo/config.toml"),
            &format!("[build]\nrustc-wrapper = \"{}\"\n", abs.display()),
        );
        let resolved = resolve_tree(&ws, root.path(), &home);
        let durable = resolved.rustc_wrapper().expect("wrapper");
        assert!(!durable.starts_with('/'), "{durable}");
        assert!(!durable.contains(ws.to_string_lossy().as_ref()), "{durable}");
        assert_eq!(durable, "tools/sccache");
        let local = resolved.rustc_wrapper_local().expect("local");
        assert!(local.contains("sccache"));
    }

    #[test]
    fn unresolved_does_not_hash_as_absent() {
        let (root, home, ws, _) = fixture();
        let complete = resolve_tree(&ws, root.path(), &home);
        write_config(&ws.join(".cargo/config.toml"), "include = [\"missing.toml\"]\n");
        let unresolved = resolve_tree(&ws, root.path(), &home);
        assert_ne!(complete.subject_digest(), unresolved.subject_digest());
        assert!(complete.is_complete());
        assert!(!unresolved.is_complete());
    }

    #[test]
    fn non_string_wrapper_value_is_unresolvable() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = 1\n");
        let resolved = resolve_tree(&ws, root.path(), &home);
        assert!(!resolved.is_complete());
        assert!(resolved.rustc_wrapper().is_none());
    }

    #[test]
    fn parent_relative_wrapper_resolves_from_the_parent_origin_not_the_workspace() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("cargo-home");
        let parent = root.path().join("parent");
        let ws = parent.join("workspace");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(ws.join(".cargo")).unwrap();
        write_config(
            &parent.join(".cargo/config.toml"),
            "[build]\nrustc-wrapper = \"tools/rel-parent\"\n",
        );
        write_config(
            &ws.join(".cargo/config.toml"),
            "[build]\nrustc-wrapper = \"tools/rel-parent\"\n",
        );
        let from_parent = {
            fs::remove_file(ws.join(".cargo/config.toml")).unwrap();
            resolve_tree(&ws, root.path(), &home)
        };
        write_config(
            &ws.join(".cargo/config.toml"),
            "[build]\nrustc-wrapper = \"tools/rel-parent\"\n",
        );
        let from_workspace = resolve_tree(&ws, root.path(), &home);
        assert_ne!(from_parent.subject_digest(), from_workspace.subject_digest());
        assert_eq!(from_workspace.rustc_wrapper(), Some("tools/rel-parent"));
        let parent_durable = from_parent.rustc_wrapper().expect("parent wrapper");
        assert_ne!(parent_durable, "tools/rel-parent");
        assert!(parent_durable.starts_with("rel-parent@sha256:"), "{parent_durable}");
        let parent_local = from_parent.rustc_wrapper_local().expect("parent local");
        assert!(parent_local.ends_with("parent/tools/rel-parent"), "{parent_local}");
        assert!(from_parent.is_complete());
        assert!(from_workspace.is_complete());
    }

    #[test]
    fn cargo_home_relative_wrapper_resolves_from_parent_of_cargo_home() {
        let (root, home, ws, _) = fixture();
        write_config(
            &home.join("config.toml"),
            "[build]\nrustc-wrapper = \"home-tools/rel-home\"\n",
        );
        let resolved = resolve_tree(&ws, root.path(), &home);
        let durable = resolved.rustc_wrapper().expect("wrapper");
        assert!(durable.starts_with("rel-home@sha256:"), "{durable}");
        let local = resolved.rustc_wrapper_local().expect("local");
        let expected = home.parent().expect("home parent").join("home-tools").join("rel-home");
        assert_eq!(local, expected.to_string_lossy().replace('\\', "/"));
        assert!(resolved.is_complete());
    }

    #[test]
    fn env_relative_wrapper_resolves_from_the_workspace_root() {
        let (root, home, ws, _) = fixture();
        let mut env = isolated_env(&home);
        env.insert(WRAPPER_ENV, "tools/env-rel");
        env.insert(WORKSPACE_WRAPPER_ENV, "");
        let resolved = resolve_compiler_wrappers_in(&ws, &env, Some(root.path()), &RealFs);
        assert_eq!(resolved.rustc_wrapper(), Some("tools/env-rel"));
        let local = resolved.rustc_wrapper_local().expect("local");
        assert_eq!(local, ws.join("tools/env-rel").to_string_lossy().replace('\\', "/"));
        assert!(resolved.is_complete());
    }

    #[test]
    fn distinct_absolute_wrappers_with_the_same_basename_have_distinct_digests() {
        let (root, home, ws, _) = fixture();
        let first_path = root.path().join("usr").join("bin").join("sccache");
        let second_path = root.path().join("opt").join("sccache");
        write_config(
            &ws.join(".cargo/config.toml"),
            &format!("[build]\nrustc-wrapper = \"{}\"\n", first_path.display()),
        );
        let first = resolve_tree(&ws, root.path(), &home);
        write_config(
            &ws.join(".cargo/config.toml"),
            &format!("[build]\nrustc-wrapper = \"{}\"\n", second_path.display()),
        );
        let second = resolve_tree(&ws, root.path(), &home);
        let first_durable = first.rustc_wrapper().expect("first");
        let second_durable = second.rustc_wrapper().expect("second");
        assert!(first_durable.starts_with("sccache@sha256:"), "{first_durable}");
        assert!(second_durable.starts_with("sccache@sha256:"), "{second_durable}");
        assert_ne!(first_durable, second_durable);
        assert_ne!(first.subject_digest(), second.subject_digest());
        assert!(!first_durable.contains(root.path().to_string_lossy().as_ref()));
        assert!(!second_durable.contains(root.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn whitespace_only_env_is_a_present_wrapper_and_empty_string_still_clears() {
        let (root, home, ws, _) = fixture();
        write_config(&ws.join(".cargo/config.toml"), "[build]\nrustc-wrapper = \"from-config\"\n");
        let mut spaces = isolated_env(&home);
        spaces.insert(WRAPPER_ENV, "   ");
        spaces.insert(WORKSPACE_WRAPPER_ENV, "");
        let spaced = resolve_compiler_wrappers_in(&ws, &spaces, Some(root.path()), &RealFs);
        assert_eq!(spaced.rustc_wrapper(), Some("   "));
        assert!(spaced.is_complete());

        let mut empty = isolated_env(&home);
        empty.insert(WRAPPER_ENV, "");
        empty.insert(WORKSPACE_WRAPPER_ENV, "");
        let cleared = resolve_compiler_wrappers_in(&ws, &empty, Some(root.path()), &RealFs);
        assert!(cleared.rustc_wrapper().is_none());
        assert_ne!(spaced.subject_digest(), cleared.subject_digest());
        assert!(cleared.is_complete());
    }

    #[test]
    fn newline_in_a_wrapper_value_does_not_collide_with_another_slot_assignment() {
        let (root, home, ws, _) = fixture();
        let mut embedded = isolated_env(&home);
        embedded.insert(WRAPPER_ENV, "a\nb");
        embedded.insert(WORKSPACE_WRAPPER_ENV, "");
        let with_newline = resolve_compiler_wrappers_in(&ws, &embedded, Some(root.path()), &RealFs);

        let mut split = isolated_env(&home);
        split.insert(WRAPPER_ENV, "a");
        split.insert(WORKSPACE_WRAPPER_ENV, "b");
        let split_slots = resolve_compiler_wrappers_in(&ws, &split, Some(root.path()), &RealFs);

        let mut injected = isolated_env(&home);
        injected.insert(WRAPPER_ENV, "foo\nrustc_workspace_wrapper=present:bar");
        injected.insert(WORKSPACE_WRAPPER_ENV, "");
        let injected_value =
            resolve_compiler_wrappers_in(&ws, &injected, Some(root.path()), &RealFs);

        let mut honest = isolated_env(&home);
        honest.insert(WRAPPER_ENV, "foo");
        honest.insert(WORKSPACE_WRAPPER_ENV, "bar");
        let honest_split = resolve_compiler_wrappers_in(&ws, &honest, Some(root.path()), &RealFs);

        assert_ne!(with_newline.subject_digest(), split_slots.subject_digest());
        assert_ne!(injected_value.subject_digest(), honest_split.subject_digest());
        assert_eq!(with_newline.rustc_wrapper(), Some("a\nb"));
        assert!(with_newline.rustc_workspace_wrapper().is_none());
    }
}
