//! Reuse the verified hosted runner for a finite set of main-xtask children.
//!
//! This changes process launch only. The existing gate shell owns output,
//! timeouts, retries and exit status; the policy command stays in the receipt.
//! Local Cargo execution and auxiliary/feature/profile builds stay unchanged.

use color_eyre::eyre::{Context, ContextCompat, Result, ensure};
use std::path::{Path, PathBuf};

// Whole-command matches only. A policy edit must deliberately renew this
// correspondence; never strip an arbitrary Cargo prefix or shell expression.
const COMMANDS: &[(&str, &str, &[&str])] = &[
    (
        "lsp_smoke",
        "cargo run --locked -p xtask -- lsp-smoke-atomic --receipt target/receipts/artifacts/lsp_smoke_children.json",
        &["lsp-smoke-atomic", "--receipt", "target/receipts/artifacts/lsp_smoke_children.json"],
    ),
    (
        "common_corpus_clean",
        "cargo run --locked -p xtask -- parser-corpus-sweep --manifest .ci/common-corpus-manifest.txt --enforce --receipt",
        &[
            "parser-corpus-sweep",
            "--manifest",
            ".ci/common-corpus-manifest.txt",
            "--enforce",
            "--receipt",
        ],
    ),
    ("msrv_authority_sync", "cargo xtask gate-policy check", &["gate-policy", "check"]),
    (
        "release_trust_invariants",
        "cargo xtask release-trust-invariants check",
        &["release-trust-invariants", "check"],
    ),
    (
        "adr_link_check",
        "cargo xtask ci-hygiene check-doc-links docs/adr",
        &["ci-hygiene", "check-doc-links", "docs/adr"],
    ),
    (
        "non_rust_inventory_check",
        "cargo xtask non-rust inventory --check",
        &["non-rust", "inventory", "--check"],
    ),
    (
        "completion_candidate_ledger",
        "cargo xtask completion-candidates check",
        &["completion-candidates", "check"],
    ),
    (
        "must_context_check",
        "cargo xtask ci-hygiene check-must-context",
        &["ci-hygiene", "check-must-context"],
    ),
    ("lint_policy", "cargo xtask check-lint-policy", &["check-lint-policy"]),
    (
        "doctest_enforcement",
        "cargo xtask ci-hygiene check-doctest-enforcement",
        &["ci-hygiene", "check-doctest-enforcement"],
    ),
    (
        "agent_ledgers_validate",
        "cargo xtask agent ledgers validate --format json",
        &["agent", "ledgers", "validate", "--format", "json"],
    ),
    (
        "code_action_generation_ledger",
        "cargo xtask check-code-action-generation-ledger",
        &["check-code-action-generation-ledger"],
    ),
    (
        "ignored_tests_check_refs",
        "just ignored-tests-check-refs",
        &["ignored-tests", "--check-issue-refs"],
    ),
    ("serial_test_ratchet", "just ci-serial-test-ratchet", &["ci-hygiene", "check-serial-test"]),
    ("regex_static_ratchet", "just ci-regex-static-ratchet", &["ci-hygiene", "check-regex-static"]),
];

fn arguments(command: &str) -> Option<&'static [&'static str]> {
    COMMANDS.iter().find_map(|(_, declared, args)| (*declared == command).then_some(*args))
}

fn child_command(executable: &Path, args: &[&str]) -> Result<String> {
    let executable = executable.to_str().context("shared xtask path is not UTF-8")?;
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    Ok(format!(
        "exec {} {}",
        quote(executable),
        args.iter().map(|arg| quote(arg)).collect::<Vec<_>>().join(" ")
    ))
}

pub(super) fn hosted_command(command: &str) -> Result<Option<String>> {
    let Some(args) = arguments(command) else { return Ok(None) };
    // xtask_artifact.py exports this only after verifying the same run's
    // producer, source, workspace and toolchain. Absence retains local Cargo
    // freshness behavior. Presence cannot select a different executable.
    let Some(claimed) = std::env::var_os("CI_XTASK") else { return Ok(None) };
    let current = std::env::current_exe()
        .context("resolving the active shared xtask")?
        .canonicalize()
        .context("canonicalizing the active shared xtask")?;
    let claimed = PathBuf::from(claimed)
        .canonicalize()
        .context("resolving the verified CI_XTASK executable")?;
    ensure!(claimed == current, "CI_XTASK does not identify the current xtask executable");
    child_command(&current, args).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn simple_recipe_command<'a>(justfile: &'a str, name: &str) -> Result<&'a str> {
        // These three wrappers contain only one main-xtask invocation and
        // optional progress echoes. Bind the complete recipe, its header and
        // relevant global settings so a future prerequisite, environment,
        // feature/profile or cleanup step cannot be silently bypassed.
        let expected = match name {
            "ignored-tests-check-refs" => {
                "    cargo --locked xtask ignored-tests --check-issue-refs\n"
            }
            "ci-serial-test-ratchet" => concat!(
                "    @echo \"🧩  Checking parallel-unsafe test serialization ratchet...\"\n",
                "    @cargo xtask ci-hygiene check-serial-test\n",
                "    @echo \"✅ Parallel-unsafe test ratchet passed\"\n",
            ),
            "ci-regex-static-ratchet" => concat!(
                "    @echo \"🔎 Checking regex-static ratchet (library source only)...\"\n",
                "    @cargo xtask ci-hygiene check-regex-static\n",
                "    @echo \"✅ Regex-static ratchet passed\"\n",
            ),
            _ => color_eyre::eyre::bail!("unreviewed just wrapper: {name}"),
        };
        for line in justfile.lines() {
            assert!(
                !["set ", "export ", "import ", "mod "]
                    .iter()
                    .any(|prefix| line.starts_with(prefix)),
                "renew shared dispatch when just execution settings change: {line}"
            );
        }
        let header = format!("\n{name}:\n");
        let (before, after) =
            justfile.split_once(&header).context("simple recipe header changed")?;
        let preceding =
            before.lines().rev().find(|line| !line.is_empty() && !line.starts_with('#'));
        assert!(
            !preceding.is_some_and(|line| line.starts_with('[')),
            "renew shared dispatch after recipe attributes change: {name}"
        );
        let body_len = after
            .split_inclusive('\n')
            .take_while(|line| line.starts_with([' ', '\t']) || line.trim().is_empty())
            .map(str::len)
            .sum::<usize>();
        let body = &after[..body_len];
        ensure!(
            body.trim_end_matches('\n') == expected.trim_end_matches('\n'),
            "renew shared dispatch after wrapper semantics change: {name}"
        );
        body.lines()
            .map(|line| line.trim().trim_start_matches('@'))
            .find(|line| line.starts_with("cargo "))
            .context("missing main-xtask recipe command")
    }

    #[test]
    fn shared_xtask_simple_recipe_rejects_additional_steps_after_blank_lines() -> Result<()> {
        let root = crate::utils::project_root()?;
        let justfile = fs::read_to_string(root.join("justfile"))?;
        for extra in ["\n    ./new-required-check\n", "\n\t./new-required-check\n"] {
            let changed = justfile.replacen(
                "    cargo --locked xtask ignored-tests --check-issue-refs\n",
                &format!("    cargo --locked xtask ignored-tests --check-issue-refs\n{extra}"),
                1,
            );
            assert_ne!(changed, justfile);
            assert!(
                simple_recipe_command(&changed, "ignored-tests-check-refs").is_err(),
                "an extra wrapper step after a blank line must prevent dispatch"
            );
        }
        Ok(())
    }

    #[test]
    fn shared_xtask_commands_match_policy_and_preserve_target_selection() -> Result<()> {
        let root = crate::utils::project_root()?;
        let policy = super::super::load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))?;
        let justfile = fs::read_to_string(root.join("justfile"))?;
        for (name, command, args) in COMMANDS {
            let gate =
                policy.gates.iter().find(|gate| gate.name == *name).context("missing gate")?;
            assert_eq!(
                gate.command.trim(),
                *command,
                "renew dispatch after policy changes: {name}"
            );
            assert_eq!(arguments(command), Some(*args));
            let main_command = match command.strip_prefix("just ") {
                Some(recipe) => simple_recipe_command(&justfile, recipe)?,
                None => command,
            };
            // These are main-binary invocations with no requested build mode.
            let declared_args = main_command
                .strip_prefix("cargo xtask ")
                .or_else(|| main_command.strip_prefix("cargo --locked xtask "))
                .or_else(|| main_command.strip_prefix("cargo run --locked -p xtask -- "))
                .context("unsupported declared main-xtask command")?;
            assert_eq!(declared_args.split_whitespace().collect::<Vec<_>>(), *args);
            for suffix in [" --features legacy", " --profile agent", " && echo extra"] {
                assert_eq!(arguments(&format!("{command}{suffix}")), None);
            }
        }
        for command in [
            "cargo run --locked -p xtask --bin parser-behavior-proof -- --check",
            "cargo run --locked -p xtask --features legacy -- parser-corpus-sweep",
            "cargo run --locked -p xtask --profile agent -- check-lint-policy",
            "cargo test -p xtask --bin xtask --locked",
            "cargo run --locked -p perl-ci-hygiene -- check-doc-links docs/adr",
            "env MODE=other cargo xtask check-lint-policy",
        ] {
            assert_eq!(arguments(command), None, "must retain real Cargo: {command}");
        }
        Ok(())
    }

    #[test]
    fn shared_xtask_child_preserves_literal_argv_and_failure_under_existing_executor() -> Result<()>
    {
        let temp = tempdir()?;
        let executable = temp.path().join("xtask with ' quotes; $literal");
        fs::write(&executable, "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\nexit 17\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        let args = ["gate-policy", "value with ' quote; $literal", "$(not-a-command)"];
        let command = child_command(&executable, &args)?;
        let result = super::super::run_shell_command_with_retries(
            &command,
            &temp.path().join("gate.log"),
            5,
            1,
            1,
            "argv-fixture",
        )
        .map_err(|error| color_eyre::eyre::eyre!("{error}"))?;
        assert_eq!(result.exit_code, 17, "a failed child is never converted to success");
        assert!(!result.timed_out);
        assert_eq!(result.stdout, args.map(|arg| format!("<{arg}>\n")).concat());
        assert!(!result.stdout.contains("retry"), "a non-timeout failure is not retried");
        Ok(())
    }

    #[test]
    fn shared_xtask_child_keeps_watchdog_and_timeout_retry() -> Result<()> {
        let temp = tempdir()?;
        let executable = temp.path().join("slow xtask");
        fs::write(&executable, "#!/bin/sh\nprintf 'child-started\\n'\nsleep 30\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        let result = super::super::run_shell_command_with_retries(
            &child_command(&executable, &["check-lint-policy"])?,
            &temp.path().join("gate.log"),
            1,
            1,
            1,
            "timeout-fixture",
        )
        .map_err(|error| color_eyre::eyre::eyre!("{error}"))?;
        assert_eq!(result.exit_code, 124);
        assert!(result.timed_out);
        assert!(result.stdout.contains("child-started"));
        assert!(
            result.stdout.contains("attempt 2/2"),
            "the existing retry owner must run both attempts"
        );
        Ok(())
    }
}
