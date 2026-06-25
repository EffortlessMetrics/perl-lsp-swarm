use std::path::{Path, PathBuf};

/// Quote a single argument for use inside a `cmd.exe /V:OFF /S /C "..."` command line.
///
/// ## cmd.exe quoting rules inside double-quoted regions
///
/// Once cmd.exe sees an opening `"` it enters a quoted region. Inside that region:
///
/// - Characters like `&`, `|`, `<`, `>`, `(`, and `)` are literal; they do not
///   need `^` escaping.
/// - `^` is also literal in a quoted region, so doubling it would change the
///   argument seen by the child process.
/// - `%` is still processed by the variable-substitution pass, which runs before
///   the shell-metachar pass and is not suppressed by quoting. Double it (`%%`)
///   to produce a literal `%`.
/// - `!` would be processed by the delayed-expansion pass when `/V:ON` is in
///   effect. We invoke cmd.exe with `/V:OFF` to suppress this entirely, so `!`
///   needs no escaping here.
/// - To embed a literal `"` inside a double-quoted cmd.exe token, use `""` (the
///   cmd.exe shell convention). The `\"` form is for `CommandLineToArgvW` (the
///   Win32 C-runtime argv parser), which is a different parser from the cmd.exe
///   shell command-line parser.
pub(crate) fn windows_quote_for_cmd(arg: &str) -> String {
    let mut escaped = String::with_capacity(arg.len() + 2);
    escaped.push('"');
    for ch in arg.chars() {
        match ch {
            '%' => escaped.push_str("%%"),
            '"' => escaped.push_str("\"\""),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

/// Resolve a bare program name (no path separator, no extension) to an absolute
/// path by searching the `PATH` environment variable directories.
///
/// # Security invariant
///
/// The current working directory is **never** consulted, even though
/// `where.exe` (the previous implementation) searches the CWD first.
/// An attacker who can plant files in the workspace directory opened by the
/// LSP server (e.g. `perltidy.exe` in the project root) must not be able to
/// hijack tool invocations — this is the Windows binary-planting / CWD-RCE
/// attack class.
///
/// Absolute paths and paths containing a separator pass through unchanged via
/// the early return so callers can always use pre-resolved paths safely.
pub(crate) fn resolve_windows_program(program: &str) -> Option<String> {
    let program_path = Path::new(program);
    let has_separator = program.contains('\\') || program.contains('/');
    let has_extension = program_path.extension().is_some();
    if has_separator || has_extension {
        return Some(program.to_string());
    }

    // Collect all candidate paths from PATH × PATHEXT.
    let path_dirs = path_dirs_from_env();
    let path_exts = pathext_from_env();

    let candidates: Vec<String> = path_dirs
        .iter()
        .flat_map(|dir| {
            path_exts.iter().filter_map(|ext| {
                let mut candidate = dir.join(program);
                candidate.set_extension(ext.trim_start_matches('.'));
                if candidate.is_file() { candidate.to_str().map(str::to_string) } else { None }
            })
        })
        .collect();

    let cwd = std::env::current_dir().ok()?;
    let candidate_refs: Vec<&str> = candidates.iter().map(String::as_str).collect();
    select_path_candidate(&candidate_refs, &cwd)
}

/// Pure selection function: given a list of fully-resolved candidate paths and
/// the current working directory, return the best candidate that is **not**
/// located under `cwd`, applying `windows_program_priority` to break ties.
///
/// This function is extracted for unit-testability: callers inject arbitrary
/// candidate lists and CWD values so the security invariant can be verified
/// without touching the real file system or environment.
///
/// Returns `None` when every candidate is under `cwd` (refuse to run a planted
/// binary) or when `candidates` is empty (tool genuinely not on PATH).
pub(crate) fn select_path_candidate(candidates: &[&str], cwd: &Path) -> Option<String> {
    let cwd_canon = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());

    candidates
        .iter()
        .filter(|&&c| {
            let candidate_parent = Path::new(c).parent().unwrap_or(Path::new(""));
            let parent_canon = std::fs::canonicalize(candidate_parent)
                .unwrap_or_else(|_| candidate_parent.to_path_buf());
            parent_canon != cwd_canon
        })
        .max_by_key(|&&c| windows_program_priority(c))
        .map(|&s| s.to_string())
}

/// Returns the list of directories in the `PATH` environment variable.
fn path_dirs_from_env() -> Vec<PathBuf> {
    std::env::var_os("PATH").map(|val| std::env::split_paths(&val).collect()).unwrap_or_default()
}

/// Returns the list of extensions from `PATHEXT`, falling back to the standard
/// Windows default when the variable is absent.
fn pathext_from_env() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(str::to_string)
        .collect()
}

pub(crate) fn windows_program_priority(candidate: &str) -> u8 {
    match Path::new(candidate)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
    {
        Some(ext) if ext == "exe" => 5,
        Some(ext) if ext == "com" => 4,
        Some(ext) if ext == "cmd" => 3,
        Some(ext) if ext == "bat" => 2,
        Some(_) => 1,
        None => 0,
    }
}

pub(crate) fn windows_requires_cmd_shell(program: &str) -> bool {
    Path::new(program)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd"))
        .unwrap_or(false)
}
