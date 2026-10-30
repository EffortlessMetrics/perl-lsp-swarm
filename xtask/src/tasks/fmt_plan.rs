//! Bounded rustfmt spawn planning for the workspace formatter.
//!
//! `cargo fmt` groups every target root of a package into one rustfmt argv.
//! On Windows that argv is subject to `CreateProcessW`'s 32,767 UTF-16-unit
//! ceiling (os error 206 / `ERROR_FILENAME_EXCED_RANGE`). This module plans
//! the same denominator into deterministic batches that stay inside a declared
//! budget; it never drops a file to make the command fit.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// `CreateProcessW` `lpCommandLine` limit, including the terminating NUL.
pub(crate) const WINDOWS_CREATEPROCESS_LIMIT: usize = 32_767;

/// Declared per-spawn budget.
///
/// Half the CreateProcess ceiling so quoting, absolute-path expansion, and
/// rustfmt's own flag prefix stay inside the OS limit with ≥2× headroom.
/// Every host uses this bound: Linux `ARG_MAX` is larger, so a Windows-safe
/// spawn is a portable spawn. The budget is not inferred from one short tree.
pub(crate) const FORMATTER_SPAWN_BUDGET: usize = 16_384;

const _: () = assert!(FORMATTER_SPAWN_BUDGET < WINDOWS_CREATEPROCESS_LIMIT);

/// When the program is an unresolved basename (`rustfmt`), CreateProcess
/// substitutes the absolute PATH result. Reserve enough UTF-16 units for a
/// typical rustup toolchain path so the estimate cannot undershoot into os 206.
pub(crate) const UNRESOLVED_PROGRAM_PATH_RESERVE: usize = 1_024;

/// One rustfmt target root: the file cargo-fmt would put on argv, plus the
/// edition rustfmt must be given for that target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormatRoot {
    pub edition: String,
    pub path: PathBuf,
}

/// One planned rustfmt invocation. `files` is the complete, ordered subset
/// for this spawn; `estimated_command_len` is the Windows-encoded size
/// including the terminating NUL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormatterBatch {
    pub edition: String,
    pub files: Vec<PathBuf>,
    pub estimated_command_len: usize,
}

/// Inputs that are constant for one formatter run besides the file list.
#[derive(Debug, Clone)]
pub(crate) struct FormatterPlanArgs<'a> {
    pub program: &'a str,
    pub config_path: Option<&'a Path>,
    pub check: bool,
    pub budget: usize,
}

/// Planning failed in a way that is not formatting drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FormatterPlanError {
    /// A single file plus the fixed rustfmt flags already exceeds the bound.
    /// Dropping it would shrink coverage; succeeding would misreport a spawn
    /// limit as a clean format. The caller must fail closed without spawning.
    FileExceedsBudget { path: PathBuf, estimated: usize, budget: usize },
}

impl std::fmt::Display for FormatterPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileExceedsBudget { path, estimated, budget } => write!(
                f,
                "formatter argv for {} is {estimated} UTF-16 units, over the declared bound of \
                 {budget}; this is a process-spawn limit, not formatting drift",
                path.display()
            ),
        }
    }
}

/// Deduplicate roots by path (first wins) and pack them into bounded batches.
///
/// Roots are grouped by edition because rustfmt accepts one `--edition` per
/// spawn. Within an edition, first-seen order is preserved so the plan stays
/// deterministic without re-sorting cargo metadata.
///
/// Check and apply share this function; only `args.check` differs, which
/// changes flag overhead, not the governed file set.
pub(crate) fn plan_formatter_batches(
    roots: &[FormatRoot],
    args: &FormatterPlanArgs<'_>,
) -> Result<Vec<FormatterBatch>, FormatterPlanError> {
    let unique = unique_roots_preserve_order(roots);
    let groups = group_by_edition_preserve_order(&unique);

    let mut batches = Vec::new();
    for (edition, files) in groups {
        batches.extend(pack_edition_files(&edition, files, args)?);
    }
    Ok(batches)
}

/// The flattened, ordered file list a plan will format. Used to prove check
/// and apply consume the same denominator.
#[cfg(test)]
pub(crate) fn planned_files(batches: &[FormatterBatch]) -> Vec<&Path> {
    batches.iter().flat_map(|batch| batch.files.iter().map(PathBuf::as_path)).collect()
}

/// Windows `CreateProcessW` command-line length of `program` plus `args`,
/// including the terminating NUL, in UTF-16 units.
///
/// An unresolved basename occupies at least [`UNRESOLVED_PROGRAM_PATH_RESERVE`]
/// units so PATH expansion cannot silently overflow a short estimate.
pub(crate) fn windows_command_line_len(program: &str, args: &[&str]) -> usize {
    let mut len = program_slot_len(program);
    for arg in args {
        len = len.saturating_add(1);
        len = len.saturating_add(utf16_len(&windows_quote_arg(arg)));
    }
    len.saturating_add(1)
}

/// Quote one argument the way Rust's Windows `std::process::Command` does
/// for `Quote::Auto` (spaces/tabs/newlines/quotes/`"` empty).
pub(crate) fn windows_quote_arg(arg: &str) -> String {
    let needs_quotes =
        arg.is_empty() || arg.chars().any(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '"'));
    if !needs_quotes {
        return arg.to_string();
    }

    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                out.push('\\');
            }
            '"' => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                out.push('\\');
                out.push('"');
                backslashes = 0;
            }
            _ => {
                backslashes = 0;
                out.push(c);
            }
        }
    }
    for _ in 0..backslashes {
        out.push('\\');
    }
    out.push('"');
    out
}

fn unique_roots_preserve_order(roots: &[FormatRoot]) -> Vec<FormatRoot> {
    let mut seen = HashSet::with_capacity(roots.len());
    let mut unique = Vec::with_capacity(roots.len());
    for root in roots {
        if seen.insert(root.path.clone()) {
            unique.push(root.clone());
        }
    }
    unique
}

fn group_by_edition_preserve_order(roots: &[FormatRoot]) -> Vec<(String, Vec<PathBuf>)> {
    let mut groups: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for root in roots {
        match groups.iter_mut().find(|(edition, _)| edition == &root.edition) {
            Some((_, files)) => files.push(root.path.clone()),
            None => groups.push((root.edition.clone(), vec![root.path.clone()])),
        }
    }
    groups
}

fn pack_edition_files(
    edition: &str,
    files: Vec<PathBuf>,
    args: &FormatterPlanArgs<'_>,
) -> Result<Vec<FormatterBatch>, FormatterPlanError> {
    let fixed = rustfmt_fixed_args(edition, args.config_path, args.check);
    let mut batches = Vec::new();
    let mut current: Vec<PathBuf> = Vec::new();

    for file in files {
        let mut candidate_files = current.clone();
        candidate_files.push(file.clone());
        let candidate_len = estimated_command_len(args.program, &fixed, &candidate_files);
        if candidate_len <= args.budget {
            current.push(file);
            continue;
        }

        if current.is_empty() {
            return Err(FormatterPlanError::FileExceedsBudget {
                path: file,
                estimated: candidate_len,
                budget: args.budget,
            });
        }

        let sealed_len = estimated_command_len(args.program, &fixed, &current);
        batches.push(FormatterBatch {
            edition: edition.to_string(),
            files: std::mem::take(&mut current),
            estimated_command_len: sealed_len,
        });

        current.push(file.clone());
        let alone_len = estimated_command_len(args.program, &fixed, &current);
        if alone_len > args.budget {
            return Err(FormatterPlanError::FileExceedsBudget {
                path: file,
                estimated: alone_len,
                budget: args.budget,
            });
        }
    }

    if !current.is_empty() {
        let sealed_len = estimated_command_len(args.program, &fixed, &current);
        batches.push(FormatterBatch {
            edition: edition.to_string(),
            files: current,
            estimated_command_len: sealed_len,
        });
    }
    Ok(batches)
}

fn rustfmt_fixed_args(edition: &str, config_path: Option<&Path>, check: bool) -> Vec<String> {
    let mut args = vec!["--edition".to_string(), edition.to_string()];
    if let Some(path) = config_path {
        args.push("--config-path".to_string());
        args.push(path.to_string_lossy().into_owned());
    }
    if check {
        args.push("--check".to_string());
    }
    args
}

fn estimated_command_len(program: &str, fixed: &[String], files: &[PathBuf]) -> usize {
    let mut args: Vec<String> = Vec::with_capacity(fixed.len() + files.len());
    args.extend(fixed.iter().cloned());
    args.extend(files.iter().map(|path| path.to_string_lossy().into_owned()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    windows_command_line_len(program, &refs)
}

fn program_slot_len(program: &str) -> usize {
    let quoted = utf16_len(&windows_quote_arg(program));
    if program_is_unresolved_basename(program) {
        quoted.max(UNRESOLVED_PROGRAM_PATH_RESERVE)
    } else {
        quoted
    }
}

fn program_is_unresolved_basename(program: &str) -> bool {
    !program.contains('/') && !program.contains('\\') && !program.contains(':')
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(edition: &str, path: &str) -> FormatRoot {
        FormatRoot { edition: edition.to_string(), path: PathBuf::from(path) }
    }

    fn args<'a>(budget: usize, check: bool, config: Option<&'a Path>) -> FormatterPlanArgs<'a> {
        FormatterPlanArgs { program: "rustfmt", config_path: config, check, budget }
    }

    fn flatten(batches: &[FormatterBatch]) -> Vec<String> {
        planned_files(batches).into_iter().map(|path| path.display().to_string()).collect()
    }

    #[test]
    fn quoting_leaves_simple_args_unquoted() {
        assert_eq!(windows_quote_arg("rustfmt"), "rustfmt");
        assert_eq!(windows_quote_arg("src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn quoting_wraps_spaces_and_empty_args() {
        assert_eq!(windows_quote_arg(""), r#""""#);
        assert_eq!(windows_quote_arg("hello world"), r#""hello world""#);
    }

    #[test]
    fn quoting_escapes_embedded_quotes_and_trailing_backslashes() {
        assert_eq!(windows_quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(windows_quote_arg(r"hello world\"), r#""hello world\\""#);
    }

    #[test]
    fn command_line_len_includes_spaces_and_nul() {
        // "rustfmt" is a basename, so the program slot is the reserve.
        let len = windows_command_line_len("rustfmt", &["--check", "a.rs"]);
        assert_eq!(
            len,
            UNRESOLVED_PROGRAM_PATH_RESERVE + 1 + "--check".len() + 1 + "a.rs".len() + 1
        );
        assert!(len < WINDOWS_CREATEPROCESS_LIMIT);
        assert!(len <= FORMATTER_SPAWN_BUDGET);
    }

    #[test]
    fn resolved_program_uses_its_actual_quoted_length() {
        let program =
            r"C:\Users\dev\.rustup\toolchains\1.95.0-x86_64-pc-windows-msvc\bin\rustfmt.exe";
        let len = windows_command_line_len(program, &["--check"]);
        assert_eq!(len, program.len() + 1 + "--check".len() + 1);
        assert!(
            len < UNRESOLVED_PROGRAM_PATH_RESERVE,
            "this rustup path is shorter than the reserve"
        );
    }

    #[test]
    fn empty_roots_yield_no_batches() {
        let batches = plan_formatter_batches(&[], &args(FORMATTER_SPAWN_BUDGET, true, None))
            .expect("empty input is a valid empty plan");
        assert!(batches.is_empty());
    }

    #[test]
    fn overlapping_roots_are_selected_exactly_once() {
        let roots = vec![
            root("2024", "crates/a/src/lib.rs"),
            root("2024", "crates/a/src/main.rs"),
            root("2021", "crates/a/src/lib.rs"),
        ];
        let batches = plan_formatter_batches(&roots, &args(FORMATTER_SPAWN_BUDGET, true, None))
            .expect("overlapping roots must still plan");
        assert_eq!(flatten(&batches), vec!["crates/a/src/lib.rs", "crates/a/src/main.rs"]);
        let mut seen = HashSet::new();
        for file in planned_files(&batches) {
            assert!(seen.insert(file.to_path_buf()), "duplicate selection: {}", file.display());
        }
    }

    #[test]
    fn mixed_editions_stay_in_separate_spawns() {
        let roots = vec![root("2021", "legacy.rs"), root("2024", "modern.rs")];
        let batches = plan_formatter_batches(&roots, &args(FORMATTER_SPAWN_BUDGET, false, None))
            .expect("mixed editions must plan");
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].edition, "2021");
        assert_eq!(batches[1].edition, "2024");
        assert_eq!(flatten(&batches), vec!["legacy.rs", "modern.rs"]);
    }

    #[test]
    fn a_package_that_exceeds_the_windows_bound_is_split_not_truncated() {
        // Independent of the author's absolute path: a long Windows-style
        // prefix plus enough files that one unbounded argv would cross the
        // declared budget.
        let prefix = r"\\?\C:\Users\contributor\source\repos\perl-lsp-swarm\crates\synthetic\tests";
        let count = 400usize;
        let roots: Vec<FormatRoot> = (0..count)
            .map(|index| root("2024", &format!("{prefix}\\file_{index:04}.rs")))
            .collect();

        let unbounded = estimated_command_len(
            "rustfmt",
            &rustfmt_fixed_args("2024", None, true),
            &roots.iter().map(|root| root.path.clone()).collect::<Vec<_>>(),
        );
        assert!(
            unbounded > FORMATTER_SPAWN_BUDGET,
            "fixture must force multiple operations; unbounded={unbounded}"
        );
        assert!(
            unbounded > WINDOWS_CREATEPROCESS_LIMIT,
            "fixture must also exceed the real CreateProcess ceiling; unbounded={unbounded}"
        );

        let batches = plan_formatter_batches(&roots, &args(FORMATTER_SPAWN_BUDGET, true, None))
            .expect("a long denominator must pack, not fail closed");
        assert!(batches.len() >= 2, "expected multiple bounded operations, got {}", batches.len());

        let files = flatten(&batches);
        assert_eq!(files.len(), count, "last file or last batch must not be omitted");
        assert_eq!(files.first(), Some(&format!("{prefix}\\file_0000.rs")));
        assert_eq!(files.last(), Some(&format!("{prefix}\\file_{:04}.rs", count - 1)));

        let mut seen = HashSet::new();
        for file in &files {
            assert!(seen.insert(file.clone()), "duplicate: {file}");
        }
        for batch in &batches {
            assert!(
                batch.estimated_command_len <= FORMATTER_SPAWN_BUDGET,
                "batch of {} files estimated at {}",
                batch.files.len(),
                batch.estimated_command_len
            );
            assert!(
                batch.estimated_command_len < WINDOWS_CREATEPROCESS_LIMIT,
                "estimated spawn still above CreateProcess: {}",
                batch.estimated_command_len
            );
        }
    }

    #[test]
    fn a_tight_budget_still_covers_every_file_exactly_once() {
        let roots: Vec<FormatRoot> =
            (0..12).map(|index| root("2024", &format!("f{index:02}.rs"))).collect();
        // Small enough that several files cannot share a spawn, large enough
        // that each short relative path plus flags still fits alone.
        let budget = UNRESOLVED_PROGRAM_PATH_RESERVE + 64;
        let batches = plan_formatter_batches(&roots, &args(budget, true, None))
            .expect("tight budget must pack");
        assert!(batches.len() >= 2, "tight budget must force multiple operations");
        assert_eq!(flatten(&batches).len(), 12);
        assert_eq!(flatten(&batches).last().map(String::as_str), Some("f11.rs"));
        for batch in &batches {
            assert!(batch.estimated_command_len <= budget);
        }
    }

    #[test]
    fn a_single_file_over_the_budget_is_instrument_failure_not_clean() {
        let huge = "x".repeat(FORMATTER_SPAWN_BUDGET);
        let roots = vec![root("2024", &huge)];
        let error = plan_formatter_batches(&roots, &args(FORMATTER_SPAWN_BUDGET, true, None))
            .expect_err("an over-budget file must not plan as success");
        match error {
            FormatterPlanError::FileExceedsBudget { estimated, budget, .. } => {
                assert!(estimated > budget);
                assert_eq!(budget, FORMATTER_SPAWN_BUDGET);
            }
        }
        let rendered = error.to_string();
        assert!(rendered.contains("process-spawn limit"), "{rendered}");
        assert!(rendered.contains("not formatting drift"), "{rendered}");
    }

    #[test]
    fn check_and_apply_cover_the_same_files_in_the_same_order() {
        let config = Path::new(r"C:\repo\rustfmt.toml");
        let roots: Vec<FormatRoot> = (0..80)
            .map(|index| root("2024", &format!(r"\\?\C:\repo\crates\pkg\tests\case_{index:03}.rs")))
            .collect();
        let check =
            plan_formatter_batches(&roots, &args(2_048, true, Some(config))).expect("check plan");
        let apply =
            plan_formatter_batches(&roots, &args(2_048, false, Some(config))).expect("apply plan");
        assert_eq!(flatten(&check), flatten(&apply));
        assert_eq!(
            check.iter().map(|batch| batch.edition.as_str()).collect::<Vec<_>>(),
            apply.iter().map(|batch| batch.edition.as_str()).collect::<Vec<_>>(),
        );
        assert!(check.iter().all(|batch| batch.estimated_command_len <= 2_048));
        assert!(apply.iter().all(|batch| batch.estimated_command_len <= 2_048));
    }

    #[test]
    fn there_is_no_file_count_cap_on_the_remainder() {
        let roots: Vec<FormatRoot> =
            (0..1_000).map(|index| root("2024", &format!("n{index}.rs"))).collect();
        let batches = plan_formatter_batches(&roots, &args(FORMATTER_SPAWN_BUDGET, false, None))
            .expect("a thousand short paths must all be planned");
        assert_eq!(flatten(&batches).len(), 1_000);
        assert_eq!(flatten(&batches)[999], "n999.rs");
    }
}
