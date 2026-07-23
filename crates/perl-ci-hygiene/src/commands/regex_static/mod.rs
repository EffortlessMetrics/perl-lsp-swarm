use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use crate::{
    display_path, first_cfg_test_line_number, read_lines, read_usize_file,
    walk_rust_source_files_for_ci_checks,
};

use self::lazy_scope::{LazyStaticScope, code_only};

mod lazy_scope;

/// Matches the three regex-compilation constructors called out by issue #2897:
/// `Regex::new(`, `RegexBuilder::new(`, and `Regex::builder(`. The leading `\b`
/// anchors the match to a token boundary, so `bytes::Regex::new(` still matches
/// (the `:` before `Regex` is a boundary) while an unrelated user type like
/// `CachedRegex::new(` does not. `RegexSet::new(` never matches either constructor.
static REGEX_CTOR_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\b(?:Regex::new|RegexBuilder::new|Regex::builder)\("));

fn regex_from_static(
    regex: &'static LazyLock<Result<Regex, regex::Error>>,
    label: &str,
) -> Result<&'static Regex> {
    regex.as_ref().map_err(|err| eyre!("{label} regex failed to compile: {err}"))
}

/// Enforce that regex constructors (`Regex::new` / `RegexBuilder::new` /
/// `Regex::builder`) in library source live inside a lazily-evaluated static
/// (`LazyLock`, `LazyCell`, `once_cell::sync::Lazy`, or an `OnceLock`/`OnceCell`
/// `get_or_init` closure).
///
/// A `Regex::new(...)` inside a function body recompiles the pattern on every call —
/// regex compilation is expensive, so hot paths pay a repeated cost. Wrapping the
/// pattern in a lazy static compiles it exactly once. See issue #2897.
///
/// Detection is line-based, mirroring the sibling ratchets (`check_print_in_lib`,
/// `cmd_check_unsafe_prod`). Test code is excluded two ways: whole `tests/` files
/// via [`walk_rust_source_files_for_ci_checks`], and inline `#[cfg(test)]` modules
/// via [`first_cfg_test_line_number`]. Calls inside a lazy-static initializer are
/// recognized by [`LazyStaticScope`].
///
/// The baseline is stored in `ci/regex_static_baseline.txt`; the check fails if the
/// current count exceeds it, and prints a NOTE when the count drops below (ratchet
/// down by editing the baseline file).
pub(crate) fn check_regex_static(repo_root: &Path) -> Result<i32> {
    let ctor_re = regex_from_static(&REGEX_CTOR_RE, "regex constructor")?;
    let mut offenders = Vec::new();

    for path in walk_rust_source_files_for_ci_checks(repo_root)? {
        let rel = display_path(repo_root, &path);
        let lines = read_lines(&path)?;
        let test_start = first_cfg_test_line_number(&path).unwrap_or(usize::MAX);

        let mut lazy_scope = LazyStaticScope::default();

        for (index, line) in lines.iter().enumerate() {
            let line_no = index + 1;
            if line_no >= test_start {
                break;
            }

            // Match, count, and track scope over code-only text — string literals,
            // char literals, and trailing comments are stripped so their content
            // can neither trip a false match nor corrupt delimiter tracking.
            let code = code_only(line);

            if !lazy_scope.allows_current_line(&code) {
                // Count every constructor on the line, not just the first, so two
                // per-call regexes on one line cost two units of ratchet budget.
                for _ in ctor_re.find_iter(&code) {
                    offenders.push(format!("{rel}:{line_no}:{}", line.trim()));
                }
            }

            lazy_scope.observe_line(&code);
        }
    }

    let baseline = read_usize_file(&repo_root.join("ci/regex_static_baseline.txt"), 0)?;
    println!(
        "regex constructors outside lazy statics: {} (baseline: {})",
        offenders.len(),
        baseline
    );
    if offenders.len() > baseline {
        println!(
            "FAIL: regex-constructor count ({}) exceeds baseline ({})",
            offenders.len(),
            baseline
        );
        println!();
        println!("Offenders (wrap the pattern in a LazyLock<Regex> / OnceLock static):");
        for line in offenders.iter().take(20) {
            println!("  {line}");
        }
        println!();
        println!("Compiling a Regex in a function body recompiles it on every call. Prefer:");
        println!("  static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r\"...\").unwrap());");
        println!("For a fixed pattern chosen at runtime, use an OnceLock<Regex> with get_or_init.");
        println!();
        println!(
            "If the pattern is genuinely built from runtime/user-supplied input (rare) it cannot be"
        );
        println!(
            "made static — that is an accepted exception; bump ci/regex_static_baseline.txt in a"
        );
        println!("reviewed commit. If you removed regex constructors, ratchet the baseline down.");
        return Ok(1);
    }

    if offenders.len() < baseline {
        println!(
            "NOTE: count ({}) is below baseline ({}). Update ci/regex_static_baseline.txt to ratchet down.",
            offenders.len(),
            baseline
        );
    }

    Ok(0)
}
