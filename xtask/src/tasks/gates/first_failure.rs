use super::FirstFailure;

/// Parse the first failing test name, panic site, and message from `cargo test` stdout.
///
/// Returns `None` only if the output contains no recognisable failure markers (e.g. a
/// pure compilation error with no test output). All three sub-fields (`test`, `site`,
/// `message`) are individually optional because any one may be absent in edge cases.
///
/// # Patterns detected
///
/// * Test name — `test <path> ... FAILED` or `---- <path> stdout ----`
/// * Panic site — `panicked at '<file>:<line>:<col>:'` (Rust <1.73 style) or
///   `panicked at <file>:<line>:<col>:` (Rust ≥1.73 style)
/// * Message — the first non-empty line that follows the `panicked at` line
pub fn parse_first_failure(output: &str, exit_code: i32) -> Option<FirstFailure> {
    let mut test_name: Option<String> = None;
    let mut site: Option<String> = None;
    let mut message: Option<String> = None;

    let lines: Vec<&str> = output.lines().collect();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("test ") && trimmed.ends_with("... FAILED") {
            let inner = trimmed
                .strip_prefix("test ")
                .and_then(|s| s.strip_suffix("... FAILED"))
                .map(str::trim);
            if let Some(name) = inner
                && !name.is_empty()
            {
                test_name = Some(name.to_string());
                break;
            }
        }
        if test_name.is_none() && trimmed.starts_with("---- ") && trimmed.ends_with(" stdout ----")
        {
            let inner = trimmed
                .strip_prefix("---- ")
                .and_then(|s| s.strip_suffix(" stdout ----"))
                .map(str::trim);
            if let Some(name) = inner
                && !name.is_empty()
            {
                test_name = Some(name.to_string());
            }
        }
    }

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(panic_pos) = trimmed.find("panicked at ") {
            let rest = &trimmed[panic_pos + "panicked at ".len()..];

            site = parse_panic_site_new_style(rest).or_else(|| parse_panic_site_old_style(rest));
            message = lines[idx + 1..]
                .iter()
                .find(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string());

            break;
        }
    }

    if test_name.is_some() || site.is_some() {
        Some(FirstFailure { test: test_name, site, message, exit_code })
    } else {
        None
    }
}

fn parse_panic_site_new_style(rest: &str) -> Option<String> {
    let rest = rest.trim_end_matches(':');
    let parts: Vec<&str> = rest.splitn(4, ':').collect();
    match parts.len() {
        2.. => {
            let (path_part, line_part) = if parts[0].len() == 1
                && parts[0].chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && parts.len() >= 3
            {
                (format!("{}:{}", parts[0], parts[1]), parts[2])
            } else {
                (parts[0].to_string(), parts[1])
            };
            if line_part.parse::<u64>().is_ok() && !path_part.is_empty() {
                return Some(format!("{}:{}", path_part, line_part));
            }
            None
        }
        _ => None,
    }
}

fn parse_panic_site_old_style(rest: &str) -> Option<String> {
    let loc_start = rest.rfind("', ").map(|i| i + 3)?;
    let loc = &rest[loc_start..];
    parse_panic_site_new_style(loc)
}

/// Check whether a gate command is a `cargo test`-class command.
///
/// Returns `true` for commands whose first word-token is `cargo` and second is `test`,
/// ignoring leading whitespace and path prefixes. A leading `env` invocation
/// (optionally followed by `-u NAME` unset flags and/or `NAME=VALUE`
/// assignments, per env(1)'s `[flags/assignments] command` grammar) is
/// transparent: the merge-gate `lsp_smoke` gate runs its test binary as
/// `env -u RUSTC_WRAPPER cargo test ...`, and its log is as much a cargo
/// test log as any other.
pub fn is_cargo_test_command(command: &str) -> bool {
    // Gate commands may chain setup steps ahead of the test invocation (for
    // example `cargo build -p perllsp --locked && cargo test ...`). The test
    // output whose failures must be extracted comes from the final segment,
    // so recognition applies to the last `&&`-separated segment.
    let final_segment = command.split("&&").last().unwrap_or("").trim();
    let mut tokens = final_segment.split_whitespace();
    let mut first = tokens.next().unwrap_or("");
    if first == "env" {
        // Skip env(1) arguments until the wrapped command: assignments
        // (`NAME=VALUE`), single-token options, and option+argument pairs
        // such as `-u NAME`. Anything else begins the wrapped command.
        loop {
            let token = tokens.next().unwrap_or("");
            if token.is_empty() {
                return false;
            }
            if token.contains('=') || (token.starts_with('-') && token != "-") {
                if matches!(token, "-u" | "--unset") {
                    tokens.next();
                }
                continue;
            }
            first = token;
            break;
        }
    }
    let is_cargo = first == "cargo" || first.ends_with("/cargo") || first.contains("\\cargo");
    is_cargo && tokens.next().is_some_and(|t| t == "test")
}
