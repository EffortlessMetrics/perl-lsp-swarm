use std::env;
use std::fs;
use std::path::{Component, Path};

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

/// Write the selection manifest atomically: stage to a temporary path, then
/// rename over the durable name so a partial write can never become accepted.
fn store_selection_manifest(manifest: &SelectionManifest) -> Result<(), String> {
    fs::write(SELECTION_MANIFEST_TMP_PATH, selection_manifest_json(manifest))
        .map_err(|error| format!("failed to stage selection manifest: {error}"))?;
    fs::rename(SELECTION_MANIFEST_TMP_PATH, SELECTION_MANIFEST_PATH).map_err(|error| {
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
        let command_settings = perllsp_command_settings(worktree)?;
        let command = match command_settings.path.as_deref() {
            Some(path) if path.trim().is_empty() => {
                return Err("lsp.perllsp.binary.path must not be empty".to_string());
            }
            Some(path) => path.to_string(),
            None => self.perllsp_binary(language_server_id, worktree)?,
        };
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

        // A durable manifest that names exactly this binary path while offline
        // verification rejected it marks corrupted or tampered bytes: replace
        // the subject wholesale instead of re-accepting what is on disk.
        let rejected_subject = load_selection_manifest_in(Path::new("."))
            .is_some_and(|manifest| manifest.installed_path == binary_path);
        let binary_exists = fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file());
        let disposition = cold_disposition(binary_exists, rejected_subject);

        match disposition {
            ColdDisposition::DownloadFresh | ColdDisposition::ReplaceRejected => {
                if fs::metadata(&version_dir).is_ok() {
                    fs::remove_dir_all(&version_dir).map_err(|error| {
                        format!(
                            "failed to remove incomplete perllsp download `{version_dir}`: {error}"
                        )
                    })?;
                }

                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Downloading,
                );
                if let Err(error) = zed::download_file(&asset.download_url, &version_dir, file_type)
                {
                    self.update_state = UpdateState::TransportFailed;
                    return Err(format!("failed to download EffortlessMetrics perllsp: {error}"));
                }

                if !fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file()) {
                    self.update_state = UpdateState::CandidateRejected;
                    return Err(format!(
                        "downloaded `{asset_name}` but did not find expected binary `{binary_path}`"
                    ));
                }
            }
            ColdDisposition::ReuseExisting => {}
        }

        // Bind the exact installed identity only after the candidate is fully
        // staged, verified, and executable on disk; the manifest promotion is
        // atomic and last, so a partial install can never become accepted
        // current (cache_contract.replace_only_after ends at executable_ready).
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

        if !matches!(os, zed::Os::Windows) {
            if let Err(error) = zed::make_file_executable(&binary_path) {
                self.update_state = UpdateState::CandidateRejected;
                return Err(format!("failed to make downloaded perllsp executable: {error}"));
            }
        }

        if let Err(error) = store_selection_manifest(&manifest) {
            self.update_state = UpdateState::CandidateRejected;
            return Err(error);
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

    if let Some(overrides) = binary.env {
        shell_env.extend(overrides);
    }

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
    let Ok(entries) = fs::read_dir(".") else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
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
}
