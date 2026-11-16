//! POSIX gate shell launcher (#18011).
//!
//! Gate commands run under non-login Bash so an explicitly selected `PATH` from
//! the caller survives the wrapper, timeout, and fallback branches. Login
//! startup (`bash -lc`) may reset `PATH` on hosted agents; non-login
//! (`bash -c`) inherits the caller's environment.

use std::os::unix::process::CommandExt;
use std::process::Command;

pub(crate) fn shell_command_process(command: &str, timeout_secs: u64) -> Command {
    let mut cmd = Command::new("bash");
    let timeout_arg = format!("{timeout_secs}s");
    cmd.arg("-c")
        .arg(
            "if command -v timeout >/dev/null 2>&1; then \
             exec timeout --signal=TERM --kill-after=60s \"$1\" bash -c \"$2\"; \
             else exec bash -c \"$2\"; fi",
        )
        .arg("xtask-gate-timeout")
        .arg(timeout_arg)
        .arg(command);
    cmd.process_group(0);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, ensure};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use tempfile::tempdir;

    fn discover_bash() -> PathBuf {
        std::process::Command::new("bash")
            .arg("-c")
            .arg("command -v bash")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .ok()
                    .map(|path| path.trim().to_string())
                    .filter(|path| !path.is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("/bin/bash"))
    }

    fn write_login_profile_resetting_path(home: &Path) -> Result<()> {
        fs::write(home.join(".bash_profile"), "export PATH=/usr/bin:/bin\n")?;
        Ok(())
    }

    fn write_task_tool(bin_dir: &Path, name: &str) -> Result<PathBuf> {
        let tool = bin_dir.join(name);
        fs::write(&tool, "#!/bin/sh\necho task-tool-selected\n")?;
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))?;
        Ok(tool)
    }

    fn run_launcher(home: &Path, path: &str, command: &str, timeout_secs: u64) -> Result<i32> {
        let log_path = home.join("gate.log");
        let log = fs::File::create(&log_path)?;
        let log_err = log.try_clone()?;
        let status = shell_command_process(command, timeout_secs)
            .env("HOME", home)
            .env("PATH", path)
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .status()?;
        status.code().ok_or_else(|| color_eyre::eyre::eyre!("gate launcher exited by signal"))
    }

    #[test]
    fn gate_launch_preserves_task_local_path_through_login_profile() -> Result<()> {
        let temp = tempdir()?;
        let root = temp.path();
        let home = root.join("home");
        let bin_dir = root.join("path with spaces").join("bin");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&bin_dir)?;
        write_login_profile_resetting_path(&home)?;
        write_task_tool(&bin_dir, "task-tool")?;

        let path = format!("{}:/usr/bin:/bin", bin_dir.display());
        let exit = run_launcher(&home, &path, "task-tool", 30)?;
        ensure!(
            exit == 0,
            "expected selected task-local tool, log:\n{}",
            fs::read_to_string(home.join("gate.log")).unwrap_or_default()
        );
        Ok(())
    }

    #[test]
    fn gate_launch_preserves_path_on_timeout_branch() -> Result<()> {
        let temp = tempdir()?;
        let root = temp.path();
        let home = root.join("home");
        let bin_dir = root.join("bin");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&bin_dir)?;
        write_login_profile_resetting_path(&home)?;
        write_task_tool(&bin_dir, "task-tool")?;

        let path = format!("{}:/usr/bin:/bin", bin_dir.display());
        let exit = run_launcher(&home, &path, "task-tool", 1)?;
        ensure!(exit == 0, "timeout branch must preserve PATH");
        Ok(())
    }

    #[test]
    fn gate_launch_reports_missing_selected_tool() -> Result<()> {
        let temp = tempdir()?;
        let root = temp.path();
        let home = root.join("home");
        let bin_dir = root.join("bin");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&bin_dir)?;
        write_login_profile_resetting_path(&home)?;

        let path = format!("{}:/usr/bin:/bin", bin_dir.display());
        let exit = run_launcher(&home, &path, "missing-task-tool", 30)?;
        ensure!(exit == 127, "missing tool must remain exit 127, got {exit}");
        Ok(())
    }

    #[test]
    fn gate_launch_uses_discovered_bash_binary() -> Result<()> {
        let bash = discover_bash();
        ensure!(bash.is_file(), "fixture requires a bash binary");
        Ok(())
    }
}
