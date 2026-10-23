//! Production unwrap/expect and panic-family ratchet.
//!
//! The two scanners share one walk so neither can hide the other behind an
//! early return. #16253. The print layer is a separate seam: both FAIL lines
//! must still be written when both counts exceed their baselines. #16426.

use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::io::{self, Write};
use std::path::Path;
use std::sync::LazyLock;

use crate::{
    display_path, first_cfg_test_line_number, production_source_files_for_ci_checks, read_lines,
    read_usize_file,
};

static UNWRAP_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\.unwrap\(|\.expect\("));
static PANIC_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(panic!\(|todo!\(|unimplemented!\(|unreachable!\()"));
static COMMENT_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| Regex::new(r"^\s*//"));

const OFFENDER_PRINT_LIMIT: usize = 10;

fn compiled(
    regex: &'static LazyLock<Result<Regex, regex::Error>>,
    label: &str,
) -> Result<&'static Regex> {
    regex.as_ref().map_err(|err| eyre!("{label} regex failed to compile: {err}"))
}

/// CLI entry used by `main`. Crate-visible because dispatch lives in another
/// module; scan, report, and print stay private — same-module tests see them.
pub(crate) fn check_unwraps_prod(repo_root: &Path) -> Result<i32> {
    check_unwraps_prod_to(repo_root, &mut io::stdout())
}

fn check_unwraps_prod_to(repo_root: &Path, out: &mut impl Write) -> Result<i32> {
    scan_prod_unwraps_and_panics(repo_root)?.write_to(out)
}

/// The two production-line checks share a single walk so neither scanner
/// reads ahead of the other. Splitting them apart used to mean an early
/// return on the first failure hid the second; keeping them in one struct
/// forces the printing layer to see both lists before deciding an exit
/// code. #16253.
struct ProdUnwrapsAndPanicsReport {
    unwrap_offenders: Vec<String>,
    panic_offenders: Vec<String>,
    unwrap_baseline: usize,
    panic_baseline: usize,
}

impl ProdUnwrapsAndPanicsReport {
    /// Write both count lines, then both FAIL lines when they apply, then the
    /// exit code. An early return after the unwrap FAIL is the defect #16426
    /// pins: the panic FAIL would never reach `out`.
    fn write_to(&self, out: &mut impl Write) -> Result<i32> {
        let unwrap_failed = self.unwrap_offenders.len() > self.unwrap_baseline;
        let panic_failed = self.panic_offenders.len() > self.panic_baseline;

        writeln!(
            out,
            "unwrap/expect: {} (baseline: {})",
            self.unwrap_offenders.len(),
            self.unwrap_baseline
        )?;
        if unwrap_failed {
            writeln!(
                out,
                "FAIL: unwrap/expect count ({}) exceeds baseline ({})",
                self.unwrap_offenders.len(),
                self.unwrap_baseline
            )?;
            writeln!(out)?;
            writeln!(out, "Offenders:")?;
            for line in self.unwrap_offenders.iter().take(OFFENDER_PRINT_LIMIT) {
                writeln!(out, "{line}")?;
            }
        }

        writeln!(
            out,
            "panic-family macros: {} (baseline: {})",
            self.panic_offenders.len(),
            self.panic_baseline
        )?;
        if panic_failed {
            writeln!(
                out,
                "FAIL: panic-family count ({}) exceeds baseline ({})",
                self.panic_offenders.len(),
                self.panic_baseline
            )?;
            writeln!(out)?;
            writeln!(out, "Offenders:")?;
            for line in self.panic_offenders.iter().take(OFFENDER_PRINT_LIMIT) {
                writeln!(out, "{line}")?;
            }
            writeln!(
                out,
                "If you removed panic-family macros, update ci/panic_prod_baseline.txt with the new lower count."
            )?;
        }
        Ok(if unwrap_failed || panic_failed { 1 } else { 0 })
    }
}

fn scan_prod_unwraps_and_panics(repo_root: &Path) -> Result<ProdUnwrapsAndPanicsReport> {
    let unwrap_re = compiled(&UNWRAP_RE, "unwrap/expect")?;
    let panic_re = compiled(&PANIC_RE, "panic-family")?;
    let comment_re = compiled(&COMMENT_RE, "comment")?;
    let mut unwrap_offenders = Vec::new();
    let mut panic_offenders = Vec::new();

    let sources = production_source_files_for_ci_checks(repo_root)?;

    for path in sources.iter() {
        let rel = display_path(repo_root, path);
        let lines = read_lines(path)?;
        let test_start = first_cfg_test_line_number(path).unwrap_or(usize::MAX);
        for (index, line) in lines.iter().enumerate() {
            let line_no = index + 1;
            if line_no >= test_start {
                continue;
            }
            if !comment_re.is_match(line)
                && unwrap_re.is_match(line)
                && !(line.contains("self.expect(")
                    || line.contains("s.expect(")
                    || line.contains("self.context.expect("))
            {
                unwrap_offenders.push(format!("{rel}:{line_no}:{line}"));
            }
            if panic_re.is_match(line)
                && !comment_re.is_match(line)
                && !is_allowlisted_prod_panic_hit(line)
            {
                panic_offenders.push(format!("{rel}:{line_no}:{line}"));
            }
        }
    }

    let unwrap_baseline = read_usize_file(&repo_root.join("ci/unwrap_prod_baseline.txt"), 0)?;
    let panic_baseline = read_usize_file(&repo_root.join("ci/panic_prod_baseline.txt"), 0)?;
    Ok(ProdUnwrapsAndPanicsReport {
        unwrap_offenders,
        panic_offenders,
        unwrap_baseline,
        panic_baseline,
    })
}

fn is_allowlisted_prod_panic_hit(line: &str) -> bool {
    // Path is not part of the decision: the heredoc anti-patterns module moved,
    // and the same message conventions appear in other crates. Two conventions:
    //   • "... regex failed to compile" — used in heredoc anti-pattern initializers
    //   • "... is a known-good static pattern ..." — used in other static regex initializers
    line.contains("regex failed to compile") || line.contains("known-good static pattern")
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::ensure;
    use std::path::PathBuf;

    /// A repository tree laid out on disk, removed when the test ends.
    struct RepoTree {
        root: PathBuf,
    }

    impl RepoTree {
        fn new(label: &str) -> Result<Self> {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default();
            let root = std::env::temp_dir()
                .join(format!("unwraps-prod-{label}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(root.join("crates/demo/src"))?;
            std::fs::create_dir_all(root.join("ci"))?;
            Ok(Self { root })
        }

        fn lib(&self, contents: &str) -> Result<()> {
            std::fs::write(self.root.join("crates/demo/src/lib.rs"), contents)?;
            Ok(())
        }

        fn baselines(&self, unwrap: usize, panic: usize) -> Result<()> {
            std::fs::write(self.root.join("ci/unwrap_prod_baseline.txt"), format!("{unwrap}\n"))?;
            std::fs::write(self.root.join("ci/panic_prod_baseline.txt"), format!("{panic}\n"))?;
            Ok(())
        }

        fn scan(&self) -> Result<ProdUnwrapsAndPanicsReport> {
            scan_prod_unwraps_and_panics(&self.root)
        }

        fn render(&self) -> Result<(i32, String)> {
            let mut buf = Vec::new();
            let code = check_unwraps_prod_to(&self.root, &mut buf)?;
            let text = String::from_utf8(buf).map_err(|err| eyre!("report is not UTF-8: {err}"))?;
            Ok((code, text))
        }
    }

    impl Drop for RepoTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn report(
        unwrap_offenders: &[&str],
        panic_offenders: &[&str],
        unwrap_baseline: usize,
        panic_baseline: usize,
    ) -> ProdUnwrapsAndPanicsReport {
        ProdUnwrapsAndPanicsReport {
            unwrap_offenders: unwrap_offenders.iter().map(|line| (*line).to_string()).collect(),
            panic_offenders: panic_offenders.iter().map(|line| (*line).to_string()).collect(),
            unwrap_baseline,
            panic_baseline,
        }
    }

    fn rendered(report: &ProdUnwrapsAndPanicsReport) -> Result<(i32, String)> {
        let mut buf = Vec::new();
        let code = report.write_to(&mut buf)?;
        let text = String::from_utf8(buf).map_err(|err| eyre!("report is not UTF-8: {err}"))?;
        Ok((code, text))
    }

    // ── print layer (#16426) ─────────────────────────────────────────────────
    //
    // The #16253 regression test pinned the scan (both lists populated, exit 1)
    // but not the bytes written. A future early-return inside write_to after
    // the unwrap FAIL would still pass that test. These cases feed a constructed
    // report so a print-only defect cannot hide behind a scan.

    #[test]
    fn print_writes_both_fail_lines_when_both_counts_exceed_baseline() -> Result<()> {
        let (code, out) = rendered(&report(&["u"], &["p"], 0, 0))?;
        ensure!(code == 1, "expected exit 1, got {code}; output was {out}");
        // The report inputs are deterministic, so pin the complete rendered
        // bytes: line order, both Offenders blocks, offender values, and the
        // panic baseline-update hint. A substring check would let an
        // ordering or duplication defect reach the report unnoticed.
        let expected = concat!(
            "unwrap/expect: 1 (baseline: 0)\n",
            "FAIL: unwrap/expect count (1) exceeds baseline (0)\n",
            "\n",
            "Offenders:\n",
            "u\n",
            "panic-family macros: 1 (baseline: 0)\n",
            "FAIL: panic-family count (1) exceeds baseline (0)\n",
            "\n",
            "Offenders:\n",
            "p\n",
            "If you removed panic-family macros, update ci/panic_prod_baseline.txt with the new lower count.\n",
        );
        ensure!(out == expected, "unexpected report bytes:\n{out}");
        Ok(())
    }

    #[test]
    fn print_omits_panic_fail_when_only_unwrap_exceeds_baseline() -> Result<()> {
        let (code, out) = rendered(&report(&["u"], &["p"], 0, 1))?;
        ensure!(code == 1, "expected exit 1, got {code}; output was {out}");
        ensure!(
            out.contains("FAIL: unwrap/expect count (1) exceeds baseline (0)"),
            "missing unwrap FAIL; output was {out}"
        );
        ensure!(
            !out.contains("FAIL: panic-family"),
            "panic FAIL must be absent when the panic count is at baseline; output was {out}"
        );
        Ok(())
    }

    #[test]
    fn print_omits_unwrap_fail_when_only_panic_exceeds_baseline() -> Result<()> {
        let (code, out) = rendered(&report(&["u"], &["p"], 1, 0))?;
        ensure!(code == 1, "expected exit 1, got {code}; output was {out}");
        ensure!(
            out.contains("FAIL: panic-family count (1) exceeds baseline (0)"),
            "missing panic FAIL; output was {out}"
        );
        ensure!(
            !out.contains("FAIL: unwrap/expect"),
            "unwrap FAIL must be absent when the unwrap count is at baseline; output was {out}"
        );
        Ok(())
    }

    #[test]
    fn print_exits_zero_when_counts_equal_baseline() -> Result<()> {
        let (code, out) = rendered(&report(&["u"], &["p"], 1, 1))?;
        ensure!(code == 0, "equal-to-baseline must pass, got {code}; output was {out}");
        ensure!(!out.contains("FAIL:"), "no FAIL lines when at baseline; output was {out}");
        ensure!(
            out.contains("unwrap/expect: 1 (baseline: 1)"),
            "count line still prints on success; output was {out}"
        );
        ensure!(
            out.contains("panic-family macros: 1 (baseline: 1)"),
            "panic count line still prints on success; output was {out}"
        );
        Ok(())
    }

    #[test]
    fn print_exits_zero_when_counts_are_below_baseline() -> Result<()> {
        let (code, out) = rendered(&report(&["u"], &[], 5, 5))?;
        ensure!(code == 0, "below-baseline must pass, got {code}; output was {out}");
        ensure!(!out.contains("FAIL:"), "no FAIL lines below baseline; output was {out}");
        Ok(())
    }

    #[test]
    fn print_caps_offenders_without_dropping_them_from_the_scan_count() -> Result<()> {
        let unwrap_offenders: Vec<String> =
            (0..=OFFENDER_PRINT_LIMIT).map(|i| format!("unwrap-{i}")).collect();
        let report = ProdUnwrapsAndPanicsReport {
            unwrap_offenders,
            panic_offenders: Vec::new(),
            unwrap_baseline: 0,
            panic_baseline: 0,
        };
        ensure!(
            report.unwrap_offenders.len() == OFFENDER_PRINT_LIMIT + 1,
            "fixture must exceed the print cap"
        );
        let (code, out) = rendered(&report)?;
        ensure!(code == 1, "expected exit 1, got {code}; output was {out}");
        ensure!(
            out.contains("FAIL: unwrap/expect count (11) exceeds baseline (0)"),
            "count must reflect the full scan, not the print cap; output was {out}"
        );
        ensure!(out.contains("unwrap-0"), "first offender must print; output was {out}");
        ensure!(
            out.contains(&format!("unwrap-{}", OFFENDER_PRINT_LIMIT - 1)),
            "last printed offender must appear; output was {out}"
        );
        ensure!(
            !out.contains(&format!("unwrap-{OFFENDER_PRINT_LIMIT}")),
            "offender past the print cap must not appear; output was {out}"
        );
        Ok(())
    }

    // ── production composition ───────────────────────────────────────────────

    #[test]
    fn check_unwraps_prod_reports_both_failures_when_both_exceed_baseline() -> Result<()> {
        // Regression for the early-return defect called out in #16253, now with
        // the print-layer pin from #16426: the command that main.rs dispatches
        // must both populate the lists and write both FAIL lines.
        let tree = RepoTree::new("both_fail")?;
        tree.lib(
            "pub fn unwrap_call() { let _ = \"x\".parse::<i32>().unwrap(); }\n\
             pub fn panic_call() { unreachable!(\"dual\"); }\n",
        )?;
        tree.baselines(0, 0)?;

        let scanned = tree.scan()?;
        ensure!(
            scanned.unwrap_offenders.len() == 1,
            "expected exactly one unwrap offender; got {:?}",
            scanned.unwrap_offenders
        );
        ensure!(
            scanned.panic_offenders.len() == 1,
            "expected exactly one panic-family offender; got {:?}",
            scanned.panic_offenders
        );

        let (code, out) = tree.render()?;
        ensure!(code == 1, "expected exit 1 when both checks fail; output was {out}");
        ensure!(
            out.contains("FAIL: unwrap/expect"),
            "command path must print the unwrap FAIL; output was {out}"
        );
        ensure!(
            out.contains("FAIL: panic-family"),
            "command path must print the panic FAIL; output was {out}"
        );
        Ok(())
    }

    // ── scan exclusions ──────────────────────────────────────────────────────

    #[test]
    fn scan_skips_commented_unwrap_and_panic_lines() -> Result<()> {
        let tree = RepoTree::new("comments")?;
        tree.lib(
            "// let _ = x.unwrap();\n\
             // panic!(\"no\");\n\
             pub fn ok() {}\n",
        )?;
        tree.baselines(0, 0)?;
        let scanned = tree.scan()?;
        ensure!(
            scanned.unwrap_offenders.is_empty(),
            "commented unwrap must not count; got {:?}",
            scanned.unwrap_offenders
        );
        ensure!(
            scanned.panic_offenders.is_empty(),
            "commented panic must not count; got {:?}",
            scanned.panic_offenders
        );
        let (code, out) = tree.render()?;
        ensure!(code == 0, "comments-only tree must pass; output was {out}");
        Ok(())
    }

    #[test]
    fn scan_skips_self_expect_forms_but_still_counts_other_expect() -> Result<()> {
        let tree = RepoTree::new("self-expect")?;
        tree.lib(
            "fn a(&self) { let _ = self.expect(\"x\"); }\n\
             fn b(s: T) { let _ = s.expect(\"x\"); }\n\
             fn c(&self) { let _ = self.context.expect(\"x\"); }\n\
             fn d(foo: T) { let _ = foo.expect(\"x\"); }\n",
        )?;
        tree.baselines(0, 0)?;
        let scanned = tree.scan()?;
        ensure!(
            scanned.unwrap_offenders.len() == 1,
            "only foo.expect should count; got {:?}",
            scanned.unwrap_offenders
        );
        ensure!(
            scanned.unwrap_offenders.iter().any(|line| line.contains("foo.expect")),
            "the counted offender must be foo.expect; got {:?}",
            scanned.unwrap_offenders
        );
        Ok(())
    }

    #[test]
    fn scan_skips_allowlisted_panic_messages_but_still_counts_others() -> Result<()> {
        let tree = RepoTree::new("allowlist")?;
        tree.lib(
            "fn a() { unreachable!(\"regex failed to compile\"); }\n\
             fn b() { unreachable!(\"known-good static pattern\"); }\n\
             fn c() { unreachable!(\"other\"); }\n",
        )?;
        tree.baselines(0, 0)?;
        let scanned = tree.scan()?;
        ensure!(
            scanned.panic_offenders.len() == 1,
            "only the non-allowlisted panic should count; got {:?}",
            scanned.panic_offenders
        );
        ensure!(
            scanned.panic_offenders.iter().any(|line| line.contains("other")),
            "the counted offender must be the non-allowlisted line; got {:?}",
            scanned.panic_offenders
        );
        Ok(())
    }

    #[test]
    fn allowlisted_prod_panic_hit_matches_message_conventions() -> Result<()> {
        ensure!(
            is_allowlisted_prod_panic_hit(
                r#"        Err(_) => unreachable!("FORMAT_PATTERN regex failed to compile"),"#
            ),
            "heredoc 'regex failed to compile' must match"
        );
        ensure!(
            is_allowlisted_prod_panic_hit(
                r#"        Err(err) => unreachable!("GLOBAL_VAR_ASSIGNMENT_RE is a known-good static pattern: {err}"),"#
            ),
            "known-good static pattern must match"
        );
        ensure!(
            !is_allowlisted_prod_panic_hit(r#"                        _ => unreachable!(),"#),
            "bare unreachable!() must not match"
        );
        Ok(())
    }

    #[test]
    fn allowlisted_prod_panic_hit_all_seven_heredoc_patterns() -> Result<()> {
        let all_seven = [
            r#"        Err(_) => unreachable!("FORMAT_PATTERN regex failed to compile"),"#,
            r#"        Err(_) => unreachable!("BEGIN_BLOCK_PATTERN regex failed to compile"),"#,
            r#"        Err(_) => unreachable!("DYNAMIC_DELIMITER_PATTERN regex failed to compile"),"#,
            r#"        Err(_) => unreachable!("SOURCE_FILTER_PATTERN regex failed to compile"),"#,
            r#"        Err(_) => unreachable!("REGEX_HEREDOC_PATTERN regex failed to compile"),"#,
            r#"        Err(_) => unreachable!("EVAL_HEREDOC_PATTERN regex failed to compile"),"#,
            r#"    Err(_) => unreachable!("TIE_PATTERN regex failed to compile"),"#,
        ];
        for line in all_seven {
            ensure!(
                is_allowlisted_prod_panic_hit(line),
                "heredoc initializer must be allowlisted: {line}"
            );
        }
        Ok(())
    }
}
