use std::env;
use std::fs;

use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

const SERVER_PATH: &str = "node_modules/.bin/perlnavigator";
const PACKAGE_NAME: &str = "perlnavigator-server";
const PERLNAVIGATOR_SERVER_ID: &str = "perlnavigator-server";

const PERL_LSP_SERVER_ID: &str = "perl-lsp";
const PERL_LSP_REPO: &str = "tree-sitter-perl/perl-tree-sitter-lsp";

const PERLLSP_SERVER_ID: &str = "perllsp";
const PERLLSP_REPO: &str = "EffortlessMetrics/perl-lsp";

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
        if let Some(path) = &self.perllsp_path {
            if fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
                return Ok(path.clone());
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            PERLLSP_REPO,
            zed::GithubReleaseOptions { require_assets: true, pre_release: false },
        )?;
        let version = normalize_release_version(&release.version);
        let (os, arch) = zed::current_platform();
        let target = perllsp_target(os, arch)?;
        let (archive_ext, file_type) = match os {
            zed::Os::Windows => ("zip", zed::DownloadedFileType::Zip),
            _ => ("tar.gz", zed::DownloadedFileType::GzipTar),
        };
        let asset_name = perllsp_asset_name(version, target, archive_ext);
        let asset =
            release.assets.iter().find(|asset| asset.name == asset_name).ok_or_else(|| {
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
            zed::download_file(&asset.download_url, &version_dir, file_type).map_err(|error| {
                format!("failed to download EffortlessMetrics perllsp: {error}")
            })?;

            if !fs::metadata(&binary_path).is_ok_and(|metadata| metadata.is_file()) {
                return Err(format!(
                    "downloaded `{asset_name}` but did not find expected binary `{binary_path}`"
                ));
            }

            if !matches!(os, zed::Os::Windows) {
                zed::make_file_executable(&binary_path)?;
            }
            // Retain older perllsp versions: the extension API does not report
            // successful server startup, so deleting them here would discard the
            // known-good fallback before Zed observes launch success.
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
        if argument == "--stdio" {
            if !saw_stdio {
                normalized.push(argument);
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
    matches!(
        argument,
        "mcp"
            | "--mcp"
            | "--socket"
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
    ) || argument.starts_with("--socket=")
        || argument.starts_with("--port=")
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
            "managed perllsp downloads do not yet claim Windows ARM64; install a proven compatible perllsp binary on PATH"
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
        Self { did_find_server: false, perl_lsp_path: None, perllsp_path: None }
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
    fn non_lsp_modes_fail_closed() {
        for argument in [
            "mcp",
            "--mcp",
            "--socket",
            "--socket=127.0.0.1:9257",
            "--port",
            "--port=9257",
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
