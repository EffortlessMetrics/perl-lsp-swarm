//! Bounded read-only Git process seam for the candidate handoff.
//!
//! Every call is an inspection: nothing here fetches, writes to a source
//! repository, updates a ref, or requires credentials. Object import happens
//! only into a caller-owned temporary object database.
//!
//! Boundedness is enforced rather than assumed. A handoff is produced from,
//! and validated against, input an executor may not control: one commit
//! carrying a very large blob, a pathological path inventory, or a Git child
//! that stalls must not be able to hang or exhaust the process. Every
//! invocation therefore runs under a wall-clock deadline and an output cap,
//! with the child killed and reaped when either is breached.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Wall-clock ceiling for one Git invocation.
pub const GIT_DEADLINE: Duration = Duration::from_mins(2);

/// Ceiling on bytes captured from one Git stream.
///
/// This is the transport ceiling too: `pack-objects` writes the candidate pack
/// to standard output, so a candidate whose pack exceeds this cap is refused
/// rather than buffered.
pub const MAX_GIT_OUTPUT_BYTES: usize = 512 * 1024 * 1024;

/// Ceiling on captured diagnostic text, which never needs to be large.
const MAX_GIT_STDERR_BYTES: usize = 64 * 1024;

/// Polling interval while waiting for a child to exit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Configuration forced on every invocation, so ambient settings cannot move a
/// result the manifest calls host-independent.
///
/// `pack.threads` keeps pack generation from varying with host parallelism.
/// The rename settings matter for the same reason and were previously left
/// ambient: `-M` asks for rename detection, but `diff.renameLimit` decides how
/// hard Git tries, and Git silently reports renames as add/delete pairs once
/// that limit is hit. The producer reads the source repository, where local
/// config applies, while `check` recomputes in a bare temporary database where
/// it does not — so a host-local limit could make a sound envelope fail
/// recomputation on the receiver, or two hosts disagree about the same
/// candidate. Pinning the value makes both sides ask Git the same question.
const FORCED_CONFIG: &[&str] =
    &["pack.threads=1", "diff.renames=true", "diff.renameLimit=32767", "core.quotePath=false"];

/// Git's repository-local environment, cleared on every invocation.
///
/// `check` claims to validate a candidate using only the objects the envelope
/// carries. Inheriting `GIT_ALTERNATE_OBJECT_DIRECTORIES` or
/// `GIT_OBJECT_DIRECTORY` would break exactly that: Git could resolve a blob
/// the transport omitted from the receiver's own store, and an incomplete
/// envelope would validate on a machine that happened to have the object.
///
/// This mirrors `git rev-parse --local-env-vars`; the
/// `local_env_list_matches_git` control fails if Git's own list grows past it.
pub const GIT_LOCAL_ENV_VARS: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_INDEX_VERSION",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    "GIT_DEFAULT_HASH",
    "GIT_DEFAULT_REF_FORMAT",
];

/// Raw result of one Git invocation.
#[derive(Debug)]
pub struct GitOutput {
    /// Process exit code, absent when the process was signalled.
    pub code: Option<i32>,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Captured standard output as raw bytes, for binary-producing commands.
    pub stdout_bytes: Vec<u8>,
}

impl GitOutput {
    /// Whether Git reported success.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.code == Some(0)
    }

    /// First useful diagnostic line, for bounded receipt text.
    #[must_use]
    pub fn diagnostic(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return stderr.lines().next().unwrap_or(stderr).to_string();
        }
        match self.code {
            Some(code) => format!("git exited with status {code}"),
            None => "git terminated without an exit status".to_string(),
        }
    }
}

/// Run Git inside `repository`, capturing bounded output.
///
/// [`FORCED_CONFIG`] is applied to every invocation so results do not vary with
/// host configuration. Errors are returned as text rather than raised, because
/// callers classify instrument failure explicitly.
pub fn run_git(repository: &Path, arguments: &[&str]) -> Result<GitOutput, String> {
    run_bounded(repository, arguments, None)
}

/// Run Git with `stdin_bytes` written to the child's standard input.
///
/// Takes ownership rather than copying. The largest caller hands over a whole
/// candidate pack, and cloning it here would hold two copies of a transport
/// that is already allowed to reach [`MAX_GIT_OUTPUT_BYTES`].
pub fn run_git_with_stdin(
    repository: &Path,
    arguments: &[&str],
    stdin_bytes: Vec<u8>,
) -> Result<GitOutput, String> {
    run_bounded(repository, arguments, Some(stdin_bytes))
}

/// Spawn Git and collect its streams under a deadline and an output cap.
///
/// Both streams are drained concurrently on their own threads. Draining
/// serially would deadlock as soon as Git filled the pipe it was not being
/// read from, which is the ordinary case for `pack-objects`.
fn run_bounded(
    repository: &Path,
    arguments: &[&str],
    stdin_bytes: Option<Vec<u8>>,
) -> Result<GitOutput, String> {
    let label = arguments.join(" ");
    let mut command = Command::new("git");
    command.args(FORCED_CONFIG.iter().flat_map(|setting| ["-c", setting]));
    // Neutering ambient configuration also took `safe.directory` with it, and
    // Git reads that setting from global or system config *only* — a repository
    // cannot whitelist itself. Without this, `create` fails on any checkout the
    // running user does not own: the ordinary container, CI, and devcontainer
    // shape, and one of the environments this format exists to serve. Naming
    // the one directory the caller already asked us to inspect restores that
    // without reopening host configuration to anything else.
    command.arg("-c").arg(format!("safe.directory={}", repository.display()));
    command
        .args(arguments)
        .current_dir(repository)
        // Validation must never block on, or acquire, a credential. Every
        // command here is local plumbing, so a prompt would only ever be a
        // sign that something unexpected reached the network.
        .env("GIT_TERMINAL_PROMPT", "0")
        // Ambient configuration is not the receiver's to inherit. Clearing the
        // repository-local environment closed only one route into the
        // validator: `git init --bare` honours `init.templateDir` from *global*
        // config, and a template carrying `objects/info/alternates` lets the
        // host's own object store answer for a blob the transport omitted — so
        // an incomplete envelope validates on the machine that happens to have
        // the object, which is exactly what this format promises cannot happen.
        // Pointing both config files at an empty file makes every host-level
        // setting inert, so what remains is the explicit list above.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for variable in GIT_LOCAL_ENV_VARS {
        command.env_remove(variable);
    }
    // Set rather than cleared, and deliberately after the loop that clears the
    // rest. `refs/replace` makes Git serve substitute content under an
    // original object's id, which is precisely the deception this format must
    // not transport: the manifest would describe replacement content while the
    // pack carried the literal object, and the envelope would fail its own
    // recomputation. Clearing the variable *enables* replacement, so the list
    // above must not have the last word on this one.
    command.env("GIT_NO_REPLACE_OBJECTS", "1");
    command
        .stdin(if stdin_bytes.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child =
        command.spawn().map_err(|error| format!("failed to execute git {label}: {error}"))?;

    let stdin_handle = match (stdin_bytes, child.stdin.take()) {
        (Some(bytes), Some(mut stdin)) => Some(std::thread::spawn(move || {
            use std::io::Write as _;
            // A closed pipe is the child exiting early, which the exit status
            // already reports; it is not itself an error here.
            let _ = stdin.write_all(&bytes);
            drop(stdin);
        })),
        (Some(_), None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("git {label} stdin was not available"));
        }
        (None, _) => None,
    };

    let stdout_reader = child
        .stdout
        .take()
        .map(|stream| spawn_capped_reader(stream, MAX_GIT_OUTPUT_BYTES))
        .ok_or_else(|| format!("git {label} stdout was not available"))?;
    let stderr_reader = child
        .stderr
        .take()
        .map(|stream| spawn_capped_reader(stream, MAX_GIT_STDERR_BYTES))
        .ok_or_else(|| format!("git {label} stderr was not available"))?;

    let status = wait_with_deadline(&mut child, &label)?;

    if let Some(handle) = stdin_handle {
        let _ = handle.join();
    }
    let stdout_bytes = stdout_reader.collect(&label, "stdout")?;
    let stderr_bytes = stderr_reader.collect(&label, "stderr")?;

    Ok(GitOutput {
        code: status,
        stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
        stdout_bytes,
    })
}

/// Wait for the child, killing and reaping it if the deadline passes.
fn wait_with_deadline(child: &mut Child, label: &str) -> Result<Option<i32>, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code()),
            Ok(None) => {}
            Err(error) => {
                // Returning here would leave the child running and its pipe
                // threads attached, which is the leak the deadline arm below
                // exists to prevent. A polling failure is not a reason to
                // abandon the process.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("failed to await git {label}: {error}"));
            }
        }
        if started.elapsed() >= GIT_DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "git {label} exceeded the {}s deadline and was terminated",
                GIT_DEADLINE.as_secs()
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// A stream being drained on its own thread under a byte cap.
struct CappedReader {
    receiver: mpsc::Receiver<Result<Vec<u8>, String>>,
    handle: std::thread::JoinHandle<()>,
}

impl CappedReader {
    /// Join the reader thread and return its bytes, or why it refused.
    fn collect(self, label: &str, stream: &str) -> Result<Vec<u8>, String> {
        let received = self.receiver.recv();
        let _ = self.handle.join();
        match received {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(detail)) => Err(format!("git {label} {stream}: {detail}")),
            Err(_) => Err(format!("git {label} {stream} reader did not report")),
        }
    }
}

/// Drain `stream` on its own thread, refusing more than `cap` bytes.
///
/// Over-limit input is still drained, into a sink rather than a buffer, so
/// the child is never left blocked on a full pipe and the ceiling this
/// reader just enforced is not defeated on the way out.
fn spawn_capped_reader<R: Read + Send + 'static>(mut stream: R, cap: usize) -> CappedReader {
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut collected: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 64 * 1024];
        let outcome = loop {
            match stream.read(&mut buffer) {
                Ok(0) => break Ok(collected),
                Ok(count) => {
                    if collected.len() + count > cap {
                        // Keep draining so the child is not left blocked on a
                        // full pipe while the caller tears it down, but refuse
                        // the result.
                        break Err(format!("output exceeded the {cap}-byte ceiling"));
                    }
                    collected.extend_from_slice(&buffer[..count]);
                }
                Err(error) => break Err(format!("read failed: {error}")),
            }
        };
        let _ = sender.send(outcome);
        // Drain any remainder so the writer can finish and exit. Copying into
        // a sink keeps that drain O(1) in memory: buffering it would defeat
        // the very ceiling this reader just enforced.
        let _ = std::io::copy(&mut stream, &mut std::io::sink());
    });
    CappedReader { receiver, handle }
}

/// Observed Git version string, or a stable placeholder.
#[must_use]
pub fn git_version(repository: &Path) -> String {
    match run_git(repository, &["--version"]) {
        Ok(output) if output.succeeded() => output.stdout.trim().to_string(),
        _ => "not_available".to_string(),
    }
}

/// Whether `value` is a full lowercase 40-character hex object ID.
///
/// Abbreviated IDs are refused everywhere identity is claimed: a short SHA
/// cannot distinguish one object from a colliding prefix.
#[must_use]
pub fn is_full_object_id(value: &str) -> bool {
    value.len() == 40
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
