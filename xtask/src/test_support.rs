#[cfg(unix)]
use color_eyre::eyre::{Context, Result, eyre};
#[cfg(unix)]
use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
    sync::{Mutex, MutexGuard},
};
#[cfg(unix)]
use tempfile::TempDir;

#[cfg(unix)]
pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

#[cfg(unix)]
pub struct FakeCargo {
    _guard: MutexGuard<'static, ()>,
    _dir: TempDir,
    log_path: PathBuf,
    previous_path: Option<OsString>,
    previous_log: Option<OsString>,
    previous_metadata: Option<OsString>,
}

#[cfg(unix)]
struct FakeCargoFiles {
    _dir: TempDir,
    log_path: PathBuf,
    metadata_path: PathBuf,
}

#[cfg(unix)]
impl FakeCargoFiles {
    fn create() -> Result<Self> {
        let dir = TempDir::new().context("create fake cargo tempdir")?;
        let log_path = dir.path().join("cargo.log");
        let metadata_path = dir.path().join("metadata.json");
        let manifest_path = dir.path().join("Cargo.toml");
        fs::write(
            &manifest_path,
            "[package]\nname = \"fake\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .context("write fake manifest")?;
        let manifest_json =
            serde_json::to_string(&manifest_path.to_string_lossy()).context("encode manifest")?;
        fs::write(
            &metadata_path,
            format!(
                "{{\"packages\":[{{\"id\":\"fake 0.1.0 (path+file:///fake)\",\"name\":\"fake\",\"manifest_path\":{manifest_json},\"edition\":\"2024\"}}],\"workspace_members\":[\"fake 0.1.0 (path+file:///fake)\"]}}"
            ),
        )
        .context("write fake metadata")?;

        let cargo_path = dir.path().join("cargo");
        fs::write(
            &cargo_path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$XTASK_FAKE_CARGO_LOG\"\nif [ \"$1\" = \"metadata\" ]; then\n  cat \"$XTASK_FAKE_CARGO_METADATA\"\n  exit 0\nfi\nexit 0\n",
        )
        .context("write fake cargo script")?;
        let mut permissions =
            fs::metadata(&cargo_path).context("stat fake cargo script")?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&cargo_path, permissions).context("chmod fake cargo script")?;

        Ok(Self { _dir: dir, log_path, metadata_path })
    }

    fn child_command(&self, test_name: &str) -> Result<Command> {
        let mut path_entries = vec![self._dir.path().to_path_buf()];
        if let Some(path) = env::var_os("PATH") {
            path_entries.extend(env::split_paths(&path));
        }
        let joined_path = env::join_paths(path_entries).context("join fake cargo PATH")?;
        let mut command = Command::new(env::current_exe().context("locate test executable")?);
        command.args(["--exact", test_name, "--nocapture"]);
        command.env("PATH", joined_path);
        command.env("XTASK_FAKE_CARGO_LOG", &self.log_path);
        command.env("XTASK_FAKE_CARGO_METADATA", &self.metadata_path);
        command.env("XTASK_FAKE_CARGO_CHILD", "1");
        Ok(command)
    }
}

#[cfg(unix)]
pub struct FakeCargoChild {
    files: FakeCargoFiles,
    output: Output,
}

#[cfg(unix)]
impl FakeCargoChild {
    pub fn run(test_name: &str) -> Result<Self> {
        // `child_command` snapshots the parent PATH to preserve the host's
        // command lookup after prepending the fake Cargo directory.  Keep the
        // environment lock through that snapshot and the spawn: a concurrent
        // `FakeCargo::install` must not replace PATH between those operations.
        let _environment_guard =
            ENV_LOCK.lock().map_err(|_| eyre!("fake cargo environment lock poisoned"))?;
        let files = FakeCargoFiles::create()?;
        let output = files
            .child_command(test_name)
            .context("configure fake cargo child")?
            .output()
            .context("run fake cargo child")?;
        Ok(Self { files, output })
    }

    pub fn status(&self) -> std::process::ExitStatus {
        self.output.status
    }

    pub fn stderr(&self) -> &[u8] {
        &self.output.stderr
    }

    pub fn invocations(&self) -> Result<Vec<String>> {
        let raw = fs::read_to_string(&self.files.log_path)
            .context("read fake cargo child invocation log")?;
        Ok(raw.lines().map(str::to_string).collect())
    }
}

#[cfg(unix)]
impl FakeCargo {
    pub fn install() -> Result<Self> {
        let guard = ENV_LOCK.lock().map_err(|_| eyre!("fake cargo environment lock poisoned"))?;
        let files = FakeCargoFiles::create()?;
        let FakeCargoFiles { _dir: dir, log_path, metadata_path } = files;

        let previous_path = env::var_os("PATH");
        let previous_log = env::var_os("XTASK_FAKE_CARGO_LOG");
        let previous_metadata = env::var_os("XTASK_FAKE_CARGO_METADATA");
        let mut path_entries = vec![dir.path().to_path_buf()];
        if let Some(path) = &previous_path {
            path_entries.extend(env::split_paths(path));
        }
        let joined_path = env::join_paths(path_entries).context("join fake cargo PATH")?;

        // SAFETY: this helper holds ENV_LOCK for its lifetime and restores the
        // process environment in Drop before releasing the lock.
        unsafe {
            env::set_var("PATH", joined_path);
            env::set_var("XTASK_FAKE_CARGO_LOG", &log_path);
            env::set_var("XTASK_FAKE_CARGO_METADATA", &metadata_path);
        }

        Ok(Self {
            _guard: guard,
            _dir: dir,
            log_path,
            previous_path,
            previous_log,
            previous_metadata,
        })
    }

    pub fn invocations(&self) -> Vec<String> {
        let raw = fs::read_to_string(&self.log_path).unwrap_or_default();
        raw.lines().map(str::to_string).collect()
    }

    pub fn child_requested() -> bool {
        env::var_os("XTASK_FAKE_CARGO_CHILD").is_some()
    }
}

#[cfg(unix)]
impl Drop for FakeCargo {
    fn drop(&mut self) {
        let mut process_environment = ProcessEnvironment;
        restore_env(&mut process_environment, "PATH", self.previous_path.as_ref());
        restore_env(&mut process_environment, "XTASK_FAKE_CARGO_LOG", self.previous_log.as_ref());
        restore_env(
            &mut process_environment,
            "XTASK_FAKE_CARGO_METADATA",
            self.previous_metadata.as_ref(),
        );
    }
}

#[cfg(unix)]
trait EnvironmentRestorer {
    fn set_var(&mut self, key: &str, value: &OsString);
    fn remove_var(&mut self, key: &str);
}

#[cfg(unix)]
struct ProcessEnvironment;

#[cfg(unix)]
impl EnvironmentRestorer for ProcessEnvironment {
    fn set_var(&mut self, key: &str, value: &OsString) {
        // SAFETY: FakeCargo holds ENV_LOCK for its lifetime and restores the
        // process environment before releasing the lock.
        unsafe { env::set_var(key, value) }
    }

    fn remove_var(&mut self, key: &str) {
        // SAFETY: FakeCargo holds ENV_LOCK for its lifetime and restores the
        // process environment before releasing the lock.
        unsafe { env::remove_var(key) }
    }
}

#[cfg(unix)]
fn restore_env<E: EnvironmentRestorer>(environment: &mut E, key: &str, value: Option<&OsString>) {
    match value {
        Some(value) => environment.set_var(key, value),
        None => environment.remove_var(key),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{EnvironmentRestorer, FakeCargo, restore_env};
    use color_eyre::eyre::{Context, Result};
    use std::{env, ffi::OsString, process::Command};

    #[test]
    fn fake_cargo_records_invocations_through_log_path() -> Result<()> {
        let fake_cargo = FakeCargo::install()?;

        let status = Command::new("cargo")
            .args(["fmt", "-p", "xtask"])
            .status()
            .context("run fake cargo")?;

        assert!(status.success(), "fake cargo command should succeed");
        assert_eq!(fake_cargo.invocations(), vec!["fmt -p xtask"]);
        Ok(())
    }

    #[test]
    fn restore_env_removes_vars_that_were_absent() -> Result<()> {
        #[derive(Default)]
        struct RecordingEnvironment {
            removals: Vec<String>,
            assignments: Vec<(String, OsString)>,
        }

        impl EnvironmentRestorer for RecordingEnvironment {
            fn set_var(&mut self, key: &str, value: &OsString) {
                self.assignments.push((key.to_owned(), value.clone()));
            }

            fn remove_var(&mut self, key: &str) {
                self.removals.push(key.to_owned());
            }
        }

        let mut environment = RecordingEnvironment::default();
        restore_env(&mut environment, "XTASK_FAKE_CARGO_RESTORE_NONE_TEST", None);

        assert_eq!(
            environment.removals,
            vec!["XTASK_FAKE_CARGO_RESTORE_NONE_TEST"],
            "restore_env(None) should request removal without mutating process state",
        );
        assert!(environment.assignments.is_empty());
        Ok(())
    }

    #[test]
    fn process_environment_removes_an_absent_original_in_a_fresh_child() -> Result<()> {
        const TEST_NAME: &str =
            "test_support::tests::process_environment_removes_an_absent_original_in_a_fresh_child";
        const CHILD_MARKER: &str = "XTASK_FAKE_CARGO_RESTORE_NONE_CHILD";

        if env::var_os(CHILD_MARKER).is_some() {
            let fake_cargo = FakeCargo::install()?;
            drop(fake_cargo);
            if env::var_os("XTASK_FAKE_CARGO_LOG").is_some()
                || env::var_os("XTASK_FAKE_CARGO_METADATA").is_some()
            {
                return Err(color_eyre::eyre::eyre!(
                    "FakeCargo drop did not remove originally absent variables"
                ));
            }
            return Ok(());
        }

        let output = Command::new(env::current_exe().context("locate test executable")?)
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_MARKER, "1")
            .env_remove("XTASK_FAKE_CARGO_LOG")
            .env_remove("XTASK_FAKE_CARGO_METADATA")
            .output()
            .context("run real environment restoration child")?;
        if !output.status.success() {
            return Err(color_eyre::eyre::eyre!(
                "environment restoration child failed: {}",
                String::from_utf8_lossy(&output.stderr),
            ));
        }
        Ok(())
    }
}
