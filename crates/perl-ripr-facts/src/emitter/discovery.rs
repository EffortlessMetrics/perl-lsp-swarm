//! Filesystem-walk helpers shared by every fact emitter.
//!
//! `collect_t_files`/`collect_pm_files`/`collect_perl_files` are pure
//! directory walks (no parsing) reused by [`super::test_facts`],
//! [`super::relations`], [`super::boundaries`], and [`super::owners`] — kept
//! in one place so the `relative_path` each returns stays byte-identical to
//! the path `super::owners::emit_files_and_owners` derives for the same file
//! (#3342, #3361 both fixed a divergence here).

/// Collect all `.t` files under `<root>/t`. Returns (absolute_path,
/// relative_path, content), where `relative_path` is `strip_prefix(root)` —
/// byte-identical to the path `emit_files_and_owners` derives for the same
/// file. #3361: the previous `split_once("/t/")` heuristic diverged from that
/// path whenever `root` had an ancestor segment named `t` (e.g. `t/lib/Proj`,
/// `some/t/proj`), dangling `test.file_id` / `boundary.file_id` against
/// `files[]`; both now strip the same `root` (as #3342 fixed for `.pm` files).
pub(crate) fn collect_t_files(root: &std::path::Path) -> Vec<(String, String, String)> {
    let mut result = Vec::new();
    collect_t_files_recursive(&root.join("t"), root, &mut result);
    result
}

fn collect_t_files_recursive(
    dir: &std::path::Path,
    root: &std::path::Path,
    result: &mut Vec<(String, String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_symlink = entry.file_type().is_ok_and(|file_type| file_type.is_symlink());
        if path.is_dir() {
            // Do not descend into symlinked directories — a directory-symlink
            // loop would otherwise recurse infinitely (`is_dir()` follows the
            // link). A symlinked `.t` *file* (below) is still read.
            if is_symlink {
                continue;
            }
            collect_t_files_recursive(&path, root, result);
        } else if path.extension().is_some_and(|ext| ext == "t") {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            result.push((path.to_string_lossy().to_string(), relative, content));
        }
    }
}

/// Collect all `.pm` files under `<root>/lib`. Returns (relative_path, content),
/// where `relative_path` is `strip_prefix(root)` — byte-identical to the path
/// `emit_files_and_owners` derives for the same file. #3342: the previous
/// `split_once("/lib/")` heuristic diverged from that path whenever `root` had
/// an ancestor segment named `lib` (e.g. `vendor/lib/proj`, `t/lib/...`),
/// re-dangling a relation's resolved `owner_id`; both now strip the same `root`.
pub(crate) fn collect_pm_files(root: &std::path::Path) -> Vec<(String, String)> {
    let mut result = Vec::new();
    collect_pm_files_recursive(&root.join("lib"), root, &mut result);
    result
}

fn collect_pm_files_recursive(
    dir: &std::path::Path,
    root: &std::path::Path,
    result: &mut Vec<(String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_pm_files_recursive(&path, root, result);
        } else if path.extension().is_some_and(|ext| ext == "pm") {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            result.push((relative, content));
        }
    }
}

/// Recursively collect the repo-relative forward-slash paths of Perl
/// source/test files under `root`, in deterministic (sorted) order.
///
/// Skips hidden directories and common build trees so a workspace scan stays
/// bounded. Content is read per-file by the caller (not here) so that read
/// failures can be reported as limitations rather than silently dropped.
pub(crate) fn collect_perl_files(root: &str) -> Vec<String> {
    let root_path = std::path::Path::new(root);
    let mut result = Vec::new();
    collect_perl_files_recursive(root_path, root_path, &mut result);
    result.sort();
    result
}

fn collect_perl_files_recursive(
    dir: &std::path::Path,
    root: &std::path::Path,
    result: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let is_symlink = entry.file_type().is_ok_and(|file_type| file_type.is_symlink());
        if path.is_dir() {
            // Do not descend into symlinked directories — a directory-symlink
            // loop would otherwise recurse infinitely (`is_dir()` follows the
            // link). Also skip hidden dirs and non-source build trees. A
            // symlinked source *file* (below) is still read.
            if is_symlink
                || name.starts_with('.')
                || matches!(name.as_ref(), "target" | "blib" | "node_modules" | "_build")
            {
                continue;
            }
            collect_perl_files_recursive(&path, root, result);
        } else if is_perl_source_file(&name) {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            result.push(relative);
        }
    }
}

/// Whether a file name is a Perl source or test file the emitter parses.
fn is_perl_source_file(name: &str) -> bool {
    name.ends_with(".pm")
        || name.ends_with(".pl")
        || name.ends_with(".psgi")
        || name.ends_with(".t")
}

/// Determine file role from path extension.
pub(crate) fn file_role_from_path(path: &str) -> &'static str {
    if path.ends_with(".t") {
        "test"
    } else if path.ends_with(".pm") || path.ends_with(".pl") || path.ends_with(".psgi") {
        "source"
    } else if path.ends_with("Makefile.PL")
        || path.ends_with("Build.PL")
        || path.ends_with("cpanfile")
    {
        "config"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emitter::test_facts::emit_tests_and_oracles;

    #[cfg(unix)]
    #[test]
    fn collect_t_files_reads_symlinked_t_file() {
        use std::os::unix::fs::symlink;
        // A symlinked `.t` file under `t/` must still be discovered + read — only
        // symlinked *directories* are skipped (loop safety).
        let root = std::env::temp_dir().join("perl-p4-symlink-t");
        let _ = std::fs::remove_dir_all(&root);
        let shared = root.join("shared");
        let t_dir = root.join("t");
        std::fs::create_dir_all(&shared).expect("mkdir shared");
        std::fs::create_dir_all(&t_dir).expect("mkdir t");
        std::fs::write(shared.join("real.t"), "use Test::More;\nok(1);\n").expect("write real");
        symlink(shared.join("real.t"), t_dir.join("linked.t")).expect("symlink .t");

        let (tests, _oracles, _provenance, _limitations) =
            emit_tests_and_oracles(root.to_str().expect("utf8 root"));
        assert!(
            tests.iter().any(|t| t["name"] == "t/linked.t"),
            "symlinked .t file must be read, not silently dropped"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn file_role_from_path_classifies_correctly() {
        assert_eq!(file_role_from_path("lib/My/App.pm"), "source");
        assert_eq!(file_role_from_path("script/run.pl"), "source");
        assert_eq!(file_role_from_path("t/app.t"), "test");
        assert_eq!(file_role_from_path("app.psgi"), "source");
        assert_eq!(file_role_from_path("Makefile.PL"), "config");
        assert_eq!(file_role_from_path("cpanfile"), "config");
        assert_eq!(file_role_from_path("README.md"), "unknown");
    }
}
