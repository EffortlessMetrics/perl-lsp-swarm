//! Exercise the emitted public CLI completion in a clean native Windows shell.
#![cfg(windows)]

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn generated_powershell_completion_works_without_profile_namespace_imports()
-> Result<(), Box<dyn std::error::Error>> {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_perllsp"));
    let generated = Command::new(binary).args(["--completion", "powershell"]).output()?;
    assert!(generated.status.success(), "completion generation failed: {generated:?}");
    let alias = Command::new(binary).args(["--completion", "pwsh"]).output()?;
    assert!(alias.status.success(), "pwsh completion generation failed: {alias:?}");
    assert_eq!(generated.stdout, alias.stdout, "both aliases must emit the same completion");

    // Native command resolution must select Cargo's exact public binary, even
    // when the caller has another perllsp on PATH. Isolate filesystem fallback.
    let workspace = tempfile::tempdir()?;
    let binary_dir = binary.parent().ok_or("candidate binary has no parent")?;
    let mut paths = vec![binary_dir.to_path_buf()];
    if let Some(inherited) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&inherited));
    }
    let path = std::env::join_paths(paths)?;
    let script = format!(
        "{}\n{}\n",
        String::from_utf8(generated.stdout)?,
        include_str!("fixtures/powershell_completion_probe.ps1")
    );

    // Windows PowerShell ships with Windows. A missing or failing shell is a
    // failed instrument, never a silent skip. Feed commands rather than load a
    // script file, so execution-policy settings and user profiles are untouched.
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "-"])
        .current_dir(workspace.path())
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let input_result = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(script.as_bytes()),
        None => Err(std::io::Error::other("PowerShell stdin was not piped")),
    };
    if let Err(error) = input_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("PowerShell completion probe exceeded 15 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "native completion failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "clean completion must not write errors: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("native completion probe passed"),
        "PowerShell must execute the probe rather than merely exit: {output:?}"
    );
    Ok(())
}
