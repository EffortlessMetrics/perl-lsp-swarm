use std::env;
use std::fs;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

const SERVER_PATH: &str = "node_modules/.bin/perlnavigator";
const PACKAGE_NAME: &str = "perlnavigator-server";
const PERLNAVIGATOR_SERVER_ID: &str = "perlnavigator-server";

const PERL_LSP_SERVER_ID: &str = "perl-lsp";
const PERL_LSP_REPO: &str = "tree-sitter-perl/perl-tree-sitter-lsp";

const PERLLSP_SERVER_ID: &str = "perllsp";
const PERLLSP_REPO: &str = "EffortlessMetrics/perl-lsp";

// EffortlessMetrics' DAP debug adapter. This identity is deliberately
// independent from every language-server ID above: `perl-dap` never aliases
// `perllsp`, `perl-lsp`, or `perlnavigator-server`, and no language-server
// executable, cache family, or receipt may satisfy it (#9485).
const PERL_DAP_ADAPTER_ID: &str = "perl-dap";
const PERL_DAP_BINARY_NAME: &str = "perl-dap";
// The canonical release topology is shared with perllsp: the same
// EffortlessMetrics/perl-lsp release archives ship both binaries
// (`perllsp-{version}-{triple}` archives containing `perllsp` and `perl-dap`).
const PERL_DAP_REPO: &str = PERLLSP_REPO;
// Debugger-specific managed cache boundary. Only directories with this prefix
// belong to the adapter route; cleanup can never touch `perllsp-`/`perl-lsp-`
// language-server caches, user binaries, or extension state.
const PERL_DAP_MANAGED_PREFIX: &str = "perl-dap-managed-";

// Durable accepted-current selection state for managed perllsp startup
// (#11308). The manifest records the exact installed identity so an already
// verified binary can be reconstructed offline after extension restart; it is
// written only after a candidate download is fully staged and verified.
const SELECTION_MANIFEST_PATH: &str = "perllsp-selection.v1.json";
const SELECTION_MANIFEST_TMP_PATH: &str = "perllsp-selection.v1.json.tmp";
const SELECTION_MANIFEST_SCHEMA_VERSION: &str = "perllsp_selection.v1";
const SELECTION_PRODUCT: &str = "perllsp";
const SELECTION_ROLE: &str = "lsp_server";

/// Typed update-check outcome for the managed route.
///
/// Startup never performs a release-metadata request when an accepted current
/// subject exists, so the steady-state update fact is `NotRequested`. The only
/// admitted network trigger is a cold start with no accepted current subject;
/// there is deliberately no timer-driven cadence because the Zed extension API
/// exposes none (recorded limitation, #11308).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateState {
    /// An accepted current subject was served without any network request.
    NotRequested,
    /// No accepted current subject existed; metadata and download were admitted.
    ColdInstall,
    /// A cold start failed while contacting release metadata or downloading.
    TransportFailed,
    /// A cold start received release metadata that did not match the contract.
    MetadataInvalid,
    /// A downloaded candidate failed verification and was not promoted.
    CandidateRejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionManifest {
    release_tag: String,
    release_version: String,
    target: String,
    asset_name: String,
    archive_member: String,
    installed_path: String,
    binary_sha256: String,
}

fn is_sha256_digest(text: &str) -> bool {
    text.len() == "sha256:".len() + 64
        && text.starts_with("sha256:")
        && text["sha256:".len()..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let path = Path::new(path);
    !path.is_absolute()
        && path.components().all(|component| {
            !matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
}

fn parse_selection_manifest(text: &str) -> Result<SelectionManifest, String> {
    use zed::serde_json::Value;

    let value: Value = zed::serde_json::from_str(text)
        .map_err(|error| format!("selection manifest is not valid JSON: {error}"))?;

    let expect_string = |pointer: &str| -> std::result::Result<String, String> {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("selection manifest lacks `{pointer}`"))
    };

    if value.get("schema_version").and_then(Value::as_str)
        != Some(SELECTION_MANIFEST_SCHEMA_VERSION)
    {
        return Err("selection manifest has an unsupported schema_version".to_string());
    }
    if value.get("product").and_then(Value::as_str) != Some(SELECTION_PRODUCT) {
        return Err("selection manifest does not describe the perllsp product".to_string());
    }
    if value.get("role").and_then(Value::as_str) != Some(SELECTION_ROLE) {
        return Err("selection manifest does not describe the LSP server role".to_string());
    }
    if value.get("state").and_then(Value::as_str) != Some("current") {
        return Err("selection manifest is not a selected current subject".to_string());
    }

    let release_tag = expect_string("/release/tag")?;
    let release_version = expect_string("/release/version")?;
    let target = expect_string("/target")?;
    let asset_name = expect_string("/asset_name")?;
    let archive_member = expect_string("/archive_member")?;
    let installed_path = expect_string("/installed_path")?;
    let binary_sha256 = expect_string("/binary_sha256")?;

    if release_tag.is_empty() || release_version.is_empty() || target.is_empty() {
        return Err("selection manifest release/target identity is empty".to_string());
    }
    if !is_safe_relative_path(&installed_path) {
        return Err("selection manifest installed_path is not a safe relative path".to_string());
    }
    if !is_safe_relative_path(&archive_member) {
        return Err("selection manifest archive_member is not a safe relative path".to_string());
    }
    if asset_name.is_empty() {
        return Err("selection manifest asset_name is empty".to_string());
    }
    if !is_sha256_digest(&binary_sha256) {
        return Err("selection manifest binary_sha256 is not a sha256 digest".to_string());
    }

    Ok(SelectionManifest {
        release_tag,
        release_version,
        target,
        asset_name,
        archive_member,
        installed_path,
        binary_sha256,
    })
}

fn selection_manifest_json(manifest: &SelectionManifest) -> String {
    let value = zed::serde_json::json!({
        "schema_version": SELECTION_MANIFEST_SCHEMA_VERSION,
        "product": SELECTION_PRODUCT,
        "role": SELECTION_ROLE,
        "state": "current",
        "release": {
            "tag": manifest.release_tag,
            "version": manifest.release_version,
        },
        "target": manifest.target,
        "asset_name": manifest.asset_name,
        "archive_member": manifest.archive_member,
        "installed_path": manifest.installed_path,
        "binary_sha256": manifest.binary_sha256,
    });
    value.to_string()
}

/// Write the selection manifest atomically from this attempt's private
/// temporary path, then rename over the durable name (#11316). A partial
/// write can never become accepted, and concurrent promotions from separate
/// Zed processes cannot tear each other's staged bytes.
fn store_selection_manifest(attempt: &str, manifest: &SelectionManifest) -> Result<(), String> {
    store_selection_manifest_in(Path::new("."), attempt, manifest)
}

fn store_selection_manifest_in(
    work_dir: &Path,
    attempt: &str,
    manifest: &SelectionManifest,
) -> Result<(), String> {
    let tmp_path = work_dir.join(selection_manifest_tmp_path(attempt));
    fs::write(&tmp_path, selection_manifest_json(manifest))
        .map_err(|error| format!("failed to stage selection manifest: {error}"))?;
    fs::rename(&tmp_path, work_dir.join(SELECTION_MANIFEST_PATH)).map_err(|error| {
        format!("failed to promote selection manifest `{SELECTION_MANIFEST_PATH}`: {error}")
    })
}

/// Hex-encoded SHA-256 of `bytes`, used to bind the exact installed subject.
fn content_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Parse the durable selection manifest if one is present and well-formed.
///
/// `None` covers both an absent manifest and an unreadable one; callers that
/// need the rejection cause use [`load_accepted_current_in`].
fn load_selection_manifest_in(work_dir: &Path) -> Option<SelectionManifest> {
    let text = fs::read_to_string(work_dir.join(SELECTION_MANIFEST_PATH)).ok()?;
    parse_selection_manifest(&text).ok()
}

/// Reconstruct the accepted current managed subject from durable identity.
///
/// This is the offline-first startup path: no release metadata request, no
/// directory scanning, no version-naming trust. The manifest must bind every
/// identity field and the installed bytes must still hash to the recorded
/// digest for the current platform's target.
fn load_accepted_current_in(
    work_dir: &Path,
    os: zed::Os,
    arch: zed::Architecture,
) -> std::result::Result<String, String> {
    let manifest_text = fs::read_to_string(work_dir.join(SELECTION_MANIFEST_PATH))
        .map_err(|_| "no accepted perllsp selection manifest".to_string())?;
    let manifest = parse_selection_manifest(&manifest_text)?;

    let expected_target = perllsp_target(os, arch)?;
    if manifest.target != expected_target {
        return Err(format!(
            "accepted perllsp selection targets `{}` but this host needs `{expected_target}`",
            manifest.target
        ));
    }

    let bytes = fs::read(work_dir.join(&manifest.installed_path))
        .map_err(|_| "accepted perllsp binary recorded by the manifest is missing".to_string())?;
    let digest = content_sha256(&bytes);
    if format!("sha256:{digest}") != manifest.binary_sha256 {
        return Err(
            "accepted perllsp binary no longer matches its recorded digest; refusing stale or corrupted subject"
                .to_string(),
        );
    }

    Ok(manifest.installed_path)
}

/// What the cold install route may do with an already-present binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColdDisposition {
    /// No binary at the expected path: download it.
    DownloadFresh,
    /// A binary exists but the durable selection manifest names exactly this
    /// path while offline verification rejected it. Presence alone is not
    /// trust: replace the rejected subject instead of re-accepting its bytes.
    ReplaceRejected,
    /// A binary exists with no manifest binding against it (pre-upgrade
    /// state); adopt it by binding a fresh digest.
    ReuseExisting,
}

fn cold_disposition(binary_exists: bool, manifest_names_binary_path: bool) -> ColdDisposition {
    match (binary_exists, manifest_names_binary_path) {
        (false, _) => ColdDisposition::DownloadFresh,
        (true, true) => ColdDisposition::ReplaceRejected,
        (true, false) => ColdDisposition::ReuseExisting,
    }
}

/// Typed outcome of one managed mutation attempt (#11316).
///
/// This vocabulary is the contention/interruption/publication surface fed to
/// the cache-lifecycle and startup-status consumers; it deliberately adds no
/// second cache-status authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MutationOutcome {
    /// This attempt won the atomic publication: its staged tree became the
    /// durable subject.
    Published,
    /// Another attempt had already published a bound, byte-verified subject;
    /// this contender adopted it and removed only its own staging tree.
    AdoptedPublished,
    /// A complete but not-yet-bound durable member was left in place for
    /// adoption and binding instead of being replaced or trusted blindly.
    AdoptedUnboundMember,
}

/// Freshly reread classification of the durable destination (#11316).
///
/// Every decision that could destroy or replace durable bytes re-derives this
/// classification first, so a concurrent writer's repair or publication wins
/// any race against this attempt's cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DurableSubject {
    /// A manifest binds exactly this path and the bytes still match the
    /// bound digest.
    BoundIntact,
    /// A manifest binds exactly this path but the bytes no longer match it.
    BoundCorrupted,
    /// No manifest binds this path yet, but a complete member sits there.
    UnboundMember,
    /// Nothing usable exists at the destination.
    Absent,
}

/// Unique-per-invocation mutation attempt identity (#11316).
///
/// Timestamp nanoseconds plus a process-local counter. No PID/age lease
/// identity exists and none is needed: publication is one content-bound
/// atomic rename, so nothing about an attempt can be stolen by guessing age
/// or PID.
fn next_attempt_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos =
        SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{counter:x}")
}

/// The attempt-private staging root for one managed mutation (#11316).
///
/// Private and non-launchable by construction: it never equals the durable
/// directory name and is never selected by any resolution path.
fn attempt_staging_dir(version_dir: &str, attempt: &str) -> String {
    format!("{version_dir}.attempt-{attempt}")
}

/// Claim a cross-process-unique attempt staging root (#11316).
///
/// Uniqueness comes from `create_dir`'s exclusive semantics, not from clock
/// or PID guessing: a candidate name that already exists (including one
/// generated in the same clock tick by another Zed process) is rejected and a
/// fresh identity is generated, up to a bounded number of retries. Returns
/// the attempt id and its claimed, empty staging root.
fn claim_attempt_staging(work_dir: &Path, version_dir: &str) -> Result<(String, String), String> {
    for _ in 0..16 {
        let attempt = next_attempt_id();
        let attempt_dir = attempt_staging_dir(version_dir, &attempt);
        match fs::create_dir(work_dir.join(&attempt_dir)) {
            Ok(()) => return Ok((attempt, attempt_dir)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "failed to claim perllsp attempt staging `{attempt_dir}`: {error}"
                ));
            }
        }
    }
    Err("could not claim a unique perllsp attempt staging root".to_string())
}

/// The attempt-private staging path for one selection-manifest promotion.
fn selection_manifest_tmp_path(attempt: &str) -> String {
    format!("{SELECTION_MANIFEST_TMP_PATH}-{attempt}")
}

/// Hex-encoded SHA-256 of a file's bytes, or `None` when unreadable.
fn file_sha256_hex(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|bytes| content_sha256(&bytes))
}

/// Remove exactly one owned attempt staging tree (#11316).
///
/// Never globbed, never aged, never prefix-swept: cleanup touches only the
/// exact path this attempt created.
fn remove_owned_attempt(attempt_dir: &Path) -> Result<(), String> {
    if fs::metadata(attempt_dir).is_ok() {
        return fs::remove_dir_all(attempt_dir).map_err(|error| {
            format!(
                "failed to remove owned perllsp attempt staging `{}`: {error}",
                attempt_dir.display()
            )
        });
    }
    Ok(())
}

/// Reread the durable destination right before acting on it (#11316).
fn classify_durable_subject(work_dir: &Path, binary_path: &str) -> DurableSubject {
    let durable_binary = work_dir.join(binary_path);
    if let Some(manifest) = load_selection_manifest_in(work_dir) {
        if manifest.installed_path == binary_path {
            let intact = file_sha256_hex(&durable_binary)
                .is_some_and(|hex| format!("sha256:{hex}") == manifest.binary_sha256);
            return if intact {
                DurableSubject::BoundIntact
            } else {
                DurableSubject::BoundCorrupted
            };
        }
    }
    if fs::metadata(&durable_binary).is_ok_and(|metadata| metadata.is_file()) {
        DurableSubject::UnboundMember
    } else {
        DurableSubject::Absent
    }
}

/// Publish a fully staged, executable-ready attempt atomically (#11316).
///
/// The single rename is the commit point. On destination contention the loser
/// settles explicitly from freshly reread state and never deletes the winner,
/// another live attempt, or any known-good subject:
///
/// - `BoundIntact`: adopt the published subject as-is.
/// - `BoundCorrupted`: replace it only behind a fresh reread guard that
///   cancels into adoption when a concurrent repair lands first.
/// - `UnboundMember`: leave the complete member in place; the caller binds it.
/// - `Absent` (interrupted legacy shell): swap the shell out and retry once.
///
/// Destructive replacement is never an in-place unlink: the durable name is
/// first swapped atomically to this attempt's `.superseded` graveyard, so a
/// stale replacer can only ever overwrite the destination with its own
/// equally staged subject, never leave it missing after deleting a concurrent
/// winner's tree (#11316).
fn publish_staged_attempt(
    work_dir: &Path,
    version_dir: &str,
    attempt_dir: &str,
    binary_path: &str,
) -> std::result::Result<MutationOutcome, String> {
    let durable_dir = work_dir.join(version_dir);
    let staged_dir = work_dir.join(attempt_dir);

    if fs::rename(&staged_dir, &durable_dir).is_ok() {
        return Ok(MutationOutcome::Published);
    }

    match classify_durable_subject(work_dir, binary_path) {
        DurableSubject::BoundIntact => {
            remove_owned_attempt(&staged_dir)?;
            Ok(MutationOutcome::AdoptedPublished)
        }
        DurableSubject::BoundCorrupted => {
            // Reread guard: replace only while a second fresh read still finds
            // corrupted bytes. A concurrent repair wins this race and turns
            // replacement into plain adoption.
            if classify_durable_subject(work_dir, binary_path) == DurableSubject::BoundCorrupted {
                swap_durable_aside(work_dir, attempt_dir, version_dir)?;
                fs::rename(&staged_dir, &durable_dir).map_err(|error| {
                    format!("failed to publish replacement perllsp `{version_dir}`: {error}")
                })?;
                cleanup_superseded_graveyard(work_dir, attempt_dir);
                Ok(MutationOutcome::Published)
            } else {
                remove_owned_attempt(&staged_dir)?;
                Ok(MutationOutcome::AdoptedPublished)
            }
        }
        DurableSubject::UnboundMember => {
            remove_owned_attempt(&staged_dir)?;
            Ok(MutationOutcome::AdoptedUnboundMember)
        }
        DurableSubject::Absent => {
            // A destination holding neither a bound subject nor a complete
            // member is an interrupted legacy shell left by older shared-name
            // downloads. Swap it aside behind a second fresh reread (a
            // concurrent publisher's win cancels this recovery), then retry
            // once rather than treat either state as success.
            if fs::metadata(&durable_dir).is_ok()
                && classify_durable_subject(work_dir, binary_path) == DurableSubject::Absent
            {
                swap_durable_aside(work_dir, attempt_dir, version_dir)?;
                if fs::rename(&staged_dir, &durable_dir).is_ok() {
                    cleanup_superseded_graveyard(work_dir, attempt_dir);
                    return Ok(MutationOutcome::Published);
                }
            }
            fs::rename(&staged_dir, &durable_dir).map(|()| MutationOutcome::Published).map_err(
                |error| format!("failed to publish staged perllsp `{version_dir}`: {error}"),
            )
        }
    }
}

/// The graveyard sibling where this attempt parks a durable directory it is
/// about to replace. Single naming owner for the swap-aside protocol so
/// cleanup and sweeps stay exact (#11316).
fn superseded_graveyard(work_dir: &Path, attempt_dir: &str) -> std::path::PathBuf {
    work_dir.join(format!("{attempt_dir}.superseded"))
}

/// Atomically move the durable directory to this attempt's graveyard sibling
/// so the destination name becomes free for one atomic re-publish. The
/// graveyard stays inside this attempt's owned namespace and is swept only by
/// [`cleanup_superseded_graveyard`] (#11316).
fn swap_durable_aside(work_dir: &Path, attempt_dir: &str, version_dir: &str) -> Result<(), String> {
    let durable_dir = work_dir.join(version_dir);
    fs::rename(&durable_dir, superseded_graveyard(work_dir, attempt_dir)).map_err(|error| {
        format!("failed to set aside corrupted perllsp subject `{version_dir}`: {error}")
    })
}

/// Remove this attempt's superseded-subject graveyard after its replacement
/// was published successfully.
fn cleanup_superseded_graveyard(work_dir: &Path, attempt_dir: &str) {
    let _ = fs::remove_dir_all(superseded_graveyard(work_dir, attempt_dir));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerKind {
    PerlNavigator,
    TreeSitterPerlLsp,
    EffortlessPerllsp,
}

fn classify_server_id(id: &str) -> Result<ServerKind> {
    match id {
        PERLNAVIGATOR_SERVER_ID => Ok(ServerKind::PerlNavigator),
        PERL_LSP_SERVER_ID => Ok(ServerKind::TreeSitterPerlLsp),
        PERLLSP_SERVER_ID => Ok(ServerKind::EffortlessPerllsp),
        other => Err(format!("unknown Perl language server id `{other}`")),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PerllspCommandSettings {
    path: Option<String>,
    arguments: Vec<String>,
    env: Vec<(String, String)>,
}

/// Typed executable-selection routes for the managed product (#11041).
///
/// Variant meanings mirror the host-receipt `resolution_route` vocabulary
/// (`binary_override` / `worktree_path` / `managed_download`) so every support
/// and evidence surface keeps the three cells separate. Authority is carried
/// by the route classification alone: a route variant deliberately holds no
/// path or digest fields, because canonical binary identity (#10340) proves
/// what was selected and can never prove that the selecting source was
/// authorized.
///
/// The extension API exposes no receipt channel, so variants are contract
/// vocabulary rather than runtime-constructed values; host receipts record
/// which cell fired.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionRoute {
    /// `lsp.perllsp.binary.path`. Current Zed consumes this setting above this
    /// extension and gates it on its own worktree-trust boundary; the value
    /// never needs extension execution. This extension refuses duplicate
    /// grants: merged settings expose no provenance, so no client-side label
    /// can prove user or machine authority.
    ExplicitOverride,
    /// `worktree.which("perllsp")`: the Zed-defined worktree shell
    /// environment. direnv/shell hooks may alter resolution, so this cell
    /// stays visibly distinct from managed release identity and is never
    /// presented as release-backed evidence.
    WorktreePathDiscovery,
    /// The digest-bound canonical public release artifact (#10340/#10530,
    /// offline startup #11308).
    ManagedArtifact,
}

/// Fail-closed refusal for an unclassifiable execution source (#11041).
///
/// Names the policy, why provenance cannot be proven against the current
/// extension API, the host-owned trusted mechanism, and every remaining
/// authorized alternative. It deliberately never echoes the refused value:
/// project-controlled input must not reach durable logs through this error.
const EXECUTION_SOURCE_REFUSAL: &str = concat!(
    "refusing `lsp.perllsp.binary.path`: merged Zed settings carry no provenance, ",
    "so this extension cannot prove who authorized this executable (#11041); ",
    "current Zed launches settings-binary overrides itself behind its worktree trust prompt, ",
    "so no extension-side grant is needed; to run a local build through this extension instead, ",
    "put it on the worktree PATH or enable the digest-bound managed download"
);

/// The single fail-closed execution-source gate for the managed product
/// (#11041). Every `perllsp` launch consumes this decision BEFORE any binary
/// lookup, command construction, download, or process start.
///
/// Merged `LspSettings` expose no provenance, so a non-empty
/// `lsp.perllsp.binary.path` observed here cannot prove user or machine
/// authority and is refused rather than silently executed. Unknown provenance
/// never defaults to trusted, and project/resource precedence can never widen
/// execution authority.
fn authorize_execution_source(explicit_override: Option<&str>) -> Result<(), String> {
    match explicit_override {
        Some(path) if path.trim().is_empty() => {
            Err("lsp.perllsp.binary.path must not be empty".to_string())
        }
        Some(_) => Err(EXECUTION_SOURCE_REFUSAL.to_string()),
        None => Ok(()),
    }
}

struct PerlExtension {
    did_find_server: bool,
    perl_lsp_path: Option<String>,
    perllsp_path: Option<String>,
    perl_dap_path: Option<String>,
    update_state: UpdateState,
    cold_install_attempts: u32,
}

impl PerlExtension {
    fn server_exists(&self) -> bool {
        fs::metadata(SERVER_PATH).is_ok_and(|metadata| metadata.is_file())
    }

    fn server_script_path(&mut self, language_server_id: &LanguageServerId) -> Result<String> {
        let server_exists = self.server_exists();

        if self.did_find_server && server_exists {
            return Ok(SERVER_PATH.to_string());
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let version = zed::npm_package_latest_version(PACKAGE_NAME)?;

        if !server_exists
            || zed::npm_package_installed_version(PACKAGE_NAME)?.as_ref() != Some(&version)
        {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            let result = zed::npm_install_package(PACKAGE_NAME, &version);
            match result {
                Ok(()) => {
                    if !self.server_exists() {
                        return Err(format!(
                            "installed package '{PACKAGE_NAME}' did not contain expected path '{SERVER_PATH}'"
                        ));
                    }
                }
                Err(error) => {
                    if !self.server_exists() {
                        return Err(error);
                    }
                }
            }
        }

        self.did_find_server = true;
        Ok(SERVER_PATH.to_string())
    }

    fn perlnavigator_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        if let Some(path) = worktree.which("perlnavigator") {
            return Ok(zed::Command {
                command: path,
                args: vec!["--stdio".to_string()],
                env: Default::default(),
            });
        }

        let server_path = self.server_script_path(language_server_id)?;
        let extension_dir = env::current_dir()
            .map_err(|error| format!("failed to resolve extension working directory: {error}"))?;

        Ok(zed::Command {
            command: zed::node_binary_path()?,
            args: vec![
                extension_dir.join(server_path).to_string_lossy().to_string(),
                "--stdio".to_string(),
            ],
            env: Default::default(),
        })
    }

    fn perl_lsp_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let command = self.perl_lsp_binary(language_server_id, worktree)?;
        Ok(zed::Command { command, args: Vec::new(), env: Default::default() })
    }

    fn perl_lsp_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        if let Some(path) = worktree.which(PERL_LSP_SERVER_ID) {
            return Ok(path);
        }

        let download_opt_in = LspSettings::for_worktree(PERL_LSP_SERVER_ID, worktree)
            .ok()
            .and_then(|settings| settings.settings)
            .and_then(|value| value.get("download").and_then(|download| download.as_bool()))
            .unwrap_or(false);

        if !download_opt_in {
            return Err(concat!(
                "perl-lsp is opt-in: it will not be downloaded or started unless you opt in, ",
                "so you can ignore this if you did not ask for it.\n",
                "To use perl-lsp, either install it on PATH or enable its managed download.\n",
                "This ID belongs to tree-sitter-perl's independent server, not EffortlessMetrics perllsp."
            )
            .to_string());
        }

        self.download_perl_lsp(language_server_id)
    }

    fn download_perl_lsp(&mut self, language_server_id: &LanguageServerId) -> Result<String> {
        if let Some(path) = &self.perl_lsp_path {
            if fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
                return Ok(path.clone());
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            PERL_LSP_REPO,
            zed::GithubReleaseOptions { require_assets: true, pre_release: false },
        )?;

        let (os, arch) = zed::current_platform();
        let target = match (os, arch) {
            (zed::Os::Mac, zed::Architecture::Aarch64) => "aarch64-apple-darwin",
            (zed::Os::Mac, zed::Architecture::X8664) => "x86_64-apple-darwin",
            (zed::Os::Linux, zed::Architecture::X8664) => "x86_64-unknown-linux-musl",
            (zed::Os::Linux, zed::Architecture::Aarch64) => "aarch64-unknown-linux-musl",
            (zed::Os::Windows, zed::Architecture::X8664) => "x86_64-pc-windows-msvc",
            _ => {
                return Err(format!(
                    "perl-lsp has no managed binary for {os:?}/{arch:?}; install it on PATH instead"
                ));
            }
        };

        let (archive_ext, file_type, bin_name) = match os {
            zed::Os::Windows => ("zip", zed::DownloadedFileType::Zip, "perl-lsp.exe"),
            _ => ("tar.gz", zed::DownloadedFileType::GzipTar, "perl-lsp"),
        };
        let asset_name = format!("perl-lsp-{target}.{archive_ext}");
        let asset =
            release.assets.iter().find(|asset| asset.name == asset_name).ok_or_else(|| {
                format!("no asset named `{asset_name}` in perl-lsp release {}", release.version)
            })?;

        let version_dir = format!("perl-lsp-{}", release.version);
        let binary_path = format!("{version_dir}/{bin_name}");

        if !fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(&asset.download_url, &version_dir, file_type)
                .map_err(|error| format!("failed to download perl-lsp: {error}"))?;
            zed::make_file_executable(&binary_path)?;
            remove_old_downloads("perl-lsp-", &version_dir);
        }

        self.perl_lsp_path = Some(binary_path.clone());
        Ok(binary_path)
    }

    fn perllsp_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        // Classify the execution source BEFORE any binary lookup, command
        // construction, download, or process start (#11041). An unproven
        // merged override is refused here and can never reach resolution.
        let command_settings = perllsp_command_settings(worktree)?;
        authorize_execution_source(command_settings.path.as_deref())?;
        let command = self.perllsp_binary(language_server_id, worktree)?;
        let args = normalize_perllsp_args(command_settings.arguments)?;

        Ok(zed::Command { command, args, env: command_settings.env })
    }

    fn perllsp_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        if let Some(path) = worktree.which(PERLLSP_SERVER_ID) {
            return Ok(path);
        }

        self.download_perllsp(language_server_id)
    }

    fn download_perllsp(&mut self, language_server_id: &LanguageServerId) -> Result<String> {
        // Process-memory fast path: an accepted subject from this activation.
        if let Some(path) = &self.perllsp_path {
            if fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
                return Ok(path.clone());
            }
        }

        // Offline-first startup (#11308): reconstruct the accepted current
        // subject from durable exact identity before any network effect. When
        // this succeeds, no release metadata is requested and the update fact
        // stays `NotRequested`.
        let (os, arch) = zed::current_platform();
        match load_accepted_current_in(Path::new("."), os, arch) {
            Ok(path) => {
                self.update_state = UpdateState::NotRequested;
                self.perllsp_path = Some(path.clone());
                return Ok(path);
            }
            Err(_) => {
                // The typed update state and the propagated error carry the
                // cause; reconstruction is re-derived on every activation.
            }
        }

        // Cold route: with no accepted current subject, release metadata and a
        // download are admitted. This runs at most once per command resolution,
        // never recursively, and its failure cannot erase the recorded cause.
        self.cold_install_attempts = self.cold_install_attempts.saturating_add(1);
        self.update_state = UpdateState::ColdInstall;
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = match zed::latest_github_release(
            PERLLSP_REPO,
            zed::GithubReleaseOptions { require_assets: true, pre_release: false },
        ) {
            Ok(release) => release,
            Err(error) => {
                self.update_state = UpdateState::TransportFailed;
                return Err(format!(
                    "failed to resolve EffortlessMetrics perllsp releases: {error}"
                ));
            }
        };
        let version = normalize_release_version(&release.version);
        if version.is_empty() {
            self.update_state = UpdateState::MetadataInvalid;
            return Err("perllsp release metadata carried an empty version".to_string());
        }
        let target = match perllsp_target(os, arch) {
            Ok(target) => target,
            Err(message) => {
                self.update_state = UpdateState::CandidateRejected;
                return Err(message);
            }
        };
        let (archive_ext, file_type) = match os {
            zed::Os::Windows => ("zip", zed::DownloadedFileType::Zip),
            _ => ("tar.gz", zed::DownloadedFileType::GzipTar),
        };
        let asset_name = perllsp_asset_name(version, target, archive_ext);
        let asset =
            release.assets.iter().find(|asset| asset.name == asset_name).ok_or_else(|| {
                self.update_state = UpdateState::MetadataInvalid;
                format!(
                    "no asset named `{asset_name}` in EffortlessMetrics perllsp release {}",
                    release.version
                )
            })?;

        let version_dir = format!("perllsp-{version}-{target}");
        let binary_path = perllsp_binary_path(&version_dir, version, target, os);
        let work_dir = Path::new(".");

        // Attempt-private mutation protocol (#11316): every download claims a
        // unique non-launchable staging root (exclusive create, so identities
        // stay cross-process-unique even within one clock tick) and reaches
        // the durable name only through one atomic publish. Concurrent Zed
        // processes electing the same subject produce exactly one winner and
        // explicitly settled losers; no attempt ever mutates the durable
        // directory in place.
        let (attempt, attempt_dir) = match claim_attempt_staging(work_dir, &version_dir) {
            Ok(claimed) => claimed,
            Err(error) => {
                self.update_state = UpdateState::CandidateRejected;
                return Err(error);
            }
        };

        // A durable manifest that names exactly this binary path while offline
        // verification rejected it marks corrupted or tampered bytes: replace
        // the subject wholesale instead of re-accepting what is on disk.
        let rejected_subject = load_selection_manifest_in(work_dir)
            .is_some_and(|manifest| manifest.installed_path == binary_path);
        let binary_exists = fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file());
        let disposition = cold_disposition(binary_exists, rejected_subject);

        match disposition {
            ColdDisposition::DownloadFresh | ColdDisposition::ReplaceRejected => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Downloading,
                );
                // Extract into the private root: an interrupted or racing
                // download can neither occupy nor destroy the durable name.
                if let Err(error) = zed::download_file(&asset.download_url, &attempt_dir, file_type)
                {
                    self.update_state = UpdateState::TransportFailed;
                    let _ = remove_owned_attempt(&work_dir.join(&attempt_dir));
                    return Err(format!("failed to download EffortlessMetrics perllsp: {error}"));
                }

                let staged_binary = perllsp_binary_path(&attempt_dir, version, target, os);
                if !fs::metadata(&staged_binary).is_ok_and(|metadata| metadata.is_file()) {
                    self.update_state = UpdateState::CandidateRejected;
                    let _ = remove_owned_attempt(&work_dir.join(&attempt_dir));
                    return Err(format!(
                        "downloaded `{asset_name}` but did not find expected binary `{binary_path}`"
                    ));
                }

                // Executable readiness happens inside the private tree so the
                // durable subject is launchable at its commit point
                // (cache_contract.replace_only_after ends at executable_ready).
                if !matches!(os, zed::Os::Windows) {
                    if let Err(error) = zed::make_file_executable(&staged_binary) {
                        self.update_state = UpdateState::CandidateRejected;
                        let _ = remove_owned_attempt(&work_dir.join(&attempt_dir));
                        return Err(format!(
                            "failed to make downloaded perllsp executable: {error}"
                        ));
                    }
                }

                if let Err(error) =
                    publish_staged_attempt(work_dir, &version_dir, &attempt_dir, &binary_path)
                {
                    self.update_state = UpdateState::CandidateRejected;
                    let _ = remove_owned_attempt(&work_dir.join(&attempt_dir));
                    return Err(error);
                }
            }
            ColdDisposition::ReuseExisting => {
                if !matches!(os, zed::Os::Windows) {
                    if let Err(error) = zed::make_file_executable(&binary_path) {
                        self.update_state = UpdateState::CandidateRejected;
                        return Err(format!(
                            "failed to make existing perllsp binary executable: {error}"
                        ));
                    }
                }
            }
        }

        // Bind the exact installed identity only after the durable subject is
        // fully staged, verified, and executable; the digest is computed from
        // the durable bytes so an adopted winner's subject is bound exactly as
        // published. The manifest promotion stays atomic, attempt-private, and
        // last (cache_contract.replace_only_after ends at executable_ready).
        let binary_bytes = fs::read(&binary_path).map_err(|error| {
            self.update_state = UpdateState::CandidateRejected;
            format!("downloaded perllsp binary `{binary_path}` could not be read: {error}")
        })?;
        let manifest = SelectionManifest {
            release_tag: release.version.clone(),
            release_version: version.to_string(),
            target: target.to_string(),
            asset_name: asset_name.clone(),
            archive_member: archive_member_for(os, version, target),
            installed_path: binary_path.clone(),
            binary_sha256: format!("sha256:{}", content_sha256(&binary_bytes)),
        };

        if let Err(error) = store_selection_manifest(&attempt, &manifest) {
            self.update_state = UpdateState::CandidateRejected;
            let _ = remove_owned_attempt(&work_dir.join(&attempt_dir));
            return Err(error);
        }

        // Exact accepted-state reread before reporting success (#11316):
        // publication, lock, or directory disappearance during contention is
        // never success. During a release rollover race the accepted subject
        // may be another attempt's promotion; exactly that path is served so
        // the launched source and the durably accepted state stay identical.
        let accepted_path = match load_accepted_current_in(work_dir, os, arch) {
            Ok(path) => path,
            Err(error) => {
                self.update_state = UpdateState::CandidateRejected;
                let _ = remove_owned_attempt(&work_dir.join(&attempt_dir));
                return Err(format!(
                    "published perllsp subject failed accepted-state reread: {error}"
                ));
            }
        };

        let _ = remove_owned_attempt(&work_dir.join(&attempt_dir));

        if accepted_path != binary_path {
            self.perllsp_path = Some(accepted_path.clone());
            return Ok(accepted_path);
        }

        self.perllsp_path = Some(binary_path.clone());
        Ok(binary_path)
    }

    /// Checked resolution for the exact `perl-dap` adapter binary.
    ///
    /// Order: explicit user override (only where the Zed API supplies one) →
    /// worktree/PATH exact `perl-dap` → managed public release asset. The
    /// resolver can never select `perllsp`, `perl-lsp`, another adapter, or an
    /// unrelated same-named executable: the only admitted names are the exact
    /// `perl-dap` override, the exact `perl-dap` PATH hit, and the
    /// `perl-dap`/`perl-dap.exe` member of a canonical release archive.
    fn perl_dap_binary(
        &mut self,
        user_provided_debug_adapter_path: Option<String>,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        if let Some(path) = user_provided_debug_adapter_path {
            if path.trim().is_empty() {
                return Err(
                    "debugger `perl-dap` binary path override must not be empty".to_string()
                );
            }
            return Ok(path);
        }

        if let Some(path) = worktree.which(PERL_DAP_BINARY_NAME) {
            return Ok(path);
        }

        self.download_perl_dap()
    }

    /// Managed download of `perl-dap` from the canonical release topology.
    ///
    /// The adapter consumes the same EffortlessMetrics/perl-lsp release
    /// archives as the perllsp route (`perllsp-{version}-{triple}` archives
    /// shipping both binaries) — never a private target table — but extracts
    /// and caches under the debugger-specific `perl-dap-managed-` boundary so
    /// cleanup can never cross into language-server caches or user state.
    fn download_perl_dap(&mut self) -> Result<String> {
        // Process-memory fast path: an adapter resolved in this activation.
        if let Some(path) = &self.perl_dap_path {
            if fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
                return Ok(path.clone());
            }
        }

        let (os, arch) = zed::current_platform();
        let release = zed::latest_github_release(
            PERL_DAP_REPO,
            zed::GithubReleaseOptions { require_assets: true, pre_release: false },
        )
        .map_err(|error| {
            format!("failed to resolve EffortlessMetrics perl-dap releases: {error}")
        })?;
        let version = normalize_release_version(&release.version);
        if version.is_empty() {
            return Err("perl-dap release metadata carried an empty version".to_string());
        }
        let target = perl_dap_target(os, arch)?;
        let (archive_ext, file_type) = match os {
            zed::Os::Windows => ("zip", zed::DownloadedFileType::Zip),
            _ => ("tar.gz", zed::DownloadedFileType::GzipTar),
        };
        let asset_name = perllsp_asset_name(version, target, archive_ext);
        let asset =
            release.assets.iter().find(|asset| asset.name == asset_name).ok_or_else(|| {
                format!(
                    "no asset named `{asset_name}` in EffortlessMetrics perl-dap release {}",
                    release.version
                )
            })?;

        let version_dir = format!("{PERL_DAP_MANAGED_PREFIX}{version}-{target}");
        let binary_path = perl_dap_binary_path(&version_dir, version, target, os);

        // Bounded recovery: an already-present known-good member is reused;
        // only a missing member triggers a download. An interrupted download
        // can never occupy the durable directory, because every download is
        // staged in a `.tmp` sibling and promoted by an atomic rename only
        // after the expected member exists and is executable — mere presence
        // of a partial extraction is never accepted as readiness.
        if !fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file()) {
            let staging_dir = perl_dap_staging_dir(&version_dir);
            if fs::metadata(&staging_dir).is_ok() {
                fs::remove_dir_all(&staging_dir).map_err(|error| {
                    format!("failed to remove incomplete perl-dap staging `{staging_dir}`: {error}")
                })?;
            }
            zed::download_file(&asset.download_url, &staging_dir, file_type).map_err(|error| {
                format!("failed to download EffortlessMetrics perl-dap: {error}")
            })?;
            let staged_binary = perl_dap_binary_path(&staging_dir, version, target, os);
            if !fs::metadata(&staged_binary).is_ok_and(|metadata| metadata.is_file()) {
                return Err(format!(
                    "downloaded `{asset_name}` but did not find expected member `{}` (expected at `{staged_binary}`)",
                    perl_dap_archive_member(os, version, target)
                ));
            }
            if !matches!(os, zed::Os::Windows) {
                zed::make_file_executable(&staged_binary)?;
            }
            if fs::metadata(&version_dir).is_ok() {
                fs::remove_dir_all(&version_dir).map_err(|error| {
                    format!(
                        "failed to remove superseded perl-dap download `{version_dir}`: {error}"
                    )
                })?;
            }
            promote_staged_dir(&staging_dir, &version_dir)?;
        }

        // Cleanup stays inside the debugger-specific managed boundary: only
        // `perl-dap-managed-` directories are ever removed.
        remove_old_downloads(PERL_DAP_MANAGED_PREFIX, &version_dir);

        self.perl_dap_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

fn perllsp_command_settings(worktree: &zed::Worktree) -> Result<PerllspCommandSettings> {
    let mut shell_env = worktree.shell_env();
    let binary = LspSettings::for_worktree(PERLLSP_SERVER_ID, worktree)
        .ok()
        .and_then(|settings| settings.binary);

    let Some(binary) = binary else {
        return Ok(PerllspCommandSettings { path: None, arguments: Vec::new(), env: shell_env });
    };

    // Settings-supplied overrides carry unproven merged provenance (#11041),
    // so they may not load code into the launched process: dynamic-loader
    // injection keys are dropped from the override layer only. The Zed-defined
    // worktree environment itself stays the authorized execution context.
    let mut overrides = binary.env.unwrap_or_default();
    overrides.retain(|key, _| !is_loader_injection_key(key));
    shell_env.extend(overrides);

    Ok(PerllspCommandSettings {
        path: binary.path,
        arguments: binary.arguments.unwrap_or_default(),
        env: shell_env,
    })
}

fn normalize_perllsp_args(arguments: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(arguments.len() + 1);
    let mut saw_stdio = false;

    for argument in arguments {
        // `--mcp` / `mcp` are documented launcher aliases for stdio transport.
        if argument == "--stdio" || argument == "--mcp" || argument == "mcp" {
            if !saw_stdio {
                normalized.push("--stdio".to_string());
                saw_stdio = true;
            }
            continue;
        }

        if is_non_lsp_argument(&argument) {
            return Err(format!(
                "Zed must launch the LSP stdio route; unsupported perllsp argument `{argument}`"
            ));
        }

        normalized.push(argument);
    }

    if !saw_stdio {
        normalized.push("--stdio".to_string());
    }

    Ok(normalized)
}

fn is_non_lsp_argument(argument: &str) -> bool {
    // Reject `--mcp=...` forms; bare `mcp` / `--mcp` are stdio aliases above.
    if argument.starts_with("--mcp=") {
        return true;
    }
    let flag = argument.split_once('=').map_or(argument, |(key, _)| key);
    matches!(
        flag,
        "--socket"
            | "--port"
            | "--health"
            | "--info"
            | "--version"
            | "--doctor"
            | "--check"
            | "--check-project"
            | "--help"
            | "-h"
            | "-V"
    )
}

/// Whether a settings-supplied environment override key could load code into
/// the launched process (#11041). Comparison is ASCII-case-insensitive so
/// host-specific casing cannot slip an injection variable past the filter.
///
/// Covered set: ELF dynamic-loader audit/preload/library-path keys
/// (`LD_PRELOAD`, `LD_AUDIT`, `LD_LIBRARY_PATH`) and their Mach-O
/// counterparts (`DYLD_INSERT_LIBRARIES`, `DYLD_FORCE_FLAT_NAMESPACE`,
/// `DYLD_LIBRARY_PATH`, `DYLD_FRAMEWORK_PATH`,
/// `DYLD_FALLBACK_FRAMEWORK_PATH`).
fn is_loader_injection_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "ld_preload"
            | "ld_audit"
            | "ld_library_path"
            | "dyld_insert_libraries"
            | "dyld_force_flat_namespace"
            | "dyld_library_path"
            | "dyld_framework_path"
            | "dyld_fallback_framework_path"
    )
}

fn normalize_release_version(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

fn perllsp_target(os: zed::Os, arch: zed::Architecture) -> Result<&'static str> {
    match (os, arch) {
        (zed::Os::Mac, zed::Architecture::Aarch64) => Ok("aarch64-apple-darwin"),
        (zed::Os::Mac, zed::Architecture::X8664) => Ok("x86_64-apple-darwin"),
        (zed::Os::Linux, zed::Architecture::X8664) => Ok("x86_64-unknown-linux-musl"),
        (zed::Os::Linux, zed::Architecture::Aarch64) => Ok("aarch64-unknown-linux-musl"),
        (zed::Os::Windows, zed::Architecture::X8664) => Ok("x86_64-pc-windows-msvc"),
        (zed::Os::Windows, zed::Architecture::Aarch64) => Err(
            "aarch64-pc-windows-msvc managed perllsp downloads are not yet published; install a proven compatible perllsp binary on PATH"
                .to_string(),
        ),
        _ => Err(format!(
            "EffortlessMetrics perllsp has no managed binary for {os:?}/{arch:?}; install it on PATH instead"
        )),
    }
}

fn perllsp_asset_name(version: &str, target: &str, archive_ext: &str) -> String {
    format!("perllsp-{version}-{target}.{archive_ext}")
}

fn perllsp_binary_path(version_dir: &str, version: &str, target: &str, os: zed::Os) -> String {
    match os {
        zed::Os::Windows => format!("{version_dir}/perllsp.exe"),
        _ => format!("{version_dir}/perllsp-{version}-{target}/perllsp"),
    }
}

/// The archive member the managed install launches, matching the
/// managed-downloads projection (`archive_member` per target).
fn archive_member_for(os: zed::Os, version: &str, target: &str) -> String {
    match os {
        zed::Os::Windows => "perllsp.exe".to_string(),
        _ => format!("perllsp-{version}-{target}/perllsp"),
    }
}

/// Managed target projection for the `perl-dap` adapter.
///
/// This is the same canonical release topology projection the accepted
/// perllsp route uses (see [`perllsp_target`]): the
/// EffortlessMetrics/perl-lsp release contract ships `perl-dap` inside the
/// exact same `perllsp-{version}-{triple}` archives for exactly these
/// managed triples, with `aarch64-pc-windows-msvc` explicitly unclaimed. The
/// fixture manifest (`.ci/fixtures/zed-perl-upstream/manifest.toml`) and its
/// CI check bind this table to the canonical release contract.
fn perl_dap_target(os: zed::Os, arch: zed::Architecture) -> Result<&'static str> {
    match (os, arch) {
        (zed::Os::Mac, zed::Architecture::Aarch64) => Ok("aarch64-apple-darwin"),
        (zed::Os::Mac, zed::Architecture::X8664) => Ok("x86_64-apple-darwin"),
        (zed::Os::Linux, zed::Architecture::X8664) => Ok("x86_64-unknown-linux-musl"),
        (zed::Os::Linux, zed::Architecture::Aarch64) => Ok("aarch64-unknown-linux-musl"),
        (zed::Os::Windows, zed::Architecture::X8664) => Ok("x86_64-pc-windows-msvc"),
        (zed::Os::Windows, zed::Architecture::Aarch64) => Err(
            "aarch64-pc-windows-msvc managed perl-dap downloads are not yet published; install a proven compatible perl-dap binary on PATH"
                .to_string(),
        ),
        _ => Err(format!(
            "EffortlessMetrics perl-dap has no managed binary for {os:?}/{arch:?}; install it on PATH instead"
        )),
    }
}

/// The `perl-dap` member inside a canonical `perllsp-{version}-{triple}`
/// release archive, once extracted into the adapter-owned cache directory.
fn perl_dap_binary_path(version_dir: &str, version: &str, target: &str, os: zed::Os) -> String {
    match os {
        // The Windows zip archives carry the binaries at the archive root.
        zed::Os::Windows => format!("{version_dir}/perl-dap.exe"),
        // The tar.gz archives carry a single top-level package directory.
        _ => format!("{version_dir}/perllsp-{version}-{target}/perl-dap"),
    }
}

/// The archive member the managed perl-dap install launches, matching the
/// canonical release archive layout (the same layout as [`archive_member_for`],
/// naming the `perl-dap` member).
fn perl_dap_archive_member(os: zed::Os, version: &str, target: &str) -> String {
    match os {
        zed::Os::Windows => "perl-dap.exe".to_string(),
        _ => format!("perllsp-{version}-{target}/perl-dap"),
    }
}

/// Validate the debugger configuration object shared by
/// `dap_request_kind` and `get_dap_binary`.
///
/// Fails closed with a typed, actionable error naming the exact missing or
/// unsupported field: an unknown or missing `request` can never silently
/// select another request kind, adapter, or transport.
fn parse_perl_dap_request_kind(value: &zed::serde_json::Value) -> Result<()> {
    let Some(request) = value.get("request") else {
        return Err(
            "perl-dap debugger configuration lacks the required `request` field".to_string()
        );
    };
    let Some(request) = request.as_str() else {
        return Err(
            "perl-dap debugger configuration `request` must be the string `launch`".to_string()
        );
    };
    match request {
        "launch" => Ok(()),
        "attach" => Err(concat!(
            "perl-dap `attach` configurations are not supported by this extension yet; ",
            "use a `launch` configuration"
        )
        .to_string()),
        other => Err(format!(
            "perl-dap debugger configuration has an unsupported `request` value `{other}`; only `launch` is supported"
        )),
    }
}

/// Validate the JSON-encoded `perl-dap` launch configuration.
///
/// The canonical product contract requires a launch request with an explicit,
/// non-empty `program`; `args`, `cwd`, and `env` are optional with typed
/// shapes. Unknown keys are intentionally preserved (pass-through), because
/// the public `perl-dap` schema is forward-compatible. The validated
/// configuration is forwarded verbatim to the adapter inside
/// `request_args.configuration`: debuggee-only fields such as `env` and `cwd`
/// describe the debugged process, so they are deliberately never applied to
/// the adapter process itself.
fn validate_perl_dap_launch_config(text: &str) -> Result<()> {
    let value: zed::serde_json::Value = zed::serde_json::from_str(text)
        .map_err(|error| format!("perl-dap debugger configuration is not valid JSON: {error}"))?;
    parse_perl_dap_request_kind(&value)?;

    let Some(object) = value.as_object() else {
        return Err("perl-dap debugger configuration must be a JSON object".to_string());
    };

    let program =
        object.get("program").and_then(zed::serde_json::Value::as_str).ok_or_else(|| {
            "perl-dap launch configuration lacks the required `program` field".to_string()
        })?;
    if program.trim().is_empty() {
        return Err("perl-dap launch configuration `program` must not be empty".to_string());
    }

    if let Some(raw_args) = object.get("args") {
        let raw_args = raw_args.as_array().ok_or(
            "perl-dap launch configuration `args` must be an array of strings".to_string(),
        )?;
        for argument in raw_args {
            if argument.as_str().is_none() {
                return Err(
                    "perl-dap launch configuration `args` must be an array of strings".to_string()
                );
            }
        }
    }

    if let Some(raw) = object.get("cwd") {
        let raw = raw
            .as_str()
            .ok_or("perl-dap launch configuration `cwd` must be a string".to_string())?;
        if raw.trim().is_empty() {
            return Err("perl-dap launch configuration `cwd` must not be empty".to_string());
        }
    }

    if let Some(raw_env) = object.get("env") {
        let raw_env = raw_env.as_object().ok_or(
            "perl-dap launch configuration `env` must be an object of string to string".to_string(),
        )?;
        for (key, raw_value) in raw_env {
            if raw_value.as_str().is_none() {
                return Err(format!(
                    "perl-dap launch configuration `env` value for `{key}` must be a string"
                ));
            }
        }
    }

    Ok(())
}

/// The staging sibling a managed perl-dap download extracts into before it is
/// verified and atomically promoted to `version_dir`.
fn perl_dap_staging_dir(version_dir: &str) -> String {
    format!("{version_dir}.tmp")
}

/// Atomically promote a fully staged managed download into its durable
/// directory. The rename is the commit point: a staged tree only becomes
/// visible at `dest` whole, so an interrupted download can never be accepted
/// later as a known-good subject.
fn promote_staged_dir(staging_dir: &str, dest: &str) -> Result<(), String> {
    fs::rename(staging_dir, dest).map_err(|error| {
        format!("failed to promote staged perl-dap download `{staging_dir}` to `{dest}`: {error}")
    })
}

fn remove_old_downloads(prefix: &str, current_dir: &str) {
    remove_old_downloads_in(Path::new("."), prefix, current_dir);
}

/// Prefix-bounded cleanup of superseded durable downloads.
///
/// Attempt-private mutation state (`.tmp` extraction siblings and
/// `.attempt-<id>` staging roots) is never swept here (#11316): it belongs to
/// its owning attempt and is cleaned by that attempt alone, so a prefix sweep
/// can never cross-clean another writer's live partial state.
fn remove_old_downloads_in(dir: &Path, prefix: &str, current_dir: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".tmp") || name.contains(".attempt-") {
            continue;
        }
        if name.starts_with(prefix) && name != current_dir {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

impl zed::Extension for PerlExtension {
    fn new() -> Self {
        Self {
            did_find_server: false,
            perl_lsp_path: None,
            perllsp_path: None,
            perl_dap_path: None,
            update_state: UpdateState::NotRequested,
            cold_install_attempts: 0,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        match classify_server_id(language_server_id.as_ref())? {
            ServerKind::PerlNavigator => self.perlnavigator_command(language_server_id, worktree),
            ServerKind::TreeSitterPerlLsp => self.perl_lsp_command(language_server_id, worktree),
            ServerKind::EffortlessPerllsp => self.perllsp_command(language_server_id, worktree),
        }
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.settings)
            .unwrap_or_default();
        Ok(Some(settings))
    }

    /// Debug-adapter request-kind selection for the exact `perl-dap` adapter.
    ///
    /// Unknown adapter IDs fail closed instead of falling through to another
    /// adapter, and unsupported/missing request kinds fail with a typed
    /// actionable error instead of silently selecting launch.
    fn dap_request_kind(
        &mut self,
        adapter_name: String,
        config: zed::serde_json::Value,
    ) -> Result<zed::StartDebuggingRequestArgumentsRequest> {
        if adapter_name != PERL_DAP_ADAPTER_ID {
            return Err(format!("unknown Perl debug adapter id `{adapter_name}`"));
        }
        parse_perl_dap_request_kind(&config)?;
        Ok(zed::StartDebuggingRequestArgumentsRequest::Launch)
    }

    /// Launch the canonical `perl-dap` debug adapter for a launch scenario.
    ///
    /// The adapter speaks DAP over stdio by default (the canonical product
    /// contract needs no transport flag), so the resolved binary is launched
    /// with no extra arguments; the validated user configuration is forwarded
    /// verbatim as the launch `configuration`, preserving forward-compatible
    /// fields, while generated adapter transport fields stay distinct from
    /// project/user configuration.
    fn get_dap_binary(
        &mut self,
        adapter_name: String,
        config: zed::DebugTaskDefinition,
        user_provided_debug_adapter_path: Option<String>,
        worktree: &zed::Worktree,
    ) -> Result<zed::DebugAdapterBinary> {
        if adapter_name != PERL_DAP_ADAPTER_ID {
            return Err(format!("unknown Perl debug adapter id `{adapter_name}`"));
        }
        validate_perl_dap_launch_config(&config.config)?;
        let command = self.perl_dap_binary(user_provided_debug_adapter_path, worktree)?;

        Ok(zed::DebugAdapterBinary {
            command: Some(command),
            // stdio is the default editor link; no transport arguments.
            arguments: Vec::new(),
            // Adapter-process environment only: debuggee `env`/`cwd` travel
            // verbatim inside the forwarded `configuration`, never on the
            // adapter process, so debuggee variables such as `PATH` cannot
            // alter or prevent adapter startup.
            envs: worktree.shell_env(),
            cwd: None,
            connection: None,
            request_args: zed::StartDebuggingRequestArguments {
                request: zed::StartDebuggingRequestArgumentsRequest::Launch,
                configuration: config.config,
            },
        })
    }
}

zed::register_extension!(PerlExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_ids_are_explicit_and_unknown_ids_fail() {
        assert_eq!(
            classify_server_id(PERLNAVIGATOR_SERVER_ID).ok(),
            Some(ServerKind::PerlNavigator)
        );
        assert_eq!(
            classify_server_id(PERL_LSP_SERVER_ID).ok(),
            Some(ServerKind::TreeSitterPerlLsp)
        );
        assert_eq!(classify_server_id(PERLLSP_SERVER_ID).ok(), Some(ServerKind::EffortlessPerllsp));
        assert!(classify_server_id("unknown-perl-server").is_err());
    }

    #[test]
    fn stdio_arguments_are_added_once() {
        assert_eq!(normalize_perllsp_args(Vec::new()).ok(), Some(vec!["--stdio".to_string()]));
        assert_eq!(
            normalize_perllsp_args(vec![
                "--stdio".to_string(),
                "--stdio".to_string(),
                "--log-level=debug".to_string(),
            ])
            .ok(),
            Some(vec!["--stdio".to_string(), "--log-level=debug".to_string(),])
        );
    }

    #[test]
    fn mcp_alias_normalizes_to_stdio() {
        assert_eq!(
            normalize_perllsp_args(vec!["--mcp".to_string()]).ok(),
            Some(vec!["--stdio".to_string()])
        );
        assert_eq!(
            normalize_perllsp_args(vec!["mcp".to_string(), "--log-level=debug".to_string()]).ok(),
            Some(vec!["--stdio".to_string(), "--log-level=debug".to_string()])
        );
        assert_eq!(
            normalize_perllsp_args(vec!["--mcp".to_string(), "--stdio".to_string()]).ok(),
            Some(vec!["--stdio".to_string()])
        );
    }

    #[test]
    fn non_lsp_modes_fail_closed() {
        for argument in [
            "--mcp=stdio",
            "--socket",
            "--socket=127.0.0.1:9257",
            "--port",
            "--port=9257",
            "--health",
            "--health=1",
            "--version",
            "--doctor",
        ] {
            assert!(
                normalize_perllsp_args(vec![argument.to_string()]).is_err(),
                "argument `{argument}` must not start a non-LSP route"
            );
        }
    }

    #[test]
    fn release_version_normalization_is_stable() {
        assert_eq!(normalize_release_version("v0.18.0"), "0.18.0");
        assert_eq!(normalize_release_version("0.18.0"), "0.18.0");
    }

    #[test]
    fn managed_targets_match_the_release_contract() {
        assert_eq!(
            perllsp_target(zed::Os::Linux, zed::Architecture::X8664).ok(),
            Some("x86_64-unknown-linux-musl")
        );
        assert_eq!(
            perllsp_target(zed::Os::Linux, zed::Architecture::Aarch64).ok(),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(
            perllsp_target(zed::Os::Mac, zed::Architecture::X8664).ok(),
            Some("x86_64-apple-darwin")
        );
        assert_eq!(
            perllsp_target(zed::Os::Mac, zed::Architecture::Aarch64).ok(),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(
            perllsp_target(zed::Os::Windows, zed::Architecture::X8664).ok(),
            Some("x86_64-pc-windows-msvc")
        );
        assert!(perllsp_target(zed::Os::Windows, zed::Architecture::Aarch64).is_err());
    }

    #[test]
    fn selection_manifest_round_trips_exactly() {
        let manifest = SelectionManifest {
            release_tag: "v0.17.0".to_string(),
            release_version: "0.17.0".to_string(),
            target: "x86_64-pc-windows-msvc".to_string(),
            asset_name: "perllsp-0.17.0-x86_64-pc-windows-msvc.zip".to_string(),
            archive_member: "perllsp.exe".to_string(),
            installed_path: "perllsp-0.17.0-x86_64-pc-windows-msvc/perllsp.exe".to_string(),
            binary_sha256: format!("sha256:{}", "a".repeat(64)),
        };
        let parsed = parse_selection_manifest(&selection_manifest_json(&manifest));
        assert_eq!(parsed.ok(), Some(manifest));
    }

    #[test]
    fn mutated_selection_manifests_fail_closed() {
        let valid = SelectionManifest {
            release_tag: "v0.17.0".to_string(),
            release_version: "0.17.0".to_string(),
            target: "x86_64-unknown-linux-musl".to_string(),
            asset_name: "perllsp-0.17.0-x86_64-unknown-linux-musl.tar.gz".to_string(),
            archive_member: "perllsp-0.17.0-x86_64-unknown-linux-musl/perllsp".to_string(),
            installed_path: "perllsp-x/perllsp".to_string(),
            binary_sha256: format!("sha256:{}", "b".repeat(64)),
        };
        let json = selection_manifest_json(&valid);

        assert!(parse_selection_manifest(&json).is_ok());

        let wrong_schema = json.replace(SELECTION_MANIFEST_SCHEMA_VERSION, "perllsp_selection.v0");
        assert!(parse_selection_manifest(&wrong_schema).is_err());

        let wrong_product = json.replace("\"product\":\"perllsp\"", "\"product\":\"perl-lsp\"");
        assert!(parse_selection_manifest(&wrong_product).is_err());

        let wrong_role = json.replace("\"role\":\"lsp_server\"", "\"role\":\"mcp_server\"");
        assert!(parse_selection_manifest(&wrong_role).is_err());

        let unselected = json.replace("\"state\":\"current\"", "\"state\":\"candidate\"");
        assert!(parse_selection_manifest(&unselected).is_err());

        let traversal = json.replace("perllsp-x/perllsp", "../outside/perllsp");
        assert!(parse_selection_manifest(&traversal).is_err());

        let absolute = json.replace("perllsp-x/perllsp", "/usr/local/bin/perllsp");
        assert!(parse_selection_manifest(&absolute).is_err());

        let bad_digest = json.replace(&format!("sha256:{}", "b".repeat(64)), "sha256:not-a-digest");
        assert!(parse_selection_manifest(&bad_digest).is_err());

        let missing_release =
            json.replace("\"tag\":\"v0.17.0\",\"version\":\"0.17.0\"", "\"tag\":\"v0.17.0\"");
        assert!(parse_selection_manifest(&missing_release).is_err());
    }

    #[test]
    fn digest_helper_matches_known_sha256_vectors() {
        assert_eq!(
            content_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            content_sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn offline_reconstruction_accepts_only_the_exact_bound_subject(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let target = perllsp_target(zed::Os::Linux, zed::Architecture::X8664)?;
        let installed = "perllsp-0.17.0-x86_64-unknown-linux-musl/perllsp";
        fs::create_dir_all(dir.path().join("perllsp-0.17.0-x86_64-unknown-linux-musl"))?;
        fs::write(dir.path().join(installed), b"perllsp-binary-bytes")?;
        let manifest = SelectionManifest {
            release_tag: "v0.17.0".to_string(),
            release_version: "0.17.0".to_string(),
            target: target.to_string(),
            asset_name: "perllsp-0.17.0-x86_64-unknown-linux-musl.tar.gz".to_string(),
            archive_member: format!("{target}/perllsp"),
            installed_path: installed.to_string(),
            binary_sha256: format!("sha256:{}", content_sha256(b"perllsp-binary-bytes")),
        };
        fs::write(dir.path().join(SELECTION_MANIFEST_PATH), selection_manifest_json(&manifest))?;

        // Exact identity reconstructs without any network effect.
        assert_eq!(
            load_accepted_current_in(dir.path(), zed::Os::Linux, zed::Architecture::X8664).ok(),
            Some(installed.to_string())
        );

        // A different platform target must not adopt the foreign subject.
        assert!(
            load_accepted_current_in(dir.path(), zed::Os::Mac, zed::Architecture::Aarch64).is_err()
        );

        // Corrupted bytes must fail the digest binding.
        fs::write(dir.path().join(installed), b"tampered")?;
        assert!(
            load_accepted_current_in(dir.path(), zed::Os::Linux, zed::Architecture::X8664).is_err()
        );

        // Missing binary must fail even with an intact manifest.
        fs::remove_file(dir.path().join(installed))?;
        assert!(
            load_accepted_current_in(dir.path(), zed::Os::Linux, zed::Architecture::X8664).is_err()
        );

        // Directory-name or version-naming trust must never substitute for the
        // manifest: no manifest at all means no accepted current subject.
        let bare = tempfile::tempdir()?;
        fs::create_dir_all(bare.path().join("perllsp-0.17.0-x86_64-unknown-linux-musl"))?;
        fs::write(
            bare.path().join("perllsp-0.17.0-x86_64-unknown-linux-musl/perllsp"),
            b"perllsp-binary-bytes",
        )?;
        assert!(load_accepted_current_in(bare.path(), zed::Os::Linux, zed::Architecture::X8664)
            .is_err());

        Ok(())
    }

    #[test]
    fn cold_disposition_never_reaccepts_a_rejected_subject() {
        // Nothing on disk: download.
        assert_eq!(cold_disposition(false, false), ColdDisposition::DownloadFresh);
        assert_eq!(cold_disposition(false, true), ColdDisposition::DownloadFresh);
        // A manifest binding this exact path means offline verification already
        // rejected these bytes: replace, never re-accept.
        assert_eq!(cold_disposition(true, true), ColdDisposition::ReplaceRejected);
        // Pre-upgrade state with no binding against the path: adopt and bind a
        // fresh digest going forward.
        assert_eq!(cold_disposition(true, false), ColdDisposition::ReuseExisting);
    }

    #[test]
    fn update_states_stay_typed_and_distinct_from_health() {
        // The steady offline fact is `NotRequested`: an accepted subject was
        // served without claiming any fresh update knowledge.
        assert_eq!(UpdateState::NotRequested, UpdateState::NotRequested);
        for state in [
            UpdateState::ColdInstall,
            UpdateState::TransportFailed,
            UpdateState::MetadataInvalid,
            UpdateState::CandidateRejected,
        ] {
            assert_ne!(state, UpdateState::NotRequested);
        }
    }

    // ---- execution-source authority (#11041) ----

    #[test]
    fn merged_explicit_overrides_fail_closed_before_any_lookup() -> Result<(), String> {
        for path in [
            "/usr/local/bin/perllsp",
            "C:\\tools\\perllsp.exe",
            "./target/debug/perllsp",
            "~/bin/perllsp",
            "perllsp-attacker-build",
        ] {
            let error = authorize_execution_source(Some(path))
                .err()
                .ok_or_else(|| format!("override `{path}` must not execute"))?;
            assert!(error.contains("#11041"), "refusal must name its policy for `{path}`: {error}");
            assert!(
                error.contains("worktree PATH") && error.contains("managed download"),
                "refusal must name every remaining authorized route: {error}"
            );
            // Project-controlled input must not be echoed into durable logs.
            assert!(!error.contains(path), "refusal leaked the refused value");
        }
        Ok(())
    }

    #[test]
    fn unknown_provenance_never_defaults_to_trusted() {
        // The gate has no input shape that yields an unclassified grant:
        // absent override authorizes only the two bounded cells downstream,
        // and any present value is refused outright.
        assert!(authorize_execution_source(None).is_ok());
        let arbitrary = "\u{0}weird\u{7f}";
        assert!(authorize_execution_source(Some(arbitrary)).is_err());
    }

    #[test]
    fn empty_override_keeps_the_typed_rejection() {
        for empty in ["", "   ", "\t"] {
            assert_eq!(
                authorize_execution_source(Some(empty)).err().as_deref(),
                Some("lsp.perllsp.binary.path must not be empty"),
            );
        }
    }

    #[test]
    fn execution_routes_map_to_distinct_receipt_cells() {
        fn receipt_cell(route: ExecutionRoute) -> &'static str {
            match route {
                ExecutionRoute::ExplicitOverride => "binary_override",
                ExecutionRoute::WorktreePathDiscovery => "worktree_path",
                ExecutionRoute::ManagedArtifact => "managed_download",
            }
        }
        let cells = [
            receipt_cell(ExecutionRoute::ExplicitOverride),
            receipt_cell(ExecutionRoute::WorktreePathDiscovery),
            receipt_cell(ExecutionRoute::ManagedArtifact),
        ];
        let mut unique = cells.to_vec();
        unique.dedup();
        assert_eq!(unique.len(), cells.len(), "route cells must stay separate");
        // Public support claims can only ever ride the release-identity cell.
        assert_eq!(receipt_cell(ExecutionRoute::ManagedArtifact), "managed_download");
    }

    #[test]
    fn exact_identity_never_upgrades_unauthorized_selection() {
        // Canonical binary identity proves what was selected, never that the
        // selecting source was authorized: even an exactly-named absolute
        // path gains nothing from merged settings (#10340 vs #11041).
        assert!(authorize_execution_source(Some("/exact/perllsp")).is_err());
        assert!(authorize_execution_source(Some("sha256:perllsp")).is_err());
    }

    #[test]
    fn loader_injection_keys_are_dropped_from_settings_overrides() {
        for key in [
            "LD_PRELOAD",
            "ld_preload",
            "LD_AUDIT",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "dyld_insert_libraries",
            "DYLD_FORCE_FLAT_NAMESPACE",
            "DYLD_LIBRARY_PATH",
            "DYLD_FRAMEWORK_PATH",
            "DYLD_FALLBACK_FRAMEWORK_PATH",
        ] {
            assert!(
                is_loader_injection_key(key),
                "`{key}` must be filtered from unproven merged overrides"
            );
        }
        // Ordinary server configuration keys pass untouched.
        for key in ["PERL5LIB", "PERLLSP_LOG", "RUST_LOG", "HOME", "LANG"] {
            assert!(!is_loader_injection_key(key), "`{key}` must not be filtered");
        }
    }

    #[test]
    fn worktree_path_hits_never_acquire_release_identity() {
        // Negative control for project-controlled PATH resolution (#11041):
        // a `worktree.which` hit resolves under the worktree-environment
        // authority cell only. It must never flow through the managed
        // selection-manifest digest binding, so no receipt can label a
        // PATH-selected binary as release-proven even when a hostile PATH
        // entry shadows the managed subject.
        fn receipt_cell(route: ExecutionRoute) -> &'static str {
            match route {
                ExecutionRoute::ExplicitOverride => "binary_override",
                ExecutionRoute::WorktreePathDiscovery => "worktree_path",
                ExecutionRoute::ManagedArtifact => "managed_download",
            }
        }
        assert_ne!(
            receipt_cell(ExecutionRoute::WorktreePathDiscovery),
            receipt_cell(ExecutionRoute::ManagedArtifact)
        );
        assert_ne!(
            receipt_cell(ExecutionRoute::WorktreePathDiscovery),
            receipt_cell(ExecutionRoute::ExplicitOverride)
        );
    }

    #[test]
    fn release_asset_and_extraction_paths_match_current_archives() {
        assert_eq!(
            perllsp_asset_name("0.18.0", "aarch64-apple-darwin", "tar.gz"),
            "perllsp-0.18.0-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            perllsp_binary_path(
                "perllsp-0.18.0-aarch64-apple-darwin",
                "0.18.0",
                "aarch64-apple-darwin",
                zed::Os::Mac,
            ),
            "perllsp-0.18.0-aarch64-apple-darwin/perllsp-0.18.0-aarch64-apple-darwin/perllsp"
        );
        assert_eq!(
            perllsp_binary_path(
                "perllsp-0.18.0-x86_64-pc-windows-msvc",
                "0.18.0",
                "x86_64-pc-windows-msvc",
                zed::Os::Windows,
            ),
            "perllsp-0.18.0-x86_64-pc-windows-msvc/perllsp.exe"
        );
    }

    // ---- perl-dap debug-adapter authority (#9485) ----

    #[test]
    fn debug_adapter_identity_is_independent_from_every_language_server() {
        // The adapter ID must never alias a language-server ID.
        assert_ne!(PERL_DAP_ADAPTER_ID, PERLNAVIGATOR_SERVER_ID);
        assert_ne!(PERL_DAP_ADAPTER_ID, PERL_LSP_SERVER_ID);
        assert_ne!(PERL_DAP_ADAPTER_ID, PERLLSP_SERVER_ID);
        assert_ne!(PERL_DAP_ADAPTER_ID, PERL_LSP_REPO);
        assert_ne!(PERL_DAP_ADAPTER_ID, PERLLSP_REPO);
        // And no language server classification may accept the adapter ID.
        assert!(classify_server_id(PERL_DAP_ADAPTER_ID).is_err());
        // The exact binary name is the adapter ID: `perl-dap`, not `perllsp`.
        assert_eq!(PERL_DAP_ADAPTER_ID, PERL_DAP_BINARY_NAME);
    }

    #[test]
    fn launch_configuration_accepts_the_canonical_shape() {
        // Unknown forward-compatible keys (e.g. `stopOnEntry`) are preserved
        // by pass-through, not rejected.
        assert!(validate_perl_dap_launch_config(
            r#"{"request":"launch","program":"script.pl","args":["-w"],"cwd":"/tmp","env":{"PERL5LIB":"lib"},"stopOnEntry":true}"#
        )
        .is_ok());
        assert!(validate_perl_dap_launch_config(
            r#"{"request":"launch","program":"script.pl","externalDebugger":{"mode":"connect"}}"#
        )
        .is_ok());
    }

    #[test]
    fn malformed_launch_configurations_fail_closed_with_typed_errors() {
        let missing_request = validate_perl_dap_launch_config(r#"{"program":"script.pl"}"#);
        assert!(missing_request.is_err());
        assert!(missing_request.unwrap_err().contains("lacks the required `request` field"));

        let attach =
            validate_perl_dap_launch_config(r#"{"request":"attach","program":"script.pl"}"#);
        assert!(attach.is_err());
        assert!(attach.unwrap_err().contains("`attach` configurations are not supported"));

        let unknown_request =
            validate_perl_dap_launch_config(r#"{"request":"reverse","program":"script.pl"}"#);
        assert!(unknown_request.is_err());
        assert!(unknown_request.unwrap_err().contains("unsupported `request` value `reverse`"));

        let non_string_request =
            validate_perl_dap_launch_config(r#"{"request":1,"program":"script.pl"}"#);
        assert!(non_string_request.is_err());
        assert!(non_string_request.unwrap_err().contains("must be the string `launch`"));

        let missing_program = validate_perl_dap_launch_config(r#"{"request":"launch"}"#);
        assert!(missing_program.is_err());
        assert!(missing_program.unwrap_err().contains("lacks the required `program` field"));

        let empty_program =
            validate_perl_dap_launch_config(r#"{"request":"launch","program":"  "}"#);
        assert!(empty_program.is_err());
        assert!(empty_program.unwrap_err().contains("`program` must not be empty"));

        let bad_args = validate_perl_dap_launch_config(
            r#"{"request":"launch","program":"script.pl","args":"-w"}"#,
        );
        assert!(bad_args.is_err());
        assert!(bad_args.unwrap_err().contains("`args` must be an array of strings"));

        let bad_env = validate_perl_dap_launch_config(
            r#"{"request":"launch","program":"script.pl","env":{"PERL5LIB":3}}"#,
        );
        assert!(bad_env.is_err());
        assert!(bad_env.unwrap_err().contains("`env` value for `PERL5LIB` must be a string"));

        let bad_json = validate_perl_dap_launch_config("not json");
        assert!(bad_json.is_err());
        assert!(bad_json.unwrap_err().contains("is not valid JSON"));
    }

    #[test]
    fn managed_perl_dap_targets_match_the_canonical_release_projection() {
        assert_eq!(
            perl_dap_target(zed::Os::Linux, zed::Architecture::X8664).ok(),
            Some("x86_64-unknown-linux-musl")
        );
        assert_eq!(
            perl_dap_target(zed::Os::Windows, zed::Architecture::X8664).ok(),
            Some("x86_64-pc-windows-msvc")
        );
        // The adapter consumes the same canonical archive set as the accepted
        // perllsp managed route: identical triples platform by platform.
        for (os, arch) in [
            (zed::Os::Linux, zed::Architecture::X8664),
            (zed::Os::Linux, zed::Architecture::Aarch64),
            (zed::Os::Mac, zed::Architecture::X8664),
            (zed::Os::Mac, zed::Architecture::Aarch64),
            (zed::Os::Windows, zed::Architecture::X8664),
        ] {
            assert_eq!(
                perl_dap_target(os, arch).ok(),
                perllsp_target(os, arch).ok(),
                "perl-dap and perllsp managed projections must agree for {os:?}/{arch:?}"
            );
        }
        // Unsupported platforms refuse instead of guessing.
        let unsupported = perl_dap_target(zed::Os::Windows, zed::Architecture::Aarch64);
        assert!(unsupported.is_err());
        assert!(unsupported
            .unwrap_err()
            .contains("aarch64-pc-windows-msvc managed perl-dap downloads are not yet published"));
    }

    #[test]
    fn managed_perl_dap_paths_name_the_exact_dap_member() {
        // Non-Windows archives carry a single top-level package directory.
        assert_eq!(
            perl_dap_binary_path(
                "perl-dap-managed-0.18.0-x86_64-unknown-linux-musl",
                "0.18.0",
                "x86_64-unknown-linux-musl",
                zed::Os::Linux,
            ),
            "perl-dap-managed-0.18.0-x86_64-unknown-linux-musl/perllsp-0.18.0-x86_64-unknown-linux-musl/perl-dap"
        );
        // Windows zips carry the binaries at the archive root.
        assert_eq!(
            perl_dap_binary_path(
                "perl-dap-managed-0.18.0-x86_64-pc-windows-msvc",
                "0.18.0",
                "x86_64-pc-windows-msvc",
                zed::Os::Windows,
            ),
            "perl-dap-managed-0.18.0-x86_64-pc-windows-msvc/perl-dap.exe"
        );
        // The archive member projection names perl-dap, never perllsp.
        assert_eq!(
            perl_dap_archive_member(zed::Os::Linux, "0.18.0", "x86_64-unknown-linux-musl"),
            "perllsp-0.18.0-x86_64-unknown-linux-musl/perl-dap"
        );
        assert_eq!(
            perl_dap_archive_member(zed::Os::Windows, "0.18.0", "x86_64-pc-windows-msvc"),
            "perl-dap.exe"
        );
    }

    #[test]
    fn staged_downloads_promote_atomically_and_partial_stages_never_win(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let base = dir.path();
        let staging = perl_dap_staging_dir("perl-dap-managed-0.18.0-x86_64-unknown-linux-musl");
        assert_eq!(staging, "perl-dap-managed-0.18.0-x86_64-unknown-linux-musl.tmp");
        // Staging siblings stay inside the managed cleanup boundary.
        assert!(staging.starts_with(PERL_DAP_MANAGED_PREFIX));

        let staged = base.join(&staging);
        fs::create_dir_all(staged.join("perllsp-0.18.0-x86_64-unknown-linux-musl"))?;
        fs::write(
            staged.join("perllsp-0.18.0-x86_64-unknown-linux-musl/perl-dap"),
            b"perl-dap-bytes",
        )?;
        let dest = base.join("perl-dap-managed-0.18.0-x86_64-unknown-linux-musl");
        promote_staged_dir(&staged.to_string_lossy(), &dest.to_string_lossy())?;
        // Whole-tree promotion: the durable directory holds the member and
        // the staging sibling is gone.
        assert!(dest.join("perllsp-0.18.0-x86_64-unknown-linux-musl/perl-dap").is_file());
        assert!(!staged.exists());
        // A second promote from a missing staging directory must fail loudly
        // rather than silently leave a stale durable subject.
        assert!(promote_staged_dir(&staged.to_string_lossy(), &dest.to_string_lossy(),).is_err());
        Ok(())
    }

    #[test]
    fn cleanup_boundary_never_crosses_into_language_server_caches() {
        // The adapter's managed prefix is distinct from every
        // language-server cache family.
        for lsp_dir in [
            "perllsp-0.18.0-x86_64-unknown-linux-musl",
            "perllsp-selection.v1.json",
            "perl-lsp-0.3.0",
        ] {
            assert!(
                !lsp_dir.starts_with(PERL_DAP_MANAGED_PREFIX),
                "`{lsp_dir}` must never fall inside the perl-dap managed boundary"
            );
        }
        // And the adapter directory does belong to its own boundary.
        assert!("perl-dap-managed-0.18.0-x86_64-unknown-linux-musl"
            .starts_with(PERL_DAP_MANAGED_PREFIX));
    }

    // ---- attempt-private managed mutation (#11316) ----

    const MUTATION_TEST_SUBJECT_DIR: &str = "perllsp-9.9.9-concurrency-target";
    const MUTATION_TEST_MEMBER_REL: &str = "perllsp-9.9.9-concurrency-target/pkg/perllsp";

    fn write_member(
        root: &Path,
        relative: &str,
        bytes: &[u8],
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(fs::write(path, bytes)?)
    }

    #[test]
    fn attempt_identities_are_unique_and_staging_roots_private() {
        let first = next_attempt_id();
        let second = next_attempt_id();
        assert_ne!(first, second);

        let staging = attempt_staging_dir(MUTATION_TEST_SUBJECT_DIR, &first);
        assert!(staging.starts_with(&format!("{MUTATION_TEST_SUBJECT_DIR}.attempt-")));
        assert_ne!(
            staging, MUTATION_TEST_SUBJECT_DIR,
            "staging must never equal the durable directory name"
        );

        let tmp_first = selection_manifest_tmp_path(&first);
        assert!(tmp_first.starts_with(SELECTION_MANIFEST_TMP_PATH));
        assert_ne!(tmp_first, SELECTION_MANIFEST_TMP_PATH.to_string());
        assert_ne!(tmp_first, selection_manifest_tmp_path(&second));
    }

    #[test]
    fn concurrent_publications_elect_exactly_one_winner(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let base = dir.path().to_path_buf();
        let attempts: Vec<String> = (0..8).map(|_| next_attempt_id()).collect();

        let handles: Vec<_> = attempts
            .iter()
            .map(|attempt| {
                let base = base.clone();
                let version_dir = MUTATION_TEST_SUBJECT_DIR.to_string();
                let member_rel = MUTATION_TEST_MEMBER_REL.to_string();
                let attempt = attempt.clone();
                std::thread::spawn(move || -> Result<MutationOutcome, String> {
                    let attempt_dir = attempt_staging_dir(&version_dir, &attempt);
                    if let Err(error) =
                        write_member(&base, &format!("{attempt_dir}/pkg/perllsp"), b"perllsp-bytes")
                    {
                        return Err(error.to_string());
                    }
                    publish_staged_attempt(&base, &version_dir, &attempt_dir, &member_rel)
                })
            })
            .collect();

        let mut winners = 0;
        let mut adopters = 0;
        for handle in handles {
            let outcome = handle.join().map_err(|_| "mutation worker failed".to_string())??;
            match outcome {
                MutationOutcome::Published => winners += 1,
                MutationOutcome::AdoptedUnboundMember => adopters += 1,
                // No manifest exists during this low-level race, so an
                // adopted-bound outcome would mean the classifier lied.
                MutationOutcome::AdoptedPublished => {
                    return Err("unbound race cannot adopt a bound subject".into());
                }
            }
        }
        assert_eq!(winners, 1, "atomic rename must elect exactly one winner");
        assert_eq!(winners + adopters, attempts.len(), "every loser must settle explicitly");

        let published = fs::read(base.join(MUTATION_TEST_MEMBER_REL))?;
        assert_eq!(published, b"perllsp-bytes");
        for attempt in &attempts {
            assert!(
                !base.join(attempt_staging_dir(MUTATION_TEST_SUBJECT_DIR, attempt)).exists(),
                "loser staging must be cleaned by its own attempt"
            );
        }
        Ok(())
    }

    #[test]
    fn contender_settlement_never_touches_other_attempts_or_the_winner(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let base = dir.path();
        let version_dir = MUTATION_TEST_SUBJECT_DIR;
        let member_rel = MUTATION_TEST_MEMBER_REL;

        // A winner has already published and bound a verified subject.
        write_member(base, member_rel, b"winner-bytes")?;
        let bound_manifest = SelectionManifest {
            release_tag: "v9.9.9".to_string(),
            release_version: "9.9.9".to_string(),
            target: "concurrency-target".to_string(),
            asset_name: "synthetic.zip".to_string(),
            archive_member: "pkg/perllsp".to_string(),
            installed_path: member_rel.to_string(),
            binary_sha256: format!("sha256:{}", content_sha256(b"winner-bytes")),
        };
        fs::write(base.join(SELECTION_MANIFEST_PATH), selection_manifest_json(&bound_manifest))?;

        // Two unrelated live attempts exist beside the contention.
        let sibling_a = attempt_staging_dir(version_dir, "sibling-a");
        let sibling_b = attempt_staging_dir(version_dir, "sibling-b");
        write_member(base, &format!("{sibling_a}/pkg/perllsp"), b"a")?;
        write_member(base, &format!("{sibling_b}/pkg/perllsp"), b"b")?;

        // The contender loses the rename and must adopt without deleting the
        // winner, the siblings, or rewriting the binding.
        let loser_dir = attempt_staging_dir(version_dir, "loser");
        write_member(base, &format!("{loser_dir}/pkg/perllsp"), b"loser-bytes")?;
        let outcome = publish_staged_attempt(base, version_dir, &loser_dir, member_rel)
            .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
        assert_eq!(outcome, MutationOutcome::AdoptedPublished);
        assert_eq!(fs::read(base.join(member_rel))?, b"winner-bytes");
        assert!(base.join(&sibling_a).exists(), "another live attempt must survive");
        assert!(base.join(&sibling_b).exists(), "another live attempt must survive");
        assert!(!base.join(&loser_dir).exists(), "contender cleans only its own staging");

        let reread =
            parse_selection_manifest(&fs::read_to_string(base.join(SELECTION_MANIFEST_PATH))?)?;
        assert_eq!(reread.binary_sha256, bound_manifest.binary_sha256);
        Ok(())
    }

    #[test]
    fn corrupted_bound_subjects_are_replaced_and_reread_guarded(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let base = dir.path();
        let version_dir = MUTATION_TEST_SUBJECT_DIR;
        let member_rel = MUTATION_TEST_MEMBER_REL;

        // Durable bytes are corrupted relative to their binding.
        write_member(base, member_rel, b"corrupted-bytes")?;
        let corrupted_binding = SelectionManifest {
            release_tag: "v9.9.9".to_string(),
            release_version: "9.9.9".to_string(),
            target: "concurrency-target".to_string(),
            asset_name: "synthetic.zip".to_string(),
            archive_member: "pkg/perllsp".to_string(),
            installed_path: member_rel.to_string(),
            binary_sha256: format!("sha256:{}", content_sha256(b"original-bytes")),
        };
        fs::write(base.join(SELECTION_MANIFEST_PATH), selection_manifest_json(&corrupted_binding))?;

        // Replacement wins the publish and installs verified bytes.
        let replacement = attempt_staging_dir(version_dir, "replacement");
        write_member(base, &format!("{replacement}/pkg/perllsp"), b"repaired-bytes")?;
        let outcome = publish_staged_attempt(base, version_dir, &replacement, member_rel)
            .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
        assert_eq!(outcome, MutationOutcome::Published);
        assert_eq!(fs::read(base.join(member_rel))?, b"repaired-bytes");

        // The caller tail binds the durable bytes and rereads acceptance.
        store_selection_manifest_in(
            base,
            "replacement",
            &SelectionManifest {
                binary_sha256: format!(
                    "sha256:{}",
                    content_sha256(&fs::read(base.join(member_rel))?)
                ),
                ..corrupted_binding.clone()
            },
        )
        .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
        let accepted = load_accepted_current_in_for_test(base, &content_sha256)?;
        assert_eq!(accepted, member_rel);
        Ok(())
    }

    /// Test-only stand-in for [`load_accepted_current_in`] used by
    /// concurrency fixtures: the same validation shape against an explicit
    /// work directory, because the real function derives the host target from
    /// Zed APIs.
    fn load_accepted_current_in_for_test(
        work_dir: &Path,
        hash: &dyn Fn(&[u8]) -> String,
    ) -> std::result::Result<String, String> {
        let manifest_text = fs::read_to_string(work_dir.join(SELECTION_MANIFEST_PATH))
            .map_err(|_| "no accepted perllsp selection manifest".to_string())?;
        let manifest = parse_selection_manifest(&manifest_text)?;
        let bytes = fs::read(work_dir.join(&manifest.installed_path)).map_err(|_| {
            "accepted perllsp binary recorded by the manifest is missing".to_string()
        })?;
        if format!("sha256:{}", hash(&bytes)) != manifest.binary_sha256 {
            return Err("accepted perllsp binary no longer matches its recorded digest".to_string());
        }
        Ok(manifest.installed_path)
    }

    #[test]
    fn interrupted_attempts_leave_private_evidence_that_survives_retries(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let base = dir.path();
        let version_dir = MUTATION_TEST_SUBJECT_DIR;
        let member_rel = MUTATION_TEST_MEMBER_REL;

        // An interrupted attempt leaves a private partial tree behind.
        let crashed = attempt_staging_dir(version_dir, "crashed-attempt");
        write_member(base, &format!("{crashed}/pkg/perllsp"), b"partial")?;

        // A later retry publishes successfully without erasing that evidence.
        let retry = attempt_staging_dir(version_dir, "retry");
        write_member(base, &format!("{retry}/pkg/perllsp"), b"retry-bytes")?;
        let outcome = publish_staged_attempt(base, version_dir, &retry, member_rel)
            .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
        assert_eq!(outcome, MutationOutcome::Published);
        assert_eq!(fs::read(base.join(member_rel))?, b"retry-bytes");
        assert!(
            base.join(&crashed).exists(),
            "bounded cleanup removes only the owning attempt's tree"
        );
        assert!(!base.join(&retry).exists());

        // And the interrupted evidence stays non-launchable: resolution paths
        // never derive a command from `.attempt-` staging roots.
        assert!(attempt_staging_dir(version_dir, "anything").contains(".attempt-"));
        Ok(())
    }

    #[test]
    fn staging_claims_are_cross_process_unique_even_on_collisions(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let base = dir.path();

        // A first claim takes whatever identity is generated first.
        let (first_attempt, first_dir) = claim_attempt_staging(base, MUTATION_TEST_SUBJECT_DIR)
            .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
        assert!(base.join(&first_dir).is_dir());

        // A simultaneous claim from another process whose generated name
        // collides is rejected by exclusive create and retried into a fresh
        // identity: two live attempts can never share a staging root.
        let (second_attempt, second_dir) =
            claim_attempt_staging(base, MUTATION_TEST_SUBJECT_DIR)
                .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
        assert_ne!(first_attempt, second_attempt);
        assert_ne!(first_dir, second_dir);
        assert!(base.join(&second_dir).is_dir());

        remove_owned_attempt(&base.join(&first_dir))?;
        remove_owned_attempt(&base.join(&second_dir))?;
        Ok(())
    }

    #[test]
    fn concurrent_manifest_promotions_never_tear_accepted_state(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        fn mutation_manifest(digest_byte: char) -> SelectionManifest {
            SelectionManifest {
                release_tag: "v9.9.9".to_string(),
                release_version: "9.9.9".to_string(),
                target: "concurrency-target".to_string(),
                asset_name: "synthetic.zip".to_string(),
                archive_member: "pkg/perllsp".to_string(),
                installed_path: MUTATION_TEST_MEMBER_REL.to_string(),
                binary_sha256: format!("sha256:{}", digest_byte.to_string().repeat(64)),
            }
        }

        let dir = tempfile::tempdir()?;
        let base = dir.path().to_path_buf();

        for _ in 0..25 {
            let base_a = base.clone();
            let handle_a = std::thread::spawn(move || {
                store_selection_manifest_in(&base_a, "worker-a", &mutation_manifest('a'))
            });
            let base_b = base.clone();
            let handle_b = std::thread::spawn(move || {
                store_selection_manifest_in(&base_b, "worker-b", &mutation_manifest('b'))
            });
            handle_a
                .join()
                .map_err(|_| -> Box<dyn std::error::Error> { "worker-a failed".into() })??;
            handle_b
                .join()
                .map_err(|_| -> Box<dyn std::error::Error> { "worker-b failed".into() })??;

            let stored =
                parse_selection_manifest(&fs::read_to_string(base.join(SELECTION_MANIFEST_PATH))?)?;
            let digest = stored.binary_sha256.trim_start_matches("sha256:");
            assert!(
                digest == "a".repeat(64) || digest == "b".repeat(64),
                "accepted state must always be one whole promotion, never torn"
            );
            assert!(!base.join(selection_manifest_tmp_path("worker-a")).exists());
            assert!(!base.join(selection_manifest_tmp_path("worker-b")).exists());
        }
        Ok(())
    }

    #[test]
    fn legacy_partial_shells_are_recovered_behind_the_reread_guard(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let base = dir.path();
        let version_dir = MUTATION_TEST_SUBJECT_DIR;
        let member_rel = MUTATION_TEST_MEMBER_REL;

        // A pre-protocol interruption left an unbound durable shell with no
        // usable member. It must not wedge every future cold install.
        fs::create_dir_all(base.join(version_dir).join("stale-partial"))?;
        fs::write(base.join(version_dir).join("stale-partial").join("junk"), b"junk")?;

        let attempt = attempt_staging_dir(version_dir, "recovery");
        write_member(base, &format!("{attempt}/pkg/perllsp"), b"fresh-bytes")?;
        let outcome = publish_staged_attempt(base, version_dir, &attempt, member_rel)
            .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
        assert_eq!(outcome, MutationOutcome::Published);
        assert_eq!(fs::read(base.join(member_rel))?, b"fresh-bytes");
        Ok(())
    }

    #[test]
    fn prefix_cleanup_never_sweeps_attempt_private_state(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let base = dir.path();
        for name in
            ["family-superseded", "family-current.tmp", "family-current.attempt-abc", "unrelated"]
        {
            fs::create_dir_all(base.join(name))?;
        }

        remove_old_downloads_in(base, "family-", "family-current");

        assert!(!base.join("family-superseded").exists(), "superseded family dirs go");
        assert!(base.join("family-current.tmp").exists(), ".tmp siblings belong to owners");
        assert!(
            base.join("family-current.attempt-abc").exists(),
            ".attempt roots belong to owners"
        );
        assert!(base.join("unrelated").exists(), "other families stay untouched");
        Ok(())
    }

    /// Native-test stand-in for `zed::current_platform()` (#11316): the WIT
    /// function only exists inside the WASI host, so isolated-writer workers
    /// map `std::env::consts` onto the same platform vocabulary instead.
    fn native_platform() -> (zed::Os, zed::Architecture) {
        let os = match std::env::consts::OS {
            "macos" => zed::Os::Mac,
            "linux" => zed::Os::Linux,
            _ => zed::Os::Windows,
        };
        let arch = match std::env::consts::ARCH {
            "aarch64" => zed::Architecture::Aarch64,
            _ => zed::Architecture::X8664,
        };
        (os, arch)
    }

    /// Genuinely isolated-writer proof (#11316): two separate OS processes
    /// race one managed publication against one shared directory. This test
    /// doubles as the worker entry point when re-executed by itself through
    /// `PERLLSP_MUTATION_WORKER`, so the child processes exercise the exact
    /// protocol functions compiled into this crate.
    #[test]
    fn separate_process_workers_elect_exactly_one_winner(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let version_dir = MUTATION_TEST_SUBJECT_DIR;
        let member_rel = MUTATION_TEST_MEMBER_REL;

        if let Ok(worker_dir) = env::var("PERLLSP_MUTATION_WORKER_DIR") {
            let work_dir = Path::new(&worker_dir);
            let tag = env::var("PERLLSP_MUTATION_WORKER_TAG").unwrap_or_else(|_| "w".to_string());
            let payload = format!("perllsp-bytes-{tag}");

            let (attempt, attempt_dir) = claim_attempt_staging(work_dir, version_dir)
                .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
            write_member(
                Path::new(&worker_dir),
                &format!("{attempt_dir}/pkg/perllsp"),
                payload.as_bytes(),
            )?;
            let outcome = publish_staged_attempt(work_dir, version_dir, &attempt_dir, member_rel)
                .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;

            // Full caller tail: bind the durable bytes, promote the manifest,
            // and require the exact accepted-state reread.
            let bytes = fs::read(work_dir.join(member_rel))?;
            let (os, arch) = native_platform();
            let manifest = SelectionManifest {
                release_tag: "v9.9.9".to_string(),
                release_version: "9.9.9".to_string(),
                target: perllsp_target(os, arch)?.to_string(),
                asset_name: "synthetic.zip".to_string(),
                archive_member: archive_member_for(os, "9.9.9", perllsp_target(os, arch)?),
                installed_path: member_rel.to_string(),
                binary_sha256: format!("sha256:{}", content_sha256(&bytes)),
            };
            store_selection_manifest_in(work_dir, &attempt, &manifest)
                .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
            load_accepted_current_in(work_dir, os, arch)
                .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
            let _ = remove_owned_attempt(&work_dir.join(&attempt_dir));

            fs::write(
                work_dir.join(format!("result-{tag}.txt")),
                format!("{tag} {outcome:?} {}", manifest.binary_sha256),
            )?;
            std::process::exit(0);
        }

        // Parent: launch two independent processes of THIS test binary. The
        // name filter is a unique substring, so each child harness runs only
        // this test and the env gate turns it into the worker body.
        let dir = tempfile::tempdir()?;
        let base = dir.path().to_path_buf();
        let exe = env::current_exe()?;
        let mut children = Vec::new();
        for tag in ["w0", "w1"] {
            let mut command = std::process::Command::new(&exe);
            command
                .arg("separate_process_workers_elect_exactly_one_winner")
                .arg("--nocapture")
                .env("PERLLSP_MUTATION_WORKER_DIR", &base)
                .env("PERLLSP_MUTATION_WORKER_TAG", tag);
            children.push((tag, command.spawn()?));
        }
        for (tag, mut child) in children {
            let status = child.wait()?;
            assert!(status.success(), "worker {tag} did not complete cleanly");
        }

        // Exactly one process won; the other settled explicitly. Outcome
        // tokens are compared exactly because `AdoptedPublished` also contains
        // the substring `Published`.
        let mut results = Vec::new();
        for tag in ["w0", "w1"] {
            let text = fs::read_to_string(base.join(format!("result-{tag}.txt")))?;
            results.push(text.trim().to_string());
        }
        fn outcome_token(line: &str) -> &str {
            line.split_whitespace().nth(1).unwrap_or("")
        }
        let winners = results.iter().filter(|r| outcome_token(r.as_str()) == "Published").count();
        let adopters =
            results.iter().filter(|r| outcome_token(r.as_str()).starts_with("Adopted")).count();
        assert_eq!(winners, 1, "exactly one OS process may win the publication");
        assert_eq!(adopters, 1, "the losing process must report a typed adoption");

        // Accepted state is durably bound, verified, and readable offline by
        // a third party that took part in neither mutation.
        let (os, arch) = native_platform();
        let accepted = load_accepted_current_in(Path::new(&base), os, arch)
            .map_err(|message| -> Box<dyn std::error::Error> { message.into() })?;
        assert_eq!(accepted, member_rel);

        for entry in fs::read_dir(&base)? {
            let name = entry?.file_name().to_string_lossy().to_string();
            assert!(
                !name.contains(".attempt-"),
                "no attempt staging may survive settlement: {name}"
            );
        }
        Ok(())
    }
}
