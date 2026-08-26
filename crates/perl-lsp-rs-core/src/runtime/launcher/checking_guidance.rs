//! Canonical user-facing vocabulary for native vs real-Perl checking.
//!
//! Current shipped commands (not the planned #10766 / #10672 split):
//!
//! - [`CHECK_FLAG`]: in-process native parser check of listed files
//! - [`CHECK_PROJECT_FLAG`]: native project parsability report at a fixed 80%
//!   threshold, not a strict all-clean check
//!
//! Real-Perl compile observation currently exists as editor/DAP `perl -c`, not
//! as `perllsp --check-perl`. Help, completions, and current docs must not
//! present unshipped flags as live commands.

/// Listed-file native parser check.
pub(crate) const CHECK_FLAG: &str = "--check";
/// Project-wide native parsability report.
pub(crate) const CHECK_PROJECT_FLAG: &str = "--check-project";

/// Short description reused by `--help` and descriptive shell completions.
pub(crate) const CHECK_DESCRIPTION: &str = "Native in-process parser check of listed files";
/// Short description reused by `--help` and descriptive shell completions.
pub(crate) const CHECK_PROJECT_DESCRIPTION: &str =
    "Native parsability report (80% threshold; not a strict all-clean check)";

/// Example-line comment for `--check`.
pub(crate) const CHECK_EXAMPLE_COMMENT: &str = "native listed-file parser check";
/// Example-line comment for `--check-project`.
pub(crate) const CHECK_PROJECT_EXAMPLE_COMMENT: &str = "native parsability report (80%)";

/// Phrases current `--help` must carry so native vs report vs real-Perl stay distinct.
pub(crate) const REQUIRED_HELP_PHRASES: &[&str] = &[
    CHECK_DESCRIPTION,
    "does not execute project Perl",
    CHECK_PROJECT_DESCRIPTION,
    "Advisories remain visible but non-blocking",
    "Need fast native feedback on listed files?",
    "Need a project parser coverage metric?",
    "Need configured Perl's compile observation?",
    "perl -c",
];

/// Unshipped flags that current copy must not recommend as live `perllsp` commands.
pub(crate) const UNSHIPPED_PERLLSP_FLAGS: &[&str] =
    &["--check-project-strict", "--parsability-report", "--check-perl"];

/// Current user-facing markdown that must use the shipped checking vocabulary.
///
/// Historical articles, archived session reports, specs, and changelogs are
/// classified elsewhere and are not rewritten by this guard.
pub(crate) const CURRENT_DOC_PATHS: &[&str] = &[
    "docs/reference/CHECKING.md",
    "docs/reference/CONFIG.md",
    "docs/reference/CONFIGURATION.md",
    "docs/tutorials/GETTING_STARTED.md",
    "docs/INDEX.md",
    "docs/contributing/DEBUGGING_LSP_SERVER.md",
    "docs/how-to/TROUBLESHOOTING.md",
    "docs/EDITORS/CODEX_CLI_SETUP.md",
    "docs/EDITORS/COC_NEOVIM_SETUP.md",
    "docs/EDITORS/CURSOR_SETUP.md",
    "docs/EDITORS/EMACS_SETUP.md",
    "docs/EDITORS/HELIX_SETUP.md",
    "docs/EDITORS/KIRO_SETUP.md",
    "docs/EDITORS/NEOVIM_SETUP.md",
    "docs/EDITORS/OPENCODE_SETUP.md",
    "docs/EDITORS/SUBLIME_SETUP.md",
    "docs/EDITORS/TRAE_SETUP.md",
    "docs/EDITORS/VIM_SETUP.md",
    "vscode-extension/README.md",
];

/// One terminology finding in a current surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    /// Stable rule id for tests and error text.
    pub rule: &'static str,
    /// 1-based line number.
    pub line: usize,
    /// Trimmed violating line.
    pub excerpt: String,
    /// Canonical replacement guidance.
    pub replacement: &'static str,
}

/// Scan current-copy text. Historical/quoted/negated uses are not findings.
pub(crate) fn scan_current_copy(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        push_line_findings(line, idx + 1, &mut findings);
    }
    findings
}

fn push_line_findings(line: &str, line_no: usize, findings: &mut Vec<Finding>) {
    if has_listed_file_check_flag(line)
        && contains_ci(line, "syntax check")
        && !contains_ci(line, "native")
        && !is_negated(line)
    {
        findings.push(Finding {
            rule: "bare_syntax_check",
            line: line_no,
            excerpt: line.to_string(),
            replacement: "Name the validator: `perllsp --check` is a native in-process parser check, not a generic syntax check and not `perl -c`.",
        });
    }

    if has_shipped_check_project(line)
        && (contains_ci(line, "strict")
            || contains_ci(line, "all-valid")
            || contains_ci(line, "all files parse clean"))
        && !is_negated(line)
    {
        findings.push(Finding {
            rule: "parsability_called_strict",
            line: line_no,
            excerpt: line.to_string(),
            replacement: "`perllsp --check-project` is a native parsability report at a fixed 80% threshold, not a strict all-clean check. Listed-file native checking is `--check`.",
        });
    }

    if contains_ci(line, "80%") && contains_ci(line, "strict syntax") && !is_negated(line) {
        findings.push(Finding {
            rule: "threshold_called_strict_syntax",
            line: line_no,
            excerpt: line.to_string(),
            replacement: "The 80% figure is the `--check-project` parsability threshold, not strict syntax validation.",
        });
    }

    if has_native_check_command(line)
        && (contains_ci(line, "perl -c")
            || contains_ci(line, "runs perl")
            || contains_ci(line, "execute project perl"))
        && !is_negated(line)
    {
        findings.push(Finding {
            rule: "native_said_to_run_perl",
            line: line_no,
            excerpt: line.to_string(),
            replacement: "Native `--check` / `--check-project` are in-process and do not execute project Perl. `perl -c` is the editor/DAP real-Perl path.",
        });
    }

    if has_native_check_command(line) && contains_ci(line, "sandbox") && !is_negated(line) {
        findings.push(Finding {
            rule: "checking_called_sandboxed",
            line: line_no,
            excerpt: line.to_string(),
            replacement: "Do not claim sandboxing. Native checks do not execute Perl; `perl -c` does execute compile-phase code and is not sandboxed.",
        });
    }

    for flag in UNSHIPPED_PERLLSP_FLAGS {
        if line.contains(&format!("perllsp {flag}")) && !is_negated(line) {
            findings.push(Finding {
                rule: "unshipped_flag_recommended",
                line: line_no,
                excerpt: line.to_string(),
                replacement: "Do not recommend `--check-project-strict`, `--parsability-report`, or `--check-perl` as current commands. They are unshipped (#10766 / #10672).",
            });
        }
    }
}

fn has_listed_file_check_flag(line: &str) -> bool {
    line.contains("`--check`")
        || line.contains("'--check'")
        || line.contains("\"--check\"")
        || line.contains("--check ")
        || line.contains("--check<")
        || line.ends_with("--check")
}

fn has_shipped_check_project(line: &str) -> bool {
    if !line.contains(CHECK_PROJECT_FLAG) {
        return false;
    }
    line.replace("--check-project-strict", "").contains(CHECK_PROJECT_FLAG)
}

fn has_native_check_command(line: &str) -> bool {
    has_listed_file_check_flag(line) || has_shipped_check_project(line)
}

fn contains_ci(line: &str, needle: &str) -> bool {
    line.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

fn is_negated(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "not ",
        "n't",
        "never ",
        "wrong",
        "do not",
        "does not",
        "must not",
        "cannot ",
        "there is no",
        "there are no",
        "unshipped",
        "not shipped",
        "does not exist",
        "is not a current",
        "not a current",
        "not on current",
    ];
    MARKERS.iter().any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const FIRST_RED_FIXTURE: &str =
        "perllsp --check-project .  # strict syntax validation at 80%\n";
    const BARE_SYNTAX_CHECK: &str = "perllsp --check lib/MyModule.pm         # syntax check\n";
    const ALL_FILES_CLEAN: &str = "perllsp --check-project . && echo \"All files parse clean\"\n";
    const NATIVE_RUNS_PERL: &str = "perllsp --check file.pl runs perl -c\n";
    const UNSHIPPED_STRICT: &str = "perllsp --check-project-strict lib/\n";
    const UNSHIPPED_PERL: &str = "perllsp --check-perl saved.pl\n";
    const CORRECT_CHECK: &str =
        "perllsp --check lib/MyModule.pm         # native listed-file parser check\n";
    const CORRECT_PROJECT: &str =
        "perllsp --check-project lib/            # native parsability report (80%)\n";
    const NEGATED_STRICT: &str =
        "`--check-project` is not a strict all-clean check; it is an 80% parsability report.\n";
    const NEGATED_UNSHIPPED: &str = "There is no `perllsp --check-perl` command on current main.\n";
    const REAL_PERL_EDITOR: &str =
        "Perl: Check Syntax runs `perl -c` and may execute BEGIN/use/source filters.\n";
    const HISTORICAL_NAVIGATOR: &str = "PerlNavigator calls `perl -c` for syntax checking.\n";

    fn rules_in(findings: &[Finding]) -> Vec<&'static str> {
        findings.iter().map(|finding| finding.rule).collect()
    }

    #[test]
    fn first_red_fixture_rejects_80_percent_strict_syntax_validation() {
        let findings = scan_current_copy(FIRST_RED_FIXTURE);
        assert!(
            rules_in(&findings).contains(&"parsability_called_strict")
                || rules_in(&findings).contains(&"threshold_called_strict_syntax"),
            "80% report called strict syntax validation must fail, got {findings:?}"
        );
        assert!(
            findings.iter().any(|finding| finding.replacement.contains("parsability report")),
            "failure must name the canonical parsability-report replacement, got {findings:?}"
        );
    }

    #[test]
    fn current_help_defect_rejects_bare_syntax_check() {
        let findings = scan_current_copy(BARE_SYNTAX_CHECK);
        assert_eq!(rules_in(&findings), vec!["bare_syntax_check"], "{findings:?}");
    }

    #[test]
    fn check_project_all_files_parse_clean_is_rejected() {
        let findings = scan_current_copy(ALL_FILES_CLEAN);
        assert_eq!(rules_in(&findings), vec!["parsability_called_strict"], "{findings:?}");
    }

    #[test]
    fn native_check_must_not_claim_to_run_perl() {
        let findings = scan_current_copy(NATIVE_RUNS_PERL);
        assert_eq!(rules_in(&findings), vec!["native_said_to_run_perl"], "{findings:?}");
    }

    #[test]
    fn unshipped_flags_are_rejected_as_live_commands() {
        assert_eq!(
            rules_in(&scan_current_copy(UNSHIPPED_STRICT)),
            vec!["unshipped_flag_recommended"]
        );
        assert_eq!(
            rules_in(&scan_current_copy(UNSHIPPED_PERL)),
            vec!["unshipped_flag_recommended"]
        );
        assert!(
            scan_current_copy("perllsp --parsability-report lib/\n")
                .iter()
                .any(|finding| finding.rule == "unshipped_flag_recommended")
        );
    }

    #[test]
    fn correct_current_examples_pass() {
        assert!(scan_current_copy(CORRECT_CHECK).is_empty());
        assert!(scan_current_copy(CORRECT_PROJECT).is_empty());
        assert!(scan_current_copy(NEGATED_STRICT).is_empty());
        assert!(scan_current_copy(NEGATED_UNSHIPPED).is_empty());
        assert!(scan_current_copy(REAL_PERL_EDITOR).is_empty());
    }

    #[test]
    fn historical_competitor_syntax_check_is_not_a_current_cli_defect() {
        assert!(
            scan_current_copy(HISTORICAL_NAVIGATOR).is_empty(),
            "competitor `perl -c` prose without `--check` must not trigger unbounded rewrite"
        );
    }

    #[test]
    fn opposite_direction_incomplete_report_must_not_pass_as_strict() {
        let findings = scan_current_copy(
            "perllsp --check-project .  # Assessment: PASS (80.0% parsable) is the strict all-valid project check\n",
        );
        assert!(
            findings.iter().any(|finding| finding.rule == "parsability_called_strict"),
            "percentage pass must not be documented as all-valid strict, got {findings:?}"
        );
    }

    #[test]
    fn articles_are_classified_historical_not_scanned() {
        assert!(
            CURRENT_DOC_PATHS.iter().all(|path| !path.starts_with("docs/articles/")),
            "historical articles must stay classified, not scanned as current copy"
        );
        assert!(
            CURRENT_DOC_PATHS.iter().all(|path| !path.starts_with("docs/project/SESSION")),
            "archived session reports must stay classified historical"
        );
    }

    #[test]
    fn help_text_uses_canonical_checking_vocabulary() {
        let help = crate::runtime::launcher::help_text();
        let findings = scan_current_copy(&help);
        assert!(findings.is_empty(), "help_text findings: {findings:?}");
        for phrase in REQUIRED_HELP_PHRASES {
            assert!(help.contains(phrase), "help must contain {phrase:?}\n{help}");
        }
        for flag in UNSHIPPED_PERLLSP_FLAGS {
            assert!(!help.contains(flag), "help must not advertise unshipped {flag}");
        }
        assert!(help.contains(CHECK_FLAG));
        assert!(help.contains(CHECK_PROJECT_FLAG));
    }

    #[test]
    fn completions_share_canonical_checking_descriptions() {
        for shell in ["bash", "zsh", "fish", "powershell"] {
            let script = crate::runtime::launcher::shell_completion(shell)
                .unwrap_or_else(|| panic!("{shell} completions"));
            assert!(script.contains(CHECK_FLAG), "{shell} missing --check");
            assert!(script.contains(CHECK_PROJECT_FLAG), "{shell} missing --check-project");
            for flag in UNSHIPPED_PERLLSP_FLAGS {
                assert!(!script.contains(flag), "{shell} advertises unshipped {flag}");
            }
        }
        for shell in ["zsh", "fish", "powershell"] {
            let script = crate::runtime::launcher::shell_completion(shell)
                .unwrap_or_else(|| panic!("{shell} completions"));
            assert!(script.contains(CHECK_DESCRIPTION), "{shell} must reuse CHECK_DESCRIPTION");
            assert!(
                script.contains(CHECK_PROJECT_DESCRIPTION),
                "{shell} must reuse CHECK_PROJECT_DESCRIPTION"
            );
        }
    }

    #[test]
    fn current_docs_use_shipped_checking_vocabulary() {
        let root = workspace_root();
        let mut missing = Vec::new();
        let mut findings = Vec::new();
        for rel in CURRENT_DOC_PATHS {
            let path = root.join(rel);
            let Ok(text) = std::fs::read_to_string(&path) else {
                missing.push(*rel);
                continue;
            };
            for finding in scan_current_copy(&text) {
                findings
                    .push(format!("{rel}:{} [{}] {}", finding.line, finding.rule, finding.excerpt));
            }
        }
        assert!(missing.is_empty(), "missing current checking docs: {missing:?}");
        assert!(findings.is_empty(), "current-copy findings:\n{}", findings.join("\n"));
    }

    #[test]
    fn checking_reference_carries_the_decision_table_and_limits() {
        let text = std::fs::read_to_string(workspace_root().join("docs/reference/CHECKING.md"))
            .expect("docs/reference/CHECKING.md");
        for needle in [
            "Need fast native feedback on listed files?",
            "Need a project parser coverage metric?",
            "Need configured Perl's compile observation?",
            CHECK_FLAG,
            CHECK_PROJECT_FLAG,
            "80%",
            "does not execute",
            "BEGIN",
            "No Perl files",
            "not a strict all-clean",
            "There is no `perllsp --check-perl`",
            "There is no `perllsp --check-project-strict`",
        ] {
            assert!(text.contains(needle), "CHECKING.md must contain {needle:?}");
        }
        assert!(
            !text.contains("SARIF") && !text.contains("application/sarif"),
            "do not document unshipped JSON/SARIF project-check as current"
        );
    }

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|crates| crates.parent())
            .expect("crates/perl-lsp-rs-core -> workspace")
            .to_path_buf()
    }
}
