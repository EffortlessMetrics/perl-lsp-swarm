//! Marker-based Perl dependency-manager include-path detection.

use std::path::Path;

const CARTON_INCLUDE_PATH: &str = "local/lib/perl5";
const CARMEL_INCLUDE_PATH: &str = "vendor/lib/perl5";

/// Detect include paths produced by Carton and Carmel in a workspace root.
///
/// Carton is identified by `cpanfile` plus either `carton.lock` or
/// `cpanfile.snapshot`. Carmel is identified by `cpanfile` plus an existing
/// `vendor/lib/perl5` directory. The returned paths are workspace-relative
/// and ordered with Carton before Carmel when both markers are present.
///
/// These are runtime-derived roots (#4998): their safety class is "bounded,
/// hard-coded, workspace-relative literals" and must stay that way. They are
/// appended to the effective include-path projection only after
/// resource-scope validation conventions; a future metadata-derived absolute
/// root must enter through an explicitly classified source instead
/// (configuration observation train, #10817).
#[must_use]
pub fn detect_dependency_include_paths(workspace_root: &Path) -> Vec<String> {
    let has_cpanfile = workspace_root.join("cpanfile").is_file();
    if !has_cpanfile {
        return Vec::new();
    }

    let mut paths = Vec::new();
    let has_carton_lock = workspace_root.join("carton.lock").is_file();
    let has_cpanfile_snapshot = workspace_root.join("cpanfile.snapshot").is_file();
    if has_carton_lock || has_cpanfile_snapshot {
        paths.push(CARTON_INCLUDE_PATH.to_string());
    }

    if workspace_root.join(CARMEL_INCLUDE_PATH).is_dir() {
        paths.push(CARMEL_INCLUDE_PATH.to_string());
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::detect_dependency_include_paths;

    #[test]
    fn detects_carton_from_lockfile() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;

        assert_eq!(detect_dependency_include_paths(workspace.path()), vec!["local/lib/perl5"]);
        Ok(())
    }

    #[test]
    fn detects_carton_from_cpanfile_snapshot() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("cpanfile.snapshot"), "snapshot\n")?;

        assert_eq!(detect_dependency_include_paths(workspace.path()), vec!["local/lib/perl5"]);
        Ok(())
    }

    #[test]
    fn detects_carmel_from_vendor_root() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::create_dir_all(workspace.path().join("vendor/lib/perl5"))?;

        assert_eq!(detect_dependency_include_paths(workspace.path()), vec!["vendor/lib/perl5"]);
        Ok(())
    }

    #[test]
    fn detects_both_managers_in_stable_order() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;
        std::fs::create_dir_all(workspace.path().join("vendor/lib/perl5"))?;

        assert_eq!(
            detect_dependency_include_paths(workspace.path()),
            vec!["local/lib/perl5", "vendor/lib/perl5"]
        );
        Ok(())
    }

    #[test]
    fn requires_expected_markers() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;
        assert!(detect_dependency_include_paths(workspace.path()).is_empty());

        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        assert_eq!(detect_dependency_include_paths(workspace.path()), vec!["local/lib/perl5"]);

        std::fs::remove_file(workspace.path().join("carton.lock"))?;
        assert!(detect_dependency_include_paths(workspace.path()).is_empty());

        std::fs::create_dir_all(workspace.path().join("vendor/lib/perl5"))?;
        assert_eq!(detect_dependency_include_paths(workspace.path()), vec!["vendor/lib/perl5"]);
        Ok(())
    }

    /// Recurrence gate (#4998): runtime-derived dependency roots must stay
    /// relative workspace-contained literals. If a future change makes this
    /// detector return absolute or traversal-capable paths from project
    /// metadata, they would inherit resource-scope validation by convention;
    /// that writer must instead be explicitly classified first.
    #[test]
    fn detected_roots_stay_relative_and_workspace_contained()
    -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        std::fs::write(workspace.path().join("cpanfile"), "requires 'JSON';\n")?;
        std::fs::write(workspace.path().join("carton.lock"), "snapshot\n")?;
        std::fs::create_dir_all(workspace.path().join("vendor/lib/perl5"))?;

        for path in detect_dependency_include_paths(workspace.path()) {
            let candidate = std::path::Path::new(&path);
            assert!(!candidate.is_absolute(), "runtime-derived root must not be absolute: {path}");
            assert!(
                !candidate.components().any(|c| c == std::path::Component::ParentDir),
                "runtime-derived root must not traverse: {path}"
            );
            assert!(
                workspace.path().join(&path).starts_with(workspace.path()),
                "runtime-derived root must stay inside the workspace: {path}"
            );
        }
        Ok(())
    }
}
