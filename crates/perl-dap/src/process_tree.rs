//! One owned-subprocess-tree termination convention for `perl-dap` (#15538).
//!
//! `Child::kill()` reaches only the directly owned child — `TerminateProcess`
//! on Windows, `SIGKILL` on Unix — so a descendant (a pager or readline
//! helper under a real `perl -d`, a grandchild of a measurement harness)
//! outlives every bounded kill. This module is the single owner of the
//! tree-termination discipline used by all owned-subprocess sites:
//!
//! - spawn owned children through [`prepare_owned_command`]. On Unix the
//!   child becomes its own process-group leader (`setpgid(0, 0)` pre-exec),
//!   which is what makes the group kill below both complete (descendants
//!   inherit the group) and safe (no unrelated process belongs to it).
//!   Windows needs no spawn-time state.
//! - terminate through [`terminate_tree_and_reap`] or
//!   [`terminate_descendants`]. Windows attributes descendants at kill time
//!   by walking a toolhelp snapshot of the live parent→child PID tree from
//!   the direct child; the walk runs before the direct child dies, so
//!   descendants remain attributable. Unix kills the whole process group.
//!
//! Descendant termination is best-effort by contract: a descendant that has
//! already exited mid-walk is not an error, and an unrelated-group outcome
//! cannot arise for children spawned through [`prepare_owned_command`]. The
//! direct child's own kill and reap stay reported and remain the authority.

use std::process::{Child, Command, ExitStatus};

/// Apply the spawn-time half of the owned-tree discipline to `command`.
///
/// Must be called before `spawn` for the termination half to reach
/// descendants on Unix. Infallible: on Windows it is a no-op, and on Unix a
/// `setpgid` failure aborts the child's exec and surfaces as an ordinary
/// spawn error.
pub(crate) fn prepare_owned_command(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // `setpgid(0, 0)` runs pre-exec in the child: the child leads a fresh
        // process group, so one `killpg` reaches it and every descendant.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        // Tree termination walks the snapshot at kill time; nothing to
        // prepare here.
        let _ = command;
    }
}

/// Kill every descendant of the owned `child` — on Unix, its whole process
/// group — while leaving the direct child's fate to the caller.
///
/// Best-effort and idempotent: already-exited descendants and an
/// already-dead group are not errors. The group kill also signals a live
/// direct child; that is always the caller's next action, and a
/// pending-reap zombie is unaffected by `SIGKILL`.
pub(crate) fn terminate_descendants(child: &Child) {
    #[cfg(unix)]
    {
        use nix::errno::Errno;
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid;
        match signal::killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL) {
            // The group is gone: the direct child already reaped out, or no
            // group was ever led (child not spawned through
            // `prepare_owned_command`); the caller's direct kill still runs.
            Err(Errno::ESRCH) | Ok(()) => {}
            Err(error) => {
                tracing::warn!(
                    pid = child.id(),
                    %error,
                    "Failed to signal the owned process group"
                );
            }
        }
    }
    #[cfg(windows)]
    {
        for pid in descendant_pids(child.id()) {
            // A descendant can exit between the snapshot and the terminate;
            // that outcome is exactly the one this call wants.
            let _ = terminate_pid(pid);
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child;
    }
}

/// Kill the owned `child`'s descendants, then the direct child, then reap
/// the direct child. The single bounded-deadline exit path for owned
/// subprocesses.
///
/// # Errors
///
/// Propagates `Child::wait` failures. Descendant termination is
/// best-effort, and a direct-child kill failure is logged: the reap below
/// is the authoritative result either way.
pub(crate) fn terminate_tree_and_reap(child: &mut Child) -> std::io::Result<ExitStatus> {
    if let Ok(Some(status)) = child.try_wait() {
        return Ok(status);
    }
    terminate_descendants(child);
    if let Err(error) = child.kill() {
        tracing::warn!(pid = child.id(), %error, "Failed to kill the owned child");
    }
    child.wait()
}

/// Live descendant PIDs of `root`, innermost-first irrelevant; empty when
/// the snapshot cannot be taken (table-walk failure is logged, and the
/// caller's direct-child kill remains the fallback).
#[cfg(windows)]
fn descendant_pids(root: u32) -> Vec<u32> {
    use std::collections::{HashMap, HashSet, VecDeque};
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    // SAFETY: CreateToolhelp32Snapshot takes two scalars and dereferences
    // nothing. It reports failure in band as INVALID_HANDLE_VALUE, which the
    // next line checks before the handle is used, and every normal return
    // path that reaches past that check closes the handle exactly once. The
    // handle is a raw HANDLE, not RAII-owned, so that guarantee covers this
    // function's normal control flow, not an unwind from a panic between
    // acquisition and `CloseHandle`.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        tracing::warn!(
            pid = root,
            error = %std::io::Error::last_os_error(),
            "Failed to snapshot the process table for owned-tree termination"
        );
        return Vec::new();
    }
    let mut parent_to_children: HashMap<u32, Vec<u32>> = HashMap::new();
    // SAFETY: PROCESSENTRY32W is a C struct of integers and a fixed-size
    // WCHAR array, so the all-zero bit pattern is a valid value for it and
    // `zeroed` is sound here. dwSize is set to the struct's own size on the
    // next line, which is the contract Process32FirstW documents and the only
    // way it knows how much of the buffer it may write. `snapshot` is a live
    // handle: INVALID_HANDLE_VALUE returned above. `&mut entry` is a valid,
    // aligned, exclusive borrow that outlives both calls, and the loop stops
    // on the documented zero return rather than reading past the table.
    // CloseHandle runs once, on the single exit from this block.
    unsafe {
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                parent_to_children
                    .entry(entry.th32ParentProcessID)
                    .or_default()
                    .push(entry.th32ProcessID);
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    // Walk the parent→child edges from the direct child. A visited set
    // bounds the walk even if the table carries a parent-PID cycle.
    let mut descendants = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let mut queue: VecDeque<u32> =
        parent_to_children.get(&root).cloned().unwrap_or_default().into();
    while let Some(pid) = queue.pop_front() {
        if !visited.insert(pid) {
            continue;
        }
        descendants.push(pid);
        if let Some(children) = parent_to_children.get(&pid) {
            queue.extend(children.iter().copied());
        }
    }
    descendants
}

/// Terminate one PID. `Err` when the process could not be terminated (most
/// commonly because it exited first); callers decide which outcomes matter.
#[cfg(windows)]
fn terminate_pid(pid: u32) -> std::io::Result<()> {
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
    use winapi::um::winnt::PROCESS_TERMINATE;
    // SAFETY: OpenProcess takes scalars only and reports failure as a null
    // handle, which is checked before TerminateProcess ever sees it. The
    // handle is closed exactly once on both the success and failure paths,
    // and last_os_error is read before CloseHandle so the close cannot
    // overwrite the error being reported.
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let terminated = TerminateProcess(handle, 1);
        let error = std::io::Error::last_os_error();
        CloseHandle(handle);
        if terminated == 0 {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Environment selector for a re-invoked copy of this test binary.
    const CONTROL_ROLE: &str = "PERL_LSP_DAP_TREE_KILL_CONTROL_ROLE";
    /// Scratch directory handed to every control role.
    const CONTROL_DIR: &str = "PERL_LSP_DAP_TREE_KILL_CONTROL_DIR";
    /// libtest filter that selects only the control entry point, so the
    /// re-invocation does not run the whole suite recursively.
    const CONTROL_TEST: &str = "process_tree::tests::tree_kill_control_role";

    fn scratch_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "perl-dap-tree-kill-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0),
        ))
    }

    fn write_file(path: &Path, contents: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(contents.as_bytes())
    }

    /// Spawn a re-invoked copy of this test binary in a control role.
    ///
    /// `prepare` mirrors how the role's process was created in the wild:
    /// the owned parent goes through `prepare_owned_command`, while its
    /// grandchild is an ordinary child that inherits the parent's group —
    /// exactly like a pager under `perl -d`. Preparing the grandchild too
    /// would give it its own group and let it escape the Unix group kill,
    /// which would make this control unprovable rather than discriminating.
    fn spawn_current_test_binary(
        role: &str,
        control_dir: &Path,
        prepare: bool,
    ) -> std::io::Result<std::process::Child> {
        let exe = std::env::current_exe()?;
        let mut command = std::process::Command::new(exe);
        command
            .args(["--exact", CONTROL_TEST, "--nocapture", "--test-threads=1"])
            .env(CONTROL_ROLE, role)
            .env(CONTROL_DIR, control_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if prepare {
            prepare_owned_command(&mut command);
        }
        command.spawn()
    }

    fn wait_for_file(path: &Path, bound: Duration) -> std::io::Result<String> {
        let expiry = Instant::now() + bound;
        loop {
            match std::fs::read_to_string(path) {
                Ok(contents) => return Ok(contents),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= expiry {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("waited for {}", path.display()),
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn touch(path: &Path) -> std::io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0)
            .to_string();
        write_file(path, &stamp)
    }

    /// Tick `path` until killed. A safety valve bounds the loop well past
    /// every bound the observing test uses, so a control that escapes its
    /// kill (the mutation check's surviving grandchild, for one) still
    /// exits instead of stranding a live process on a runner.
    fn tick_loop(path: PathBuf) -> ! {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let _ = touch(&path);
            if Instant::now() >= deadline {
                std::process::exit(0);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Entry point for re-invoked copies of this test binary. In an ordinary
    /// suite run the role env is unset and this test is a no-op; with
    /// `--exact` it acts as the requested control role instead of returning.
    #[test]
    fn tree_kill_control_role() {
        let Ok(role) = std::env::var(CONTROL_ROLE) else {
            return;
        };
        let Ok(control_dir) = std::env::var(CONTROL_DIR) else {
            return;
        };
        let control_dir = PathBuf::from(control_dir);
        match role.as_str() {
            "descendant" => {
                let _ = write_file(
                    &control_dir.join("descendant.pid"),
                    &std::process::id().to_string(),
                );
                tick_loop(control_dir.join("descendant.heartbeat"));
            }
            "parent" => {
                // Spawn a real grandchild as an unprepared ordinary child,
                // then wait to be killed. The parent never exits on its
                // own; the deadline path owns it.
                let descendant = match spawn_current_test_binary("descendant", &control_dir, false)
                {
                    Ok(descendant) => descendant,
                    Err(_) => tick_loop(control_dir.join("parent.heartbeat")),
                };
                let _ =
                    write_file(&control_dir.join("parent.pid"), &std::process::id().to_string());
                if wait_for_file(&control_dir.join("descendant.pid"), Duration::from_secs(10))
                    .is_ok()
                {
                    let _ = write_file(&control_dir.join("parent.armed"), "armed");
                }
                // Hold the grandchild handle so its identity stays bound to
                // this parent, then tick until the tree kill lands.
                let _descendant_keeper = descendant;
                tick_loop(control_dir.join("parent.heartbeat"));
            }
            _ => {}
        }
    }

    /// Whether `pid` names a live process right now. PID reuse is bounded
    /// here by the short watch window and the fresh scratch identity.
    fn process_alive(pid: u32) -> bool {
        #[cfg(windows)]
        {
            pid_in_windows_snapshot(pid)
        }
        #[cfg(unix)]
        {
            // Signal 0 probes existence without delivering anything; a
            // reaped PID reports ESRCH. Uses the nix safe wrapper rather
            // than raw libc so no unsafe block is needed.
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
        }
    }

    #[cfg(windows)]
    fn pid_in_windows_snapshot(pid: u32) -> bool {
        use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
        use winapi::um::tlhelp32::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut found = false;
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    if entry.th32ProcessID == pid {
                        found = true;
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            found
        }
    }

    /// The discriminating control from #15538: a bounded kill of an owned
    /// subprocess must reach a real grandchild, which the earlier synthetic
    /// controls (no grandchild) could not observe. On Windows the tree kill
    /// is the snapshot walk; on Unix the group kill; both land here.
    #[test]
    fn terminating_an_owned_tree_kills_a_real_grandchild() -> TestResult {
        let scratch = scratch_dir("grandchild");
        std::fs::create_dir_all(&scratch)?;
        let mut parent = spawn_current_test_binary("parent", &scratch, true)
            .map_err(|error| format!("spawn tree-kill control parent: {error}"))?;

        // The control structure guarantees a real grandchild: the parent
        // only arms after its own descendant has reported a PID.
        wait_for_file(&scratch.join("parent.armed"), Duration::from_secs(20))
            .map_err(|error| format!("tree-kill control never armed: {error}"))?;
        let descendant_pid: u32 =
            wait_for_file(&scratch.join("descendant.pid"), Duration::from_secs(5))?
                .trim()
                .parse()
                .map_err(|error| format!("control descendant pid: {error}"))?;
        if !process_alive(descendant_pid) {
            return Err("control descendant died before the kill".into());
        }

        // The control is killed by design; the reap is the contract.
        terminate_tree_and_reap(&mut parent)
            .map_err(|error| format!("terminate owned tree: {error}"))?;

        // The grandchild must die with the tree. A generous bound absorbs
        // AV/DLP scheduling jitter on contended Windows runners (#15817).
        let expiry = Instant::now() + Duration::from_secs(10);
        loop {
            if !process_alive(descendant_pid) {
                break;
            }
            if Instant::now() >= expiry {
                std::fs::remove_dir_all(&scratch).ok();
                return Err(
                    format!("grandchild {descendant_pid} survived the owned-tree kill").into()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        std::fs::remove_dir_all(&scratch)?;
        Ok(())
    }

    /// Terminating an already-exited owned child stays a clean no-op reaped
    /// exit, not an instrument failure.
    #[test]
    fn terminating_an_already_exited_tree_is_a_clean_reap() -> TestResult {
        let mut command = already_exited_child_command();
        prepare_owned_command(&mut command);
        let mut child = command.spawn().map_err(|error| format!("spawn: {error}"))?;
        child.wait()?;
        let status = terminate_tree_and_reap(&mut child)?;
        assert!(status.success());
        Ok(())
    }

    /// A cross-platform command that exits successfully on its own.
    fn already_exited_child_command() -> std::process::Command {
        let mut command = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd.exe");
            command.args(["/C", "exit", "0"]);
            command
        } else {
            std::process::Command::new("true")
        };
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        command
    }
}
