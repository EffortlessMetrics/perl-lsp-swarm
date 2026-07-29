use color_eyre::eyre::{Context, ContextCompat, Result, bail};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::utils::project_root;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Verify that checked-in Claude hook scripts are executable.
pub fn run_hook_check() -> Result<()> {
    let root = project_root()?;
    let hooks_dir = root.join(".claude/hooks");

    if !hooks_dir.exists() {
        println!("Hook executable check passed");
        return Ok(());
    }

    let mut failed = 0u32;

    for entry in fs::read_dir(&hooks_dir).context("Failed to read .claude/hooks")? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("sh") || !path.is_file() {
            continue;
        }

        if !is_executable(&path)? {
            println!("::error::Hook not executable: {}", path.display());
            failed += 1;
        }
    }

    if failed == 0 {
        println!("Hook executable check passed");
        Ok(())
    } else {
        bail!("Hook executable check failed for {failed} file(s)");
    }
}

/// Verify that every registered shell hook exists and is executable.
pub fn run_hook_registry_check() -> Result<()> {
    let root = project_root()?;
    let settings_path = root.join(".claude/settings.json");
    let settings = fs::read_to_string(&settings_path)
        .with_context(|| format!("Failed to read {}", settings_path.display()))?;

    let document: Value = serde_json::from_str(&settings)
        .with_context(|| format!("Failed to parse {}", settings_path.display()))?;

    let commands = extract_hook_commands(&document);
    if commands.is_empty() {
        println!(
            "No .sh hook scripts registered in {} -- nothing to check",
            settings_path.display()
        );
        return Ok(());
    }

    let mut failed = 0u32;
    for path in &commands {
        let abs_path = root.join(path);
        if !abs_path.exists() {
            println!("::error::Registered hook script missing: {path}");
            failed += 1;
            continue;
        }

        if !is_executable(&abs_path)? {
            println!("::error::Registered hook script not executable: {path}");
            failed += 1;
            continue;
        }

        println!("  OK: {path}");
    }

    if failed == 0 {
        println!("Hook registry check passed ({} scripts verified)", commands.len());
        Ok(())
    } else {
        bail!("Hook registry check failed for {failed} script(s)");
    }
}

/// Exercise the retained current-hazard safety hook.
///
/// Lifecycle/task/subagent telemetry hooks are intentionally absent. This gate
/// proves only the registered destructive-command and linked-worktree safety
/// contract.
pub fn run_hook_tests() -> Result<()> {
    let root = project_root()?;
    let pre_tool_use = root.join(".claude/hooks/pre-tool-use.sh");
    let worktree_guard = root.join(".claude/hooks/tests/test_pre_tool_use_worktree.sh");

    for path in [&pre_tool_use, &worktree_guard] {
        if !path.exists() {
            bail!("Required safety-hook test surface missing: {}", path.display());
        }
        if !is_executable(path)? {
            bail!("Safety-hook test surface not executable: {}", path.display());
        }
    }

    let mut pass = 0u32;
    let mut fail = 0u32;

    for (description, command, expected) in [
        ("allows safe commands", "git status", 0),
        ("allows an empty command", "", 0),
        ("allows bounded subpath cleanup", "rm -rf /tmp/perl-lsp-hook-test", 0),
        ("blocks force push", "git push --force", 2),
        ("blocks hard reset", "git reset --hard", 2),
        ("blocks cargo publish", "cargo publish", 2),
        ("blocks destructive git clean", "git clean -fd", 2),
        ("blocks force refspec", "git push origin +HEAD:main", 2),
        ("blocks shared worktree stash", "git stash", 2),
        ("blocks whole shared-temp deletion", "rm -rf /tmp", 2),
    ] {
        let payload = serde_json::json!({"tool_input": {"command": command}}).to_string();
        let output = run_script(&pre_tool_use, Some(&payload), Some(&root))?;
        assert_exit_code(
            expected,
            description,
            output.status.code().unwrap_or(-1),
            &mut pass,
            &mut fail,
        );
    }

    let worktree_output = run_script(&worktree_guard, None, Some(&root))?;
    assert_exit_code(
        0,
        "linked-worktree branch-mutation guard",
        worktree_output.status.code().unwrap_or(-1),
        &mut pass,
        &mut fail,
    );
    if !worktree_output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&worktree_output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&worktree_output.stderr));
    }

    println!("\n=== Results: {pass} passed, {fail} failed ===");
    if fail > 0 {
        bail!("safety-hook tests failed");
    }

    Ok(())
}

fn run_script(path: &Path, input: Option<&str>, current_dir: Option<&Path>) -> Result<std::process::Output> {
    let mut command = Command::new(bash_executable());
    command.arg(path);
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }

    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_PREFIX");

    if input.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command.spawn().with_context(|| format!("Failed to run {}", path.display()))?;
    if let Some(input) = input {
        let stdin = child.stdin.as_mut().context("Failed to open stdin for script")?;
        stdin.write_all(input.as_bytes()).context("Failed to write hook input")?;
    }

    child.wait_with_output().context("Failed to read script output")
}

fn is_executable(path: &Path) -> Result<bool> {
    let metadata = path.metadata().context("Failed to read script metadata")?;
    if metadata.is_dir() {
        return Ok(false);
    }

    #[cfg(unix)]
    {
        Ok(metadata.permissions().mode() & 0o111 != 0)
    }

    #[cfg(not(unix))]
    {
        Ok(true)
    }
}

fn extract_hook_commands(document: &Value) -> Vec<String> {
    let mut commands = HashSet::new();
    if let Some(root_hooks) = document.get("hooks").and_then(Value::as_object) {
        for value in root_hooks.values() {
            if let Some(entries) = value.as_array() {
                for entry in entries {
                    collect_commands(entry, &mut commands);
                    if let Some(hooks) = entry.get("hooks").and_then(Value::as_array) {
                        for hook in hooks {
                            collect_commands(hook, &mut commands);
                        }
                    }
                }
            }
        }
    }

    let mut out: Vec<String> = commands
        .into_iter()
        .filter(|command| command.ends_with(".sh"))
        .collect();
    out.sort_unstable();
    out
}

fn collect_commands(document: &Value, out: &mut HashSet<String>) {
    if let Some(command) = document.get("command").and_then(Value::as_str)
        && command.ends_with(".sh")
    {
        out.insert(normalize_hook_path(command));
    }

    if let Some(map) = document.get("hooks").and_then(Value::as_object) {
        for value in map.values() {
            collect_commands(value, out);
        }
    }

    if let Some(array) = document.get("hooks").and_then(Value::as_array) {
        for value in array {
            collect_commands(value, out);
        }
    }
}

fn normalize_hook_path(value: &str) -> String {
    let mut normalized = value.replace("\"$CLAUDE_PROJECT_DIR\"/", "");
    normalized = normalized.replace("$CLAUDE_PROJECT_DIR/", "");
    normalized
        .trim_matches('"')
        .trim_matches('\\')
        .trim()
        .to_string()
}

fn assert_exit_code(expected: i32, desc: &str, actual: i32, pass: &mut u32, fail: &mut u32) {
    if actual == expected {
        println!("  PASS: {desc} (exit {actual})");
        *pass += 1;
    } else {
        eprintln!("  FAIL: {desc} - expected exit {expected}, got {actual}");
        *fail += 1;
    }
}

fn bash_executable() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(path) = git_bash_executable() {
            return path;
        }

        let default = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
        if default.exists() {
            return default;
        }
    }

    PathBuf::from("bash")
}

#[cfg(windows)]
fn git_bash_executable() -> Option<PathBuf> {
    let output = Command::new("where.exe").arg("git.exe").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let git_exe = stdout.lines().find(|line| !line.trim().is_empty())?.trim();
    let git_root = PathBuf::from(git_exe).parent()?.parent()?.to_path_buf();
    let bash = git_root.join("bin").join("bash.exe");
    bash.exists().then_some(bash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_project_relative_hook_path() {
        assert_eq!(
            normalize_hook_path("\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/pre-tool-use.sh"),
            ".claude/hooks/pre-tool-use.sh"
        );
    }

    #[test]
    fn extracts_only_registered_shell_hooks() {
        let document: Value = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "hooks": [{
                        "type": "command",
                        "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/pre-tool-use.sh"
                    }]
                }]
            }
        });

        assert_eq!(
            extract_hook_commands(&document),
            vec![".claude/hooks/pre-tool-use.sh".to_string()]
        );
    }
}
