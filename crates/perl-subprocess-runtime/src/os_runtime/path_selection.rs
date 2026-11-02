use std::path::{Path, PathBuf};

/// Priority score for a resolved candidate path: `.exe` beats `.com` beats
/// `.cmd` beats `.bat` beats any other extension beats no extension.
///
/// This function is cross-platform so that `select_path_candidate` can be
/// exercised on Linux CI without requiring a Windows runner.  The extension
/// values are Windows-specific, but the comparison logic is pure Rust.
///
/// The non-Windows availability probe also uses this ordering through the selector.
pub(crate) fn candidate_priority<T: AsRef<Path>>(candidate: T) -> u8 {
    match candidate
        .as_ref()
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
    {
        Some(ext) if ext == "exe" => 5,
        Some(ext) if ext == "com" => 4,
        Some(ext) if ext == "cmd" => 3,
        Some(ext) if ext == "bat" => 2,
        Some(_) => 1,
        None => 0,
    }
}

/// Pure selection function: given a list of fully-resolved candidate paths and
/// the current working directory, return the best candidate that is **not**
/// located under `cwd`, applying `candidate_priority` to break ties.
///
/// This function is extracted for unit-testability: callers inject arbitrary
/// candidate lists and CWD values so the security invariant can be verified
/// without touching the real file system or environment.  Being cross-platform
/// also lets the ripr quality gate observe call paths on Linux CI runners.
///
/// ## Security invariant (two layers of defense)
///
/// 1. **Absolute-only**: any candidate that is not an absolute path is dropped
///    immediately.  A relative candidate (e.g. produced by an empty `;;` PATH
///    entry via `dir.join`) resolves against the CWD and is indistinguishable
///    from a planted binary.  This is the primary guard against the
///    empty-PATH-entry bypass.
/// 2. **CWD exclusion**: candidates whose **canonical file path** lies inside the
///    canonical CWD subtree (including the CWD directory itself) are dropped.
///    This covers immediate-parent checks, nested directories such as
///    `<cwd>/bin/tool.exe`, and symlink/junction targets that resolve into the
///    CWD even when the literal path sits outside it.  If `canonicalize` fails for
///    the candidate file, the candidate is dropped (fail closed).
///
/// Returns `None` when every candidate is filtered out or when `candidates` is
/// empty (tool genuinely not on PATH; better than running a planted binary).
///
/// Native paths are retained so non-UTF-8 PATH directories remain usable on Unix.
/// Returns `true` when `path` is exactly `cwd_canon` or a descendant of it.
fn canonical_path_is_under_cwd(path: &Path, cwd_canon: &Path) -> bool {
    path == cwd_canon || path.starts_with(cwd_canon)
}

/// Returns `true` when the candidate must be rejected because its canonical target
/// lies inside the CWD subtree, or when canonicalization fails (fail closed).
fn candidate_resolves_inside_cwd(candidate: &Path, cwd_canon: &Path) -> bool {
    match std::fs::canonicalize(candidate) {
        Ok(candidate_canon) => canonical_path_is_under_cwd(&candidate_canon, cwd_canon),
        Err(_) => true,
    }
}

pub(crate) fn select_path_candidate<T: AsRef<Path>>(
    candidates: &[T],
    cwd: &Path,
) -> Option<PathBuf> {
    let cwd_canon = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());

    candidates
        .iter()
        .filter(|c| {
            let c = (*c).as_ref();
            // Layer 1: reject any candidate that is not absolute.  A relative
            // path (e.g. produced by an empty `;;` PATH entry via `dir.join`)
            // would silently resolve against the CWD and is indistinguishable
            // from a planted binary.  This is the primary fix for the
            // empty-PATH-entry bypass.
            if !c.is_absolute() {
                return false;
            }
            !candidate_resolves_inside_cwd(c, &cwd_canon)
        })
        .max_by_key(|c| candidate_priority((*c).as_ref()))
        .map(|c| (*c).as_ref().to_path_buf())
}
