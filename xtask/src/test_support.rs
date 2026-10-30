#[cfg(unix)]
use color_eyre::eyre::{Context, Result, eyre};
#[cfg(unix)]
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[cfg(unix)]
use tempfile::TempDir;

// Fixture environment and formatter execution belong only to the re-entered child.
#[cfg(unix)]
struct FakeCargoFiles {
    _dir: TempDir,
    log_path: PathBuf,
    metadata_path: PathBuf,
    rustfmt_path: PathBuf,
    expected_formats: Vec<String>,
}

#[cfg(unix)]
impl FakeCargoFiles {
    fn create() -> Result<Self> {
        let dir = TempDir::new().context("create fake cargo tempdir")?;
        let log_path = dir.path().join("cargo.log");
        let metadata_path = dir.path().join("metadata.json");
        let config_path = dir.path().join("rustfmt.toml");
        fs::write(&config_path, "max_width = 100\n").context("write fixture config")?;
        let mut packages = Vec::new();
        let mut members = Vec::new();
        let mut expected_formats = Vec::new();
        for (name, edition) in [("alpha", "2018"), ("zeta", "2024")] {
            let package_dir = dir.path().join(name);
            let manifest_path = package_dir.join("Cargo.toml");
            let mut targets = Vec::new();
            let mut roots = Vec::new();
            for relative in ["src/lib.rs", "tests/fixture.rs"] {
                let path = package_dir.join(relative);
                let parent = path.parent().ok_or_else(|| eyre!("fixture root has no parent"))?;
                fs::create_dir_all(parent).context("create fixture package")?;
                fs::write(&path, "pub fn fixture() {}\n").context("write fixture root")?;
                targets.push(serde_json::json!({"src_path": path, "edition": edition}));
                roots.push(path.display().to_string());
            }
            fs::write(
                &manifest_path,
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"{edition}\"\n"
                ),
            )
            .context("write fake manifest")?;
            let id = format!("{name} 0.1.0 (path+file:///{name})");
            packages.push(serde_json::json!({
                "id": id, "name": name, "manifest_path": manifest_path,
                "edition": edition, "targets": targets
            }));
            members.push(id);
            expected_formats.push(format!(
                "rustfmt --edition {edition} --config-path {} --check {}",
                config_path.display(),
                roots.join(" ")
            ));
        }
        fs::write(
            &metadata_path,
            serde_json::to_vec(&serde_json::json!({
                "packages": packages, "workspace_members": members, "workspace_root": dir.path()
            }))?,
        )
        .context("write fake metadata")?;
        let cargo_path = dir.path().join("cargo");
        write_executable(
            &cargo_path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$XTASK_FAKE_CARGO_LOG\"\nif [ \"$1\" = \"metadata\" ]; then\n  cat \"$XTASK_FAKE_CARGO_METADATA\"\n  exit 0\nfi\nexit 0\n",
        )?;
        let rustfmt_path = dir.path().join("rustfmt");
        write_executable(
            &rustfmt_path,
            "#!/bin/sh\nprintf 'rustfmt %s\\n' \"$*\" >> \"$XTASK_FAKE_CARGO_LOG\"\nexit 0\n",
        )?;
        Ok(Self { _dir: dir, log_path, metadata_path, rustfmt_path, expected_formats })
    }

    fn child_command(&self, test_name: &str) -> Result<Command> {
        let mut paths = vec![self._dir.path().to_path_buf()];
        if let Some(path) = env::var_os("PATH") {
            paths.extend(env::split_paths(&path));
        }
        let mut command = Command::new(env::current_exe().context("locate test executable")?);
        command.args(["--exact", test_name, "--nocapture"]);
        command.env("PATH", env::join_paths(paths).context("join fake cargo PATH")?);
        command.env("RUSTFMT", &self.rustfmt_path);
        command.env("XTASK_FAKE_CARGO_LOG", &self.log_path);
        command.env("XTASK_FAKE_CARGO_METADATA", &self.metadata_path);
        command.env("XTASK_FAKE_CARGO_CHILD", "1");
        Ok(command)
    }
}

#[cfg(unix)]
fn write_executable(path: &Path, source: &str) -> Result<()> {
    fs::write(path, source).context("write fake tool")?;
    let mut permissions = fs::metadata(path).context("stat fake tool")?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).context("chmod fake tool")
}

#[cfg(unix)]
pub struct FakeCargoChild {
    files: FakeCargoFiles,
    output: Output,
}

#[cfg(unix)]
impl FakeCargoChild {
    pub fn run(test_name: &str) -> Result<Self> {
        let files = FakeCargoFiles::create()?;
        let output = files.child_command(test_name)?.output().context("run fake cargo child")?;
        Ok(Self { files, output })
    }

    pub fn child_requested() -> bool {
        env::var_os("XTASK_FAKE_CARGO_CHILD").is_some()
    }

    pub fn status(&self) -> std::process::ExitStatus {
        self.output.status
    }

    pub fn stderr(&self) -> &[u8] {
        &self.output.stderr
    }

    pub fn ensure_success(&self) -> Result<()> {
        if self.status().success() {
            return Ok(());
        }
        let bytes = self.stderr();
        let shown = bytes.get(..4096).unwrap_or(bytes);
        Err(eyre!("fake cargo child failed: {}", String::from_utf8_lossy(shown)))
    }

    pub fn invocations(&self) -> Result<Vec<String>> {
        let raw = fs::read_to_string(&self.files.log_path).context("read child invocation log")?;
        Ok(raw.lines().map(str::to_string).collect())
    }

    pub fn expected_format_invocations(&self) -> Vec<String> {
        self.files.expected_formats.clone()
    }

    pub fn assert_package_formatting(&self) -> Result<()> {
        let invocations = self.invocations()?;
        let observed: Vec<_> =
            invocations.iter().filter(|line| line.starts_with("rustfmt ")).cloned().collect();
        if observed != self.files.expected_formats {
            return Err(eyre!(
                "package roots/edition/config/check mismatch: observed {observed:?}, expected {:?}",
                self.files.expected_formats
            ));
        }
        if invocations.iter().any(|line| line.starts_with("fmt ")) {
            return Err(eyre!("package formatter fell back to cargo fmt: {invocations:?}"));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::FakeCargoChild;
    use color_eyre::eyre::{Context, Result, eyre};
    use std::{env, ffi::OsString, process::Command};

    fn environment_snapshot() -> Vec<Option<OsString>> {
        [
            "PATH",
            "RUSTFMT",
            "XTASK_FAKE_CARGO_LOG",
            "XTASK_FAKE_CARGO_METADATA",
            "XTASK_FAKE_CARGO_CHILD",
        ]
        .iter()
        .map(env::var_os)
        .collect()
    }

    #[test]
    fn fake_cargo_records_invocations_through_log_path() -> Result<()> {
        const TEST_NAME: &str =
            "test_support::tests::fake_cargo_records_invocations_through_log_path";
        if FakeCargoChild::child_requested() {
            let status = Command::new("cargo")
                .args(["fmt", "-p", "xtask"])
                .status()
                .context("run fake cargo")?;
            if !status.success() {
                return Err(eyre!("fake cargo command should succeed"));
            }
            return Ok(());
        }
        let original = environment_snapshot();
        let child = FakeCargoChild::run(TEST_NAME)?;
        child.ensure_success()?;
        assert_eq!(child.invocations()?, vec!["fmt -p xtask"]);
        assert_eq!(environment_snapshot(), original, "child changed the parent environment");
        Ok(())
    }

    #[test]
    fn a_panicking_fake_cargo_child_does_not_poison_later_children() -> Result<()> {
        const TEST_NAME: &str =
            "test_support::tests::a_panicking_fake_cargo_child_does_not_poison_later_children";
        if FakeCargoChild::child_requested() {
            panic!("intentional child-only harness failure");
        }
        let original = environment_snapshot();
        let failed = FakeCargoChild::run(TEST_NAME)?;
        assert!(!failed.status().success(), "failure control did not fail");
        let healthy = FakeCargoChild::run(
            "test_support::tests::fake_cargo_records_invocations_through_log_path",
        )?;
        healthy.ensure_success()?;
        assert_eq!(healthy.invocations()?, vec!["fmt -p xtask"]);
        assert_eq!(environment_snapshot(), original, "failed child changed the parent environment");
        Ok(())
    }
}
