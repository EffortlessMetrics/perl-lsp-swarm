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
    for command in continued_devex_commands(text) {
        if !commands.iter().any(|existing| existing == &command) {
            commands.push(command);
        }
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
            let command = segment.trim();
            if is_governed_devex_command(command) { Some(command.to_string()) } else { None }
        })
        .collect()
}

/// Join physical lines that end with a shell line-continuation into one logical line.
///
/// A continuation is a POSIX backslash-newline escape: the final character of the
/// physical line must be `\\` (immediately before the newline), and the trailing
/// backslash run must have odd length so the last `\\` is not itself escaped.
/// Trailing spaces after a backslash are not stripped for this test — they mean the
/// newline is not escaped. Ordinary `trim_end` applies only when assembling the
/// stored piece. This joiner does not strip env prefixes or join Markdown-split
/// lines that lack a real continuation.
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

fn continued_devex_commands(text: &str) -> Vec<String> {
    join_trailing_backslash_continuations(text)
        .into_iter()
        .filter_map(|line| {
            if !line.continued {
                return None;
            }
            let command = line.text.trim();
            is_governed_devex_command(command).then_some(command.to_string())
        })
        .collect()
}

fn is_governed_devex_command(command: &str) -> bool {
    command.starts_with("just ") || command.starts_with("cargo xtask ")
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
            !commands.iter().any(|command| command.contains("stale-recipe")),
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
    fn env_prefixed_and_markdown_split_commands_stay_unextracted() {
        let env_prefixed = inline_devex_commands("FOO=1 just test \\\n    --locked\n");
        assert!(
            !env_prefixed.iter().any(|command| command.contains("just test")),
            "env-prefixed invocations remain residual on #14868; got {env_prefixed:?}"
        );

        let markdown_split = inline_devex_commands("just pr-fast\n    --locked\n");
        assert!(
            !markdown_split.iter().any(|command| command.contains("--locked")),
            "Markdown-split commands without a backslash remain residual on #14868; got {markdown_split:?}"
        );
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
