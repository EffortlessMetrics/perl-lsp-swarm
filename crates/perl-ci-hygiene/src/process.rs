use color_eyre::eyre::{Context, Result};
use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn command_with_output(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<String> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);
    for (key, value) in env_vars {
        child.env(key, value);
    }
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    let status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if status != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(color_eyre::eyre::eyre!(
            "command '{command}' failed (exit {status}): {stderr}"
        ));
    }
    Ok(stdout)
}

pub(crate) fn command_with_output_all(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<String> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);
    for (key, value) in env_vars {
        child.env(key, value);
    }
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let status = output.status.code().unwrap_or(1);
    if status != 0 {
        return Err(color_eyre::eyre::eyre!(
            "command '{command}' failed (exit {status}): {combined}"
        ));
    }
    Ok(combined)
}

pub(crate) fn command_with_input_with_status(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
    stdin_payload: &str,
) -> Result<(i32, String)> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);
    for (key, value) in env_vars {
        child.env(key, value);
    }
    child.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = child.spawn().wrap_err_with(|| format!("running {command}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| color_eyre::eyre::eyre!("failed to open stdin for command {command}"))?;
        stdin
            .write_all(stdin_payload.as_bytes())
            .wrap_err_with(|| format!("writing to stdin for {command}"))?;
    }
    let output = child.wait_with_output().wrap_err_with(|| format!("running {command}"))?;
    let status = output.status.code().unwrap_or(1);
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok((status, combined))
}

pub(crate) fn command_output_with_status(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<(i32, String)> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);
    for (key, value) in env_vars {
        child.env(key, value);
    }
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    let status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok((status, stdout))
}

pub(crate) fn command_timed_status(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<(i32, Duration)> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);
    for (key, value) in env_vars {
        child.env(key, value);
    }
    child.stdout(Stdio::null()).stderr(Stdio::null());
    let start = Instant::now();
    let status = child.status().wrap_err_with(|| format!("running {command}"))?;
    let elapsed = start.elapsed();
    Ok((status.code().unwrap_or(1), elapsed))
}

pub(crate) fn command_with_output_allow_empty_match(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<String> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);
    for (key, value) in env_vars {
        child.env(key, value);
    }
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    let status = output.status.code().unwrap_or(1);
    if status != 0 && status != 1 {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(color_eyre::eyre::eyre!(
            "command '{command}' failed (exit {status}): {stderr}"
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn command_with_output_allow_failure(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<String> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);
    for (key, value) in env_vars {
        child.env(key, value);
    }
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn command_status(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<i32> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);
    for (key, value) in env_vars {
        child.env(key, value);
    }
    let status = child.status().wrap_err_with(|| format!("running {command}"))?;
    Ok(status.code().unwrap_or(1))
}

pub(crate) fn command_status_strict(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<()> {
    let status = command_status(repo_root, command, args, env_vars)?;
    if status != 0 {
        return Err(color_eyre::eyre::eyre!("{command} failed with code {status}"));
    }
    Ok(())
}

pub(crate) fn command_exists(command: &str) -> bool {
    let path = env::var_os("PATH");
    command_exists_in_path(command, path.as_deref())
}

fn command_exists_in_path(command: &str, path: Option<&OsStr>) -> bool {
    #[cfg(windows)]
    let suffixes: &[&str] = &[".exe", ".cmd", ".bat", ""];
    #[cfg(not(windows))]
    let suffixes: &[&str] = &[""];
    let Some(path) = path else {
        return false;
    };

    env::split_paths(path)
        .any(|dir| suffixes.iter().any(|ext| dir.join(format!("{command}{ext}")).is_file()))
}

pub(crate) fn command_output_lines(output: &str) -> Vec<String> {
    output.lines().map(str::trim).filter(|line| !line.is_empty()).map(ToString::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::command_exists_in_path;
    use std::env;
    use std::error::Error;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    type TestResult<T = ()> = Result<T, Box<dyn Error>>;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> TestResult<Self> {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = env::temp_dir().join(format!(
                "perl-ci-hygiene-command-exists-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self(path))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn command_candidate_name(command: &str) -> String {
        #[cfg(windows)]
        {
            format!("{command}.exe")
        }
        #[cfg(not(windows))]
        {
            command.to_owned()
        }
    }

    fn joined_path(paths: &[&Path]) -> TestResult<OsString> {
        Ok(env::join_paths(paths.iter().copied())?)
    }

    #[test]
    fn command_exists_in_path_rejects_directory_candidate() -> TestResult {
        let temp = TempDir::new("directory-only")?;
        let command = "ci-hygiene-probe";
        fs::create_dir(temp.path().join(command_candidate_name(command)))?;
        let path = joined_path(&[temp.path()])?;

        assert!(!command_exists_in_path(command, Some(path.as_os_str())));
        Ok(())
    }

    #[test]
    fn command_exists_in_path_accepts_regular_file_candidate() -> TestResult {
        let temp = TempDir::new("regular-file")?;
        let command = "ci-hygiene-probe";
        fs::write(temp.path().join(command_candidate_name(command)), b"")?;
        let path = joined_path(&[temp.path()])?;

        assert!(command_exists_in_path(command, Some(path.as_os_str())));
        Ok(())
    }

    #[test]
    fn command_exists_in_path_skips_missing_path_entry() -> TestResult {
        let missing_entry_root = TempDir::new("missing-before-file")?;
        let regular_file_candidate = TempDir::new("file-after-missing")?;
        let command = "ci-hygiene-probe";
        let missing_entry = missing_entry_root.path().join("not-created");
        fs::write(regular_file_candidate.path().join(command_candidate_name(command)), b"")?;
        let path = joined_path(&[missing_entry.as_path(), regular_file_candidate.path()])?;

        assert!(command_exists_in_path(command, Some(path.as_os_str())));
        Ok(())
    }

    #[test]
    fn command_exists_in_path_continues_past_directory_candidate() -> TestResult {
        let directory_candidate = TempDir::new("directory-before-file")?;
        let regular_file_candidate = TempDir::new("file-after-directory")?;
        let command = "ci-hygiene-probe";
        let candidate_name = command_candidate_name(command);
        fs::create_dir(directory_candidate.path().join(&candidate_name))?;
        let regular_file = regular_file_candidate.path().join(candidate_name);
        fs::write(&regular_file, b"")?;
        let path = joined_path(&[directory_candidate.path(), regular_file_candidate.path()])?;

        assert!(command_exists_in_path(command, Some(path.as_os_str())));
        fs::remove_file(regular_file)?;
        assert!(!command_exists_in_path(command, Some(path.as_os_str())));
        Ok(())
    }

    #[test]
    fn command_exists_in_path_returns_false_without_path() {
        assert!(!command_exists_in_path("ci-hygiene-probe", None));
    }
}
