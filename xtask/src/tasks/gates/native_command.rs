//! Native Windows gate command launcher (#17482).
//!
//! Gate commands are spelled in a small POSIX subset (the same spelling the
//! gate policy uses on Unix): whitespace-separated argv with single/double
//! quoting, leading `NAME=value` assignments, an optional `env` prefix
//! (including `env -u NAME`), and `&&`-chained segments. On Windows this
//! subset is executed natively — never through `cmd /C` — so quoting,
//! `%VAR%` expansion, `&` chaining, and `!` history expansion in the host
//! shell cannot rewrite argv, and `PERL_LSP_BIN="$PWD/…"` resolves to the
//! native `.exe` artifact.
//!
//! Anything outside the subset — pipes, `||`, `;`, redirection, command
//! substitution, backticks, variable expansion other than `$PWD`, a bare
//! `&`, newlines, subshell parentheses, or an unquoted backslash — is
//! refused before any process is spawned. Segments run serially under one
//! shared deadline; the first non-zero segment stops the chain, and a
//! watchdog timeout kills the whole descendant tree (`taskkill /T`).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// One environment mutation attached to a segment, applied in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EnvOp {
    Set(String, String),
    Unset(String),
}

/// What one `&&` segment does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeAction {
    /// Spawn a program directly (no shell).
    Exec { program: String, args: Vec<String> },
    /// `echo` builtin: writes its arguments to the gate log.
    Echo { args: Vec<String> },
    /// `exit` builtin: terminates the whole chain with this code.
    Exit(i32),
    /// `true` builtin.
    True,
    /// `false` builtin.
    False,
}

/// One parsed `&&` segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeStep {
    pub(crate) env_ops: Vec<EnvOp>,
    pub(crate) action: NativeAction,
}

/// Terminal outcome of a native command chain.
#[derive(Debug)]
pub(crate) struct NativeOutcome {
    pub(crate) exit_code: i32,
    pub(crate) timed_out: bool,
}

/// Native launch failure. `child_started` mirrors
/// [`super::GateShellError`]: parse/validation failures happen before any
/// spawn and always carry `false`.
#[derive(Debug)]
pub(crate) struct NativeError {
    pub(crate) message: String,
    pub(crate) child_started: bool,
}

impl NativeError {
    fn pre_launch(message: String) -> Self {
        Self { message, child_started: false }
    }
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for NativeError {}

fn unsupported(command: &str, detail: &str) -> NativeError {
    NativeError::pre_launch(format!(
        "unsupported shell grammar in gate command refused before launch ({detail}): {command}"
    ))
}

/// Resolve `current_dir` (possibly relative) to an absolute, lexically
/// normalized directory without touching the filesystem.
pub(crate) fn effective_working_dir(current_dir: Option<&Path>) -> Result<PathBuf, NativeError> {
    let base = match current_dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => std::env::current_dir()
            .map_err(|error| {
                NativeError::pre_launch(format!("failed to resolve working directory: {error}"))
            })?
            .join(dir),
        None => std::env::current_dir().map_err(|error| {
            NativeError::pre_launch(format!("failed to resolve working directory: {error}"))
        })?,
    };
    let mut normalized = PathBuf::new();
    for component in base.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

fn is_name_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if is_name_start(first) => chars.all(is_name_char),
        _ => false,
    }
}

/// Validate a `$…` expansion starting at `dollar` (byte index of `$` in
/// `command`). Only `$PWD` / `${PWD}` are supported; returns the byte index
/// just past the expansion.
fn validate_dollar(command: &str, dollar: usize) -> Result<usize, NativeError> {
    let rest = &command[dollar + 1..];
    if let Some(braced) = rest.strip_prefix('{') {
        return match braced.strip_prefix("PWD}") {
            Some(_) => Ok(dollar + 1 + 1 + 4),
            None => Err(unsupported(command, "only $PWD expansion is supported in gate commands")),
        };
    }
    match rest.strip_prefix("PWD") {
        Some(after) if !after.chars().next().is_some_and(is_name_char) => Ok(dollar + 1 + 3),
        _ => Err(unsupported(command, "only $PWD expansion is supported in gate commands")),
    }
}

/// Split `command` into `&&` segments, validating the supported grammar.
/// An unquoted backslash is refused: POSIX quoting would silently eat it
/// and corrupt a Windows path, so paths must be quoted (or use `/`).
fn split_chain(command: &str) -> Result<Vec<String>, NativeError> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = command.char_indices().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some((index, ch)) = chars.next() {
        if in_single {
            current.push(ch);
            if ch == '\'' {
                in_single = false;
            }
            continue;
        }
        if in_double {
            match ch {
                '"' => {
                    in_double = false;
                    current.push(ch);
                }
                '\\' => {
                    current.push(ch);
                    if let Some((_, next)) = chars.next() {
                        current.push(next);
                    }
                }
                '`' => {
                    return Err(unsupported(command, "backtick substitution is not supported"));
                }
                '$' => {
                    validate_dollar(command, index)?;
                    current.push(ch);
                }
                _ => current.push(ch),
            }
            continue;
        }
        match ch {
            '\'' => {
                in_single = true;
                current.push(ch);
            }
            '"' => {
                in_double = true;
                current.push(ch);
            }
            '\\' => {
                return Err(unsupported(
                    command,
                    "unquoted backslash is not supported; quote Windows paths or use '/'",
                ));
            }
            '&' => {
                if chars.peek().is_some_and(|(_, next)| *next == '&') {
                    chars.next();
                    segments.push(std::mem::take(&mut current));
                } else {
                    return Err(unsupported(command, "bare '&' is not supported; use '&&'"));
                }
            }
            '|' => {
                return Err(unsupported(command, "pipes and '||' are not supported"));
            }
            ';' => {
                return Err(unsupported(command, "';' separators are not supported; use '&&'"));
            }
            '>' | '<' => {
                return Err(unsupported(command, "redirection is not supported"));
            }
            '`' => {
                return Err(unsupported(command, "backtick substitution is not supported"));
            }
            '(' | ')' => {
                return Err(unsupported(command, "subshells are not supported"));
            }
            '\n' | '\r' => {
                return Err(unsupported(command, "newlines are not supported; use '&&'"));
            }
            '$' => {
                validate_dollar(command, index)?;
                current.push(ch);
            }
            _ => current.push(ch),
        }
    }
    if in_single || in_double {
        return Err(unsupported(command, "unterminated quote"));
    }
    segments.push(current);
    Ok(segments)
}

/// Placeholder for a `$` that appeared inside single quotes, where POSIX
/// quoting makes it literal. Restored after `shlex` tokenization.
const SINGLE_QUOTED_DOLLAR: char = '\u{E001}';

fn mask_single_quoted_dollars(segment: &str) -> String {
    let mut masked = String::with_capacity(segment.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = segment.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                masked.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                masked.push(ch);
            }
            '\\' if in_double => {
                if let Some(next) = chars.next() {
                    if next == '$' {
                        masked.push(SINGLE_QUOTED_DOLLAR);
                    } else {
                        masked.push(ch);
                        masked.push(next);
                    }
                } else {
                    masked.push(ch);
                }
            }
            '$' if in_single => masked.push(SINGLE_QUOTED_DOLLAR),
            _ => masked.push(ch),
        }
    }
    masked
}

fn expand_pwd(token: &str, working_dir: &Path) -> String {
    let pwd = working_dir.display().to_string();
    token.replace("${PWD}", &pwd).replace("$PWD", &pwd).replace(SINGLE_QUOTED_DOLLAR, "$")
}

/// Resolve the `PERL_LSP_BIN` value to the native executable: the gate
/// spelling names the extensionless Unix artifact (`…/target/debug/perllsp`
/// under `$PWD`), while the Windows artifact carries `.exe`.
fn resolve_perl_lsp_bin(value: &str) -> String {
    if Path::new(value).extension().is_none() { format!("{value}.exe") } else { value.to_string() }
}

fn parse_assignment(token: &str) -> Option<(String, String)> {
    let (name, value) = token.split_once('=')?;
    if is_valid_name(name) { Some((name.to_string(), value.to_string())) } else { None }
}

fn parse_step(
    command: &str,
    tokens: Vec<String>,
    working_dir: &Path,
) -> Result<NativeStep, NativeError> {
    let tokens: Vec<String> =
        tokens.into_iter().map(|token| expand_pwd(&token, working_dir)).collect();
    let mut env_ops: Vec<EnvOp> = Vec::new();
    let mut index = 0usize;
    while let Some((name, value)) = tokens.get(index).and_then(|token| parse_assignment(token)) {
        let value = if name == "PERL_LSP_BIN" { resolve_perl_lsp_bin(&value) } else { value };
        env_ops.push(EnvOp::Set(name, value));
        index += 1;
    }
    if tokens.get(index).map(String::as_str) == Some("env") {
        index += 1;
        let mut options_done = false;
        loop {
            let Some(token) = tokens.get(index) else { break };
            if !options_done {
                if token == "-u" || token == "--unset" {
                    let name = tokens
                        .get(index + 1)
                        .ok_or_else(|| unsupported(command, "'env -u' requires a variable name"))?;
                    if !is_valid_name(name) {
                        return Err(unsupported(
                            command,
                            "'env -u' requires a valid variable name",
                        ));
                    }
                    env_ops.push(EnvOp::Unset(name.clone()));
                    index += 2;
                    continue;
                } else if let Some(name) = token.strip_prefix("--unset=") {
                    if !is_valid_name(name) {
                        return Err(unsupported(
                            command,
                            "'env -u' requires a valid variable name",
                        ));
                    }
                    env_ops.push(EnvOp::Unset(name.to_string()));
                    index += 1;
                    continue;
                } else if token == "--" {
                    index += 1;
                    options_done = true;
                    continue;
                } else if token.starts_with('-') && token.len() > 1 {
                    return Err(unsupported(command, "unsupported 'env' option"));
                }
            }
            if let Some((name, value)) = parse_assignment(token) {
                let value =
                    if name == "PERL_LSP_BIN" { resolve_perl_lsp_bin(&value) } else { value };
                env_ops.push(EnvOp::Set(name, value));
                index += 1;
            } else {
                break;
            }
        }
    }
    let Some(program) = tokens.get(index) else {
        return Err(unsupported(command, "assignments without a program are not supported"));
    };
    let args: Vec<String> = tokens[index + 1..].to_vec();
    let action = match program.as_str() {
        "echo" => NativeAction::Echo { args },
        "true" => NativeAction::True,
        "false" => NativeAction::False,
        "exit" => NativeAction::Exit(parse_exit_code(command, &args)?),
        _ => NativeAction::Exec { program: program.clone(), args },
    };
    Ok(NativeStep { env_ops, action })
}

fn parse_exit_code(command: &str, args: &[String]) -> Result<i32, NativeError> {
    let mut rest: &[String] = args;
    if rest.first().is_some_and(|arg| arg.eq_ignore_ascii_case("/b")) {
        rest = &rest[1..];
    }
    match rest {
        [] => Ok(0),
        [code] => code
            .parse::<i32>()
            .map_err(|_| unsupported(command, "'exit' requires a numeric exit code")),
        _ => Err(unsupported(command, "'exit' takes at most one exit code")),
    }
}

/// Parse the supported POSIX subset into serial steps. Parses the whole
/// command before returning, so unsupported grammar anywhere in the chain
/// is refused before any segment spawns.
pub(crate) fn parse_native_command(
    command: &str,
    working_dir: &Path,
) -> Result<Vec<NativeStep>, NativeError> {
    if command.trim().is_empty() {
        return Err(unsupported(command, "empty command"));
    }
    let mut steps = Vec::new();
    for segment in split_chain(command)? {
        let segment = segment.trim();
        if segment.is_empty() {
            return Err(unsupported(command, "empty command segment around '&&'"));
        }
        let masked = mask_single_quoted_dollars(segment);
        let tokens =
            shlex::split(&masked).ok_or_else(|| unsupported(command, "unterminated quote"))?;
        if tokens.is_empty() {
            return Err(unsupported(command, "empty command segment around '&&'"));
        }
        steps.push(parse_step(command, tokens, working_dir)?);
    }
    Ok(steps)
}

/// Resolve a relative program path containing a separator (`./tool`,
/// `target/debug/tool`) against the step's working directory. Spawning it
/// unresolved would let the host resolve it against the *parent's* cwd
/// (notably for `.cmd` shims, where the batch wrapper does exactly that).
fn resolve_program(program: &str, working_dir: &Path) -> String {
    let path = Path::new(program);
    if path.is_relative() && program.contains(['/', '\\']) {
        return working_dir.join(path).display().to_string();
    }
    program.to_string()
}

fn terminate_child_tree(child: &mut Child) {
    // Kill the whole tree: grandchildren (cargo/rustc/test binaries) survive
    // a plain kill() of the direct child and keep holding target/ locks a
    // retry attempt would then contend with. `taskkill /T` walks the
    // descendants; `/F` forces.
    let pid = child.id().to_string();
    Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok();
    child.kill().ok();
}

fn append_log(log_path: &Path) -> Result<File, NativeError> {
    OpenOptions::new().append(true).open(log_path).map_err(|error| NativeError {
        message: format!("Failed to open gate log for append: {}: {error}", log_path.display()),
        child_started: true,
    })
}

/// Execute a gate command natively: parse fully, then run each segment
/// serially under one shared deadline derived from `timeout_secs`.
/// The first non-zero segment stops the chain with its own exit code; a
/// timeout kills the running segment's whole tree and reports 124.
pub(crate) fn run_native_command(
    command: &str,
    log_path: &Path,
    timeout_secs: u64,
    current_dir: Option<&Path>,
    isolate_git: bool,
) -> Result<NativeOutcome, NativeError> {
    let working_dir = effective_working_dir(current_dir)?;
    // Full parse before any side effect: unsupported grammar is refused
    // pre-launch, and no earlier segment runs when a later one is invalid.
    let steps = parse_native_command(command, &working_dir)?;
    File::create(log_path).map_err(|error| NativeError {
        message: format!("Failed to create log file: {}: {error}", log_path.display()),
        child_started: false,
    })?;

    let start = Instant::now();
    let deadline = Duration::from_secs(timeout_secs);
    let mut last_heartbeat = start;
    let mut child_started = false;

    for (step_index, step) in steps.iter().enumerate() {
        match &step.action {
            NativeAction::Echo { args } => {
                child_started = true;
                let mut log = append_log(log_path)?;
                log.write_all(args.join(" ").as_bytes()).map_err(|error| NativeError {
                    message: format!("Failed to write gate log: {error}"),
                    child_started,
                })?;
                log.write_all(b"\n").map_err(|error| NativeError {
                    message: format!("Failed to write gate log: {error}"),
                    child_started,
                })?;
            }
            NativeAction::True => {
                child_started = true;
            }
            NativeAction::False => {
                return Ok(NativeOutcome { exit_code: 1, timed_out: false });
            }
            NativeAction::Exit(code) => {
                return Ok(NativeOutcome { exit_code: *code, timed_out: false });
            }
            NativeAction::Exec { program, args } => {
                let log_file = append_log(log_path)?;
                let log_file_err = log_file.try_clone().map_err(|error| NativeError {
                    message: format!("Failed to clone log file handle: {error}"),
                    child_started,
                })?;
                let mut process = Command::new(resolve_program(program, &working_dir));
                process.args(args).current_dir(&working_dir);
                if isolate_git {
                    process = crate::git_environment::isolate(process);
                }
                for op in &step.env_ops {
                    match op {
                        EnvOp::Set(name, value) => {
                            process.env(name, value);
                        }
                        EnvOp::Unset(name) => {
                            process.env_remove(name);
                        }
                    }
                }
                let mut child = process
                    .stdout(Stdio::from(log_file))
                    .stderr(Stdio::from(log_file_err))
                    .spawn()
                    .map_err(|error| NativeError {
                        message: format!("Failed to spawn gate command: {command}: {error}"),
                        child_started,
                    })?;
                child_started = true;
                loop {
                    if let Some(status) = child.try_wait().map_err(|error| NativeError {
                        message: format!("Failed waiting on gate process: {error}"),
                        child_started,
                    })? {
                        let exit_code = status.code().unwrap_or(-1);
                        if exit_code != 0 {
                            return Ok(NativeOutcome { exit_code, timed_out: false });
                        }
                        break;
                    }
                    if start.elapsed() >= deadline {
                        terminate_child_tree(&mut child);
                        child.wait().ok();
                        return Ok(NativeOutcome { exit_code: 124, timed_out: true });
                    }
                    if last_heartbeat.elapsed() >= Duration::from_secs(30) {
                        println!(
                            "gate command still running elapsed_ms={} timeout_seconds={}",
                            start.elapsed().as_millis(),
                            timeout_secs
                        );
                        last_heartbeat = Instant::now();
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
        // The shared deadline also bounds builtin-only chains and the gap
        // between segments: an expired deadline stops the chain before the
        // next segment starts. It is not applied after the final step, so a
        // completed run keeps its real result.
        if start.elapsed() >= deadline && step_index + 1 < steps.len() {
            return Ok(NativeOutcome { exit_code: 124, timed_out: true });
        }
    }
    Ok(NativeOutcome { exit_code: 0, timed_out: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, ensure};
    use serial_test::serial;
    use std::fs;

    fn workdir() -> PathBuf {
        PathBuf::from("C:\\work dir")
    }

    #[test]
    fn parses_assignments_env_unset_and_quoted_specials() -> Result<()> {
        let steps = parse_native_command(
            "env -u STALE MARKER=\"a b&c%d!e\" probe \"two words\" 'lit $PWD' \"$PWD/bin/tool\"",
            &workdir(),
        )?;
        ensure!(steps.len() == 1, "expected one step, got {steps:?}");
        ensure!(
            steps[0].env_ops
                == vec![
                    EnvOp::Unset("STALE".to_string()),
                    EnvOp::Set("MARKER".to_string(), "a b&c%d!e".to_string()),
                ],
            "env ops drifted: {:?}",
            steps[0].env_ops
        );
        ensure!(
            steps[0].action
                == NativeAction::Exec {
                    program: "probe".to_string(),
                    args: vec![
                        "two words".to_string(),
                        "lit $PWD".to_string(),
                        "C:\\work dir/bin/tool".to_string(),
                    ],
                },
            "argv drifted: {:?}",
            steps[0].action
        );
        Ok(())
    }

    #[test]
    fn parses_prefix_assignments_and_serial_chain() -> Result<()> {
        let steps = parse_native_command("FOO=bar build && run --flag", &workdir())?;
        ensure!(steps.len() == 2, "expected two steps, got {steps:?}");
        ensure!(
            steps[0].env_ops == vec![EnvOp::Set("FOO".to_string(), "bar".to_string())],
            "prefix assignment drifted: {:?}",
            steps[0].env_ops
        );
        ensure!(
            steps[1].action
                == NativeAction::Exec {
                    program: "run".to_string(),
                    args: vec!["--flag".to_string()],
                },
            "second step drifted: {:?}",
            steps[1].action
        );
        Ok(())
    }

    #[test]
    fn perl_lsp_bin_resolves_to_native_exe_under_pwd() -> Result<()> {
        let steps = parse_native_command(
            "env PERL_LSP_BIN=\"$PWD/target/debug/perllsp\" cargo test",
            &workdir(),
        )?;
        ensure!(
            steps[0].env_ops
                == vec![EnvOp::Set(
                    "PERL_LSP_BIN".to_string(),
                    "C:\\work dir/target/debug/perllsp.exe".to_string(),
                )],
            "PERL_LSP_BIN must resolve to the native .exe: {:?}",
            steps[0].env_ops
        );
        Ok(())
    }

    #[test]
    fn unsupported_grammar_is_refused_before_launch() -> Result<()> {
        for command in [
            "probe | cat",
            "probe || probe",
            "probe; probe",
            "probe > out.txt",
            "probe < in.txt",
            "probe $(whoami)",
            "probe `whoami`",
            "probe $HOME",
            "probe one & probe two",
            "C:\\tools\\probe.exe arg",
            "probe \"unterminated",
            "probe && ",
            "FOO=bar",
            "env -u",
        ] {
            ensure!(
                parse_native_command(command, &workdir()).is_err(),
                "unsupported grammar must be refused: {command}"
            );
        }
        Ok(())
    }

    #[test]
    fn executor_runs_echo_and_preserves_exit_code() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let log_path = temp.path().join("gate.log");
        let outcome = run_native_command(
            "echo hello native && exit 42",
            &log_path,
            30,
            Some(temp.path()),
            false,
        )?;
        ensure!(outcome.exit_code == 42, "exit code drifted: {outcome:?}");
        ensure!(!outcome.timed_out, "natural exit must not time out");
        ensure!(
            fs::read_to_string(&log_path)?.contains("hello native"),
            "echo output must reach the gate log"
        );
        Ok(())
    }

    #[test]
    fn parses_env_terminator_and_assignment_before_env() -> Result<()> {
        let steps = parse_native_command(
            "env -u STALE -- PERL_LSP_BIN=\"$PWD/target/debug/perllsp\" cargo test",
            &workdir(),
        )?;
        ensure!(
            steps[0].env_ops
                == vec![
                    EnvOp::Unset("STALE".to_string()),
                    EnvOp::Set(
                        "PERL_LSP_BIN".to_string(),
                        "C:\\work dir/target/debug/perllsp.exe".to_string(),
                    ),
                ],
            "assignments after 'env --' must be consumed: {:?}",
            steps[0].env_ops
        );
        ensure!(
            steps[0].action
                == NativeAction::Exec {
                    program: "cargo".to_string(),
                    args: vec!["test".to_string()],
                },
            "program after 'env --' assignments drifted: {:?}",
            steps[0].action
        );
        let steps = parse_native_command("FOO=bar env -u STALE cargo test", &workdir())?;
        ensure!(
            steps[0].env_ops
                == vec![
                    EnvOp::Set("FOO".to_string(), "bar".to_string()),
                    EnvOp::Unset("STALE".to_string()),
                ],
            "leading assignment before 'env' must be kept: {:?}",
            steps[0].env_ops
        );
        ensure!(
            steps[0].action
                == NativeAction::Exec {
                    program: "cargo".to_string(),
                    args: vec!["test".to_string()],
                },
            "'env' after assignments must not become the program: {:?}",
            steps[0].action
        );
        Ok(())
    }

    #[test]
    fn escaped_dollar_in_double_quotes_stays_literal() -> Result<()> {
        let steps = parse_native_command(
            "MARKER=\"\\$PWD\" probe \"\\$PWD\" \"\\${PWD}\" \"\\\\$PWD\"",
            &workdir(),
        )?;
        ensure!(
            steps[0].env_ops == vec![EnvOp::Set("MARKER".to_string(), "$PWD".to_string())],
            "escaped $PWD in an assignment must stay literal: {:?}",
            steps[0].env_ops
        );
        ensure!(
            steps[0].action
                == NativeAction::Exec {
                    program: "probe".to_string(),
                    args: vec![
                        "$PWD".to_string(),
                        "${PWD}".to_string(),
                        "\\C:\\work dir".to_string(),
                    ],
                },
            "escaped dollars must stay literal while an unescaped $PWD after '\\\\' expands: {:?}",
            steps[0].action
        );
        Ok(())
    }

    #[test]
    fn resolves_extensionless_relative_program_against_working_dir() -> Result<()> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("tool.exe"), b"")?;
        let resolved = resolve_program("./tool", temp.path());
        ensure!(
            Path::new(&resolved) == temp.path().join("tool"),
            "'./tool' must resolve against the working dir even when only tool.exe exists: {resolved}"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn executor_assignment_reaches_child_with_observed_work() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let script = root.join("env-probe.ps1");
        fs::write(
            &script,
            "if ($null -eq $env:NATIVE_ASSIGN_MARKER) { Set-Content -LiteralPath $env:NATIVE_ASSIGN_OUT -Value '<unset>' } else { Set-Content -LiteralPath $env:NATIVE_ASSIGN_OUT -Value $env:NATIVE_ASSIGN_MARKER }",
        )?;
        for (label, prefix) in [
            ("prefix", "NATIVE_ASSIGN_MARKER=observed-prefix"),
            ("env", "env NATIVE_ASSIGN_MARKER=observed-env"),
        ] {
            let out = root.join(format!("{label}.txt"));
            let log_path = root.join(format!("{label}.log"));
            let command = format!(
                "{prefix} NATIVE_ASSIGN_OUT=\"{}\" powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
                out.display(),
                script.display()
            );
            let outcome = run_native_command(&command, &log_path, 60, Some(root), false)?;
            ensure!(outcome.exit_code == 0, "{label}: child failed: {outcome:?}");
            let observed = fs::read_to_string(&out)?;
            ensure!(
                observed.trim() == format!("observed-{label}"),
                "{label}: assignment did not reach the child: {observed:?}"
            );
        }
        // `env -u` removes an inherited variable before the child runs.
        unsafe {
            std::env::set_var("NATIVE_ASSIGN_MARKER", "from-parent");
        }
        let out = root.join("unset.txt");
        let log_path = root.join("unset.log");
        let command = format!(
            "env -u NATIVE_ASSIGN_MARKER NATIVE_ASSIGN_OUT=\"{}\" powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
            out.display(),
            script.display()
        );
        let outcome = run_native_command(&command, &log_path, 60, Some(root), false)?;
        unsafe {
            std::env::remove_var("NATIVE_ASSIGN_MARKER");
        }
        ensure!(outcome.exit_code == 0, "env -u child failed: {outcome:?}");
        ensure!(
            fs::read_to_string(&out)?.trim() == "<unset>",
            "env -u must remove the inherited variable"
        );
        Ok(())
    }

    #[test]
    fn executor_prerequisite_failure_prevents_runtime_child() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let marker = root.join("runtime-ran.txt");
        let script = root.join("runtime.ps1");
        fs::write(
            &script,
            format!("Set-Content -LiteralPath '{}' -Value 'ran'", marker.display()),
        )?;
        let log_path = root.join("gate.log");
        let outcome = run_native_command(
            &format!(
                "exit 1 && powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
                script.display()
            ),
            &log_path,
            60,
            Some(root),
            false,
        )?;
        ensure!(outcome.exit_code == 1, "prerequisite code must win: {outcome:?}");
        ensure!(!outcome.timed_out, "prerequisite failure is not a timeout");
        ensure!(!marker.exists(), "runtime child must not run after a failed prerequisite");
        Ok(())
    }

    #[test]
    fn executor_passes_special_characters_as_literal_argv() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let out = root.join("argv.txt");
        let script = root.join("argv.ps1");
        fs::write(
            &script,
            "Set-Content -LiteralPath $env:NATIVE_ARGV_OUT -Value ($args -join \"`n\")",
        )?;
        let log_path = root.join("gate.log");
        let command = format!(
            "NATIVE_ARGV_OUT=\"{}\" powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" \"a b\" \"x&y\" \"100%\" \"bang!z\"",
            out.display(),
            script.display()
        );
        let outcome = run_native_command(&command, &log_path, 60, Some(root), false)?;
        ensure!(outcome.exit_code == 0, "argv child failed: {outcome:?}");
        let observed: Vec<String> = fs::read_to_string(&out)?.lines().map(str::to_string).collect();
        ensure!(
            observed == vec!["a b", "x&y", "100%", "bang!z"],
            "special characters must survive as literal argv: {observed:?}"
        );
        Ok(())
    }

    #[test]
    fn executor_refuses_unsupported_grammar_before_any_spawn() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let marker = root.join("spawned.txt");
        let script = root.join("spawn.ps1");
        fs::write(
            &script,
            format!("Set-Content -LiteralPath '{}' -Value 'ran'", marker.display()),
        )?;
        let log_path = root.join("gate.log");
        let command = format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" | cat",
            script.display()
        );
        let error = run_native_command(&command, &log_path, 60, Some(root), false)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("unsupported grammar unexpectedly executed"))?;
        ensure!(!error.child_started, "refusal must be pre-launch: {error:?}");
        ensure!(!marker.exists(), "no segment may spawn when the chain is refused");
        Ok(())
    }

    /// Probe child for the timeout falsifier: with
    /// `NATIVE_TIMEOUT_PROBE_DIR` set, the "parent" mode records a spawned
    /// descendant's pid and then hangs; the "child" mode just hangs.
    /// Without the env var this is a no-op in the normal suite.
    #[test]
    fn native_timeout_probe_child() -> Result<()> {
        let Some(dir) = std::env::var_os("NATIVE_TIMEOUT_PROBE_DIR") else {
            return Ok(());
        };
        let dir = PathBuf::from(dir);
        if std::env::var("NATIVE_TIMEOUT_PROBE_MODE").as_deref() == Ok("child") {
            thread::sleep(Duration::from_secs(30));
            return Ok(());
        }
        let descendant = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "tasks::gates::native_command::tests::native_timeout_probe_child",
                "--nocapture",
            ])
            .env("NATIVE_TIMEOUT_PROBE_DIR", &dir)
            .env("NATIVE_TIMEOUT_PROBE_MODE", "child")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        fs::write(dir.join("descendant.pid"), descendant.id().to_string())?;
        thread::sleep(Duration::from_secs(30));
        Ok(())
    }

    #[test]
    fn executor_timeout_settles_descendant_tree_without_a_pass() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let pid_path = root.join("descendant.pid");
        let log_path = root.join("gate.log");
        let executable = std::env::current_exe()?;
        let outcome = run_native_command(
            &format!(
                "NATIVE_TIMEOUT_PROBE_DIR=\"{}\" NATIVE_TIMEOUT_PROBE_MODE=parent \"{}\" --exact tasks::gates::native_command::tests::native_timeout_probe_child --nocapture",
                root.display(),
                executable.display()
            ),
            &log_path,
            10,
            Some(root),
            false,
        )?;
        ensure!(outcome.timed_out, "hanging child must time out: {outcome:?}");
        ensure!(outcome.exit_code == 124, "timeout must report 124, never a pass: {outcome:?}");
        let pid = fs::read_to_string(&pid_path)
            .map_err(|error| {
                color_eyre::eyre::eyre!(
                    "descendant pid missing ({error}); gate log:\n{}",
                    fs::read_to_string(&log_path).unwrap_or_default()
                )
            })?
            .trim()
            .to_string();
        ensure!(!pid.is_empty(), "descendant pid must be recorded");
        // The descendant must be settled by the tree kill, not left running.
        let mut settled = false;
        for _ in 0..100 {
            let output =
                Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output()?;
            if output.status.success() && !String::from_utf8_lossy(&output.stdout).contains(&pid) {
                settled = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        ensure!(settled, "descendant process {pid} survived the timeout tree kill");
        Ok(())
    }

    #[test]
    fn executor_resolves_perl_lsp_bin_exe_under_relative_cwd() -> Result<()> {
        let base = effective_working_dir(None)?;
        let relative = PathBuf::from(format!("native-probe-{}", std::process::id()));
        let root = base.join(&relative);
        let artifact_dir = root.join("target").join("debug");
        fs::create_dir_all(&artifact_dir)?;
        fs::write(artifact_dir.join("perllsp.exe"), b"")?;
        let script = root.join("bin-probe.ps1");
        fs::write(
            &script,
            "Set-Content -LiteralPath $env:NATIVE_BIN_OUT -Value $env:PERL_LSP_BIN",
        )?;
        let out = root.join("bin.txt");
        let log_path = root.join("gate.log");
        let command = format!(
            "env PERL_LSP_BIN=\"$PWD/target/debug/perllsp\" NATIVE_BIN_OUT=\"{}\" powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
            out.display(),
            script.display()
        );
        let result = run_native_command(&command, &log_path, 60, Some(&relative), false);
        let observed = fs::read_to_string(&out).unwrap_or_default();
        let cleanup = fs::remove_dir_all(&root);
        let outcome = result?;
        cleanup?;
        ensure!(outcome.exit_code == 0, "PERL_LSP_BIN child failed: {outcome:?}");
        let expected = artifact_dir.join("perllsp.exe").display().to_string();
        ensure!(
            observed.trim().replace('/', "\\").eq_ignore_ascii_case(&expected.replace('/', "\\")),
            "PERL_LSP_BIN must resolve to the native .exe under the relative cwd: observed {observed:?}, expected {expected:?}"
        );
        Ok(())
    }
}
