use predicates::prelude::PredicateBooleanExt;

mod support;

fn product_command() -> assert_cmd::Command {
    assert_cmd::Command::new(perl_tdd_support::must(support::product_binary_path()))
}

#[test]
fn health_prints_ok() {
    let mut cmd = product_command();
    cmd.arg("--health").assert().success().stdout(predicates::str::contains("ok"));
}

#[test]
fn version_shows_source_revision() -> Result<(), Box<dyn std::error::Error>> {
    // The revision line is labelled by kind: "Git tag:" only when a tag is
    // actually checked out, "Git commit:" otherwise, and "Git revision:" for a
    // build made outside a git checkout. Any of the three is correct — what is
    // not acceptable is a blank value, which is what this line printed before
    // the build script checked whether `git` had actually succeeded.
    let mut cmd = product_command();
    let output = cmd.arg("--version").output()?;
    let stdout = String::from_utf8(output.stdout)?;

    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("perl-lsp"), "version should name the binary: {stdout:?}");

    let revision_line = stdout
        .lines()
        .find(|line| line.starts_with("Git "))
        .ok_or("version output is missing the source-revision line")?;
    let (label, value) = revision_line.split_once(": ").ok_or("revision line has no ': '")?;
    assert!(
        matches!(label, "Git tag" | "Git commit" | "Git revision"),
        "unexpected revision label {label:?} in {revision_line:?}"
    );
    assert!(!value.trim().is_empty(), "revision value must never be blank: {revision_line:?}");
    Ok(())
}

#[test]
fn help_prints_to_stdout() {
    let mut cmd = product_command();
    cmd.arg("--help").assert().success().stdout(predicates::str::contains("Usage:"));
}

#[test]
fn info_shows_version_and_features() {
    let mut cmd = product_command();
    cmd.arg("--info")
        .assert()
        .success()
        .stdout(predicates::str::contains("perl-lsp"))
        .stdout(predicates::str::contains("Features:"))
        .stdout(predicates::str::contains("LSP spec coverage:"));
}

#[test]
fn doctor_reports_workspace_setup() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("lib"))?;
    let config_path = dir.path().join(".perl-lsp.toml");
    if let Err(error) =
        std::fs::write(&config_path, "[perl]\ninclude_paths = [\"lib\"]\nuse_perl5lib = false\n")
    {
        return Err(format!("doctor fixture config write failed: {error}").into());
    }
    assert!(config_path.is_file(), "doctor fixture config was written");
    let dir_str = dir.path().to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.env_remove("PERL5LIB");
    let output = cmd.args(["--doctor", dir_str]).output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let workspace = dir.path().canonicalize()?;
    let lib = workspace.join("lib");
    let lines: Vec<_> = stdout.lines().collect();

    let line_with_prefix = |prefix: &str| -> Option<&str> {
        lines.iter().copied().find(|line| line.starts_with(prefix))
    };

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr, "");
    assert_eq!(lines.first().copied(), Some("perl-lsp doctor"));
    assert_eq!(lines.get(1).copied(), Some("==============="));
    assert_eq!(
        line_with_prefix("Workspace: "),
        Some(format!("Workspace: {}", workspace.display()).as_str())
    );
    assert_eq!(line_with_prefix("Project config: "), Some("Project config: loaded .perl-lsp.toml"));
    assert!(line_with_prefix("Perl: ").is_some());
    assert!(line_with_prefix("Perl version: ").is_some());
    assert!(line_with_prefix("perltidy: ").is_some());
    assert!(line_with_prefix("perlcritic: ").is_some());
    assert_eq!(line_with_prefix("PERL5LIB: "), Some("PERL5LIB: environment empty"));
    assert_eq!(line_with_prefix("PERL5LIB precedence: "), Some("PERL5LIB precedence: prepend"));
    assert_eq!(line_with_prefix("Configured includePaths:"), Some("Configured includePaths:"));
    let lib_entry = lines
        .iter()
        .copied()
        .find(|line| {
            line.starts_with("  - ")
                && line.contains(&lib.display().to_string())
                && line.contains(".perl-lsp.toml include_paths")
        })
        .ok_or("doctor output missing configured includePaths entry for lib")?;
    assert!(lib_entry.contains(".perl-lsp.toml include_paths"));
    assert_eq!(line_with_prefix("Effective @INC roots:"), Some("Effective @INC roots:"));
    assert_eq!(line_with_prefix("System @INC: "), Some("System @INC: disabled"));
    assert_eq!(line_with_prefix("Module lookup example:"), Some("Module lookup example:"));
    assert!(stdout.contains(
        "  use Foo::Bar; searches Foo/Bar.pm under the effective roots above, in order."
    ));
    assert!(stdout.contains("Next steps:"));
    assert!(
        stdout.contains(
            "  - Add missing project module roots to .perl-lsp.toml [perl].include_paths."
        )
    );
    assert!(stdout.contains(
        "  - Set PERL5LIB or use_perl5lib intentionally; doctor reports whether it participates."
    ));
    assert!(stdout.contains(
        "  - Fix roots marked unsafe; module resolution ignores relative roots that escape the workspace."
    ));
    assert!(stdout.contains(
        "  - Editor-only settings may still override this CLI report after initialization."
    ));
    assert!(stdout.contains("Claim boundary:"));
    assert!(stdout.contains(
        "  Read-only CLI report. It does not start the LSP, mutate config, scan the workspace, or apply editor-specific settings."
    ));
    assert!(stdout.ends_with('\n'));
    assert_eq!(
        stdout
            .lines()
            .filter(|line| {
                line.starts_with("  - ")
                    && line.contains(".perl-lsp.toml include_paths")
                    && line.contains(&lib.display().to_string())
            })
            .count(),
        2
    );
    Ok(())
}

#[test]
fn doctor_invalid_project_config_fails() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config_path = dir.path().join(".perl-lsp.toml");
    if let Err(error) = std::fs::write(&config_path, "[perl\ninclude_paths = [\"lib\"]") {
        return Err(format!("invalid doctor fixture config write failed: {error}").into());
    }
    assert!(config_path.is_file(), "invalid doctor fixture config was written");
    let dir_str = dir.path().to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    let output = cmd.args(["--doctor", dir_str]).output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout, "");
    assert!(stderr.contains(".perl-lsp.toml"));
    assert!(stderr.contains("syntax error"));
    Ok(())
}

#[test]
fn check_no_files_exits_with_error() {
    let mut cmd = product_command();
    cmd.arg("--check").assert().failure().stderr(predicates::str::contains("No files specified"));
}

#[test]
fn check_valid_perl_file() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("test.pl");
    std::fs::write(&file, "use strict;\nprint \"hello\\n\";\n")?;
    let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;
    let mut cmd = product_command();
    cmd.arg("--check").arg(file_str).assert().success().stdout(predicates::str::contains("ok"));
    Ok(())
}

/// Regression: `--check` reported `ok` and exited 0 on Perl that `perl -c`
/// rejects, because it read only the `Result` from `parse()` and ignored the
/// diagnostics in `errors()`. The parser recovers from each of these, so
/// `parse()` returns `Ok` for every one of them.
///
/// Each input below was confirmed rejected by real `perl -c`.
#[test]
fn check_reports_recovered_parse_errors() -> Result<(), Box<dyn std::error::Error>> {
    // (file name, source, a fragment of the expected diagnostic)
    let cases: &[(&str, &str, &str)] = &[
        ("missing_operand.pl", "my $x = ;\n", "Missing operand"),
        ("unclosed_block.pl", "sub foo {\n    my $x = 1;\n", "Unclosed block"),
        ("unclosed_paren.pl", "if ($x { print \"hi\"; }\n", "expected"),
        ("unterminated_string.pl", "print \"unterminated\n", "unknown token"),
    ];

    for (name, source, expected) in cases {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join(name);
        std::fs::write(&file, source)?;
        let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;

        let mut cmd = product_command();
        cmd.arg("--check")
            .arg(file_str)
            .assert()
            .failure()
            .stdout(predicates::str::contains("FAIL"))
            .stdout(predicates::str::contains(*expected));
    }

    Ok(())
}

/// A file can carry BOTH a fatal error and earlier recovered ones:
/// `parse_program` records recoverable diagnostics as it goes, then propagates
/// immediately on `RecursionLimit` / `NestingTooDeep` / `Cancelled` without
/// recording those. Both must be reported, and the file must count once.
///
/// This is a deliberate broadening — the pre-fix code discarded `errors()`
/// entirely in the `Err` branch and printed only the fatal message.
#[test]
fn check_reports_fatal_and_earlier_recovered_errors() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("mixed_fatal.pl");
    // A recoverable error first, then nesting past the parser's depth limit.
    let source = format!("my $x = ;\nmy $y = {}1{};\n", "(".repeat(300), ")".repeat(300));
    std::fs::write(&file, source)?;
    let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.arg("--check")
        .arg(file_str)
        .assert()
        .failure()
        // the fatal condition
        .stdout(predicates::str::contains("Nesting depth limit exceeded"))
        // and the earlier recoverable one, which the old code dropped
        .stdout(predicates::str::contains("Missing operand"));

    Ok(())
}

/// Advisory diagnostics must not fail `--check`. `ParseError::Advisory` reports
/// `blocks_clean_parse() == false`, and real `perl -c` accepts this file
/// (`advisory.pl syntax OK`), so treating it as an error would reject valid
/// Perl. The advisory is still surfaced, just not as a failure.
#[test]
fn check_advisory_diagnostics_do_not_fail() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("advisory.pl");
    // Nested quantifiers: a backtracking-risk advisory, not a syntax error.
    std::fs::write(&file, "my $r = qr/^(a+)+b$/;\nprint \"ok\\n\";\n")?;
    let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.arg("--check")
        .arg(file_str)
        .assert()
        .success()
        .stdout(predicates::str::contains("ok"))
        .stdout(predicates::str::contains("advisory:"))
        .stdout(predicates::str::contains("FAIL").not());

    Ok(())
}

/// A file whose errors are all recovered must still count toward the multi-file
/// summary and drive a non-zero exit, alongside a clean file.
#[test]
fn check_mixed_files_fails_and_counts_recovered_errors() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;

    let good = dir.path().join("good.pl");
    std::fs::write(&good, "use strict;\nprint \"hello\\n\";\n")?;
    let bad = dir.path().join("bad.pl");
    std::fs::write(&bad, "my $x = ;\n")?;

    let good_str = good.to_str().ok_or("non-UTF-8 temp path")?;
    let bad_str = bad.to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.arg("--check")
        .arg(good_str)
        .arg(bad_str)
        .assert()
        .failure()
        .stdout(predicates::str::contains("ok"))
        .stdout(predicates::str::contains("FAIL"))
        .stdout(predicates::str::contains("2 files checked, 1 with errors"));

    Ok(())
}

/// Regression: `--check` answered `ok` with exit 0 for a file real `perl`
/// rejects with `syntax error … near "print"` — a missing statement-terminating
/// semicolon, the most common Perl syntax error and so the likeliest
/// first-contact failure. A false pass from the binary that exists to validate
/// files is worse than no check at all (#5474).
#[test]
fn check_reports_a_missing_statement_semicolon() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("missing_semi.pl");
    std::fs::write(&file, "my $x = 1\nprint \"hi\";\n")?;
    let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.arg("--check")
        .arg(file_str)
        .assert()
        .failure()
        .stdout(predicates::str::contains("FAIL"))
        .stdout(predicates::str::contains("Missing `;`"))
        // the caret points at the token that proves the terminator was skipped
        .stdout(predicates::str::contains("line 2, column 1"));

    Ok(())
}

/// The control: the two places Perl permits omitting the terminator must stay
/// `ok`, or the check would reject valid Perl to catch invalid Perl.
#[test]
fn check_accepts_the_terminator_omissions_perl_permits() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    // Last statement in the file, and last statement in a block.
    let file = dir.path().join("permitted.pl");
    std::fs::write(&file, "sub f {\n    my $y = 2\n}\nmy $last = f()\n")?;
    let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.arg("--check")
        .arg(file_str)
        .assert()
        .success()
        .stdout(predicates::str::contains("ok"))
        .stdout(predicates::str::contains("Missing `;`").not());

    Ok(())
}

#[test]
fn check_nonexistent_file() {
    let mut cmd = product_command();
    cmd.arg("--check")
        .arg("/nonexistent/path/to/file.pl")
        .assert()
        .failure()
        .stderr(predicates::str::contains("error reading file"))
        .stderr(predicates::str::contains("does not exist"));
}

#[test]
fn check_path_with_file_parent_reports_missing_path() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file_parent = dir.path().join("not-a-directory");
    std::fs::write(&file_parent, "not a directory")?;
    let child = file_parent.join("file.pl");

    let mut cmd = product_command();
    cmd.args(["--check", child.to_str().ok_or("non-UTF-8 temp path")?])
        .assert()
        .failure()
        .stderr(predicates::str::contains("intermediate component"));

    Ok(())
}

#[test]
fn completion_bash_produces_output() {
    let mut cmd = product_command();
    cmd.args(["--completion", "bash"])
        .assert()
        .success()
        .stdout(predicates::str::contains("complete"));
}

#[test]
fn completion_zsh_produces_output() {
    let mut cmd = product_command();
    cmd.args(["--completion", "zsh"])
        .assert()
        .success()
        .stdout(predicates::str::contains("compdef"));
}

#[test]
fn perltidy_compat_report_prints_native_mapping() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let profile = dir.path().join(".perltidyrc");
    std::fs::write(&profile, "-l=100\n-nsok\n-q\n")?;

    let mut cmd = product_command();
    cmd.args(["--perltidy-compat-report", profile.to_str().ok_or("non-UTF-8 temp path")?])
        .assert()
        .success()
        .stdout(predicates::str::contains("# Native Format Perltidy Compatibility"))
        .stdout(predicates::str::contains("format.line_width"))
        .stdout(predicates::str::contains("format.keyword_spacing"));
    Ok(())
}

#[test]
fn perlcritic_compat_report_prints_native_mapping() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let profile = dir.path().join(".perlcriticrc");
    std::fs::write(
        &profile,
        "severity = 3\n[TestingAndDebugging::RequireUseStrict]\n[InputOutput::RequireCheckedOpen]\n",
    )?;

    let mut cmd = product_command();
    cmd.args(["--perlcritic-compat-report", profile.to_str().ok_or("non-UTF-8 temp path")?])
        .assert()
        .success()
        .stdout(predicates::str::contains("# Native Critic Perlcritic Compatibility"))
        .stdout(predicates::str::contains("native.testing.require_use_strict"))
        .stdout(predicates::str::contains("native.io.unchecked_open_close"));
    Ok(())
}

#[test]
fn check_project_missing_dir_errors() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let missing = dir.path().join("missing-project");
    let missing_str = missing.to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.args(["--check-project", missing_str])
        .assert()
        .failure()
        .stderr(predicates::str::contains("directory not found"));

    Ok(())
}

#[test]
fn check_project_file_path_errors() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("not-a-directory.pl");
    std::fs::write(&file, "use strict;\n")?;
    let file_str = file.to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.args(["--check-project", file_str])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a directory"));

    Ok(())
}

/// Source that parses into a clean AST but raises a nested-quantifier advisory.
/// The parser severity contract for this input is pinned by
/// `perl-parser-core/tests/regex_advisory_diagnostics.rs`.
const ADVISORY_ONLY_SOURCE: &str = "my $s = \"abab\";\n$s =~ /(?:[^b]*(?=(b)|(a))ab)*/;\n1;\n";

/// `--check` and `--check-project` must return the same verdict for the same
/// file. `--check` treats an advisory-only file as `ok`; `--check-project` used
/// to count every recovered diagnostic as a parse error, so the same valid Perl
/// was reported as unparsable and failed the 80% threshold.
#[test]
fn check_project_agrees_with_check_on_advisory_only_files() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let advisory = dir.path().join("advisory.pl");
    std::fs::write(&advisory, ADVISORY_ONLY_SOURCE)?;
    std::fs::write(dir.path().join("clean.pl"), "my $ok = 1;\n1;\n")?;
    let dir_str = dir.path().to_str().ok_or("non-UTF-8 temp path")?;

    let mut check = product_command();
    let check_stdout = String::from_utf8(
        check.args(["--check", advisory.to_str().ok_or("non-UTF-8 temp path")?]).output()?.stdout,
    )?;
    assert!(
        check_stdout.contains("advisory.pl: ok"),
        "--check should accept an advisory-only file, got:\n{check_stdout}"
    );

    let mut project = product_command();
    let output = project.args(["--check-project", dir_str]).output()?;
    let stdout = String::from_utf8(output.stdout)?;

    assert!(
        stdout.contains("Clean parses: 2/2"),
        "--check-project should agree that both files parse, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Assessment: PASS"),
        "--check-project should pass on valid Perl, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Advisories") && stdout.contains("Nested quantifiers"),
        "the advisory should still be reported, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Parse errors:"),
        "an advisory must not be listed as a parse error, got:\n{stdout}"
    );
    assert!(output.status.success(), "advisory-only project should exit 0");

    Ok(())
}

/// Opposite-direction control: excluding advisories from the verdict must not
/// stop `--check-project` from failing on genuinely blocking parse errors.
#[test]
fn check_project_still_fails_on_blocking_errors() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("broken.pl"), "my $value = ;\n")?;
    std::fs::write(dir.path().join("advisory.pl"), ADVISORY_ONLY_SOURCE)?;
    let dir_str = dir.path().to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    let output = cmd.args(["--check-project", dir_str]).output()?;
    let stdout = String::from_utf8(output.stdout)?;

    assert!(
        stdout.contains("Clean parses: 1/2"),
        "the blocking file must still count as unclean, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Parse errors:") && stdout.contains("broken.pl"),
        "the blocking error must still be reported, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Assessment: FAIL"),
        "50% parsable is below the 80% threshold, got:\n{stdout}"
    );
    assert!(!output.status.success(), "a blocking parse error should exit non-zero");

    Ok(())
}

/// A walk that cannot read part of the tree must say so. Silently dropping the
/// error left the report claiming a confident percentage over whatever subset
/// happened to be readable.
#[cfg(unix)]
#[test]
fn check_project_reports_paths_it_could_not_scan() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let nested = dir.path().join("sub");
    std::fs::create_dir_all(&nested)?;
    std::fs::write(dir.path().join("ok.pl"), "my $x = 1;\n1;\n")?;
    std::os::unix::fs::symlink(dir.path(), nested.join("loop"))?;
    let dir_str = dir.path().to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    let output = cmd.args(["--check-project", dir_str]).output()?;
    let stdout = String::from_utf8(output.stdout)?;

    assert!(
        stdout.contains("Paths not scanned: 1"),
        "the unreadable path should be counted, got:\n{stdout}"
    );
    assert!(
        stdout.contains("symbolic link loop"),
        "the reason should name the symlink loop, got:\n{stdout}"
    );
    assert!(
        stdout.contains("scanned files only"),
        "the assessment should be scoped to what was scanned, got:\n{stdout}"
    );

    Ok(())
}

#[test]
fn completion_fish_produces_output() {
    let mut cmd = product_command();
    cmd.args(["--completion", "fish"])
        .assert()
        .success()
        .stdout(predicates::str::contains("complete"));
}

#[test]
fn completion_unknown_shell_fails() {
    let mut cmd = product_command();
    cmd.args(["--completion", "unknown-shell"]).assert().failure();
}

#[test]
fn help_mentions_new_flags() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = product_command();
    let output = cmd.arg("--help").output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--info"), "help should mention --info");
    assert!(stdout.contains("--check"), "help should mention --check");
    assert!(stdout.contains("--doctor"), "help should mention --doctor");
    assert!(stdout.contains("--completion"), "help should mention --completion");
    Ok(())
}

#[test]
fn trailing_files_without_check_flag_errors() {
    // Trailing file arguments should require --check
    let mut cmd = product_command();
    cmd.arg("somefile.pl").assert().failure();
}

/// End-to-end CLI check for the `--ripr-facts` subcommand: the `perl-lsp`
/// binary parses a workspace, writes a `ripr-perl-facts-v1` packet to the
/// requested path, and the packet is schema-shaped and deterministic. The
/// emitter's fact extraction is covered by `perl-ripr-facts`'s own lib tests;
/// this proves the binary → `run_ripr_facts` → written-file production chain,
/// which no `--lib` test exercises (#3293 final slice).
#[test]
fn ripr_facts_emits_schema_valid_deterministic_packet() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("lib"))?;
    std::fs::create_dir_all(dir.path().join("t"))?;
    std::fs::write(
        dir.path().join("lib/Calc.pm"),
        "package Calc;\nuse strict;\nuse warnings;\n\nsub add {\n    my ($x, $y) = @_;\n    return $x + $y;\n}\n\n1;\n",
    )?;
    std::fs::write(
        dir.path().join("t/calc.t"),
        "use strict;\nuse warnings;\nuse Test::More;\nuse Calc;\n\nis(Calc::add(2, 3), 5, 'add works');\n\ndone_testing();\n",
    )?;

    // `--ripr-root` and `--ripr-out` must both be repo-relative; the tool
    // resolves them against the working directory, so run from the fixture root.
    let run = |out: &str| -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = product_command();
        cmd.current_dir(dir.path())
            .args(["--ripr-facts", "--ripr-root", ".", "--ripr-out", out])
            .assert()
            .success();
        Ok(())
    };
    run("out1.json")?;
    run("out2.json")?;

    let packet_bytes = std::fs::read(dir.path().join("out1.json"))?;
    let second_bytes = std::fs::read(dir.path().join("out2.json"))?;
    assert_eq!(
        packet_bytes, second_bytes,
        "two runs over identical inputs must produce byte-identical packets"
    );

    let packet: serde_json::Value = serde_json::from_slice(&packet_bytes)?;
    let obj = packet.as_object().ok_or("packet is not a JSON object")?;

    // The schema is `additionalProperties:false` with all 17 top-level
    // properties required, so the packet's key set must be *exactly* these 17 —
    // assert both directions (every required key present AND no extras) so an
    // accidental extra top-level field is caught as the schema violation it is.
    let required_keys = [
        "schema_version",
        "packet_id",
        "packet_status",
        "packet_fingerprint",
        "producer",
        "root",
        "input",
        "files",
        "owners",
        "changes",
        "tests",
        "oracles",
        "relations",
        "dynamic_boundaries",
        "verify_commands",
        "limitations",
        "provenance",
    ];
    for key in required_keys {
        assert!(obj.contains_key(key), "packet is missing required key `{key}`");
    }
    let actual_keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(
        obj.len(),
        required_keys.len(),
        "packet has unexpected top-level keys (schema is additionalProperties:false); \
         required {required_keys:?}, got {actual_keys:?}"
    );

    assert_eq!(packet["schema_version"], "ripr-perl-facts-v1");

    // The fingerprint is a non-null SHA-256 digest, not the old `null` placeholder.
    let fingerprint =
        packet["packet_fingerprint"].as_str().ok_or("packet_fingerprint is not a string")?;
    assert!(
        fingerprint.starts_with("sha256:"),
        "packet_fingerprint should be a sha256 digest, got `{fingerprint}`"
    );
    assert_eq!(
        fingerprint.len(),
        "sha256:".len() + 64,
        "packet_fingerprint should contain a full SHA-256 hex digest"
    );
    assert!(
        fingerprint["sha256:".len()..].bytes().all(|byte| byte.is_ascii_hexdigit()),
        "packet_fingerprint should contain only hexadecimal characters"
    );

    // The `.pm` and `.t` files were discovered as facts.
    let files = packet["files"].as_array().ok_or("files is not an array")?;
    assert!(!files.is_empty(), "files[] should not be empty for a parsed workspace");

    // The fixture was actually *parsed*, not merely read: owners come only from
    // successfully-parsed symbol declarations (a parse failure emits the file
    // fact with zero owners plus a `parse_failure` limitation), so a non-empty
    // owners[] carrying the fixture's `Calc` package proves the parser produced
    // semantic facts — file discovery alone would leave owners[] empty.
    let owners = packet["owners"].as_array().ok_or("owners is not an array")?;
    let owner_names: Vec<&str> = owners.iter().filter_map(|o| o["name"].as_str()).collect();
    assert!(
        owner_names.contains(&"Calc"),
        "owners[] should contain the parsed `Calc` package declaration, got {owner_names:?}"
    );

    Ok(())
}

/// The `--ripr-out` path is validated as repo-relative; an absolute path is
/// rejected with a clear error and a non-zero exit, and no file is written.
#[test]
fn ripr_facts_rejects_absolute_out_path() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let abs_out = dir.path().join("packet.json");
    let abs_out_str = abs_out.to_str().ok_or("non-UTF-8 temp path")?;

    let mut cmd = product_command();
    cmd.current_dir(dir.path())
        .args(["--ripr-facts", "--ripr-root", ".", "--ripr-out", abs_out_str])
        .assert()
        .failure()
        .stderr(predicates::str::contains("must be repo-relative"));

    assert!(!abs_out.exists(), "no packet should be written when the out path is rejected");
    Ok(())
}
