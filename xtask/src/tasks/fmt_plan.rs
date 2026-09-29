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
pub(crate) fn planned_files(batches: &[FormatterBatch]) -> Vec<&Path> {
    batches.iter().flat_map(|batch| batch.files.iter().map(PathBuf.as_path)).collect()
}
