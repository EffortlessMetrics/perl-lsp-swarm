use perl_parser_core::percentile::nearest_rank_percentile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Tracked scorecard consumed by `xtask update-status` parser rendering.
/// Ordinary bench/test runs must not write here; publication is opt-in.
pub(crate) const TRACKED_ARTIFACT_RELATIVE_PATH: &str =
    "docs/project/status/token_performance_scorecard.json";
const LOCAL_ARTIFACT_FILE_NAME: &str = "token_performance_scorecard.json";

/// Set to `1` to write the governed tracked scorecard under `docs/`.
/// Unset/any other value writes under `CARGO_TARGET_DIR` or `<repo>/target/`.
pub(crate) const PUBLISH_ENV: &str = "PERL_LSP_PUBLISH_TOKEN_SCORECARD";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ScoreMetric {
    pub(crate) iterations: usize,
    pub(crate) median_ns: u64,
    pub(crate) p95_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenPerformanceScorecard {
    schema_version: u32,
    generated_at_epoch_s: u64,
    metrics: BTreeMap<String, ScoreMetric>,
}

impl Default for TokenPerformanceScorecard {
    fn default() -> Self {
        Self { schema_version: 1, generated_at_epoch_s: 0, metrics: BTreeMap::new() }
    }
}

pub(crate) fn record_metric(name: &str, metric: ScoreMetric) {
    let Some(path) = find_artifact_path() else {
        return;
    };
    write_metric(&path, name, metric);
}

pub(crate) fn write_metric(path: &Path, name: &str, metric: ScoreMetric) {
    let mut scorecard = read_scorecard(path).unwrap_or_default();
    scorecard.generated_at_epoch_s = now_epoch_seconds();
    scorecard.metrics.insert(name.to_string(), metric);

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let Ok(json) = serde_json::to_string_pretty(&scorecard) else {
        return;
    };
    let _ = fs::write(path, json);
}

pub(crate) fn sample_metric<F>(iterations: usize, mut run: F) -> ScoreMetric
where
    F: FnMut(),
{
    let rounds = iterations.max(5);

    // Discard warmup rounds so allocator and cache stabilization does not
    // pollute scored samples.
    let warmup = 2usize.min(rounds);
    for _ in 0..warmup {
        run();
    }

    let mut samples: Vec<u64> = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let start = Instant::now();
        run();
        let elapsed_ns = start.elapsed().as_nanos();
        let sample = u64::try_from(elapsed_ns).map_or(u64::MAX, |value| value);
        samples.push(sample);
    }

    samples.sort_unstable();
    let n = samples.len();
    let median_ns = samples.get(n / 2).copied().unwrap_or_default();
    let p95_ns = nearest_rank_percentile(&samples, 95);

    ScoreMetric { iterations: rounds, median_ns, p95_ns }
}

pub(crate) fn publish_requested_from(value: Option<&str>) -> bool {
    value.is_some_and(|value| value == "1")
}

pub(crate) fn resolve_artifact_path(
    cwd: &Path,
    cargo_target_dir: Option<&Path>,
    publish: bool,
) -> Option<PathBuf> {
    if publish {
        return Some(find_repo_root(cwd)?.join(TRACKED_ARTIFACT_RELATIVE_PATH));
    }
    if let Some(target_dir) = cargo_target_dir.filter(|path| !path.as_os_str().is_empty()) {
        let candidate = target_dir.join(LOCAL_ARTIFACT_FILE_NAME);
        // Ordinary runs must not write the tracked scorecard, including `..`
        // spellings and directory aliases of `docs/project/status`.
        if !would_write_tracked_docs(cwd, &candidate) {
            return Some(candidate);
        }
    }
    Some(find_repo_root(cwd)?.join("target").join(LOCAL_ARTIFACT_FILE_NAME))
}

pub(crate) fn is_tracked_docs_artifact(path: &Path) -> bool {
    normalize_lexically(path).ends_with(Path::new(TRACKED_ARTIFACT_RELATIVE_PATH))
}

fn would_write_tracked_docs(cwd: &Path, candidate: &Path) -> bool {
    let resolved =
        if candidate.is_absolute() { candidate.to_path_buf() } else { cwd.join(candidate) };
    let lexical = normalize_lexically(&resolved);
    if is_tracked_docs_artifact(&lexical) {
        return true;
    }
    let Some(repo_root) = find_repo_root(cwd) else {
        return false;
    };
    // Canonicalize the unresolved path so `symlink/..` follows the link
    // before `..`, matching filesystem write behavior.
    same_existing_file_or_parent(&resolved, &repo_root.join(TRACKED_ARTIFACT_RELATIVE_PATH))
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    let _ = out.pop();
                }
                Some(Component::Prefix(_) | Component::RootDir) => {}
                Some(Component::ParentDir | Component::CurDir) | None => out.push(component),
            },
            other => out.push(other),
        }
    }
    out
}

fn same_existing_file_or_parent(left: &Path, right: &Path) -> bool {
    if let (Ok(canonical_left), Ok(canonical_right)) =
        (fs::canonicalize(left), fs::canonicalize(right))
    {
        return canonical_left == canonical_right;
    }
    let Some((left_parent, left_name)) = left.parent().zip(left.file_name()) else {
        return false;
    };
    let Some((right_parent, right_name)) = right.parent().zip(right.file_name()) else {
        return false;
    };
    if left_name != right_name {
        return false;
    }
    match (fs::canonicalize(left_parent), fs::canonicalize(right_parent)) {
        (Ok(canonical_left), Ok(canonical_right)) => canonical_left == canonical_right,
        _ => false,
    }
}

fn find_artifact_path() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let target_dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    resolve_artifact_path(
        &cwd,
        target_dir.as_deref(),
        publish_requested_from(std::env::var(PUBLISH_ENV).ok().as_deref()),
    )
}

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("docs/project/status").is_dir() && dir.join("crates/perl-token").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn read_scorecard(path: &Path) -> Option<TokenPerformanceScorecard> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
