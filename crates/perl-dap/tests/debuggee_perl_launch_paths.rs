//! Live launch-path proof for the configured debuggee Perl pin (#12594).
//!
//! Each convenience launch path must carry the same explicit `perlPath` into
//! the real adapter. PATH is deliberately made to resolve a different copy,
//! and the stopped session observes `$^X` so a PATH fallback cannot pass.

#![allow(unsafe_code)]

mod common;

#[cfg(windows)]
use common::run_bounded_command_for_test;
use common::{
    DEBUGGEE_PERL_OVERRIDE_ENV, DapWorkflowSession, probe_debuggee_perl_for_test, workflow_timeout,
};
use serial_test::serial;
use std::env;
use std::error::Error;
use std::fs;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
fn stage_perl_library_layout(
    source_perl: &Path,
    destination: &Path,
) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let source_bin = source_perl.parent().ok_or("selected Perl has no bin directory")?;
    let source_root = source_bin.parent().ok_or("selected Perl has no installation root")?;
    let stdout_path = destination.join("perl-config.stdout");
    let stderr_path = destination.join("perl-config.stderr");
    let mut command = Command::new(source_perl);
    command
        .args([
            "-MConfig",
            "-e",
            "print join(\"\\n\", $^O, grep { defined($_) && length($_) } @Config{qw(privlib archlib)})",
        ])
        .stdout(fs::File::create(&stdout_path)?)
        .stderr(fs::File::create(&stderr_path)?);
    let status = run_bounded_command_for_test(command, Duration::from_secs(5))
        .map_err(|error| format!("cannot query Perl library layout: {error}"))?;
    if !status.success() {
        return Err(format!(
            "cannot query Perl library layout: {}",
            String::from_utf8_lossy(&fs::read(&stderr_path)?)
        )
        .into());
    }
    let config_stdout = fs::read(&stdout_path)?;
    if config_stdout.len() > 4096 {
        return Err("selected Perl returned an oversized library layout".into());
    }
    let config_text = String::from_utf8_lossy(&config_stdout);
    let mut config_lines = config_text.lines();
    let os_name = config_lines.next().unwrap_or_default();
    if os_name != "MSWin32" {
        return Ok(None);
    }
    let staged_install = destination.join("perl-install");
    let mut staged_roots = Vec::new();
    for raw_root in config_lines.map(str::trim).filter(|root| !root.is_empty()) {
        let root = PathBuf::from(raw_root);
        if !root.is_dir() {
            continue;
        }
        let relative = root
            .strip_prefix(source_root)
            .map_err(|_| format!("Perl library root is outside installation root: {root:?}"))?;
        let staged = staged_install.join(relative);
        if staged_roots.iter().any(|existing: &PathBuf| existing == &staged) {
            continue;
        }
        copy_directory(&root, &staged)?;
        staged_roots.push(staged);
    }
    if staged_roots.is_empty() {
        return Err("selected Perl reported no usable library roots".into());
    }
    let staged_bin = staged_install.join(source_bin.strip_prefix(source_root)?);
    fs::create_dir_all(&staged_bin)?;
    let staged_perl = staged_bin.join(source_perl.file_name().ok_or("Perl has no filename")?);
    fs::copy(source_perl, &staged_perl)?;
    Ok(Some(staged_perl))
}

#[cfg(windows)]
fn copy_directory(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn prepare_native_perl_fixture(
    source_perl: &Path,
    destination: &Path,
) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let staged_perl = stage_perl_library_layout(source_perl, destination)?
        .ok_or("selected Perl is not a native MSWin32 build")?;
    let staged_bin = staged_perl.parent().ok_or("staged Perl has no bin directory")?;
    let source_dir = source_perl.parent().ok_or("Perl path has no parent directory")?;
    for entry in fs::read_dir(source_dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|extension| extension.to_str()) == Some("dll") {
            fs::copy(entry.path(), staged_bin.join(entry.file_name()))?;
        }
    }
    let pinned = staged_bin.join("perl5.exe");
    fs::copy(&staged_perl, &pinned)?;
    Ok((staged_perl, pinned))
}

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    #[cfg(windows)]
    fn remove(key: &'static str) -> Self {
        let previous = env::var_os(key);
        unsafe { env::remove_var(key) };
        Self { key, previous }
    }

    fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
        let previous = env::var_os(key);
        unsafe { env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => unsafe { env::set_var(self.key, value) },
            None => unsafe { env::remove_var(self.key) },
        }
    }
}

fn find_configured_or_path_pipe_perl() -> Result<Option<PathBuf>, Box<dyn Error>> {
    if let Some(configured) = env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV) {
        let candidate = PathBuf::from(configured);
        if !candidate.is_file() {
            return Err(format!(
                "{DEBUGGEE_PERL_OVERRIDE_ENV} names a missing interpreter: {}",
                candidate.display()
            )
            .into());
        }
        probe_debuggee_perl_for_test(&candidate, Duration::from_secs(10), false)
            .map_err(|reason| format!("configured interpreter is not pipe-usable: {reason}"))?;
        return Ok(Some(candidate));
    }

    let locator = if cfg!(windows) { "where.exe" } else { "which" };
    let output = Command::new(locator).arg("perl").output()?;
    if !output.status.success() {
        return Ok(None);
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let candidate = PathBuf::from(line.trim());
        if candidate.is_file()
            && probe_debuggee_perl_for_test(&candidate, Duration::from_secs(10), false).is_ok()
        {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn observe_pin_with_session(
    mut session: DapWorkflowSession,
    launch_path: &str,
    script: &str,
    cwd: &str,
) -> Result<String, String> {
    match launch_path {
        "launch" => {
            session.launch(script)?;
            session.set_breakpoints_checked(script, &[4])?;
            session.configuration_done()?;
        }
        "launch_with_stop_on_entry" => session.launch_with_stop_on_entry(script, true)?,
        "launch_with_cwd" => {
            session.launch_with_cwd(script, cwd)?;
            session.set_breakpoints_checked(script, &[4])?;
            session.configuration_done()?;
        }
        other => return Err(format!("unknown launch path {other}")),
    }
    let stopped = session.wait_stopped_with_frame()?;
    session.evaluate_expression("$^X", stopped.frame_id).map(|(value, _)| value)
}

#[test]
#[serial]
#[allow(clippy::print_stderr)]
fn all_convenience_launch_paths_reach_the_pinned_interpreter() -> Result<(), Box<dyn Error>> {
    let Some(source_perl) = find_configured_or_path_pipe_perl()? else {
        eprintln!(
            "SKIP all_convenience_launch_paths_reach_the_pinned_interpreter: Perl unavailable"
        );
        return Ok(());
    };
    let controls = tempfile::tempdir()?;
    #[cfg(windows)]
    let (ambient, pinned) =
        if let Some(staged_perl) = stage_perl_library_layout(&source_perl, controls.path())? {
            let staged_bin =
                staged_perl.parent().ok_or("staged Perl has no bin directory")?.to_path_buf();
            let pinned = staged_bin.join("perl5.exe");
            let ambient = staged_perl;
            fs::copy(&ambient, &pinned)?;
            (ambient, pinned)
        } else {
            let ambient = controls.path().join("perl.exe");
            let pinned = controls.path().join("perl5.exe");
            fs::copy(&source_perl, &ambient)?;
            fs::copy(&source_perl, &pinned)?;
            (ambient, pinned)
        };
    #[cfg(not(windows))]
    let (ambient, pinned) = {
        let ambient = controls.path().join("perl");
        let pinned = controls.path().join("perl5");
        fs::copy(&source_perl, &ambient)?;
        fs::copy(&source_perl, &pinned)?;
        (ambient, pinned)
    };
    #[cfg(windows)]
    {
        let source_dir = source_perl.parent().ok_or("Perl path has no parent directory")?;
        let destination_dir = ambient.parent().ok_or("ambient Perl has no parent directory")?;
        for entry in fs::read_dir(source_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|extension| extension.to_str()) == Some("dll") {
                fs::copy(entry.path(), destination_dir.join(entry.file_name()))?;
            }
        }
    }
    // Keep the copied pin's basename within the adapter's strict Perl-name
    // contract while still making it distinct from the ambient copy.
    for binary in [&ambient, &pinned] {
        probe_debuggee_perl_for_test(binary, Duration::from_secs(10), false)
            .map_err(|reason| format!("{} is not pipe-usable: {reason}", binary.display()))?;
    }

    let path_directory = ambient.parent().ok_or("ambient Perl has no parent directory")?;
    let mut path_value = path_directory.as_os_str().to_os_string();
    path_value.push(if cfg!(windows) { ";" } else { ":" });
    path_value.push(env::var_os("PATH").unwrap_or_default());
    let _path_guard = EnvGuard::set("PATH", &path_value);
    let script = controls.path().join("launch-paths.pl");
    fs::write(
        &script,
        "use strict;\nuse warnings;\nmy $identity_probe = 1;\n$identity_probe++;\n",
    )?;
    let script_text = script.to_string_lossy().into_owned();
    let cwd = controls.path().to_string_lossy().into_owned();
    for launch_path in ["launch", "launch_with_stop_on_entry", "launch_with_cwd"] {
        let session = DapWorkflowSession::new_with_perl(workflow_timeout(), Some(&pinned))?;
        let reported = observe_pin_with_session(session, launch_path, &script_text, &cwd)
            .map_err(|error| format!("{launch_path} failed: {error}"))?;
        common::assert_pinned_identity(&reported, &pinned, &ambient, launch_path)
            .map_err(std::io::Error::other)?;
    }

    let _pin_guard = EnvGuard::set(DEBUGGEE_PERL_OVERRIDE_ENV, pinned.as_os_str());
    for launch_path in ["launch", "launch_with_stop_on_entry", "launch_with_cwd"] {
        let session = DapWorkflowSession::new(workflow_timeout())?;
        let configured_identity =
            observe_pin_with_session(session, launch_path, &script_text, &cwd)
                .map_err(|error| format!("configured {launch_path} failed: {error}"))?;
        common::assert_pinned_identity(
            &configured_identity,
            &pinned,
            &ambient,
            &format!("configured {launch_path}"),
        )
        .map_err(std::io::Error::other)?;
    }
    Ok(())
}

#[test]
#[cfg(windows)]
#[serial]
fn copied_native_fixture_requires_staged_core_library() -> Result<(), Box<dyn Error>> {
    let source = env::var_os(DEBUGGEE_PERL_OVERRIDE_ENV)
        .ok_or("strict fixture proof requires a configured native Perl pin")?;
    let source = PathBuf::from(source);
    let controls = tempfile::tempdir()?;
    let (ambient, pinned) = prepare_native_perl_fixture(&source, controls.path())?;
    probe_debuggee_perl_for_test(&pinned, Duration::from_secs(10), false)
        .map_err(|reason| format!("staged native fixture was not pipe-usable: {reason}"))?;

    let staged_install =
        ambient.parent().and_then(Path::parent).ok_or("staged Perl has no installation root")?;
    let staged_lib = staged_install.join("lib");
    fs::remove_dir_all(&staged_lib)?;
    let failure = probe_debuggee_perl_for_test(&pinned, Duration::from_secs(10), false)
        .err()
        .ok_or("copied fixture unexpectedly found perl5db without staged core library")?;
    if !failure.to_ascii_lowercase().contains("perl5db") {
        return Err(format!("missing staged library lost its diagnostic: {failure}").into());
    }
    Ok(())
}

#[test]
#[cfg(windows)]
#[serial]
fn windows_pipe_launch_configures_perl_debugger_transport() -> Result<(), Box<dyn Error>> {
    let locator = Command::new("where.exe").arg("perl").output()?;
    if !locator.status.success() {
        std::io::Write::write_all(
            &mut std::io::stderr(),
            b"SKIP windows_pipe_launch_configures_perl_debugger_transport: Perl unavailable\n",
        )?;
        return Ok(());
    }
    let perl = String::from_utf8_lossy(&locator.stdout)
        .lines()
        .map(str::trim)
        .map(PathBuf::from)
        .find(|candidate| candidate.is_file())
        .ok_or("where.exe returned no Perl executable")?;
    let workspace = tempfile::tempdir()?;
    let script = workspace.path().join("pipe-transport.pl");
    fs::write(
        &script,
        "use strict;\nuse warnings;\nmy $executed = 41;\n$executed++;\nprint \"executed\\n\";\n",
    )?;
    let _emacs_guard = EnvGuard::remove("EMACS");
    let mut session = DapWorkflowSession::new_with_perl(workflow_timeout(), Some(&perl))?;
    let script_text = script.to_string_lossy().into_owned();
    session.launch_pinned(&perl, &script_text)?;
    session.set_breakpoints_checked(&script_text, &[5])?;
    session.configuration_done()?;
    let stopped = session.wait_stopped_with_frame()?;
    if stopped.line != 5 {
        return Err(format!("pipe launch stopped at unexpected line {}", stopped.line).into());
    }
    let (value, _) = session.evaluate_expression("$executed", stopped.frame_id)?;
    if value.split_whitespace().last() != Some("42") {
        return Err(format!("debuggee did not execute the expected program: {value}").into());
    }
    Ok(())
}
