//! Developer doctor task.
//! Mirrors `scripts/devex-doctor.sh` checks with native Rust execution.

use color_eyre::eyre::{Context, Result, bail};
use perl_lsp_rs_core::config::{PerlOracleEnv, PerlToolchainProfile, WorkspaceConfig};
use std::{
    env,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn run() -> Result<()> {
    let root = repository_root()?;
    env::set_current_dir(&root)
        .with_context(|| format!("failed to switch to repository root: {}", root.display()))?;

    let mut missing_required = false;

    println!("Repository: {}", root.display());
    println!();
    println!("== Required ==");

    check_command("cargo", "cargo", &mut missing_required);
    check_command("rustfmt", "rustfmt", &mut missing_required);
    check_command("rustup", "rustup", &mut missing_required);

    show_version("rustc", "rustc", &["--version"]);
    show_version("cargo", "cargo", &["--version"]);

    println!();
    println!("== Recommended ==");
    check_command_optional("just", "just");
    check_command_optional("nix", "nix");
    check_command_optional("git", "git");
    check_command_optional("rg", "rg");
    check_command_optional("cargo-audit", "cargo-audit");

    println!();
    println!("== Rust components ==");
    if has_command("rustup") {
        let installed_components = get_installed_rustup_components();
        check_rust_component(
            "rustfmt",
            true,
            &mut missing_required,
            installed_components.as_deref(),
        );
        check_rust_component(
            "clippy",
            true,
            &mut missing_required,
            installed_components.as_deref(),
        );
    } else {
        warn("rustup unavailable; cannot verify components");
    }

    println!();
    println!("== Git hooks ==");
    check_pre_push_hook();
    check_pre_commit_hook();

    println!();
    println!("== Build storage ==");
    check_build_storage(&root);

    println!();
    println!("== DAP live-session perl (advisory) ==");
    check_dap_live_session_perl();

    println!();
    if Path::new("rust-toolchain.toml").exists() {
        let status = Command::new("bash")
            .arg("scripts/check-rust-toolchain.sh")
            .arg("doctor")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("failed to run scripts/check-rust-toolchain.sh")?;
        if !status.success() {
            missing_required = true;
        }
    } else {
        warn("rust-toolchain.toml not found");
    }

    println!();
    println!("== Suggested next commands ==");
    println!("  just devex            # quick environment diagnostics");
    println!("  just pr-fast          # fast validation before a full gate");
    println!("  just ci-gate          # repo-native local gate");
    println!("  nix develop -c just ci-gate");

    if missing_required {
        fail("Missing required tools. Install Rust via https://rustup.rs");
        bail!("required checks did not pass");
    }

    println!();
    pass("Doctor completed: required tooling is available");

    Ok(())
}

fn repository_root() -> Result<PathBuf> {
    if let Some(root) = git_output(&["rev-parse", "--show-toplevel"]) {
        return Ok(PathBuf::from(root));
    }

    env::current_dir().context("failed to determine current directory")
}

fn check_build_storage(repo_root: &Path) {
    match resolved_cargo_target_dir(env::var_os("CARGO_TARGET_DIR"), repo_root) {
        Some(target_dir) if target_dir.starts_with(repo_root) => warn(&format!(
            "CARGO_TARGET_DIR is repo-local: {} (run via ./scripts/cargo-safe or just devex)",
            target_dir.display()
        )),
        Some(target_dir) => {
            pass(&format!("CARGO_TARGET_DIR is outside the worktree: {}", target_dir.display()))
        }
        None => warn("CARGO_TARGET_DIR is not set (run via ./scripts/cargo-safe or just devex)"),
    }

    let repo_target = repo_root.join("target");
    if repo_target.exists() {
        warn(&format!(
            "repo-local target directory exists: {} (inspect with just storage-doctor)",
            repo_target.display()
        ));
    } else {
        pass("no top-level repo-local target directory detected");
    }
}

fn resolved_cargo_target_dir(value: Option<OsString>, repo_root: &Path) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    if path.is_absolute() { Some(path) } else { Some(repo_root.join(path)) }
}

fn check_command(program: &str, label: &str, missing_required: &mut bool) {
    if has_command(program) {
        pass(&format!("{label}: found ({})", command_path(program).unwrap_or(program.to_string())));
    } else {
        warn(&format!("{label}: not found{}", install_hint(program)));
        *missing_required = true;
    }
}

fn check_command_optional(program: &str, label: &str) {
    if has_command(program) {
        pass(&format!("{label}: found ({})", command_path(program).unwrap_or(program.to_string())));
    } else {
        warn(&format!("{label}: not found{}", install_hint(program)));
    }
}

fn install_hint(program: &str) -> &'static str {
    match program {
        "cargo" | "rustfmt" | "rustup" => " (install via https://rustup.rs)",
        "just" => " (install: cargo install just)",
        "cargo-audit" => " (install: cargo install cargo-audit)",
        "rg" => " (install ripgrep via your package manager)",
        "nix" => " (install from https://nixos.org/download/)",
        _ => "",
    }
}

fn has_command(program: &str) -> bool {
    find_command_path(program).is_some()
}

fn command_path(program: &str) -> Option<String> {
    find_command_path(program).map(|candidate| candidate.to_string_lossy().to_string())
}

fn find_command_path(program: &str) -> Option<std::path::PathBuf> {
    let direct = Path::new(program);
    if is_executable_file(direct) {
        return Some(direct.to_path_buf());
    }

    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths).find_map(|path| {
            #[cfg(windows)]
            {
                for candidate in [
                    path.join(program),
                    path.join(format!("{program}.exe")),
                    path.join(format!("{program}.bat")),
                    path.join(format!("{program}.cmd")),
                ] {
                    if is_executable_file(&candidate) {
                        return Some(candidate);
                    }
                }
                None
            }

            #[cfg(not(windows))]
            {
                let candidate = path.join(program);
                is_executable_file(&candidate).then_some(candidate)
            }
        })
    })
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn get_installed_rustup_components() -> Option<String> {
    let output = Command::new("rustup").args(["component", "list", "--installed"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn is_component_installed(component: &str, installed_components: Option<&str>) -> bool {
    let Some(lines) = installed_components else {
        return false;
    };
    lines.lines().any(|line| {
        let value = line.split_whitespace().next().unwrap_or("");
        value == component || value.starts_with(&(format!("{component}-")))
    })
}

fn check_rust_component(
    component: &str,
    required: bool,
    missing_required: &mut bool,
    installed_components: Option<&str>,
) {
    if is_component_installed(component, installed_components) {
        pass(&format!("rustup component installed: {component}"));
        return;
    }

    let message = format!(
        "rustup component missing: {component} (install: rustup component add {component})"
    );

    warn(&message);
    if required {
        *missing_required = true;
    }
}

fn show_version(program: &str, command: &str, args: &[&str]) {
    match Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => {
            let output = String::from_utf8_lossy(&output.stdout);
            let first_line = output.lines().next().unwrap_or("");
            pass(&format!("{program} version: {first_line}"));
        }
        _ => warn(&format!("{program} version check failed")),
    }
}

fn check_pre_push_hook() {
    if !has_command("git") {
        warn("git unavailable; cannot verify pre-push hook");
        return;
    }

    let repo_root = git_output(&["rev-parse", "--show-toplevel"]);
    let git_common_dir = git_output(&["rev-parse", "--git-common-dir"]);
    let (repo_root, git_common_dir) = match (repo_root, git_common_dir) {
        (Some(root), Some(common)) => (root, common),
        _ => {
            warn("not in a git repository; cannot verify pre-push hook");
            return;
        }
    };

    let hook_path = Path::new(&git_common_dir).join("hooks").join("pre-push");
    let expected_hook = Path::new(&repo_root).join("hooks").join("pre-push");

    if !hook_path.is_file() {
        warn("pre-push hook not installed (fix: cargo xtask ci-hygiene install-githooks)");
        return;
    }

    if !is_executable(&hook_path) {
        warn(&format!("pre-push hook present but not executable: {}", hook_path.display()));
        return;
    }

    if expected_hook.is_file() {
        let installed = fs::read_to_string(&hook_path);
        let expected = fs::read_to_string(&expected_hook);
        match (installed, expected) {
            (Ok(installed), Ok(expected)) => {
                if normalize_hook_text(&installed) == normalize_hook_text(&expected) {
                    pass("pre-push hook installed and current");
                } else {
                    warn(&format!(
                        "pre-push hook installed but stale: {} (fix: cargo xtask ci-hygiene install-githooks)",
                        hook_path.display()
                    ));
                }
            }
            _ => {
                warn("unable to read pre-push hook content for staleness check");
            }
        }
        return;
    }

    pass("pre-push hook installed");
}

fn check_pre_commit_hook() {
    if !has_command("git") {
        return;
    }

    let git_common_dir = match git_output(&["rev-parse", "--git-common-dir"]) {
        Some(dir) => dir,
        None => {
            warn("not in a git repository; cannot verify pre-commit hook");
            return;
        }
    };

    let hook_path = Path::new(&git_common_dir).join("hooks").join("pre-commit");

    if !hook_path.is_file() {
        warn(
            "pre-commit hook missing or not executable (run: cargo xtask ci-hygiene install-githooks)",
        );
        return;
    }

    if !is_executable(&hook_path) {
        warn(&format!(
            "pre-commit hook present but not executable: {} (run: cargo xtask ci-hygiene install-githooks)",
            hook_path.display()
        ));
        return;
    }

    pass(&format!("git hook installed: {}", hook_path.display()));
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn normalize_hook_text(content: &str) -> String {
    let mut lines: Vec<String> =
        content.lines().map(|line| line.trim_end_matches('\r').to_string()).collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn pass(message: &str) {
    println!("✅ {message}");
}

/// #12594 item 6(b) / #12748: live-session DAP suites drive a real
/// `perl -d` debuggee over stdio pipes, which needs a *pipe-capable*
/// perl5db. The failure shape that motivated this check: on Windows the
/// first `perl` on a bash-spawned PATH is often MSYS/cygwin perl, whose
/// debugger assumes a console and hangs or misbehaves over pipes while a
/// working Strawberry perl sits later on PATH. Surface which perl the
/// launch path will see and whether its perl5db actually answers over
/// pipes. Advisory only: plenty of dev/CI boxes legitimately have no perl
/// (the suites skip there), so this never blocks doctor.
fn check_dap_live_session_perl() {
    // Resolve the interpreter exactly like a no-override DAP launch
    // (crates/perl-dap/src/debug_adapter/process.rs): the shared toolchain
    // resolver — explicit perl_path, then perlbrew → plenv → PATH — so the
    // doctor probes the same perl5db a real session would use, not merely
    // the first perl on PATH.
    let Some(perl) = PerlToolchainProfile::resolve(&WorkspaceConfig::default())
        .map(PerlToolchainProfile::into_perl_binary)
        .or_else(|| find_command_path("perl"))
    else {
        warn(
            "no perl interpreter resolved (perl_path / perlbrew / plenv / PATH): live-session \
             DAP suites (real `perl -d` over stdio pipes) cannot run. Install a pipe-capable \
             perl — system perl with perl5db on Linux/macOS, Strawberry Perl on Windows.",
        );
        return;
    };

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match PerlOracleEnv::for_version_probe(perl.clone(), cwd)
        .into_command()
        .arg("-e")
        .arg(r#"print "$^V $^O""#)
        .output()
    {
        Ok(out) if out.status.success() => {
            println!("  perl: {} ({})", perl.display(), String::from_utf8_lossy(&out.stdout));
        }
        _ => {
            println!("  perl: {} (version probe failed)", perl.display());
        }
    }

    match probe_perl5db_pipe(&perl) {
        Perl5dbProbe::Capable => {
            pass("perl5db is pipe-capable: `perl -de 0` answers `q` over piped stdio and exits")
        }
        Perl5dbProbe::Hung => warn(
            "`perl -de 0` did not exit within 10s over piped stdio — this perl's debugger \
             is not pipe-capable (MSYS/cygwin perl class). Live-session DAP suites will hang. \
             Put a native perl first on PATH (Strawberry Perl on Windows) or set `perlPath` \
             explicitly in the launch configuration.",
        ),
        Perl5dbProbe::Failed(detail) => warn(&format!(
            "perl5db pipe probe failed: {detail}. Live-session DAP suites will fail; \
             check that perl5db is installed for this interpreter."
        )),
    }
}

#[derive(Debug)]
enum Perl5dbProbe {
    Capable,
    Hung,
    Failed(String),
}

/// Spawn `perl -de 0` under the production DAP environment contract
/// (`PerlOracleEnv::for_version_probe`: deny-all-ambient, PATH re-added),
/// send `q` over piped stdio, and require a prompt, clean exit. A debugger
/// that needs a console hangs here; a missing perl5db.pl errors out here.
/// The 10s watchdog bounds the hang case. `Capable` is returned only after
/// `q` was actually delivered — any I/O or cleanup failure is `Failed`, so
/// a process that exits 0 without receiving `q` can never read as capable.
fn probe_perl5db_pipe(perl: &Path) -> Perl5dbProbe {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut command = PerlOracleEnv::for_version_probe(perl.to_path_buf(), cwd).into_command();
    command.arg("-de").arg("0").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Perl5dbProbe::Failed(format!("cannot spawn `{} -de 0`: {e}", perl.display()));
        }
    };

    match child.stdin.take() {
        Some(mut stdin) => {
            if let Err(e) = stdin.write_all(b"q\n") {
                let _ = child.kill();
                let _ = child.wait();
                return Perl5dbProbe::Failed(format!("cannot send `q` to the debugger: {e}"));
            }
        }
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Perl5dbProbe::Failed("debugger stdin was not piped".to_string());
        }
    } // stdin dropped: debugger sees EOF after `q`

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Perl5dbProbe::Capable,
            Ok(Some(status)) => {
                let mut stderr = String::new();
                let read_result = match child.stderr.take() {
                    Some(mut pipe) => pipe.read_to_string(&mut stderr).map(|_| ()),
                    None => Ok(()),
                };
                let first_line = stderr.lines().next().unwrap_or("").trim().to_string();
                return Perl5dbProbe::Failed(format!(
                    "`perl -de 0` exited {status}{}{}",
                    if first_line.is_empty() { String::new() } else { format!(": {first_line}") },
                    match read_result {
                        Ok(()) => String::new(),
                        Err(e) => format!(" (stderr unreadable: {e})"),
                    }
                ));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    if let Err(e) = child.kill() {
                        return Perl5dbProbe::Failed(format!("watchdog kill failed: {e}"));
                    }
                    if let Err(e) = child.wait() {
                        return Perl5dbProbe::Failed(format!("watchdog wait failed: {e}"));
                    }
                    return Perl5dbProbe::Hung;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Perl5dbProbe::Failed(format!("wait on `perl -de 0` failed: {e}")),
        }
    }
}

fn warn(message: &str) {
    println!("⚠️  {message}");
}

fn fail(message: &str) {
    println!("❌ {message}");
}

#[cfg(test)]
mod tests {
    use super::{find_command_path, normalize_hook_text, resolved_cargo_target_dir};
    use std::{ffi::OsString, fs, path::Path};

    #[cfg(unix)]
    fn set_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("set perms");
    }

    #[cfg(not(unix))]
    fn set_executable(_path: &std::path::Path) {}

    #[cfg(unix)]
    #[test]
    fn perl5db_probe_contract_capable_and_failed() -> Result<(), Box<dyn std::error::Error>> {
        use super::{Perl5dbProbe, probe_perl5db_pipe};

        let dir = std::env::temp_dir().join(format!("perl5db-probe-test-{}", std::process::id()));
        fs::create_dir_all(&dir)?;

        // Positive control: a "debugger" that reads the piped command and
        // exits 0 is pipe-capable.
        let good = dir.join("fake-perl-good");
        fs::write(&good, "#!/bin/sh\nread _cmd\nexit 0\n")?;
        set_executable(&good);
        let verdict = probe_perl5db_pipe(&good);
        assert!(
            matches!(verdict, Perl5dbProbe::Capable),
            "a debugger that reads `q` and exits 0 must be Capable, got {verdict:?}"
        );

        // A "debugger" that exits nonzero after the command is Failed, with
        // its first stderr line in the detail.
        let bad = dir.join("fake-perl-bad");
        fs::write(
            &bad,
            "#!/bin/sh\nread _cmd\necho 'perl5db.pl did not return a true value' >&2\nexit 3\n",
        )?;
        set_executable(&bad);
        let verdict = probe_perl5db_pipe(&bad);
        match &verdict {
            Perl5dbProbe::Failed(detail) => assert!(
                detail.contains("perl5db.pl"),
                "Failed detail must carry the first stderr line, got: {detail}"
            ),
            other => panic!("a nonzero exit must be Failed, got {other:?}"),
        }

        fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[test]
    fn normalize_hook_text_removes_crlf_and_trailing_blank_lines() {
        let input = "#!/usr/bin/env bash\r\nset -eu\r\n\r\n\r\n";
        let normalized = normalize_hook_text(input);
        assert_eq!(normalized, "#!/usr/bin/env bash\nset -eu");
    }

    #[test]
    fn normalize_hook_text_preserves_internal_blank_lines() {
        let input = "line1\n\nline3\n";
        let normalized = normalize_hook_text(input);
        assert_eq!(normalized, "line1\n\nline3");
    }

    #[test]
    fn resolved_cargo_target_dir_expands_relative_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = tempfile::tempdir()?;
        let resolved =
            resolved_cargo_target_dir(Some(OsString::from("target-agent")), temp_dir.path());

        assert_eq!(resolved, Some(temp_dir.path().join("target-agent")));
        Ok(())
    }

    #[test]
    fn resolved_cargo_target_dir_preserves_absolute_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = tempfile::tempdir()?;
        let target = temp_dir.path().join("outside-target");
        let resolved = resolved_cargo_target_dir(
            Some(target.as_os_str().to_os_string()),
            Path::new("/unused"),
        );

        assert_eq!(resolved, Some(target));
        Ok(())
    }

    #[test]
    fn find_command_path_requires_executable_for_direct_paths() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let candidate = temp_dir.path().join("doctor-test");
        fs::write(&candidate, "#!/usr/bin/env bash\nexit 0\n").expect("write file");

        #[cfg(unix)]
        {
            assert!(find_command_path(candidate.to_str().expect("path")).is_none());

            set_executable(&candidate);
            assert!(find_command_path(candidate.to_str().expect("path")).is_some());
        }

        #[cfg(not(unix))]
        {
            set_executable(&candidate);
            assert!(find_command_path(candidate.to_str().expect("path")).is_some());
        }
    }
}
