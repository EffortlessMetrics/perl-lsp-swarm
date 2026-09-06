//! DevEx documentation drift checks.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use clap::CommandFactory;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;

use crate::utils::project_root;

#[derive(Debug, Deserialize)]
struct RustToolchainFile {
    toolchain: RustToolchain,
}

#[derive(Debug, Deserialize)]
struct RustToolchain {
    channel: String,
}

pub fn run() -> Result<()> {
    let root = project_root()?;
    let report = check_devex_docs(&root)?;
    if report.errors.is_empty() {
        println!("DevEx docs drift check passed");
        return Ok(());
    }

    bail!("DevEx docs drift check failed:\n{}", report.errors.join("\n"));
}

#[derive(Debug, Default)]
struct DevexDocsReport {
    errors: Vec<String>,
}

fn check_devex_docs(root: &Path) -> Result<DevexDocsReport> {
    let mut report = DevexDocsReport::default();
    check_toolchain_docs(root, &mut report)?;
    check_documented_commands(root, &mut report)?;
    Ok(report)
}

fn check_toolchain_docs(root: &Path, report: &mut DevexDocsReport) -> Result<()> {
    let channel = pinned_toolchain_channel(root)?;
    let msrv = major_minor(&channel);

    let contributing = read(root, "CONTRIBUTING.md")?;
    require_contains(report, "CONTRIBUTING.md", &contributing, &format!("MSRV {msrv}"));
    require_contains(report, "CONTRIBUTING.md", &contributing, &format!("`{channel}`"));

    let first_pr = read(root, "docs/contributing/FIRST_PR.md")?;
    require_contains(report, "docs/contributing/FIRST_PR.md", &first_pr, &format!("MSRV {msrv}"));
    require_contains(report, "docs/contributing/FIRST_PR.md", &first_pr, &format!("`{channel}`"));

    let readme = read(root, "README.md")?;
    require_contains(report, "README.md", &readme, &format!("MSRV-{msrv}"));

    Ok(())
}

fn check_documented_commands(root: &Path, report: &mut DevexDocsReport) -> Result<()> {
    let just_recipes = just_recipes(root)?;
    let xtask_subcommands = xtask_subcommands();
    for path in
        ["CONTRIBUTING.md", "docs/reference/COMMANDS_REFERENCE.md", "docs/contributing/FIRST_PR.md"]
    {
        let text = read(root, path)?;
        for command in inline_devex_commands(&text) {
            if let Err(error) = command_exists(&command, &just_recipes, &xtask_subcommands) {
                report.errors.push(format!("{path}: {error}"));
            }
        }
    }
    Ok(())
}

fn pinned_toolchain_channel(root: &Path) -> Result<String> {
    let raw = read(root, "rust-toolchain.toml")?;
    let toolchain: RustToolchainFile =
        toml::from_str(&raw).context("parsing rust-toolchain.toml")?;
    Ok(toolchain.toolchain.channel.trim().to_string())
}

fn major_minor(channel: &str) -> String {
    let mut parts = channel.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("{major}.{minor}"),
        _ => channel.to_string(),
    }
}

fn require_contains(report: &mut DevexDocsReport, path: &str, text: &str, needle: &str) {
    if !text.contains(needle) {
        report.errors.push(format!("{path}: missing `{needle}`"));
    }
}

fn read(root: &Path, path: &str) -> Result<String> {
    fs::read_to_string(root.join(path)).with_context(|| format!("reading {path}"))
}

fn just_recipes(root: &Path) -> Result<BTreeSet<String>> {
    let justfile = read(root, "justfile")?;
    Ok(parse_just_recipes(&justfile))
}

fn parse_just_recipes(justfile: &str) -> BTreeSet<String> {
    justfile
        .lines()
        .filter_map(|line| {
            if line.starts_with(char::is_whitespace) || line.starts_with('#') {
                return None;
            }
            let (name, _) = line.split_once(':')?;
            let name = name.split_whitespace().next()?;
            if name.contains('=') || name.is_empty() {
                return None;
            }
            Some(name.to_string())
        })
        .collect()
}

fn xtask_subcommands() -> BTreeSet<String> {
    crate::Cli::command().get_subcommands().map(|command| command.get_name().to_string()).collect()
}

fn inline_devex_commands(text: &str) -> Vec<String> {
    let mut commands = backtick_devex_commands(text);
    for command in line_devex_commands(text) {
        push_unique_command(&mut commands, command);
    }
    commands
}

fn backtick_devex_commands(text: &str) -> Vec<String> {
    text.split('`')
        .enumerate()
        .filter_map(|(index, segment)| {
            if index % 2 == 0 {
                return None;
            }
            normalize_extracted_command(segment)
        })
        .collect()
}

fn line_devex_commands(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    for body in markdown_regions(text) {
        let logical =
            join_indented_flag_continuations(join_trailing_backslash_continuations(&body));
        for line in logical {
            if let Some(command) = normalize_extracted_command(&line.text) {
                push_unique_command(&mut commands, command);
            }
        }
    }
    commands
}

fn markdown_regions(text: &str) -> Vec<String> {
    let mut regions = Vec::new();
    let mut buf = String::new();
    let mut open: Option<(char, usize)> = None;

    for line in text.lines() {
        if let Some((ch, len)) = open {
            if is_closing_fence(line, ch, len) {
                flush_region(&mut regions, &mut buf);
                open = None;
                continue;
            }
            push_region_line(&mut buf, line);
            continue;
        }
        if let Some(mark) = opening_fence(line) {
            flush_region(&mut regions, &mut buf);
            open = Some(mark);
            continue;
        }
        push_region_line(&mut buf, line);
    }
    flush_region(&mut regions, &mut buf);
    regions
}

fn flush_region(regions: &mut Vec<String>, buf: &mut String) {
    if buf.is_empty() {
        return;
    }
    regions.push(std::mem::take(buf));
}

fn push_region_line(buf: &mut String, line: &str) {
    if !buf.is_empty() {
        buf.push('\n');
    }
    buf.push_str(line);
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let (ch, len, _) = fence_prefix(line)?;
    Some((ch, len))
}

fn is_closing_fence(line: &str, ch: char, len: usize) -> bool {
    let Some((close_ch, close_len, rest)) = fence_prefix(line) else {
        return false;
    };
    close_ch == ch && close_len >= len && rest.trim().is_empty()
}

fn fence_prefix(line: &str) -> Option<(char, usize, &str)> {
    let trimmed_start = line.trim_start();
    let indent = line.len().saturating_sub(trimmed_start.len());
    if indent > 3 {
        return None;
    }
    let ch = trimmed_start.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = trimmed_start.chars().take_while(|candidate| *candidate == ch).count();
    if len < 3 {
        return None;
    }
    let rest = trimmed_start.get(len..)?;
    Some((ch, len, rest))
}

/// Join physical lines that end with a shell line-continuation into one logical line.
///
/// A continuation is a POSIX backslash-newline escape: the final character of the
/// physical line must be `\\` (immediately before the newline), and the trailing
/// backslash run must have odd length so the last `\\` is not itself escaped.
/// Trailing spaces after a backslash are not stripped for this test — they mean the
/// newline is not escaped. Ordinary `trim_end` applies only when assembling the
/// stored piece.
fn join_trailing_backslash_continuations(text: &str) -> Vec<JoinedLine> {
    let mut lines = Vec::new();
    let mut buf: Option<String> = None;
    for raw in text.lines() {
        let (piece, continuation) = match shell_line_continuation(raw) {
            Some(rest) => (rest.trim_end(), true),
            None => (raw.trim_end(), false),
        };
        match buf.take() {
            Some(mut existing) => {
                existing.push(' ');
                existing.push_str(piece.trim_start());
                if continuation {
                    buf = Some(existing);
                } else {
                    lines.push(JoinedLine { text: existing, continued: true });
                }
            }
            None => {
                if continuation {
                    buf = Some(piece.to_string());
                } else {
                    lines.push(JoinedLine { text: piece.to_string(), continued: false });
                }
            }
        }
    }
    if let Some(pending) = buf {
        lines.push(JoinedLine { text: pending, continued: true });
    }
    lines
}

/// Returns the line without its escaping backslash when that backslash escapes the
/// following newline. `None` means the physical line is complete.
fn shell_line_continuation(line: &str) -> Option<&str> {
    if !line.ends_with('\\') {
        return None;
    }
    let trailing_backslashes = line.bytes().rev().take_while(|byte| *byte == b'\\').count();
    if trailing_backslashes % 2 == 0 {
        return None;
    }
    line.strip_suffix('\\')
}

#[derive(Debug)]
struct JoinedLine {
    text: String,
    continued: bool,
}

fn join_indented_flag_continuations(lines: Vec<JoinedLine>) -> Vec<JoinedLine> {
    let mut joined: Vec<JoinedLine> = Vec::new();
    for line in lines {
        if let Some(previous) = joined.last_mut()
            && is_indented_flag_continuation(&line.text)
            && normalize_extracted_command(&previous.text).is_some()
        {
            previous.text.push(' ');
            previous.text.push_str(line.text.trim());
            previous.continued = true;
            continue;
        }
        joined.push(line);
    }
    joined
}

fn is_indented_flag_continuation(text: &str) -> bool {
    let trimmed = text.trim_start();
    if trimmed.len() == text.len() || trimmed.starts_with('#') {
        return false;
    }
    if normalize_extracted_command(trimmed).is_some() {
        return false;
    }
    trimmed.starts_with('-')
}

fn normalize_extracted_command(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let without_list = strip_markdown_list_marker(line);
    let without_env = strip_leading_env_assignments(without_list).trim();
    if without_env.is_empty() || without_env.starts_with('#') {
        return None;
    }
    is_governed_devex_command(without_env).then(|| without_env.to_string())
}

fn strip_markdown_list_marker(line: &str) -> &str {
    let line = line.trim_start();
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return rest;
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return line;
    }
    match line.get(digits..) {
        Some(rest) => rest.strip_prefix(". ").unwrap_or(line),
        None => line,
    }
}

fn strip_leading_env_assignments(command: &str) -> &str {
    let mut rest = command.trim_start();
    loop {
        let Some((token, after)) = split_first_token(rest) else {
            return rest;
        };
        if !is_env_assignment_token(token) {
            return rest;
        }
        rest = after.trim_start();
    }
}

fn split_first_token(text: &str) -> Option<(&str, &str)> {
    if text.is_empty() {
        return None;
    }
    match text.find(char::is_whitespace) {
        Some(index) => text.get(..index).zip(text.get(index..)),
        None => Some((text, "")),
    }
}

fn is_env_assignment_token(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    is_posix_env_name(name)
}

fn is_posix_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(candidate) if candidate.is_ascii_alphabetic() || candidate == '_' => {
            chars.all(|candidate| candidate.is_ascii_alphanumeric() || candidate == '_')
        }
        _ => false,
    }
}

fn is_governed_devex_command(command: &str) -> bool {
    command.starts_with("just ") || command.starts_with("cargo xtask ")
}

fn push_unique_command(commands: &mut Vec<String>, command: String) {
    if !commands.iter().any(|existing| existing == &command) {
        commands.push(command);
    }
}

fn command_exists(
    command: &str,
    just_recipes: &BTreeSet<String>,
    xtask_subcommands: &BTreeSet<String>,
) -> std::result::Result<(), String> {
    let parts = command.split_whitespace().map(normalize_doc_token).collect::<Vec<_>>();
    match parts.as_slice() {
        [cmd, recipe, ..] if cmd == "just" => {
            if just_recipes.contains(recipe.as_str()) {
                Ok(())
            } else {
                Err(format!("`{command}` references missing just recipe `{recipe}`"))
            }
        }
        [cargo, xtask, subcommand, ..] if cargo == "cargo" && xtask == "xtask" => {
            if xtask_subcommands.contains(subcommand.as_str()) {
                Ok(())
            } else {
                Err(format!("`{command}` references missing cargo xtask subcommand `{subcommand}`"))
            }
        }
        _ => Ok(()),
    }
}

fn normalize_doc_token(token: &str) -> String {
    token.trim_end_matches([',', '.', ';', ':']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive_line_devex_commands(text: &str) -> Vec<String> {
        text.lines()
            .filter_map(|line| {
                let command = line.trim();
                is_governed_devex_command(command).then_some(command.to_string())
            })
            .collect()
    }

    #[test]
    fn major_minor_extracts_msrv_badge_version() {
        assert_eq!(major_minor("1.93.1"), "1.93");
        assert_eq!(major_minor("1.94"), "1.94");
    }

    #[test]
    fn inline_devex_commands_extracts_just_and_xtask_references() {
        let commands = inline_devex_commands(
            "| Need | Command |\n|---|---|\n| Fast | `just pr-fast` |\n| Fmt | `cargo xtask fmt` |\n| Rust | `cargo test -p xtask` |\n",
        );
        assert_eq!(commands, vec!["just pr-fast", "cargo xtask fmt"]);
    }

    #[test]
    fn join_trailing_backslash_continuations_joins_command_and_args() {
        let joined = join_trailing_backslash_continuations("just pr-fast \\\n    --locked\n");
        assert_eq!(joined.len(), 1, "continued physical lines must become one logical line");
        assert!(joined[0].continued, "a trailing-backslash join must be marked continued");
        assert_eq!(joined[0].text, "just pr-fast --locked");
    }

    fn assert_independent_false_join_control(text: &str, first_stored: &str) {
        let joined = join_trailing_backslash_continuations(text);
        assert_eq!(joined.len(), 2, "independent physical lines must not merge: {joined:?}");
        assert!(
            joined.iter().all(|line| !line.continued),
            "no real POSIX continuation: {joined:?}"
        );
        assert_eq!(joined[0].text, first_stored);
        assert_eq!(joined[1].text, "just stale-recipe");

        let commands = inline_devex_commands(text);
        assert!(
            commands.iter().any(|command| command == "just stale-recipe"),
            "the second recipe must remain its own extracted command: {commands:?}"
        );
        assert!(
            !commands
                .iter()
                .any(|command| command.contains("pr-fast") && command.contains("stale-recipe")),
            "a false join must not smuggle the second recipe into one continued command: {commands:?}"
        );

        let just_recipes = BTreeSet::from(["pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::new();
        assert!(
            command_exists(first_stored, &just_recipes, &xtask_subcommands).is_ok(),
            "valid first recipe stays independently visible to command_exists"
        );
        assert!(
            command_exists("just stale-recipe", &just_recipes, &xtask_subcommands).is_err(),
            "stale second recipe must remain independently visible to command_exists"
        );
    }

    #[test]
    fn whitespace_after_backslash_does_not_join_two_commands() {
        assert_independent_false_join_control(
            "just pr-fast \\  \njust stale-recipe\n",
            "just pr-fast \\",
        );
    }

    #[test]
    fn even_trailing_backslashes_do_not_continue() {
        assert_independent_false_join_control(
            "just pr-fast \\\\\njust stale-recipe\n",
            "just pr-fast \\\\",
        );
    }

    #[test]
    fn join_trailing_backslash_continuations_joins_multiple_continuations() {
        let joined = join_trailing_backslash_continuations(
            "cargo xtask fmt \\\n    --check \\\n    --all\n",
        );
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].text, "cargo xtask fmt --check --all");
        assert!(joined[0].continued);
    }

    #[test]
    fn join_trailing_backslash_continuations_keeps_uncontinued_lines() {
        let joined = join_trailing_backslash_continuations("just pr-fast\n    --locked\n");
        assert_eq!(
            joined.iter().map(|line| (line.text.as_str(), line.continued)).collect::<Vec<_>>(),
            vec![("just pr-fast", false), ("    --locked", false)]
        );
    }

    #[test]
    fn continued_backslash_commands_enter_the_governed_denominator() {
        let text = "```bash\njust pr-fast \\\n    --locked\ncargo xtask fmt \\\n    --check\n```\n";
        let commands = inline_devex_commands(text);
        assert!(
            commands.iter().any(|command| command == "just pr-fast --locked"),
            "joined just command must be extracted; got {commands:?}"
        );
        assert!(
            commands.iter().any(|command| command == "cargo xtask fmt --check"),
            "joined cargo xtask command must be extracted; got {commands:?}"
        );

        let just_recipes = BTreeSet::from(["pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::from(["fmt".to_string()]);
        assert!(
            command_exists("just pr-fast --locked", &just_recipes, &xtask_subcommands).is_ok(),
            "joined just command must tokenize through the documented-command checker"
        );
        assert!(
            command_exists("cargo xtask fmt --check", &just_recipes, &xtask_subcommands).is_ok(),
            "joined cargo xtask command must tokenize through the documented-command checker"
        );
    }

    #[test]
    fn naive_line_scan_misses_backslash_continuations() {
        let text = "just pr-fast \\\n    --locked\n";
        let naive = naive_line_devex_commands(text);
        assert_eq!(
            naive,
            vec!["just pr-fast \\"],
            "without a joiner the first physical line stays a backslash-suffixed command and continuation args are absent"
        );
        assert_eq!(inline_devex_commands(text), vec!["just pr-fast --locked"]);
    }

    #[test]
    fn dangling_trailing_backslash_still_extracts_the_command_stem() {
        let commands = inline_devex_commands("just pr-fast \\\n");
        assert_eq!(
            commands,
            vec!["just pr-fast"],
            "a continuation with no following line still yields the governed stem"
        );
    }

    #[test]
    fn env_prefixed_commands_enter_the_governed_denominator() {
        let just_recipes = BTreeSet::from(["test".to_string(), "pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::from(["fmt".to_string()]);

        let env_prefixed = inline_devex_commands("FOO=1 just test\n");
        assert_eq!(
            env_prefixed,
            vec!["just test"],
            "leading env assignments must peel so the governed command is stored"
        );
        assert!(
            command_exists(&env_prefixed[0], &just_recipes, &xtask_subcommands).is_ok(),
            "stripped env-prefixed just must tokenize through command_exists"
        );

        let multiple = inline_devex_commands("FOO=1 BAR=2 cargo xtask fmt\n");
        assert_eq!(multiple, vec!["cargo xtask fmt"]);
        assert!(command_exists(&multiple[0], &just_recipes, &xtask_subcommands).is_ok());

        let backticked = inline_devex_commands("run `FOO=1 just pr-fast` locally\n");
        assert_eq!(backticked, vec!["just pr-fast"]);

        let continued = inline_devex_commands("FOO=1 just pr-fast \\\n    --locked\n");
        assert_eq!(continued, vec!["just pr-fast --locked"]);
        assert!(command_exists(&continued[0], &just_recipes, &xtask_subcommands).is_ok());

        assert_eq!(
            inline_devex_commands("FOO= just test\n"),
            vec!["just test"],
            "an empty env value is still a leading assignment"
        );
    }

    #[test]
    fn naive_governed_prefix_scan_misses_env_assignments() {
        let text = "FOO=1 just test\n";
        let naive = naive_line_devex_commands(text);
        assert!(
            naive.is_empty(),
            "without env peeling, a line that does not start with just / cargo xtask is missed; got {naive:?}"
        );
        assert_eq!(inline_devex_commands(text), vec!["just test"]);
    }

    #[test]
    fn env_prefix_strip_does_not_eat_just_parameter_assignments() {
        let commands = inline_devex_commands("`just FOO=1 pr-fast`\n");
        assert_eq!(
            commands,
            vec!["just FOO=1 pr-fast"],
            "assignments after just are recipe parameters, not shell env prefixes"
        );
        let just_recipes = BTreeSet::from(["pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::new();
        assert!(
            command_exists("just FOO=1 pr-fast", &just_recipes, &xtask_subcommands).is_err(),
            "command_exists must keep FOO=1 as the recipe token, not skip to pr-fast"
        );
    }

    #[test]
    fn unstripped_env_prefix_would_skip_command_exists_recipe_check() {
        let just_recipes = BTreeSet::from(["pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::new();
        assert!(
            command_exists("FOO=1 just stale-recipe", &just_recipes, &xtask_subcommands).is_ok(),
            "control: storing the raw env-prefixed line would silently skip the recipe check"
        );

        let commands = inline_devex_commands("FOO=1 just stale-recipe\n");
        assert_eq!(commands, vec!["just stale-recipe"]);
        assert!(
            command_exists(&commands[0], &just_recipes, &xtask_subcommands).is_err(),
            "stripped stale recipe must remain visible to command_exists"
        );
    }

    #[test]
    fn markdown_split_indented_flags_join_without_backslash() {
        let text = "just pr-fast\n    --locked\n";
        let naive = naive_line_devex_commands(text);
        assert_eq!(
            naive,
            vec!["just pr-fast"],
            "without a Markdown-split joiner the flag line is not part of the command"
        );
        assert_eq!(inline_devex_commands(text), vec!["just pr-fast --locked"]);

        let just_recipes = BTreeSet::from(["pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::new();
        assert!(command_exists("just pr-fast --locked", &just_recipes, &xtask_subcommands).is_ok());
    }

    #[test]
    fn markdown_split_does_not_join_sibling_or_prose_lines() {
        let sibling = inline_devex_commands("just pr-fast\njust stale-recipe\n");
        assert_eq!(
            sibling,
            vec!["just pr-fast", "just stale-recipe"],
            "uncontinued sibling commands must stay independent: {sibling:?}"
        );

        let indented_command = inline_devex_commands("just pr-fast\n    just stale-recipe\n");
        assert_eq!(
            indented_command,
            vec!["just pr-fast", "just stale-recipe"],
            "an indented new command is not a flag continuation: {indented_command:?}"
        );

        let prose = inline_devex_commands("just pr-fast\n    then commit the result\n");
        assert_eq!(
            prose,
            vec!["just pr-fast"],
            "indented prose that is not a flag must not join onto the command: {prose:?}"
        );
    }

    #[test]
    fn list_wrapped_commands_enter_the_denominator() {
        assert_eq!(inline_devex_commands("- just pr-fast\n"), vec!["just pr-fast"]);
        assert_eq!(inline_devex_commands("1. cargo xtask fmt\n"), vec!["cargo xtask fmt"]);
        assert_eq!(
            inline_devex_commands("- just pr-fast\n  --locked\n"),
            vec!["just pr-fast --locked"]
        );
        assert_eq!(inline_devex_commands("- FOO=1 just test\n"), vec!["just test"]);
    }

    #[test]
    fn uncontinued_fenced_commands_enter_the_denominator() {
        let text = "```bash\njust pr-fast\ncargo xtask fmt\n```\n";
        let commands = inline_devex_commands(text);
        assert_eq!(commands, vec!["just pr-fast", "cargo xtask fmt"]);

        let just_recipes = BTreeSet::from(["pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::from(["fmt".to_string()]);
        assert!(command_exists("just pr-fast", &just_recipes, &xtask_subcommands).is_ok());
        assert!(command_exists("cargo xtask fmt", &just_recipes, &xtask_subcommands).is_ok());
    }

    #[test]
    fn fenced_comment_and_language_tag_are_not_extracted() {
        let commented = inline_devex_commands(
            "```bash\n# cargo xtask check --all\n# cargo xtask fmt\njust pr-fast\n```\n",
        );
        assert_eq!(
            commented,
            vec!["just pr-fast"],
            "commented fence lines must not enter the denominator: {commented:?}"
        );

        let language_only = inline_devex_commands("```bash\necho hello\n```\n");
        assert!(
            language_only.is_empty(),
            "fence language tags are not commands: {language_only:?}"
        );
    }

    #[test]
    fn fenced_env_prefix_and_markdown_split_compose() {
        let text = "```bash\nFOO=1 just pr-fast\n    --locked\n```\n";
        assert_eq!(inline_devex_commands(text), vec!["just pr-fast --locked"]);
    }

    #[test]
    fn non_env_prefixes_stay_unextracted() {
        let wrapped = inline_devex_commands("```bash\nnix develop -c just ci-gate\n```\n");
        assert!(
            wrapped.is_empty(),
            "nix develop -c is not an env prefix and stays outside this claim; got {wrapped:?}"
        );
        let invalid = inline_devex_commands("=FOO just pr-fast\n");
        assert!(invalid.is_empty(), "a leading '=' is not a POSIX env name: {invalid:?}");
        let quoted_spaces = inline_devex_commands("FOO=\"1 2\" just test\n");
        assert!(
            quoted_spaces.is_empty(),
            "quoted env values containing spaces stay outside this line-joiner; got {quoted_spaces:?}"
        );
    }

    #[test]
    fn tilde_fenced_commands_enter_the_denominator() {
        assert_eq!(inline_devex_commands("~~~\njust pr-fast\n~~~\n"), vec!["just pr-fast"]);
    }

    #[test]
    fn parse_just_recipes_handles_parameterized_recipes() {
        let recipes = parse_just_recipes(
            r#"
default:
status-update subsystem="":
    cargo xtask update-status --write
"#,
        );
        assert!(recipes.contains("default"));
        assert!(recipes.contains("status-update"));
    }

    #[test]
    fn command_exists_rejects_stale_documented_commands() {
        let just_recipes = BTreeSet::from(["pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::from(["fmt".to_string()]);

        assert!(command_exists("just pr-fast", &just_recipes, &xtask_subcommands).is_ok());
        assert!(command_exists("cargo xtask fmt", &just_recipes, &xtask_subcommands).is_ok());
        assert!(command_exists("just pr-fats", &just_recipes, &xtask_subcommands).is_err());
        assert!(command_exists("cargo xtask fmtt", &just_recipes, &xtask_subcommands).is_err());
    }

    #[test]
    fn command_exists_allows_trailing_punctuation_in_docs() {
        let just_recipes = BTreeSet::from(["pr-fast".to_string()]);
        let xtask_subcommands = BTreeSet::from(["fmt".to_string()]);

        assert!(command_exists("just pr-fast,", &just_recipes, &xtask_subcommands).is_ok());
        assert!(command_exists("cargo xtask fmt.", &just_recipes, &xtask_subcommands).is_ok());
    }
}
