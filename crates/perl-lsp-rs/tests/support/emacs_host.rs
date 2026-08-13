#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BASE_DRIVER: &str = r#"
(setq user-emacs-directory (file-name-as-directory (getenv "PERL_LSP_EMACS_USER_DIR")))
(setq package-user-dir (file-name-as-directory (getenv "PERL_LSP_EMACS_PACKAGE_DIR")))
(setq inhibit-startup-screen t)
(setq inhibit-startup-message t)
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmacsClientKind {
    BundledEglot,
    ExternalEglot,
    LspMode,
}

impl EmacsClientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BundledEglot => "bundled_eglot",
            Self::ExternalEglot => "external_eglot",
            Self::LspMode => "lsp_mode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationState {
    ManualClientRegistration,
    UpstreamSourceRegistration,
    UpstreamAcceptedUnreleased,
    UpstreamBuiltinReleased,
}

impl RegistrationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManualClientRegistration => "manual_client_registration",
            Self::UpstreamSourceRegistration => "upstream_source_registration",
            Self::UpstreamAcceptedUnreleased => "upstream_accepted_unreleased",
            Self::UpstreamBuiltinReleased => "upstream_builtin_released",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRootEvidenceMode {
    ManuallyBoundFixture,
    StockProjectDiscovery,
    StandardUserProjectOverride,
}

impl ProjectRootEvidenceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManuallyBoundFixture => "manually_bound_fixture",
            Self::StockProjectDiscovery => "stock_project_discovery",
            Self::StandardUserProjectOverride => "standard_user_project_override",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmacsHostSubject {
    pub emacs_executable: PathBuf,
    pub emacs_version: String,
    pub client_kind: EmacsClientKind,
    pub client_version: String,
    pub client_source: String,
    pub perllsp_executable: PathBuf,
    pub perllsp_version: String,
    pub perllsp_sha256: String,
    pub registration_state: RegistrationState,
    pub project_root_evidence_mode: ProjectRootEvidenceMode,
}

impl EmacsHostSubject {
    pub fn validate(&self) -> Result<(), String> {
        if !self.emacs_executable.is_absolute() {
            return Err("Emacs executable identity must be an absolute path".to_string());
        }
        if !self.perllsp_executable.is_absolute() {
            return Err("perllsp executable identity must be an absolute path".to_string());
        }
        if self.emacs_version.trim().is_empty() {
            return Err("Emacs version must not be empty".to_string());
        }
        if self.client_version.trim().is_empty() || self.client_source.trim().is_empty() {
            return Err("client version/source identity must not be empty".to_string());
        }
        if self.perllsp_version.trim().is_empty() {
            return Err("perllsp version must not be empty".to_string());
        }
        if !is_lower_sha256(&self.perllsp_sha256) {
            return Err("perllsp SHA-256 must be 64 lowercase hexadecimal characters".to_string());
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct EmacsHostSandbox {
    root: TempDir,
    home: PathBuf,
    user_emacs_directory: PathBuf,
    package_directory: PathBuf,
    xdg_config_home: PathBuf,
    xdg_cache_home: PathBuf,
    xdg_data_home: PathBuf,
    project_root: PathBuf,
    logs_directory: PathBuf,
    drivers_directory: PathBuf,
}

impl EmacsHostSandbox {
    pub fn new() -> io::Result<Self> {
        let root = tempfile::tempdir()?;
        let home = root.path().join("home");
        let user_emacs_directory = root.path().join("emacs-user");
        let package_directory = root.path().join("emacs-packages");
        let xdg_config_home = root.path().join("xdg-config");
        let xdg_cache_home = root.path().join("xdg-cache");
        let xdg_data_home = root.path().join("xdg-data");
        let project_root = root.path().join("project");
        let logs_directory = root.path().join("logs");
        let drivers_directory = root.path().join("drivers");

        for directory in [
            &home,
            &user_emacs_directory,
            &package_directory,
            &xdg_config_home,
            &xdg_cache_home,
            &xdg_data_home,
            &project_root,
            &logs_directory,
            &drivers_directory,
        ] {
            fs::create_dir_all(directory)?;
        }

        write_canonical_fixture(&project_root)?;

        Ok(Self {
            root,
            home,
            user_emacs_directory,
            package_directory,
            xdg_config_home,
            xdg_cache_home,
            xdg_data_home,
            project_root,
            logs_directory,
            drivers_directory,
        })
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn logs_directory(&self) -> &Path {
        &self.logs_directory
    }

    pub fn write_driver(&self, name: &str, client_body: &str) -> io::Result<PathBuf> {
        let relative = validate_driver_name(name)?;
        let path = self.drivers_directory.join(relative);
        let mut content = String::with_capacity(BASE_DRIVER.len() + client_body.len() + 2);
        content.push_str(BASE_DRIVER);
        content.push('\n');
        content.push_str(client_body);
        content.push('\n');
        fs::write(&path, content)?;
        Ok(path)
    }

    pub fn prepare_command(
        &self,
        subject: &EmacsHostSubject,
        driver: &Path,
    ) -> io::Result<PreparedEmacsCommand> {
        subject
            .validate()
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
        if !driver.starts_with(&self.drivers_directory) || !driver.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Emacs driver must be a regular file inside the sandbox driver directory",
            ));
        }

        let emacs_stdout = self.logs_directory.join("emacs-stdout.log");
        let emacs_stderr = self.logs_directory.join("emacs-stderr.log");
        let server_stderr = self.logs_directory.join("perllsp-stderr.log");
        let receipt_path = self.logs_directory.join("emacs-host-receipt.json");

        let stdout = File::create(&emacs_stdout)?;
        let stderr = File::create(&emacs_stderr)?;

        let mut command = Command::new(&subject.emacs_executable);
        command
            .arg("--quick")
            .arg("--batch")
            .arg("--load")
            .arg(driver)
            .current_dir(&self.project_root)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));

        for key in [
            "EMACSLOADPATH",
            "EMACSDATA",
            "EMACSDOC",
            "EMACSLOCKMETHOD",
            "PERL5LIB",
            "PERL_LOCAL_LIB_ROOT",
            "PERL_MB_OPT",
            "PERL_MM_OPT",
        ] {
            command.env_remove(key);
        }

        command
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CONFIG_HOME", &self.xdg_config_home)
            .env("XDG_CACHE_HOME", &self.xdg_cache_home)
            .env("XDG_DATA_HOME", &self.xdg_data_home)
            .env("PERL_LSP_EMACS_USER_DIR", &self.user_emacs_directory)
            .env("PERL_LSP_EMACS_PACKAGE_DIR", &self.package_directory)
            .env("PERL_LSP_PROJECT_ROOT", &self.project_root)
            .env("PERL_LSP_BIN", &subject.perllsp_executable)
            .env("PERL_LSP_VERSION", &subject.perllsp_version)
            .env("PERL_LSP_SHA256", &subject.perllsp_sha256)
            .env("PERL_LSP_EMACS_VERSION", &subject.emacs_version)
            .env("PERL_LSP_CLIENT_KIND", subject.client_kind.as_str())
            .env("PERL_LSP_CLIENT_VERSION", &subject.client_version)
            .env("PERL_LSP_CLIENT_SOURCE", &subject.client_source)
            .env(
                "PERL_LSP_REGISTRATION_STATE",
                subject.registration_state.as_str(),
            )
            .env(
                "PERL_LSP_PROJECT_ROOT_EVIDENCE_MODE",
                subject.project_root_evidence_mode.as_str(),
            )
            .env("PERL_LSP_SERVER_STDERR", &server_stderr)
            .env("PERL_LSP_RECEIPT_PATH", &receipt_path);

        Ok(PreparedEmacsCommand {
            command,
            emacs_stdout,
            emacs_stderr,
            server_stderr,
            receipt_path,
        })
    }
}

#[derive(Debug)]
pub struct PreparedEmacsCommand {
    pub command: Command,
    pub emacs_stdout: PathBuf,
    pub emacs_stderr: PathBuf,
    pub server_stderr: PathBuf,
    pub receipt_path: PathBuf,
}

#[derive(Debug)]
pub struct EmacsRunOutcome {
    pub status: ExitStatus,
    pub elapsed: Duration,
    pub timed_out: bool,
}

impl PreparedEmacsCommand {
    pub fn run_with_deadline(mut self, deadline: Duration) -> io::Result<EmacsRunOutcome> {
        let started = Instant::now();
        let mut child = self.command.spawn()?;

        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(EmacsRunOutcome {
                    status,
                    elapsed: started.elapsed(),
                    timed_out: false,
                });
            }

            if started.elapsed() >= deadline {
                child.kill()?;
                let status = child.wait()?;
                return Ok(EmacsRunOutcome {
                    status,
                    elapsed: started.elapsed(),
                    timed_out: true,
                });
            }

            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_driver_name(name: &str) -> io::Result<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() || path.extension() != Some(OsStr::new("el")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Emacs driver name must be a relative .el path",
        ));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Emacs driver name must not escape the sandbox",
                ));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Emacs driver name must not be empty",
        ));
    }
    Ok(normalized)
}

fn write_canonical_fixture(project_root: &Path) -> io::Result<()> {
    let lib = project_root.join("lib/My");
    let tests = project_root.join("t");
    fs::create_dir_all(&lib)?;
    fs::create_dir_all(&tests)?;

    fs::write(
        lib.join("Thing.pm"),
        "package My::Thing;\nuse strict;\nuse warnings;\nsub answer { 42 }\n1;\n",
    )?;
    fs::write(
        lib.join("Sibling.pm"),
        "package My::Sibling;\nuse strict;\nuse warnings;\nsub marker { 'SIBLING' }\n1;\n",
    )?;

    let unicode_crlf = concat!(
        "use strict;\r\n",
        "use warnings;\r\n",
        "use lib 'lib';\r\n",
        "use My::Thing;\r\n",
        "my $emoji = \"😀\";\r\n",
        "my $value = My::Thing::answer();\r\n",
        "print $value;\r\n"
    );
    fs::write(tests.join("unicode_crlf.t"), unicode_crlf.as_bytes())?;
    fs::write(
        project_root.join(".perl-lsp.toml"),
        "[perl]\ninclude_paths = [\"lib\"]\n",
    )?;
    Ok(())
}
