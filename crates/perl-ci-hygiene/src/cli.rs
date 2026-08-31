use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "perl-ci-hygiene",
    version = "0.10.0",
    about = "Native Rust versions of CI scripts"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: CliCommand,
}

#[derive(Subcommand)]
pub(crate) enum CliCommand {
    /// Benchmark perl-parser against tree-sitter-perl-c for standard cases.
    RunParserComparison,
    /// Print and apply environment caps for local safety checks.
    Preflight,
    /// Run cargo test with concurrency caps for Rust tasks.
    TestCapped {
        #[arg(trailing_var_arg = true)]
        cargo_args: Vec<String>,
    },
    /// Run E2E test subset with a shared lock to cap parallel invocations.
    E2eGate {
        #[arg(trailing_var_arg = true)]
        cargo_args: Vec<String>,
    },
    /// Run preflight checks then E2E lock-gated cargo test.
    TestE2ECapped {
        #[arg(trailing_var_arg = true)]
        cargo_args: Vec<String>,
    },
    /// Verify stacker behavior in release/debug modes.
    VerifyStacker,
    /// Run iterative parser validation and related tests/benchmarks.
    TestIterativeParser,
    /// Compare bundled parser artifacts between v2 parser modules.
    CheckV2BundleSync,
    /// Compare benchmark outputs with the Python benchmark comparator.
    CompareBenchmarks {
        // `allow_hyphen_values` is required alongside `trailing_var_arg` so
        // flags like `--fail-on-regression` forward straight through to the
        // Python comparator instead of clap rejecting them as unexpected
        // arguments (#3979).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Compare modern, C legacy, and parser outputs across sample snippets.
    RunComparison,
    /// Run quick parser benchmarks across preselected fixture files.
    QuickBench,
    /// Run pure-Rust parser benchmark across generated fixture sizes.
    SimpleBench,
    /// Profile stack-overflow behavior in debug-mode parser tests.
    ProfileStackOverflow,
    /// Build cargo package --dry-run for workspace crates with dynamic local patch config.
    CargoPackageWorkspaceDryRun {
        #[arg(trailing_var_arg = true)]
        crates: Vec<String>,
    },
    /// Emit a single initialize request against perl-lsp stdin.
    SimpleLspTest,
    /// Check workspace version sync across every tracked site.
    CheckVersionSync,
    /// Bump the workspace version across every tracked site.
    BumpVersion { version: String },
    /// Run edge case test suites, with optional benchmark/coverage submodes.
    TestEdgeCases {
        #[arg(long)]
        bench: bool,
        #[arg(long)]
        coverage: bool,
    },
    /// Generate lightweight receipt artifacts without running tests.
    QuickReceipts,
    /// Run LSP cancellation tests via pre-built test binary.
    TestLspCancellation,
    /// Generate badges from publication facts and update README files.
    GenerateBadges {
        #[arg(long)]
        check: bool,
    },
    /// Install local development git hooks.
    InstallGithooks,
    /// Check installed git hooks against the repository-generated versions.
    CheckGithooks,
    /// Check docs for machine-specific paths.
    CheckDocPaths { docs_dir: Option<String> },
    /// Check relative Markdown links in a documentation subtree.
    CheckDocLinks { docs_dir: Option<String> },
    /// Check active status docs agree with the canonical workspace version and published-crate count.
    CheckDocDrift,
    /// Enforce linked-only task-marker policy.
    CheckTodos {
        #[arg(long)]
        list: bool,
    },
    /// Prevent fatal constructs in production crates.
    ForbidFatalConstructs {
        #[arg(short, long)]
        verbose: bool,
    },
    /// Track ignored tests and enforce gate policy.
    IgnoredTestCount {
        #[arg(long)]
        update: bool,
        #[arg(long)]
        check: bool,
    },
    /// Scan docs for documentation hygiene problems.
    CheckDocHygiene,
    /// Enforce ignored test cap and trend baseline.
    CheckIgnored,
    /// Run local development quality checks mirroring CI.
    CheckLocal,
    /// Count missing_docs warnings and enforce baseline ratchet.
    CheckMissingDocs,
    /// Enforce no lock().unwrap() and similar panic-prone calls.
    CheckP0Locks,
    /// Enforce parse-error baseline against corpus audit report.
    CheckParseErrors,
    /// Ensure parser feature matrix stays in sync with latest audit report.
    CheckParserMatrix,
    /// Enforce production unsafe syntax budget.
    CheckUnsafeProd,
    /// Enforce module-scoped unwrap budgets.
    CheckUnwrapsModules,
    /// Enforce production unwrap/panic-family budgets.
    CheckUnwrapsProd,
    /// Enforce complete test-code `panic!` identities against `ci/panic_test_identities.json`.
    CheckPanicTest {
        /// Emit the complete test-source panic identity inventory without applying the legacy count gate.
        #[arg(long)]
        inventory: bool,
        /// Validate the complete inventory against an accepted identity registry.
        #[arg(long, value_name = "PATH", conflicts_with = "inventory")]
        identity_registry: Option<PathBuf>,
    },
    /// Enforce that a `.expect("…")` migrated to a `must*` helper keeps its assertion context.
    CheckMustContext {
        /// Base ref to diff `HEAD` against. Defaults to `$CI_SCOPE_BASE`,
        /// `$GITHUB_BASE_REF`, `origin/main`, `main`, then `HEAD~1`.
        #[arg(long, value_name = "REF")]
        base: Option<String>,
    },
    /// Enforce no raw print macros in library source (println!/eprintln! belong in tracing).
    CheckPrintInLib,
    /// Enforce regex constructors live in LazyLock/OnceLock statics, never per-call.
    CheckRegexStatic,
    /// Execute the quick CI mirror.
    QuickCheck,
    /// Run heredoc integration tests, using xtask when available.
    TestHeredocs,
}

#[cfg(test)]
mod tests {
    use super::{Cli, CliCommand};
    use clap::{Parser, error::ErrorKind};
    use color_eyre::eyre::{Result, eyre};
    use std::path::PathBuf;

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn cargo_passthrough_command(subcommand: &str) -> Result<CliCommand> {
        let cli = Cli::try_parse_from([
            "perl-ci-hygiene",
            subcommand,
            "--package",
            "perl-lsp-rs-core",
            "--all-targets",
            "parser::tests",
        ])?;
        Ok(cli.command)
    }

    fn edge_case_flags(args: &[&str]) -> Result<(bool, bool)> {
        let mut argv = vec!["perl-ci-hygiene", "test-edge-cases"];
        argv.extend_from_slice(args);
        let cli = Cli::try_parse_from(argv)?;
        let CliCommand::TestEdgeCases { bench, coverage } = cli.command else {
            return Err(eyre!("expected test-edge-cases command"));
        };
        Ok((bench, coverage))
    }

    #[test]
    fn compare_benchmarks_forwards_hyphenated_arguments_in_order() -> Result<()> {
        let cli = Cli::try_parse_from([
            "perl-ci-hygiene",
            "compare-benchmarks",
            "--fail-on-regression",
            "--threshold=-0.5",
            "baseline.json",
            "candidate.json",
        ])?;
        let CliCommand::CompareBenchmarks { args } = cli.command else {
            return Err(eyre!("expected compare-benchmarks command"));
        };

        assert_eq!(
            args,
            owned(
                &["--fail-on-regression", "--threshold=-0.5", "baseline.json", "candidate.json",]
            )
        );
        Ok(())
    }

    #[test]
    fn cargo_passthrough_commands_forward_wrapper_flags_without_separator() -> Result<()> {
        let expected = owned(&["--package", "perl-lsp-rs-core", "--all-targets", "parser::tests"]);

        let CliCommand::TestCapped { cargo_args } = cargo_passthrough_command("test-capped")?
        else {
            return Err(eyre!("expected test-capped command"));
        };
        assert_eq!(cargo_args, expected);

        let CliCommand::E2eGate { cargo_args } = cargo_passthrough_command("e2e-gate")? else {
            return Err(eyre!("expected e2e-gate command"));
        };
        assert_eq!(cargo_args, expected);

        let CliCommand::TestE2ECapped { cargo_args } =
            cargo_passthrough_command("test-e2e-capped")?
        else {
            return Err(eyre!("expected test-e2e-capped command"));
        };
        assert_eq!(cargo_args, expected);
        Ok(())
    }

    #[test]
    fn panic_test_modes_reject_conflicting_inputs() {
        let result = Cli::try_parse_from([
            "perl-ci-hygiene",
            "check-panic-test",
            "--inventory",
            "--identity-registry",
            "policy/panic-test-identities.json",
        ]);

        assert!(matches!(
            result,
            Err(error) if error.kind() == ErrorKind::ArgumentConflict
        ));
    }

    #[test]
    fn panic_test_identity_registry_preserves_path() -> Result<()> {
        let cli = Cli::try_parse_from([
            "perl-ci-hygiene",
            "check-panic-test",
            "--identity-registry",
            "policy/panic-test-identities.json",
        ])?;
        let CliCommand::CheckPanicTest { inventory, identity_registry } = cli.command else {
            return Err(eyre!("expected check-panic-test command"));
        };

        assert!(!inventory);
        assert_eq!(identity_registry, Some(PathBuf::from("policy/panic-test-identities.json")));
        Ok(())
    }

    #[test]
    fn edge_case_flags_are_independent() -> Result<()> {
        assert_eq!(edge_case_flags(&[])?, (false, false));
        assert_eq!(edge_case_flags(&["--bench"])?, (true, false));
        assert_eq!(edge_case_flags(&["--coverage"])?, (false, true));
        assert_eq!(edge_case_flags(&["--bench", "--coverage"])?, (true, true));
        Ok(())
    }

    #[test]
    fn badge_check_flag_defaults_off_and_enables_explicitly() -> Result<()> {
        let default_cli = Cli::try_parse_from(["perl-ci-hygiene", "generate-badges"])?;
        let CliCommand::GenerateBadges { check: default_check } = default_cli.command else {
            return Err(eyre!("expected generate-badges command"));
        };
        assert!(!default_check);

        let check_cli = Cli::try_parse_from(["perl-ci-hygiene", "generate-badges", "--check"])?;
        let CliCommand::GenerateBadges { check } = check_cli.command else {
            return Err(eyre!("expected generate-badges command"));
        };
        assert!(check);
        Ok(())
    }

    #[test]
    fn doc_path_argument_distinguishes_absent_and_supplied() -> Result<()> {
        let default_cli = Cli::try_parse_from(["perl-ci-hygiene", "check-doc-paths"])?;
        let CliCommand::CheckDocPaths { docs_dir: default_dir } = default_cli.command else {
            return Err(eyre!("expected check-doc-paths command"));
        };
        assert_eq!(default_dir, None);

        let selected_cli =
            Cli::try_parse_from(["perl-ci-hygiene", "check-doc-paths", "docs/reference"])?;
        let CliCommand::CheckDocPaths { docs_dir } = selected_cli.command else {
            return Err(eyre!("expected check-doc-paths command"));
        };
        assert_eq!(docs_dir.as_deref(), Some("docs/reference"));
        Ok(())
    }

    #[test]
    fn scalar_and_boolean_inputs_reach_their_variants() -> Result<()> {
        let version_cli = Cli::try_parse_from(["perl-ci-hygiene", "bump-version", "0.18.0"])?;
        let CliCommand::BumpVersion { version } = version_cli.command else {
            return Err(eyre!("expected bump-version command"));
        };
        assert_eq!(version, "0.18.0");

        let todos_cli = Cli::try_parse_from(["perl-ci-hygiene", "check-todos", "--list"])?;
        let CliCommand::CheckTodos { list } = todos_cli.command else {
            return Err(eyre!("expected check-todos command"));
        };
        assert!(list);

        let fatal_cli = Cli::try_parse_from(["perl-ci-hygiene", "forbid-fatal-constructs", "-v"])?;
        let CliCommand::ForbidFatalConstructs { verbose } = fatal_cli.command else {
            return Err(eyre!("expected forbid-fatal-constructs command"));
        };
        assert!(verbose);
        Ok(())
    }

    #[test]
    fn missing_and_unknown_subcommands_are_rejected() {
        assert!(Cli::try_parse_from(["perl-ci-hygiene"]).is_err());

        let unknown = Cli::try_parse_from(["perl-ci-hygiene", "unknown-command"]);
        assert!(matches!(
            unknown,
            Err(error) if error.kind() == ErrorKind::InvalidSubcommand
        ));
    }
}
