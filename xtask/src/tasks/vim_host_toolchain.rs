use color_eyre::eyre::Result;
#[cfg(test)]
use color_eyre::eyre::eyre;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use sha1::{Digest as _, Sha1};
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const TOOLCHAIN_SCHEMA_VERSION: &str = "vim_vim_lsp_host_toolchain.v1";
pub(crate) const INSTRUMENT_VERSION: u32 = 1;
const HOST_ROLE: &str = "vim_vim_lsp_host";
const SUBJECT_SCHEMA_VERSION: &str = "vim_lsp_subject.v1";
const SUBJECT_AUTHORITY_PATH: &str = ".ci/editor-clients/vim-vim-lsp-subject.v1.json";
const SUBJECT_TREE_DIGEST_ALGORITHM: &str = "git-tree-sha1";
const LOAD_MODE: &str = "caller_pinned_git_checkout_via_runtimepath_prepend";
const VIMRC_ISOLATION_POLICY: &str =
    "vim_-Nu_NONE_no_user_vimrc_no_user_runtimepath_no_ambient_plugins";
const MANIFEST_FILE_NAME: &str = "vim_vim_lsp_host_toolchain.v1.json";
const CLAIM_BOUNDARY: &str = "test-instrument identity substrate only (#11372); binds exact Vim executable and version-text bytes plus the #11369-governed vim-lsp subject into one reproducible host-toolchain role; claims no editor behavior, no support tier, no actual-editor receipt, and no product or client compatibility";
const EXECUTION_ENVIRONMENT: &str = "native_process";

#[derive(Debug)]
pub(crate) struct ProvisionArgs {
    pub output: PathBuf,
    pub vim: Option<PathBuf>,
    pub vim_lsp_source: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct VerifyArgs {
    pub manifest: PathBuf,
    pub vim: Option<PathBuf>,
    pub vim_lsp_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct InstrumentFailure {
    class: &'static str,
    detail: String,
}

impl InstrumentFailure {
    fn new(class: &'static str, detail: impl Into<String>) -> Self {
        Self { class, detail: detail.into() }
    }

    #[cfg(test)]
    pub(crate) fn class(&self) -> &'static str {
        self.class
    }
}

impl fmt::Display for InstrumentFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "instrument_failed({}): {}", self.class, self.detail)
    }
}

impl std::error::Error for InstrumentFailure {}

type InstrumentResult<T> = std::result::Result<T, InstrumentFailure>;

const CLASS_AUTHORITY_UNREADABLE: &str = "authority_unreadable";
const CLASS_ACQUISITION_UNAVAILABLE: &str = "acquisition_unavailable";
const CLASS_IDENTITY_MISMATCH: &str = "identity_mismatch";
const CLASS_SUBJECT_UNRESOLVED: &str = "subject_unresolved";

fn mismatch(detail: impl Into<String>) -> InstrumentFailure {
    InstrumentFailure::new(CLASS_IDENTITY_MISMATCH, detail)
}

#[derive(Debug, Deserialize)]
struct SubjectAuthorityDocument {
    schema_version: String,
    upstream: SubjectUpstreamDocument,
    #[serde(rename = "plugin_load_mode")]
    load_mode: SubjectLoadModeDocument,
    #[serde(rename = "expected_content_identity")]
    content_identity: SubjectContentIdentityDocument,
}

#[derive(Debug, Deserialize)]
struct SubjectUpstreamDocument {
    repository: String,
    selected_commit: String,
    tree_digest: SubjectTreeDigestDocument,
}

#[derive(Debug, Deserialize)]
struct SubjectTreeDigestDocument {
    algorithm: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct SubjectLoadModeDocument {
    mode: String,
}

#[derive(Debug, Deserialize)]
struct SubjectContentIdentityDocument {
    entry_files: Vec<SubjectEntryFileDocument>,
}

#[derive(Debug, Deserialize)]
struct SubjectEntryFileDocument {
    path: String,
    git_blob_sha1: String,
}

#[derive(Debug)]
struct SubjectAuthority {
    bytes_sha256: String,
    repository: String,
    selected_commit: String,
    tree_digest: String,
    load_mode: String,
    entry_files: Vec<(String, String)>,
}

impl SubjectAuthority {
    fn load(root: &Path) -> InstrumentResult<Self> {
        let path = root.join(SUBJECT_AUTHORITY_PATH);
        let bytes = fs::read(&path).map_err(|error| {
            InstrumentFailure::new(
                CLASS_AUTHORITY_UNREADABLE,
                format!("governed subject authority unreadable at {}: {error}", path.display()),
            )
        })?;
        let document: SubjectAuthorityDocument =
            serde_json::from_slice(&bytes).map_err(|error| {
                InstrumentFailure::new(
                    CLASS_AUTHORITY_UNREADABLE,
                    format!("governed subject authority malformed at {}: {error}", path.display()),
                )
            })?;
        if document.schema_version != SUBJECT_SCHEMA_VERSION {
            return Err(InstrumentFailure::new(
                CLASS_AUTHORITY_UNREADABLE,
                format!(
                    "subject authority schema drift at {}: expected {SUBJECT_SCHEMA_VERSION}, got {}",
                    path.display(),
                    document.schema_version
                ),
            ));
        }
        if document.upstream.tree_digest.algorithm != SUBJECT_TREE_DIGEST_ALGORITHM {
            return Err(InstrumentFailure::new(
                CLASS_AUTHORITY_UNREADABLE,
                format!(
                    "subject authority tree digest algorithm must be {SUBJECT_TREE_DIGEST_ALGORITHM}, got {}",
                    document.upstream.tree_digest.algorithm
                ),
            ));
        }
        if !is_lower_hex40(&document.upstream.selected_commit)
            || !is_lower_hex40(&document.upstream.tree_digest.value)
        {
            return Err(InstrumentFailure::new(
                CLASS_AUTHORITY_UNREADABLE,
                "subject authority commit and tree digest must be 40 lowercase hex digits",
            ));
        }
        if document.content_identity.entry_files.is_empty() {
            return Err(InstrumentFailure::new(
                CLASS_AUTHORITY_UNREADABLE,
                "subject authority entry_files must not be empty",
            ));
        }
        for entry in &document.content_identity.entry_files {
            if !is_lower_hex40(&entry.git_blob_sha1) {
                return Err(InstrumentFailure::new(
                    CLASS_AUTHORITY_UNREADABLE,
                    format!("entry file {} blob digest is not 40 lowercase hex", entry.path),
                ));
            }
        }
        Ok(Self {
            bytes_sha256: sha256_hex(&bytes),
            repository: document.upstream.repository.clone(),
            selected_commit: document.upstream.selected_commit.clone(),
            tree_digest: document.upstream.tree_digest.value.clone(),
            load_mode: document.load_mode.mode.clone(),
            entry_files: document
                .content_identity
                .entry_files
                .into_iter()
                .map(|entry| (entry.path, entry.git_blob_sha1))
                .collect(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct HostToolchainManifest {
    schema_version: String,
    claim_boundary: String,
    host_role: String,
    instrument_version: u32,
    platform: PlatformIdentity,
    cache_key: String,
    vim: VimSubjectIdentity,
    vim_lsp: VimLspSubjectBinding,
    isolation: IsolationPolicy,
    provision_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PlatformIdentity {
    os: String,
    arch: String,
    execution_environment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct VimSubjectIdentity {
    acquisition_mode: String,
    executable_sha256: String,
    version_text_sha256: String,
    version_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct VimLspSubjectBinding {
    subject_authority_path: String,
    subject_authority_sha256: String,
    selected_commit: String,
    tree_digest: String,
    entry_files: Vec<EntryFileDigest>,
    load_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct EntryFileDigest {
    path: String,
    git_blob_sha1: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct IsolationPolicy {
    vimrc_policy: String,
    user_plugins_excluded: bool,
}

trait VersionOutputProvider {
    fn version_output(&self, executable: &Path) -> InstrumentResult<Vec<u8>>;
}

struct ProductionVimVersionOutput;

impl VersionOutputProvider for ProductionVimVersionOutput {
    fn version_output(&self, executable: &Path) -> InstrumentResult<Vec<u8>> {
        let output = Command::new(executable).arg("--version").output().map_err(|error| {
            InstrumentFailure::new(
                CLASS_SUBJECT_UNRESOLVED,
                format!("failed to execute {} --version: {error}", executable.display()),
            )
        })?;
        if !output.status.success() || output.stdout.is_empty() {
            return Err(InstrumentFailure::new(
                CLASS_SUBJECT_UNRESOLVED,
                format!(
                    "{} --version produced no usable identity (status {:?})",
                    executable.display(),
                    output.status.code()
                ),
            ));
        }
        Ok(output.stdout)
    }
}

#[cfg(test)]
struct FixedVimVersionOutput<'a>(&'a [u8]);

#[cfg(test)]
impl VersionOutputProvider for FixedVimVersionOutput<'_> {
    fn version_output(&self, _executable: &Path) -> InstrumentResult<Vec<u8>> {
        Ok(self.0.to_vec())
    }
}

pub(crate) fn run_provision(args: ProvisionArgs) -> std::result::Result<(), InstrumentFailure> {
    let root = project_root().map_err(|error| {
        InstrumentFailure::new(
            CLASS_AUTHORITY_UNREADABLE,
            format!("project root unresolved: {error}"),
        )
    })?;
    let outcome = provision(&root, &args)?;
    println!("{}", outcome.manifest_path.display());
    for role in outcome.ephemeral_roles {
        println!("{role}");
    }
    Ok(())
}

pub(crate) fn run_verify(args: VerifyArgs) -> std::result::Result<(), InstrumentFailure> {
    let root = project_root().map_err(|error| {
        InstrumentFailure::new(
            CLASS_AUTHORITY_UNREADABLE,
            format!("project root unresolved: {error}"),
        )
    })?;
    let verdict = verify(&root, &args)?;
    println!("verified {}", verdict.manifest_path.display());
    println!("cache_key {}", verdict.cache_key);
    for role in verdict.ephemeral_roles {
        println!("{role}");
    }
    Ok(())
}

#[derive(Debug)]
struct ProvisionOutcome {
    manifest_path: PathBuf,
    ephemeral_roles: Vec<String>,
}

fn provision(root: &Path, args: &ProvisionArgs) -> InstrumentResult<ProvisionOutcome> {
    provision_with(root, args, &ProductionVimVersionOutput)
}

fn provision_with(
    root: &Path,
    args: &ProvisionArgs,
    provider: &dyn VersionOutputProvider,
) -> InstrumentResult<ProvisionOutcome> {
    let authority = SubjectAuthority::load(root)?;
    if authority.load_mode != LOAD_MODE {
        return Err(mismatch(format!(
            "governed load mode drifted from {LOAD_MODE}: {}",
            authority.load_mode
        )));
    }
    fs::create_dir_all(&args.output).map_err(|error| {
        InstrumentFailure::new(
            CLASS_ACQUISITION_UNAVAILABLE,
            format!("output directory unusable at {}: {error}", args.output.display()),
        )
    })?;
    let vim_lsp_dir = args.output.join("vim-lsp");
    let mut reused_cache = false;
    if vim_lsp_dir.is_dir() {
        reused_cache = verify_checkout_against_authority(&vim_lsp_dir, &authority).is_ok();
        if !reused_cache && fs::remove_dir_all(&vim_lsp_dir).is_err() {
            return Err(InstrumentFailure::new(
                CLASS_ACQUISITION_UNAVAILABLE,
                format!(
                    "warm cache at {} failed revalidation and could not be rebuilt",
                    vim_lsp_dir.display()
                ),
            ));
        }
    }
    if !reused_cache {
        acquire_vim_lsp(&vim_lsp_dir, &authority, args.vim_lsp_source.as_deref())?;
        verify_checkout_against_authority(&vim_lsp_dir, &authority)?;
    }
    let acquisition_mode;
    let vim_executable = match args.vim.as_deref() {
        Some(explicit) => {
            acquisition_mode = "explicit_subject".to_string();
            resolve_explicit_vim(explicit)?
        }
        None => {
            acquisition_mode = "path_lookup".to_string();
            find_on_path("vim", &path_env()).ok_or_else(|| {
                InstrumentFailure::new(
                    CLASS_SUBJECT_UNRESOLVED,
                    "no Vim executable found on PATH; supply an explicit subject instead",
                )
            })?
        }
    };
    let identity = capture_vim_identity(&vim_executable, provider)?;
    let manifest = build_manifest(&authority, acquisition_mode, identity);
    let serialized = serialize_manifest(&manifest, &[root, &args.output, &vim_lsp_dir])?;
    let manifest_path = args.output.join(MANIFEST_FILE_NAME);
    fs::write(&manifest_path, &serialized).map_err(|error| {
        InstrumentFailure::new(
            CLASS_ACQUISITION_UNAVAILABLE,
            format!("manifest unwritable at {}: {error}", manifest_path.display()),
        )
    })?;
    Ok(ProvisionOutcome {
        manifest_path,
        ephemeral_roles: vec![
            format!("VIM={}", vim_executable.display()),
            format!("VIM_LSP_DIR={}", canonical_display(&vim_lsp_dir)),
            "runtimepath_role=vim-lsp prepend of VIM_LSP_DIR with explicit source of plugin/lsp.vim"
                .to_string(),
            "consumer_handoff=launch/execution roles belong to #10944".to_string(),
        ],
    })
}

#[derive(Debug)]
struct VerifyOutcome {
    manifest_path: PathBuf,
    cache_key: String,
    ephemeral_roles: Vec<String>,
}

fn verify(root: &Path, args: &VerifyArgs) -> InstrumentResult<VerifyOutcome> {
    verify_with_provider(root, args, &ProductionVimVersionOutput)
}

fn verify_with_provider(
    root: &Path,
    args: &VerifyArgs,
    provider: &dyn VersionOutputProvider,
) -> InstrumentResult<VerifyOutcome> {
    let manifest_path = args.manifest.clone();
    let raw_bytes = fs::read(&manifest_path).map_err(|error| {
        InstrumentFailure::new(
            CLASS_AUTHORITY_UNREADABLE,
            format!("toolchain manifest unreadable at {}: {error}", manifest_path.display()),
        )
    })?;
    let document: HostToolchainManifest = serde_json::from_slice(&raw_bytes).map_err(|error| {
        mismatch(format!("toolchain manifest malformed at {}: {error}", manifest_path.display()))
    })?;
    if document.schema_version != TOOLCHAIN_SCHEMA_VERSION {
        return Err(mismatch(format!(
            "schema drift: expected {TOOLCHAIN_SCHEMA_VERSION}, got {}",
            document.schema_version
        )));
    }
    if document.host_role != HOST_ROLE {
        return Err(mismatch(format!(
            "host role drift: expected {HOST_ROLE}, got {}",
            document.host_role
        )));
    }
    if document.instrument_version != INSTRUMENT_VERSION {
        return Err(mismatch(format!(
            "instrument version drift: expected {INSTRUMENT_VERSION}, got {}",
            document.instrument_version
        )));
    }
    if document.provision_status != "verified" {
        return Err(mismatch(format!(
            "provision status is not verified: {}",
            document.provision_status
        )));
    }
    let authority = SubjectAuthority::load(root)?;
    if document.vim_lsp.subject_authority_path != SUBJECT_AUTHORITY_PATH {
        return Err(mismatch(format!(
            "manifest binds a foreign subject authority: {}",
            document.vim_lsp.subject_authority_path
        )));
    }
    if document.vim_lsp.subject_authority_sha256 != authority.bytes_sha256 {
        return Err(mismatch(
            "governed subject authority changed since provisioning; re-provision against the reviewed pin",
        ));
    }
    if document.vim_lsp.selected_commit != authority.selected_commit
        || document.vim_lsp.tree_digest != authority.tree_digest
        || document.vim_lsp.load_mode != authority.load_mode
    {
        return Err(mismatch("manifest pin fields diverge from the governed subject authority"));
    }
    if document.vim_lsp.entry_files.len() != authority.entry_files.len() {
        return Err(mismatch("manifest entry-file inventory diverges from governed authority"));
    }
    for (recorded, expected) in
        document.vim_lsp.entry_files.iter().zip(authority.entry_files.iter())
    {
        if recorded.path != expected.0 || recorded.git_blob_sha1 != expected.1 {
            return Err(mismatch(format!(
                "entry-file binding diverges from governed authority: {}",
                recorded.path
            )));
        }
    }
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new(""));
    let vim_lsp_dir = match args.vim_lsp_dir.as_deref() {
        Some(dir) => dir.to_path_buf(),
        None => manifest_dir.join("vim-lsp"),
    };
    verify_checkout_against_authority(&vim_lsp_dir, &authority)?;
    let acquisition_mode;
    let vim_executable = match args.vim.as_deref() {
        Some(explicit) => {
            acquisition_mode = "explicit_subject".to_string();
            resolve_explicit_vim(explicit)?
        }
        None => {
            acquisition_mode = "path_lookup".to_string();
            find_on_path("vim", &path_env()).ok_or_else(|| {
                InstrumentFailure::new(
                    CLASS_SUBJECT_UNRESOLVED,
                    "no Vim executable found on PATH; supply an explicit subject instead",
                )
            })?
        }
    };
    if document.vim.acquisition_mode != acquisition_mode {
        return Err(mismatch(format!(
            "vim acquisition mode changed between provision and verify: {} vs {acquisition_mode}",
            document.vim.acquisition_mode
        )));
    }
    let identity = capture_vim_identity(&vim_executable, provider)?;
    if document.vim.executable_sha256 != identity.executable_sha256 {
        return Err(mismatch("resolved Vim executable bytes differ from the provisioned subject"));
    }
    if document.vim.version_text_sha256 != identity.version_text_sha256 {
        return Err(mismatch("resolved Vim version text differs from the provisioned subject"));
    }
    if document.platform.os != std::env::consts::OS
        || document.platform.arch != std::env::consts::ARCH
    {
        return Err(mismatch(
            "provisioned platform identity differs from this execution environment",
        ));
    }
    let cache_key = compute_cache_key(&document);
    if document.cache_key != cache_key {
        return Err(mismatch("cache key does not reproduce from recorded identities"));
    }
    scan_for_machine_paths(
        &raw_bytes,
        &[
            root,
            manifest_dir,
            &vim_lsp_dir,
            vim_executable.parent().unwrap_or_else(|| Path::new("")),
        ],
    )?;
    Ok(VerifyOutcome {
        cache_key: document.cache_key,
        manifest_path,
        ephemeral_roles: vec![
            format!("VIM={}", vim_executable.display()),
            format!("VIM_LSP_DIR={}", canonical_display(&vim_lsp_dir)),
        ],
    })
}

#[derive(Debug)]
struct CapturedVimIdentity {
    executable_sha256: String,
    version_text_sha256: String,
    version_summary: String,
}

fn capture_vim_identity(
    executable: &Path,
    provider: &dyn VersionOutputProvider,
) -> InstrumentResult<CapturedVimIdentity> {
    let executable_bytes = fs::read(executable).map_err(|error| {
        InstrumentFailure::new(
            CLASS_SUBJECT_UNRESOLVED,
            format!("vim subject unreadable at {}: {error}", executable.display()),
        )
    })?;
    let version_output = provider.version_output(executable)?;
    let version_text = String::from_utf8_lossy(&version_output).to_string();
    let version_summary = version_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string();
    if version_summary.is_empty() {
        return Err(InstrumentFailure::new(
            CLASS_SUBJECT_UNRESOLVED,
            format!("vim subject produced empty --version text: {}", executable.display()),
        ));
    }
    Ok(CapturedVimIdentity {
        executable_sha256: sha256_hex(&executable_bytes),
        version_text_sha256: sha256_hex(version_output.as_slice()),
        version_summary,
    })
}

fn resolve_explicit_vim(explicit: &Path) -> InstrumentResult<PathBuf> {
    if !explicit.is_file() {
        return Err(InstrumentFailure::new(
            CLASS_SUBJECT_UNRESOLVED,
            format!("explicit vim subject is not a file: {}", explicit.display()),
        ));
    }
    Ok(explicit.to_path_buf())
}

fn build_manifest(
    authority: &SubjectAuthority,
    acquisition_mode: String,
    identity: CapturedVimIdentity,
) -> HostToolchainManifest {
    let manifest = HostToolchainManifest {
        schema_version: TOOLCHAIN_SCHEMA_VERSION.to_string(),
        claim_boundary: CLAIM_BOUNDARY.to_string(),
        host_role: HOST_ROLE.to_string(),
        instrument_version: INSTRUMENT_VERSION,
        platform: PlatformIdentity {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            execution_environment: EXECUTION_ENVIRONMENT.to_string(),
        },
        cache_key: String::new(),
        vim: VimSubjectIdentity {
            acquisition_mode,
            executable_sha256: identity.executable_sha256,
            version_text_sha256: identity.version_text_sha256,
            version_summary: identity.version_summary,
        },
        vim_lsp: VimLspSubjectBinding {
            subject_authority_path: SUBJECT_AUTHORITY_PATH.to_string(),
            subject_authority_sha256: authority.bytes_sha256.clone(),
            selected_commit: authority.selected_commit.clone(),
            tree_digest: authority.tree_digest.clone(),
            entry_files: authority
                .entry_files
                .iter()
                .map(|(path, blob)| EntryFileDigest {
                    path: path.clone(),
                    git_blob_sha1: blob.clone(),
                })
                .collect(),
            load_mode: LOAD_MODE.to_string(),
        },
        isolation: IsolationPolicy {
            vimrc_policy: VIMRC_ISOLATION_POLICY.to_string(),
            user_plugins_excluded: true,
        },
        provision_status: "verified".to_string(),
    };
    finalize_cache_key(manifest)
}

fn finalize_cache_key(mut manifest: HostToolchainManifest) -> HostToolchainManifest {
    let inputs = cache_key_inputs(&manifest);
    manifest.cache_key = sha256_hex(serde_json::to_vec(&inputs).unwrap_or_default().as_slice());
    manifest
}

fn compute_cache_key(manifest: &HostToolchainManifest) -> String {
    let inputs = cache_key_inputs(manifest);
    match serde_json::to_vec(&inputs) {
        Ok(bytes) => sha256_hex(&bytes),
        Err(_) => String::new(),
    }
}

fn cache_key_inputs(manifest: &HostToolchainManifest) -> serde_json::Value {
    serde_json::json!({
        "claim_boundary": manifest.claim_boundary,
        "execution_environment": manifest.platform.execution_environment,
        "host_role": manifest.host_role,
        "instrument_version": manifest.instrument_version,
        "isolation": {
            "user_plugins_excluded": manifest.isolation.user_plugins_excluded,
            "vimrc_policy": manifest.isolation.vimrc_policy,
        },
        "os": manifest.platform.os,
        "arch": manifest.platform.arch,
        "provision_status": manifest.provision_status,
        "schema_version": manifest.schema_version,
        "vim": {
            "acquisition_mode": manifest.vim.acquisition_mode,
            "executable_sha256": manifest.vim.executable_sha256,
            "version_summary": manifest.vim.version_summary,
            "version_text_sha256": manifest.vim.version_text_sha256,
        },
        "vim_lsp": {
            "entry_files": manifest
                .vim_lsp
                .entry_files
                .iter()
                .map(|entry| serde_json::json!({
                    "git_blob_sha1": entry.git_blob_sha1,
                    "path": entry.path,
                }))
                .collect::<Vec<_>>(),
            "load_mode": manifest.vim_lsp.load_mode,
            "selected_commit": manifest.vim_lsp.selected_commit,
            "subject_authority_path": manifest.vim_lsp.subject_authority_path,
            "subject_authority_sha256": manifest.vim_lsp.subject_authority_sha256,
            "tree_digest": manifest.vim_lsp.tree_digest,
        },
    })
}

fn acquire_vim_lsp(
    destination: &Path,
    authority: &SubjectAuthority,
    offline_source: Option<&Path>,
) -> InstrumentResult<()> {
    if let Some(source) = offline_source {
        run_git(
            None,
            &[
                "clone",
                "--quiet",
                "--no-hardlinks",
                &source.to_string_lossy(),
                &destination.to_string_lossy(),
            ],
        )
        .map_err(|error| {
            InstrumentFailure::new(
                CLASS_ACQUISITION_UNAVAILABLE,
                format!("offline acquisition from {} failed: {error}", source.display()),
            )
        })?;
        run_git(
            Some(destination),
            &["checkout", "--quiet", "--detach", &authority.selected_commit],
        )
        .map_err(|error| {
            InstrumentFailure::new(
                CLASS_ACQUISITION_UNAVAILABLE,
                format!(
                    "offline source lacks pinned commit {}: {error}",
                    authority.selected_commit
                ),
            )
        })?;
        return Ok(());
    }
    fs::create_dir_all(destination).map_err(|error| {
        InstrumentFailure::new(
            CLASS_ACQUISITION_UNAVAILABLE,
            format!("acquisition destination unusable at {}: {error}", destination.display()),
        )
    })?;
    run_git(Some(destination), &["init", "--quiet"])?;
    run_git(Some(destination), &["remote", "add", "origin", &authority.repository]).map_err(
        |error| {
            InstrumentFailure::new(
                CLASS_ACQUISITION_UNAVAILABLE,
                format!("network acquisition setup failed: {error}"),
            )
        },
    )?;
    run_git(Some(destination), &[
        "fetch",
        "--quiet",
        "--depth=1",
        "origin",
        &authority.selected_commit,
    ])
    .map_err(|error| {
        InstrumentFailure::new(
            CLASS_ACQUISITION_UNAVAILABLE,
            format!(
                "network fetch of pinned commit {} from {} failed: {error}; unavailable exact bytes are instrument failure, never skipped-green or product incompatibility",
                authority.selected_commit, authority.repository
            ),
        )
    })?;
    run_git(Some(destination), &["checkout", "--quiet", "--detach", "FETCH_HEAD"]).map_err(
        |error| {
            InstrumentFailure::new(
                CLASS_ACQUISITION_UNAVAILABLE,
                format!("pinned fetch checkout failed: {error}"),
            )
        },
    )?;
    Ok(())
}

fn verify_checkout_against_authority(
    checkout: &Path,
    authority: &SubjectAuthority,
) -> InstrumentResult<()> {
    let head = run_git(Some(checkout), &["rev-parse", "HEAD"]).map_err(|_| {
        mismatch(format!(
            "vim-lsp subject at {} is not a readable git checkout",
            checkout.display()
        ))
    })?;
    if head != authority.selected_commit {
        return Err(mismatch(format!(
            "checkout HEAD {} does not match governed pin {}; directory name proves nothing",
            head, authority.selected_commit
        )));
    }
    let tree = run_git(Some(checkout), &["rev-parse", "HEAD^{tree}"])
        .map_err(|error| mismatch(format!("checkout tree resolution failed: {error}")))?;
    if tree != authority.tree_digest {
        return Err(mismatch(format!(
            "checkout tree {} does not match governed tree digest {}",
            tree, authority.tree_digest
        )));
    }
    let status = run_git(Some(checkout), &["status", "--porcelain"])
        .map_err(|error| mismatch(format!("worktree cleanliness check failed: {error}")))?;
    if !status.trim().is_empty() {
        return Err(mismatch(format!(
            "vim-lsp subject worktree is dirty; only clean pinned bytes satisfy the role: {:?}",
            status.lines().take(5).collect::<Vec<_>>()
        )));
    }
    for (relative, expected_blob) in &authority.entry_files {
        let actual_blob = committed_entry_blob(checkout, relative)?;
        if &actual_blob != expected_blob {
            return Err(mismatch(format!(
                "entry file {} resolves to blob {} but governed subject requires {}",
                relative, actual_blob, expected_blob
            )));
        }
    }
    Ok(())
}

fn committed_entry_blob(checkout: &Path, relative: &str) -> InstrumentResult<String> {
    let listing =
        run_git(Some(checkout), &["ls-tree", "HEAD", "--", relative]).map_err(|error| {
            mismatch(format!("entry file {relative} missing from pinned tree: {error}"))
        })?;
    let first_line = listing
        .lines()
        .next()
        .ok_or_else(|| mismatch(format!("entry file {relative} absent from HEAD tree")))?;
    let fields: Vec<&str> = first_line.split_whitespace().collect();
    match fields.get(..3) {
        Some([_mode, kind, blob]) if *kind == "blob" && is_lower_hex40(blob) => {
            Ok((*blob).to_string())
        }
        _ => Err(mismatch(format!("entry file {relative} is not a resolvable blob at HEAD"))),
    }
}

fn serialize_manifest(
    manifest: &HostToolchainManifest,
    forbidden_locations: &[&Path],
) -> InstrumentResult<Vec<u8>> {
    let serialized = serde_json::to_vec_pretty(manifest)
        .map_err(|error| mismatch(format!("manifest serialization failed: {error}")))?;
    scan_for_machine_paths(&serialized, forbidden_locations)?;
    Ok(serialized)
}

fn scan_for_machine_paths(raw: &[u8], forbidden: &[&Path]) -> InstrumentResult<()> {
    let text = String::from_utf8_lossy(raw).to_lowercase();
    for location in forbidden {
        if location.as_os_str().is_empty() {
            continue;
        }
        let needle = location.to_string_lossy().to_lowercase();
        if needle.len() > 3 && text.contains(needle.as_str()) {
            return Err(mismatch(format!(
                "durable manifest would leak machine-specific absolute path rooted at {}",
                location.display()
            )));
        }
    }
    Ok(())
}

fn run_git(cwd: Option<&Path>, args: &[&str]) -> InstrumentResult<String> {
    let mut command = Command::new("git");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.args(args);
    let output = command.output().map_err(|error| {
        InstrumentFailure::new(
            CLASS_ACQUISITION_UNAVAILABLE,
            format!("git {args:?} could not be executed: {error}"),
        )
    })?;
    if !output.status.success() {
        return Err(InstrumentFailure::new(
            CLASS_ACQUISITION_UNAVAILABLE,
            format!(
                "git {args:?} failed ({}): {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn find_on_path(name: &str, path_env: &str) -> Option<PathBuf> {
    let list_separator = if cfg!(windows) { ";" } else { ":" };
    let candidates: &[String] = if cfg!(windows) {
        &[format!("{name}.exe"), format!("{name}.bat"), format!("{name}.cmd"), name.to_string()]
    } else {
        &[name.to_string()]
    };
    for directory in path_env.split(list_separator).filter(|d| !d.is_empty()) {
        for candidate in candidates {
            let path = Path::new(directory).join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn path_env() -> String {
    std::env::var("PATH").unwrap_or_default()
}

fn canonical_display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn project_root() -> Result<PathBuf> {
    crate::utils::project_root()
}

fn is_lower_hex40(value: &str) -> bool {
    value.len() == 40
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_lower(&digest)
}

#[cfg(test)]
fn git_blob_sha1(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", bytes.len()).as_bytes());
    hasher.update(bytes);
    hex_lower(&hasher.finalize())
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    const FIXTURE_VERSION_TEXT: &[u8] = b"VIM - Vi IMproved 9.1 FIXTURE-IDENTITY-TEXT\n";

    struct FixtureRepo {
        _guard: tempfile::TempDir,
        root: PathBuf,
        checkout: PathBuf,
        output: PathBuf,
        commit: String,
        tree: String,
        vim_executable: PathBuf,
        alternate_vim_executable: PathBuf,
    }

    fn fixture_plugin_contents() -> Vec<(&'static str, &'static str)> {
        vec![
            ("plugin/lsp.vim", "fixture plugin entry bytes A\n"),
            ("autoload/lsp.vim", "fixture autoload root bytes B\n"),
            ("autoload/lsp/utils.vim", "fixture utils bytes C\n"),
        ]
    }

    fn write_authority(root: &Path, commit: &str, tree: &str, entries: &[(String, String)]) {
        let entry_json = entries
            .iter()
            .map(|(path, blob)| format!(r#"{{"git_blob_sha1":"{blob}","path":"{path}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let document = format!(
            r#"{{
  "schema_version": "{SUBJECT_SCHEMA_VERSION}",
  "upstream": {{
    "repository": "https://fixture.invalid/vim-lsp",
    "selected_commit": "{commit}",
    "tree_digest": {{"algorithm": "{SUBJECT_TREE_DIGEST_ALGORITHM}", "value": "{tree}"}}
  }},
  "plugin_load_mode": {{"mode": "{LOAD_MODE}"}},
  "expected_content_identity": {{
    "entry_files": [{entry_json}]
  }}
}}"#
        );
        let authority_dir = root.join(".ci/editor-clients");
        fs::create_dir_all(&authority_dir).expect("authority dir");
        fs::write(authority_dir.join("vim-vim-lsp-subject.v1.json"), document)
            .expect("authority write");
    }

    fn git_quiet(cwd: &Path, args: &[&str]) {
        let status =
            StdCommand::new("git").current_dir(cwd).args(args).status().expect("git availability");
        assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    }

    fn build_fixture(tag: &str) -> Result<FixtureRepo> {
        build_fixture_variant(tag, "")
    }

    fn build_fixture_variant(tag: &str, subject_drift: &str) -> Result<FixtureRepo> {
        let guard = tempfile::TempDir::new()?;
        let root = guard.path().join(tag);
        let checkout = root.join("upstream-checkout");
        let output = root.join("toolchain-output");
        fs::create_dir_all(&checkout)?;
        git_quiet(&checkout, &["init", "--quiet"]);
        git_quiet(&checkout, &["config", "user.email", "fixture@example.invalid"]);
        git_quiet(&checkout, &["config", "user.name", "Fixture"]);
        git_quiet(&checkout, &["config", "core.autocrlf", "false"]);
        let mut entries = Vec::new();
        for (index, (relative, contents)) in fixture_plugin_contents().into_iter().enumerate() {
            let effective: String = if index == 1 {
                format!("{contents}{subject_drift}")
            } else {
                contents.to_string()
            };
            let path = checkout.join(&relative);
            fs::create_dir_all(path.parent().expect("parent"))?;
            fs::write(&path, &effective)?;
            entries.push((relative.to_string(), git_blob_sha1(effective.as_bytes())));
        }
        git_quiet(&checkout, &["add", "."]);
        git_quiet(&checkout, &["add", "."]);
        let status = StdCommand::new("git")
            .current_dir(&checkout)
            .env("GIT_AUTHOR_DATE", "2026-08-23T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-08-23T00:00:00Z")
            .args(["commit", "--quiet", "-m", "fixture subject"])
            .status()
            .expect("git availability");
        assert!(status.success(), "fixture commit failed");
        let commit = run_git(Some(&checkout), &["rev-parse", "HEAD"])
            .map_err(|error| color_eyre::eyre::eyre!("{error}"))?;
        let tree = run_git(Some(&checkout), &["rev-parse", "HEAD^{tree}"])
            .map_err(|error| color_eyre::eyre::eyre!("{error}"))?;
        write_authority(&root, &commit, &tree, &entries);
        let vim_executable = root.join("fake-vim.bin");
        fs::write(&vim_executable, "fake-vim-bytes\n")?;
        let alternate_vim_executable = root.join("fake-vim-alternate.bin");
        fs::write(&alternate_vim_executable, "different-vim-bytes\n")?;
        Ok(FixtureRepo {
            _guard: guard,
            root,
            checkout,
            output,
            commit,
            tree,
            vim_executable,
            alternate_vim_executable,
        })
    }

    fn fixture_provision_args(repo: &FixtureRepo) -> ProvisionArgs {
        ProvisionArgs {
            output: repo.output.clone(),
            vim: Some(repo.vim_executable.clone()),
            vim_lsp_source: Some(repo.checkout.clone()),
        }
    }

    const FIXTURE_VERSION_PROVIDER: FixedVimVersionOutput =
        FixedVimVersionOutput(FIXTURE_VERSION_TEXT);

    fn read_manifest(repo: &FixtureRepo) -> HostToolchainManifest {
        let bytes = fs::read(repo.output.join(MANIFEST_FILE_NAME))
            .expect("manifest exists after provision");
        serde_json::from_slice(&bytes).expect("manifest parses")
    }

    fn provision_fixture(repo: &FixtureRepo) -> ProvisionOutcome {
        provision_with(&repo.root, &fixture_provision_args(repo), &FIXTURE_VERSION_PROVIDER)
            .expect("fixture provision succeeds")
    }

    fn verify_with(
        repo: &FixtureRepo,
        vim: Option<PathBuf>,
        vim_lsp_dir: Option<PathBuf>,
    ) -> InstrumentResult<VerifyOutcome> {
        verify_with_provider(
            &repo.root,
            &VerifyArgs { manifest: repo.output.join(MANIFEST_FILE_NAME), vim, vim_lsp_dir },
            &FIXTURE_VERSION_PROVIDER,
        )
    }

    #[test]
    fn provision_binds_exact_fixture_subject_and_verifies() -> Result<()> {
        let repo = build_fixture("exact")?;
        let outcome = provision_fixture(&repo);
        let manifest = read_manifest(&repo);
        assert_eq!(manifest.schema_version, TOOLCHAIN_SCHEMA_VERSION);
        assert_eq!(manifest.host_role, HOST_ROLE);
        assert_eq!(manifest.provision_status, "verified");
        assert_eq!(manifest.vim_lsp.selected_commit, repo.commit);
        assert_eq!(manifest.vim_lsp.tree_digest, repo.tree);
        assert_eq!(manifest.vim_lsp.entry_files.len(), fixture_plugin_contents().len());
        assert_eq!(manifest.vim.acquisition_mode, "explicit_subject");
        assert_eq!(manifest.isolation.user_plugins_excluded, true);
        assert!(!manifest.cache_key.is_empty());
        assert!(outcome.manifest_path.is_file());
        verify_with(&repo, Some(repo.vim_executable.clone()), None)?;
        Ok(())
    }

    #[test]
    fn post_restore_revalidation_rejects_mutated_plugin_tree() -> Result<()> {
        let repo = build_fixture("mutated-tree")?;
        provision_fixture(&repo);
        let target = repo.output.join("vim-lsp").join("autoload/lsp.vim");
        let mut bytes = fs::read_to_string(&target)?;
        bytes.push_str("drift\n");
        fs::write(&target, bytes)?;
        let error = expect_instrument_failure(
            verify_with(&repo, Some(repo.vim_executable.clone()), None),
            "mutated runtime tree must fail verification",
        )?;
        assert_eq!(error.class(), CLASS_IDENTITY_MISMATCH);
        Ok(())
    }

    #[test]
    fn different_checkout_under_same_layout_is_rejected() -> Result<()> {
        let repo = build_fixture("wrong-tree")?;
        provision_fixture(&repo);
        let other = build_fixture_variant("wrong-tree-other", "drifted subject bytes\n")?;
        let error = expect_instrument_failure(
            verify_with(&repo, Some(repo.vim_executable.clone()), Some(other.checkout.clone())),
            "a different vim-lsp tree must fail verification regardless of layout name",
        )?;
        assert_eq!(error.class(), CLASS_IDENTITY_MISMATCH);
        Ok(())
    }

    #[test]
    fn same_version_text_with_different_vim_bytes_is_rejected() -> Result<()> {
        let repo = build_fixture("vim-bytes")?;
        provision_fixture(&repo);
        let error = expect_instrument_failure(
            verify_with(&repo, Some(repo.alternate_vim_executable.clone()), None),
            "same version text over different bytes must fail closed",
        )?;
        assert_eq!(error.class(), CLASS_IDENTITY_MISMATCH);
        Ok(())
    }

    #[test]
    fn role_or_schema_drift_in_manifest_is_rejected() -> Result<()> {
        let repo = build_fixture("role-drift")?;
        provision_fixture(&repo);
        let manifest_path = repo.output.join(MANIFEST_FILE_NAME);
        let original = fs::read_to_string(&manifest_path)?;
        for (field, replacement) in [
            (format!(r#""host_role": "{HOST_ROLE}""#), r#""host_role": "neovim_lsp_host""#),
            (
                format!(r#""schema_version": "{TOOLCHAIN_SCHEMA_VERSION}""#),
                r#""schema_version": "vim_vim_lsp_host_toolchain.v0""#,
            ),
        ] {
            let tampered = original.replace(field.as_str(), replacement);
            fs::write(&manifest_path, &tampered)?;
            let error = expect_instrument_failure(
                verify_with(&repo, Some(repo.vim_executable.clone()), None),
                "identity drift must fail verification",
            )?;
            assert_eq!(error.class(), CLASS_IDENTITY_MISMATCH);
            fs::write(&manifest_path, &original)?;
        }
        Ok(())
    }

    #[test]
    fn governed_pin_update_invalidates_existing_manifest() -> Result<()> {
        let repo = build_fixture("pin-drift")?;
        provision_fixture(&repo);
        let other = build_fixture_variant("pin-drift-other", "drifted pin bytes\n")?;
        fs::copy(
            other.root.join(".ci/editor-clients/vim-vim-lsp-subject.v1.json"),
            repo.root.join(".ci/editor-clients/vim-vim-lsp-subject.v1.json"),
        )?;
        let error = expect_instrument_failure(
            verify_with(&repo, Some(repo.vim_executable.clone()), None),
            "an updated governed pin must invalidate previously provisioned manifests",
        )?;
        assert_eq!(error.class(), CLASS_IDENTITY_MISMATCH);
        Ok(())
    }

    #[test]
    fn platform_change_breaks_cache_identity() -> Result<()> {
        let repo = build_fixture("platform-drift")?;
        provision_fixture(&repo);
        let manifest_path = repo.output.join(MANIFEST_FILE_NAME);
        let tampered = fs::read_to_string(&manifest_path)?
            .replace(&format!(r#""os": "{}""#, std::env::consts::OS), r#""os": "hypothetical-os""#);
        fs::write(&manifest_path, tampered)?;
        let error = expect_instrument_failure(
            verify_with(&repo, Some(repo.vim_executable.clone()), None),
            "platform substitution must fail the reproduced cache key",
        )?;
        assert_eq!(error.class(), CLASS_IDENTITY_MISMATCH);
        Ok(())
    }

    #[test]
    fn cache_key_changes_with_every_load_bearing_input() -> Result<()> {
        let repo = build_fixture("cache-completeness")?;
        let base = build_manifest(
            &SubjectAuthority {
                bytes_sha256: "a".repeat(40),
                repository: "https://fixture.invalid/vim-lsp".into(),
                selected_commit: repo.commit.clone(),
                tree_digest: repo.tree.clone(),
                load_mode: LOAD_MODE.into(),
                entry_files: vec![("plugin/lsp.vim".into(), "b".repeat(40))],
            },
            "path_lookup".into(),
            CapturedVimIdentity {
                executable_sha256: "c".repeat(64),
                version_text_sha256: "d".repeat(64),
                version_summary: "VIM fixture".into(),
            },
        );
        let base_key = base.cache_key.clone();
        assert_eq!(base_key.len(), 64);

        let mutate = |change: &dyn Fn(&mut HostToolchainManifest)| {
            let mut variant = base.clone();
            change(&mut variant);
            finalize_cache_key(variant).cache_key
        };

        let mutations: Vec<(&str, Box<dyn Fn(&mut HostToolchainManifest)>)> = vec![
            (
                "os",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.platform.os = "other-os".into();
                }),
            ),
            (
                "arch",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.platform.arch = "other-arch".into();
                }),
            ),
            (
                "execution_environment",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.platform.execution_environment = "container".into();
                }),
            ),
            (
                "vim_acquisition",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.vim.acquisition_mode = "explicit_subject".into();
                }),
            ),
            (
                "vim_bytes",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.vim.executable_sha256 = "0".repeat(64);
                }),
            ),
            (
                "version_text",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.vim.version_text_sha256 = "1".repeat(64);
                }),
            ),
            (
                "commit",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.vim_lsp.selected_commit = "2".repeat(40);
                }),
            ),
            (
                "tree",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.vim_lsp.tree_digest = "3".repeat(40);
                }),
            ),
            (
                "authority_bytes",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.vim_lsp.subject_authority_sha256 = "4".repeat(64);
                }),
            ),
            (
                "entry_blob",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.vim_lsp.entry_files[0].git_blob_sha1 = "5".repeat(40);
                }),
            ),
            (
                "load_mode",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.vim_lsp.load_mode = "floating_master".into();
                }),
            ),
            (
                "isolation",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.isolation.vimrc_policy = "ambient_user_vimrc_admitted".into();
                }),
            ),
            (
                "host_role",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.host_role = "relabeled_role".into();
                }),
            ),
            (
                "schema",
                Box::new(|m: &mut HostToolchainManifest| {
                    m.schema_version = "vim_vim_lsp_host_toolchain.v9".into();
                }),
            ),
        ];
        assert!(mutations.len() >= 12, "cache-key completeness sweep must stay broad");
        for (label, change) in &mutations {
            let mutated = mutate(change.as_ref());
            assert_ne!(base_key, mutated, "cache key must change when {label} changes");
        }
        Ok(())
    }

    #[test]
    fn serialization_is_deterministic_and_machine_path_independent() -> Result<()> {
        let first = build_fixture("determinism-a")?;
        let second = build_fixture("determinism-b")?;
        let first_outcome = provision_fixture(&first);
        let second_outcome = provision_fixture(&second);
        let first_bytes = fs::read(first_outcome.manifest_path.clone())?;
        let second_bytes = fs::read(second_outcome.manifest_path.clone())?;
        assert_eq!(first_bytes, second_bytes, "identical subjects must serialize identically");
        let first_text = String::from_utf8(first_bytes)?.to_lowercase();
        assert!(!first_text.contains(&first.root.to_string_lossy().to_lowercase()));
        assert!(!first_text.contains(&second.root.to_string_lossy().to_lowercase()));
        Ok(())
    }

    #[test]
    fn acquisition_failures_classify_as_instrument_failure_not_product() -> Result<()> {
        let repo = build_fixture("classification-missing-source")?;
        let args = ProvisionArgs {
            output: repo.output.clone(),
            vim: Some(repo.vim_executable.clone()),
            vim_lsp_source: Some(repo.root.join("does-not-exist")),
        };
        let error = expect_instrument_failure(
            provision_with(&repo.root, &args, &FIXTURE_VERSION_PROVIDER),
            "missing acquisition source must be an instrument failure",
        )?;
        assert_eq!(error.class(), CLASS_ACQUISITION_UNAVAILABLE);
        assert!(error.to_string().starts_with("instrument_failed(acquisition_unavailable)"));

        let repo = build_fixture("classification-no-git-source")?;
        let plain_directory = repo.root.join("plain-directory");
        fs::create_dir_all(&plain_directory)?;
        let args = ProvisionArgs {
            output: repo.output.clone(),
            vim: Some(repo.vim_executable.clone()),
            vim_lsp_source: Some(plain_directory),
        };
        let error = expect_instrument_failure(
            provision_with(&repo.root, &args, &FIXTURE_VERSION_PROVIDER),
            "non-git acquisition source must be an instrument failure",
        )?;
        assert_eq!(error.class(), CLASS_ACQUISITION_UNAVAILABLE);
        Ok(())
    }

    #[test]
    fn warm_cache_with_matching_subject_is_reused_not_rebuilt() -> Result<()> {
        let repo = build_fixture("warm-cache")?;
        provision_fixture(&repo);
        let provisioned_tree = repo.output.join("vim-lsp").join("autoload/lsp/utils.vim");
        let before = fs::read_to_string(&provisioned_tree)?;
        let args = fixture_provision_args(&repo);
        provision_with(&repo.root, &args, &FIXTURE_VERSION_PROVIDER)
            .expect("warm cache reuse succeeds");
        let after = fs::read_to_string(&provisioned_tree)?;
        assert_eq!(before, after, "matching warm cache must satisfy the role unchanged");
        verify_with(&repo, Some(repo.vim_executable.clone()), None)?;
        Ok(())
    }

    #[test]
    fn warm_cache_with_drifted_subject_is_rebuilt_and_then_verified() -> Result<()> {
        let repo = build_fixture("cache-rebuild")?;
        provision_fixture(&repo);
        let plugin_target = repo.output.join("vim-lsp").join("plugin/lsp.vim");
        fs::write(&plugin_target, "poisoned warm cache\n")?;
        let args = fixture_provision_args(&repo);
        provision_with(&repo.root, &args, &FIXTURE_VERSION_PROVIDER)
            .expect("drifted warm cache triggers rebuild");
        let restored = fs::read_to_string(&plugin_target)?.replace('\r', "");
        assert_eq!(restored, fixture_plugin_contents()[0].1);
        verify_with(&repo, Some(repo.vim_executable.clone()), None)?;
        Ok(())
    }

    #[test]
    fn find_on_path_prefers_exact_candidates_without_env_mutation() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let dir = temp.path().join("tools");
        fs::create_dir_all(&dir).expect("dir");
        let executable = dir.join(if cfg!(windows) { "vim.exe" } else { "vim" });
        fs::write(&executable, b"stub").expect("stub");
        let separator = if cfg!(windows) { ";" } else { ":" };
        let path_env = format!("{separator}{}{separator}", dir.display());
        let found = find_on_path("vim", &path_env).expect("resolution finds stub");
        assert_eq!(
            found.canonicalize().expect("canonical"),
            executable.canonicalize().expect("canonical")
        );
        assert!(find_on_path("definitely-absent-editor", &path_env).is_none());
    }

    #[test]
    fn capture_rejects_empty_version_text() -> Result<()> {
        let repo = build_fixture("empty-version")?;
        let provider = FixedVimVersionOutput(b"\n\n");
        let error = expect_instrument_failure(
            capture_vim_identity(&repo.vim_executable, &provider),
            "empty version text cannot identify a subject",
        )?;
        assert_eq!(error.class(), CLASS_SUBJECT_UNRESOLVED);
        Ok(())
    }

    #[test]
    fn git_blob_sha1_matches_git_hash_object_semantics() {
        let bytes = b"hello world\n";
        assert_eq!(git_blob_sha1(bytes), "3b18e512dba79e4c8300dd08aeb37f8e728b8dad");
    }

    #[test]
    fn explicit_vim_resolution_requires_a_real_file() -> Result<()> {
        let repo = build_fixture("explicit-resolution")?;
        let error = expect_instrument_failure(
            resolve_explicit_vim(&repo.root.join("absent-vim.bin")),
            "missing explicit subject must fail",
        )?;
        assert_eq!(error.class(), CLASS_SUBJECT_UNRESOLVED);
        Ok(())
    }

    fn expect_instrument_failure<T>(
        outcome: InstrumentResult<T>,
        message: &str,
    ) -> Result<InstrumentFailure> {
        match outcome {
            Ok(_) => Err(eyre!("{message}")),
            Err(failure) => Ok(failure),
        }
    }
}
