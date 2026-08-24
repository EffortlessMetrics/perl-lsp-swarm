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
    update_state: UpdateState,
    cold_install_attempts: u32,
    last_cold_failure: Option<String>,
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
            Err(reason) => {
                self.last_cold_failure = Some(reason);
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

        if !fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file()) {
            if fs::metadata(&version_dir).is_ok() {
                fs::remove_dir_all(&version_dir).map_err(|error| {
                    format!("failed to remove incomplete perllsp download `{version_dir}`: {error}")
                })?;
            }

            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            if let Err(error) = zed::download_file(&asset.download_url, &version_dir, file_type) {
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

        // Bind the exact installed identity only after the candidate is fully
        // staged and verified on disk; the manifest promotion is atomic and
        // last, so a partial install can never become accepted current.
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
        if let Err(error) = store_selection_manifest(&manifest) {
            self.update_state = UpdateState::CandidateRejected;
            return Err(error);
        }

        if !matches!(os, zed::Os::Windows) {
            zed::make_file_executable(&binary_path)?;
        }

        self.perllsp_path = Some(binary_path.clone());
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
            update_state: UpdateState::NotRequested,
            cold_install_attempts: 0,
            last_cold_failure: None,
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
}
