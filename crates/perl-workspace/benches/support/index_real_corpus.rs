//! Shared cold scan+index sampler for benchmark slice #5
//! `workspace_index_real_corpus` (issue #17159, benchmark matrix row 5).
//!
//! This module is included via `#[path]` from both the criterion bench
//! (`benches/workspace_index_benchmark.rs`) and the `#[ignore]`d receipt test
//! (`tests/workspace_index_real_corpus_receipt.rs`), so it must lint clean
//! under BOTH target lint sets: no `unwrap`/`expect`/`panic!` (the receipt
//! target inherits the workspace `clippy::unwrap_used`/`expect_used`/`panic`
//! denies), and no `print!`/`eprintln!` diagnostics (the workspace denies
//! `clippy::print_stdout`/`clippy::print_stderr`). Everything is
//! Result-returning; callers decide how failures surface.
//!
//! # What one COLD sample is (matrix row 5 contract)
//!
//! Copy the corpus to a fresh `TempDir` (no `.git`), create a fresh
//! [`WorkspaceIndex::new`], then time, in production startup order (mirroring
//! `crates/perl-lsp-rs/src/runtime/workspace.rs:2875` `transition_to_scanning`,
//! `:2931` `discover_perl_files_with_config_and_cancel`, `:2978`
//! `transition_to_indexing`, `:3015` read bytes, `:3091` admission, `:3164`
//! `index_file`):
//!
//! 1. SCAN — [`discover_perl_files`] with the default config and no include
//!    paths (the same seam production startup uses);
//! 2. INDEX — for each discovered path, in discovery's lexical order
//!    (`discovery/mod.rs:318`): `std::fs::read` →
//!    [`DiscoveryConfig::admits_bytes`] (the #14186 one-admission-authority) →
//!    `String::from_utf8_lossy` → [`Url::from_file_path`] →
//!    [`WorkspaceIndex::index_file`].
//!
//! COLD means a fresh tree and a fresh index per sample. The OS page cache may
//! still be warm across samples; that is a documented limitation, and the p50
//! over 5 samples damps it. There is deliberately NO warmup discard on this
//! lane — cold IS the metric (the parser-scorecard warmup precedent in
//! `perf_scorecard.rs:41-49` is for micro-benches and is intentionally not
//! copied here). WARM/re-index behavior is already covered by the existing
//! "incremental update single file" and "early exit content hash check"
//! criterion groups.

// Receipt-lane APIs (threshold constants, percentile math, the indexed
// predicate) are unused when this module is included from the criterion
// bench, which only times `cold_scan_index`; the receipt test target
// exercises them. The allow is unconditional because bench targets are also
// compiled under `cfg(test)` by `cargo check/test --benches`, so a
// cfg-gated allow would not cover both inclusion modes.
#![allow(dead_code)]

use perl_workspace::discovery::{DiscoveryConfig, DiscoveryMethod, discover_perl_files};
use perl_workspace::workspace_index::WorkspaceIndex;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use url::Url;

/// Corpus directory the benchmark matrix names (issue #17159 row 5).
pub const REAL_PROJECTS_RELATIVE: &str = "test_corpus/real_projects";

/// Receipt output path for the nightly real-project variant, relative to the
/// workspace root.
pub const RECEIPT_RELATIVE_PATH: &str = ".ci/metrics/workspace_index_real_corpus.json";

/// Receipt lane sample count: p50 of 5 in-process runs (matrix row 5).
pub const COLD_SAMPLES: usize = 5;

/// Provisional acceptance bound: <= 1 s cold scan+index per real skeleton.
/// The +/-20% alert policy applies only after this baseline is calibrated.
pub const PROVISIONAL_COLD_INDEX_LIMIT_MS: u128 = 1_000;

/// Synthetic on-disk tree shape (never skipped): 40 package directories x 10
/// modules = 400 files.
pub const SYNTHETIC_PACKAGES: u32 = 40;
/// Modules per synthetic package directory.
pub const SYNTHETIC_MODULES_PER_PACKAGE: u32 = 10;

/// Return the workspace root: the first ancestor of `CARGO_MANIFEST_DIR`
/// (walking upward) that contains `Cargo.lock` (the
/// `real_project_latency.rs:155-165` pattern).
#[must_use]
pub fn workspace_root() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let mut dir = Path::new(&manifest).to_path_buf();
    loop {
        if dir.join("Cargo.lock").exists() {
            return dir;
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return Path::new(&manifest).to_path_buf(),
        }
    }
}

/// Sorted real-corpus skeleton roots under `test_corpus/real_projects`.
///
/// Dynamic on purpose: every skeleton directory present joins the run
/// (currently `catalyst_skeleton`, `dancer2_2x_skeleton`,
/// `dancer2_skeleton`, `mojolicious_skeleton`). Empty when the corpus is
/// absent — the criterion group then skips the real entries (matrix:
/// "skip if absent") while the synthetic entry never skips.
#[must_use]
pub fn real_corpus_projects() -> Vec<PathBuf> {
    let base = workspace_root().join(REAL_PROJECTS_RELATIVE);
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Copy every regular file under `src` into `dst`, preserving the relative
/// directory layout, in deterministic lexical order. Symlinks and non-file
/// entries are skipped on purpose: a staged corpus must be plain files.
///
/// Copying (instead of running discovery in place) keeps each sample
/// hermetic and deterministic: the staged tree has no `.git`, so discovery
/// exercises its `WalkDir` fallback rather than spawning `git ls-files` with
/// `cwd = root`, which would make scan cost scale with the host repository's
/// index instead of the corpus. Returns the number of files copied.
///
/// # Errors
/// Returns a message naming the failing path when listing, creating, or
/// copying any entry fails.
pub fn stage_copy(src: &Path, dst: &Path) -> Result<usize, String> {
    let mut files = Vec::new();
    collect_regular_files(src, &mut files)?;
    files.sort();
    let mut copied = 0;
    for source in &files {
        let relative = source
            .strip_prefix(src)
            .map_err(|error| format!("strip_prefix {}: {error}", source.display()))?;
        let target = dst.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create_dir_all {}: {error}", parent.display()))?;
        }
        std::fs::copy(source, &target).map_err(|error| {
            format!("copy {} -> {}: {error}", source.display(), target.display())
        })?;
        copied += 1;
    }
    Ok(copied)
}

fn collect_regular_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|error| format!("read_dir {}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read_dir {}: {error}", dir.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("file_type {}: {error}", entry.path().display()))?;
        let path = entry.path();
        if file_type.is_file() {
            files.push(path);
        } else if file_type.is_dir() {
            collect_regular_files(&path, files)?;
        }
    }
    Ok(())
}

/// Write the deterministic synthetic on-disk tree under `root`:
/// [`SYNTHETIC_PACKAGES`] package directories x [`SYNTHETIC_MODULES_PER_PACKAGE`]
/// modules = 400 files, generated from [`generate_module`]. Returns the number
/// of files written.
///
/// # Errors
/// Returns a message naming the failing path when directory creation or file
/// writes fail.
pub fn write_synthetic_tree(root: &Path) -> Result<usize, String> {
    let mut written = 0;
    for package in 0..SYNTHETIC_PACKAGES {
        let package_dir = root.join("lib").join(format!("Gen{package:02}"));
        std::fs::create_dir_all(&package_dir)
            .map_err(|error| format!("create_dir_all {}: {error}", package_dir.display()))?;
        for module in 0..SYNTHETIC_MODULES_PER_PACKAGE {
            let index = ((package * SYNTHETIC_MODULES_PER_PACKAGE) + module) as usize;
            let path = package_dir.join(format!("Module{module:02}.pm"));
            std::fs::write(&path, generate_module(index))
                .map_err(|error| format!("write {}: {error}", path.display()))?;
            written += 1;
        }
    }
    Ok(written)
}

/// One cold scan+index sample (matrix row 5).
#[derive(Debug, Clone)]
pub struct ColdScanIndexSample {
    /// Total cold scan+index wall time (SCAN + INDEX phases).
    pub total: Duration,
    /// SCAN phase: `discover_perl_files` duration (as the seam itself reports it).
    pub discovery: Duration,
    /// INDEX phase: read bytes -> admit -> decode -> `index_file`, all files.
    pub read_admit_index: Duration,
    /// Files discovery returned (already in discovery's lexical order).
    pub files_discovered: usize,
    /// Files present in the index afterwards (`WorkspaceIndex::file_count`).
    pub files_in_index: usize,
    /// Admitted files whose `index_file` returned `Ok`.
    pub files_indexed: usize,
    /// Admitted files whose `index_file` returned `Err` (never silently dropped).
    pub index_errors: usize,
    /// Symbols in the index afterwards (`WorkspaceIndex::symbol_count`).
    pub symbols: usize,
    /// How discovery enumerated the tree: `"git"` or `"walk"`.
    pub method: &'static str,
}

impl ColdScanIndexSample {
    /// Admitted-file count for this sample: indexed Ok plus admitted-but-erroring.
    #[must_use]
    pub fn files_admitted(&self) -> usize {
        self.files_indexed + self.index_errors
    }

    /// Indexed predicate (matrix row 5): every admitted file's `index_file`
    /// returned Ok, `index.file_count() == files_indexed`, and
    /// `index.symbol_count() > 0`.
    #[must_use]
    pub fn indexed_predicate_holds(&self) -> bool {
        self.index_errors == 0 && self.files_in_index == self.files_indexed && self.symbols > 0
    }
}

/// Take one COLD scan+index sample over `root`.
///
/// Mirrors the production startup seam in order: a fresh [`WorkspaceIndex`]
/// is created first (untimed setup), then discovery runs, then the per-file
/// read -> admit -> decode -> `index_file` loop consumes discovery's lexical
/// order. Result-returning by contract so the module lints clean under both
/// the bench and test lint sets.
///
/// # Errors
/// Returns a message when reading a discovered file or deriving its file URL
/// fails.
pub fn cold_scan_index(root: &Path) -> Result<ColdScanIndexSample, String> {
    let index = WorkspaceIndex::new();
    let start = Instant::now();

    // SCAN (production seam: discover_perl_files with default config, no
    // include paths).
    let discovered = discover_perl_files(root);
    let discovery = discovered.duration;
    let files_discovered = discovered.files.len();
    let method = discovery_method_label(discovered.method);

    // INDEX (read bytes -> admission -> decode -> index_file, in discovery's
    // lexical order).
    let mut files_indexed = 0;
    let mut index_errors = 0;
    let index_start = Instant::now();
    for path in &discovered.files {
        let bytes =
            std::fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
        if !DiscoveryConfig::default().admits_bytes(path, &bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let url = Url::from_file_path(path)
            .map_err(|()| format!("discovered path is not absolute: {}", path.display()))?;
        if index.index_file(url, text).is_ok() {
            files_indexed += 1;
        } else {
            index_errors += 1;
        }
    }
    let read_admit_index = index_start.elapsed();
    let total = start.elapsed();

    Ok(ColdScanIndexSample {
        total,
        discovery,
        read_admit_index,
        files_discovered,
        files_in_index: index.file_count(),
        files_indexed,
        index_errors,
        symbols: index.symbol_count(),
        method,
    })
}

/// Stable receipt label for [`DiscoveryMethod`].
#[must_use]
pub fn discovery_method_label(method: DiscoveryMethod) -> &'static str {
    match method {
        DiscoveryMethod::Git => "git",
        DiscoveryMethod::Walk => "walk",
    }
}

/// Nearest-rank percentile over an already-**sorted** sample slice, using the
/// `perf_scorecard.rs:62-66` index arithmetic: `ceil(percent * n)` converted
/// to a 0-based index. With `COLD_SAMPLES == 5`, p95 is the max sample; the
/// receipt records `samples: 5` so readers know.
#[must_use]
pub fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    let n = sorted.len();
    if n == 0 {
        return Duration::ZERO;
    }
    let idx = (percent * n).div_ceil(100).saturating_sub(1).min(n - 1);
    sorted[idx]
}

/// Median of an already-**sorted** sample slice (`perf_scorecard.rs`
/// arithmetic: index `n / 2`, upper-of-two-middle for even n).
#[must_use]
pub fn median(sorted: &[Duration]) -> Duration {
    let n = sorted.len();
    if n == 0 {
        return Duration::ZERO;
    }
    sorted[n / 2]
}

/// Generate a realistic Perl module with ~10 symbols for scale benchmarks.
///
/// Moved here from `workspace_index_benchmark.rs` so the real-corpus slice
/// and the existing 1000/10k/5k groups share one generator.
#[must_use]
pub fn generate_module(index: usize) -> String {
    format!(
        r#"package Gen::Module{idx};
use strict;
use warnings;

our $VERSION = '1.00';

sub new {{
    my $class = shift;
    return bless {{}}, $class;
}}

sub method_a_{idx} {{
    my ($self, $x) = @_;
    return $x + {idx};
}}

sub method_b_{idx} {{
    my ($self, $y) = @_;
    return $y * {idx};
}}

sub method_c_{idx} {{
    my ($self) = @_;
    return "{idx}";
}}

sub _private_{idx} {{
    return {idx};
}}

1;
"#,
        idx = index
    )
}
