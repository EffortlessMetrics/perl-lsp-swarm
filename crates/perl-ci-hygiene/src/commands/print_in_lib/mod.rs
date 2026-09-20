use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use crate::{
    display_path, first_cfg_test_line_number, read_lines, read_usize_file,
    walk_rust_source_files_for_ci_checks,
};

use self::allow_scopes::{
    AttrJoiner, PrintAllowScope, file_has_print_allow, line_is_whole_line_comment,
};
use self::exclusions::is_excluded_for_print_check;

mod allow_scopes;
mod exclusions;

static PRINT_MACRO_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(println!|eprintln!|print!\(|eprint!\()"));
static DEBUG_ASSERTIONS_ATTR_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"#\[cfg\(debug_assertions\)\]"));

fn regex_from_static(
    regex: &'static LazyLock<Result<Regex, regex::Error>>,
    label: &str,
) -> Result<&'static Regex> {
    regex.as_ref().map_err(|err| eyre!("{label} regex failed to compile: {err}"))
}

/// Every library-source line that calls a print macro without an applicable
/// opt-out, as `"<relative path>:<line>:<trimmed source>"`.
///
/// Split out from [`check_print_in_lib`] so the scan can be exercised against a
/// synthetic tree. The unit tests in `allow_scopes` establish that an attribute
/// closes; only a whole-tree scan establishes that the line *after* it is still
/// reported, which is where every defect in this module has actually landed.
fn scan_offenders(repo_root: &Path) -> Result<Vec<String>> {
    let print_re = regex_from_static(&PRINT_MACRO_RE, "print macro")?;
    let debug_attr_re = regex_from_static(&DEBUG_ASSERTIONS_ATTR_RE, "debug assertions attribute")?;
    let mut offenders = Vec::new();

    for path in walk_rust_source_files_for_ci_checks(repo_root)? {
        if is_excluded_for_print_check(&path) {
            continue;
        }
        let rel = display_path(repo_root, &path);
        let lines = read_lines(&path)?;

        // Skip files that have a file-level opt-out attribute.
        if file_has_print_allow(&lines) {
            continue;
        }

        let test_start = first_cfg_test_line_number(&path).unwrap_or(usize::MAX);

        let mut debug_assertions_scope = PrintAllowScope::default();
        let mut print_allow_scope = PrintAllowScope::default();
        let mut attrs = AttrJoiner::default();

        for (index, line) in lines.iter().enumerate() {
            let line_no = index + 1;
            if line_no >= test_start {
                break;
            }

            if debug_attr_re.is_match(line) {
                debug_assertions_scope.note_attribute();
            }

            // The joiner has to see every line, because an attribute rustfmt
            // wrapped is only recognisable once its last line arrives.
            let completed = attrs.feed(line);
            if let Some(attr) = &completed
                && !attr.inner
            {
                print_allow_scope.note_attribute();
            }
            let inside_attribute = attrs.in_attribute() || completed.is_some();

            if line_is_whole_line_comment(line) {
                continue;
            }

            // Every line is tested for a print macro, including one the joiner
            // believes is inside an attribute. Skipping those was the more
            // dangerous shape: an attribute the joiner failed to close would
            // silently swallow the rest of the file. Nothing is lost by
            // testing them, because an attribute's own text cannot contain a
            // print macro call -- `clippy::print_stdout` is not `println!`.
            if print_re.is_match(line)
                && !debug_assertions_scope.allows_current_line()
                && !print_allow_scope.allows_current_line()
            {
                offenders.push(format!("{rel}:{line_no}:{}", line.trim()));
            }

            // Brace counting is the part that must skip them: a wrapped
            // attribute's braces are not an item's braces, and counting them
            // would close a scope the attribute is still opening.
            if inside_attribute {
                continue;
            }

            debug_assertions_scope.observe_line(line);
            print_allow_scope.observe_line(line);
        }
    }

    Ok(offenders)
}

/// Enforce that library source files do not contain raw `println!` / `eprintln!` /
/// `print!` / `eprint!` calls.
///
/// Library code should use `tracing::{debug,info,warn,error}` for all diagnostic
/// output. Raw print macros:
///   - Bypass the structured logging pipeline (no span context, no log level filtering).
///   - Appear in release builds and pollute the LSP's stdout/stderr channels.
///   - Make test output noisy when tests fail.
///
/// Allowed exceptions (enforced at the call site):
///   - Files with a file-level `#![allow(clippy::print_stderr/stdout)]` attribute (e.g.
///     `cli.rs` in the LSP binary crate — user-facing output is their product).
///   - Lines inside `#[cfg(debug_assertions)]` blocks (debug-only guardrails).
///   - The startup banner in `launcher/mod.rs` (function-level `#[allow]`).
///   - Any future deliberate exception must add the clippy allow attribute with a
///     comment explaining why.
///
/// This check mirrors the pattern of `cmd_check_unwraps_prod`. The baseline is stored
/// in `ci/print_in_lib_baseline.txt`; the check fails if the current count exceeds it.
pub(crate) fn check_print_in_lib(repo_root: &Path) -> Result<i32> {
    let offenders = scan_offenders(repo_root)?;
    let baseline = read_usize_file(&repo_root.join("ci/print_in_lib_baseline.txt"), 0)?;
    println!("print macros in library source: {} (baseline: {})", offenders.len(), baseline);
    if offenders.len() > baseline {
        println!("FAIL: print macro count ({}) exceeds baseline ({})", offenders.len(), baseline);
        println!();
        println!("Offenders (use tracing::{{debug,info,warn,error}} instead):");
        for line in offenders.iter().take(20) {
            println!("  {line}");
        }
        if let Some(withheld) = offenders.len().checked_sub(20).filter(|count| *count > 0) {
            println!("  ... and {withheld} more");
        }
        println!();
        println!("If the print macro is intentional, add #[allow(clippy::print_stderr)] or");
        println!("#[allow(clippy::print_stdout)] with a comment explaining why.");
        println!(
            "If you removed print macros, update ci/print_in_lib_baseline.txt with the new lower count."
        );
        return Ok(1);
    }

    if offenders.len() < baseline {
        println!(
            "NOTE: count ({}) is below baseline ({}). Update ci/print_in_lib_baseline.txt to ratchet down.",
            offenders.len(),
            baseline
        );
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A repository tree laid out on disk, removed when the test ends.
    ///
    /// Setup errors propagate rather than being discarded: a fixture whose
    /// `write` silently failed still runs its assertion, and then passes or
    /// fails on a tree that was never built.
    struct RepoTree {
        root: PathBuf,
    }

    impl RepoTree {
        /// The directory is unique per label, so parallel tests do not collide.
        fn new(label: &str) -> Result<Self> {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default();
            let root = std::env::temp_dir().join(format!("print-scan-{label}-{nanos}"));
            std::fs::create_dir_all(root.join("crates/probe/src"))?;
            Ok(Self { root })
        }

        /// Writes one library source file under `crates/probe/src/`.
        fn source(&self, name: &str, contents: &str) -> Result<()> {
            std::fs::write(self.root.join("crates/probe/src").join(name), contents)?;
            Ok(())
        }

        /// The offender lines the scan reports for this tree.
        fn scan(&self) -> Result<Vec<String>> {
            scan_offenders(&self.root)
        }
    }

    impl Drop for RepoTree {
        fn drop(&mut self) {
            // Cleanup is best-effort by necessity: a destructor has nowhere to
            // report to. Every path is unique per run, so a leftover directory
            // cannot make a later run observe the wrong condition.
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Asserts the scan reported exactly the given source lines, in order.
    fn assert_offender_sources(offenders: &[String], expected: &[&str]) {
        let sources: Vec<&str> = offenders
            .iter()
            .map(|entry| entry.rsplit_once(':').map_or(entry.as_str(), |(_, source)| source))
            .collect();
        assert_eq!(sources, expected, "offenders were {offenders:?}");
    }

    // ── trailing comment after an opt-out ────────────────────────────────────

    #[test]
    fn a_trailing_comment_on_an_opt_out_does_not_hide_a_later_print() -> Result<()> {
        // The joiner used to decide closure from the line's last two characters,
        // so `] // why` never closed it. The scan loop then skipped every
        // remaining line of the file, and a production print after it was never
        // reported. Closing the joiner is proved in `allow_scopes`; this proves
        // the line after it still reaches the offender list.
        let tree = RepoTree::new("trailing-comment")?;
        tree.source(
            "lib.rs",
            concat!(
                "#[allow(clippy::print_stdout)] // the banner is this command's product\n",
                "pub fn banner() {\n",
                "    println!(\"allowed\");\n",
                "}\n",
                "\n",
                "pub fn leaked() {\n",
                "    println!(\"reported\");\n",
                "}\n",
            ),
        )?;
        assert_offender_sources(&tree.scan()?, &["println!(\"reported\");"]);
        Ok(())
    }

    #[test]
    fn an_opt_out_with_a_trailing_comment_still_covers_its_own_body() -> Result<()> {
        // The opposite direction: the repair must not cost the attribute its
        // scope. Without this pair, deleting the joiner entirely would pass the
        // control above.
        let tree = RepoTree::new("trailing-comment-covers")?;
        tree.source(
            "lib.rs",
            concat!(
                "#[allow(clippy::print_stdout)] // the banner is this command's product\n",
                "pub fn banner() {\n",
                "    println!(\"allowed\");\n",
                "}\n",
            ),
        )?;
        assert_offender_sources(&tree.scan()?, &[]);
        Ok(())
    }

    // ── balanced one-line item ───────────────────────────────────────────────

    #[test]
    fn an_opted_out_one_line_item_does_not_exempt_a_later_print() -> Result<()> {
        // `fn banner() { println!("…"); }` has a net brace delta of zero and no
        // trailing semicolon, so the pending attribute used to survive it and
        // exempt everything that followed.
        let tree = RepoTree::new("one-line-item")?;
        tree.source(
            "lib.rs",
            concat!(
                "#[expect(clippy::print_stdout, reason = \"command product\")]\n",
                "pub fn banner() { println!(\"allowed\"); }\n",
                "\n",
                "pub fn leaked() {\n",
                "    println!(\"reported\");\n",
                "}\n",
            ),
        )?;
        assert_offender_sources(&tree.scan()?, &["println!(\"reported\");"]);
        Ok(())
    }

    #[test]
    fn an_opted_out_one_line_item_is_still_exempt_itself() -> Result<()> {
        let tree = RepoTree::new("one-line-item-covers")?;
        tree.source(
            "lib.rs",
            concat!(
                "#[expect(clippy::print_stdout, reason = \"command product\")]\n",
                "pub fn banner() { println!(\"allowed\"); }\n",
            ),
        )?;
        assert_offender_sources(&tree.scan()?, &[]);
        Ok(())
    }

    // ── cfg_attr(test, …) is not a production opt-out ────────────────────────

    #[test]
    fn a_cfg_attr_test_allowance_does_not_exempt_the_file() -> Result<()> {
        // Four library roots carry exactly this attribute. Admitting it as an
        // unconditional inner opt-out exempted those whole files in every
        // configuration, which is the reverse of what the attribute says.
        let tree = RepoTree::new("cfg-attr-test")?;
        tree.source(
            "lib.rs",
            concat!(
                "//! A library root.\n",
                "#![cfg_attr(test, allow(clippy::print_stdout))]\n",
                "\n",
                "pub fn leaked() {\n",
                "    println!(\"reported\");\n",
                "}\n",
            ),
        )?;
        assert_offender_sources(&tree.scan()?, &["println!(\"reported\");"]);
        Ok(())
    }

    #[test]
    fn an_unconditional_file_level_allowance_still_exempts_the_file() -> Result<()> {
        // The paired direction: a real `#![allow(clippy::print_stdout)]` is the
        // documented opt-out and must keep working, or the control above would
        // pass on a checker that simply stopped honouring inner attributes.
        let tree = RepoTree::new("file-level-allow")?;
        tree.source(
            "lib.rs",
            concat!(
                "//! A CLI report module.\n",
                "#![allow(clippy::print_stdout)]\n",
                "\n",
                "pub fn report() {\n",
                "    println!(\"allowed\");\n",
                "}\n",
            ),
        )?;
        assert_offender_sources(&tree.scan()?, &[]);
        Ok(())
    }

    // ── the scan reports position, not just a count ──────────────────────────

    #[test]
    fn an_offender_carries_its_relative_path_and_line_number() -> Result<()> {
        let tree = RepoTree::new("offender-shape")?;
        tree.source("lib.rs", "pub fn leaked() {\n    println!(\"reported\");\n}\n")?;
        let offenders = tree.scan()?;
        assert_eq!(offenders.len(), 1, "offenders were {offenders:?}");
        let entry = &offenders[0];
        assert!(entry.contains("probe/src/lib.rs:2:"), "entry was {entry}");
        Ok(())
    }
}
