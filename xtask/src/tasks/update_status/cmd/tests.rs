use super::*;

#[test]
fn run_cmd_merged_discards_failed_command_output() -> color_eyre::eyre::Result<()> {
    let output = run_cmd_merged(
        Path::new("."),
        &["rustc", "--definitely-invalid-update-status-option"],
        Duration::from_secs(10),
    );
    assert_eq!(
        output, "",
        "failed merged command output must not be treated as valid discovery data"
    );
    Ok(())
}

#[test]
fn run_cmd_merged_rejects_successful_reap_after_terminal_observation_failure() {
    assert!(
        !merged_output_is_acceptable(true, Some(true)),
        "timeout or poll failure must remain terminal when kill races or fails and reap succeeds"
    );
}

#[cfg(unix)]
fn unix_process_is_alive(pid: u32) -> bool {
    let pid = pid.to_string();
    Command::new("kill")
        .args(["-0", &pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn wait_for_unix_process_exit(pid: u32, timeout: Duration) -> bool {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        if !unix_process_is_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    !unix_process_is_alive(pid)
}

#[cfg(unix)]
#[test]
fn run_cmd_merged_timeout_kills_descendant_process_group() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let pid_file = dir.path().join("descendant.pid");
    let pid_path = pid_file.to_string_lossy().into_owned();
    let script = "sleep 30 & child=$!; printf '%s' \"$child\" > \"$1\"; wait \"$child\"";
    let started_at = Instant::now();

    let output = run_cmd_merged(
        Path::new("."),
        &["sh", "-c", script, "sh", &pid_path],
        Duration::from_secs(1),
    );

    color_eyre::eyre::ensure!(output.is_empty(), "timed-out output must be rejected");
    color_eyre::eyre::ensure!(
        started_at.elapsed() < Duration::from_secs(4),
        "process-tree timeout exceeded its bounded return window"
    );
    let pid_text = std::fs::read_to_string(&pid_file)?;
    let pid = pid_text
        .trim()
        .parse::<u32>()
        .map_err(|error| color_eyre::eyre::eyre!("invalid descendant PID {pid_text:?}: {error}"))?;
    color_eyre::eyre::ensure!(
        wait_for_unix_process_exit(pid, Duration::from_secs(2)),
        "descendant process {pid} survived process-group termination"
    );
    Ok(())
}

#[cfg(windows)]
fn windows_process_is_alive(pid: u32) -> std::io::Result<bool> {
    let pid = pid.to_string();
    let filter = format!("PID eq {pid}");
    let output = Command::new("tasklist").args(["/FI", &filter, "/NH"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(output.status.success() && stdout.split_whitespace().any(|field| field == pid.as_str()))
}

#[cfg(windows)]
#[test]
fn run_cmd_merged_timeout_kills_descendant_process_tree() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let pid_file = dir.path().join("descendant.pid");
    let escaped_pid_path = pid_file.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$child = Start-Process -FilePath powershell.exe \
         -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30') \
         -PassThru; [System.IO.File]::WriteAllText('{escaped_pid_path}', \
         [string]$child.Id); Wait-Process -Id $child.Id"
    );
    let started_at = Instant::now();

    let output = run_cmd_merged(
        Path::new("."),
        &["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", &script],
        Duration::from_secs(3),
    );

    color_eyre::eyre::ensure!(output.is_empty(), "timed-out output must be rejected");
    color_eyre::eyre::ensure!(
        started_at.elapsed() < Duration::from_secs(8),
        "process-tree timeout exceeded its bounded return window"
    );
    let pid_text = std::fs::read_to_string(&pid_file)?;
    let pid = pid_text
        .trim()
        .parse::<u32>()
        .map_err(|error| color_eyre::eyre::eyre!("invalid descendant PID {pid_text:?}: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && windows_process_is_alive(pid)? {
        std::thread::sleep(Duration::from_millis(50));
    }
    color_eyre::eyre::ensure!(
        !windows_process_is_alive(pid)?,
        "descendant process {pid} survived taskkill tree termination"
    );
    Ok(())
}
