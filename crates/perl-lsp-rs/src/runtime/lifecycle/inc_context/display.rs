use perl_lsp_rs_core::providers::missing_module::ModuleSearchPathDisplay;
use perl_module::{IncRoot, IncRootKind};
use perl_parser_core::path_security::validate_workspace_path;

pub(super) fn search_display_paths(
    roots: &[IncRoot],
    workspace_root: &std::path::Path,
) -> Vec<ModuleSearchPathDisplay> {
    roots
        .iter()
        .map(|root| {
            let base = root_source_label(&root.source).to_string();
            let label = annotate_root_label(&base, root, workspace_root);
            ModuleSearchPathDisplay::new(root.path.to_string_lossy().into_owned(), label)
        })
        .collect()
}

fn inc_root_is_probed_directly(kind: IncRootKind) -> bool {
    matches!(
        kind,
        IncRootKind::ExternalAbsolute
            | IncRootKind::Perl5LibEnv
            | IncRootKind::InterpreterStartup
            | IncRootKind::RuntimeDerived
    )
}

fn root_directory_absolute(root: &IncRoot, workspace_root: &std::path::Path) -> std::path::PathBuf {
    if root.path.is_absolute() {
        root.path.clone()
    } else if matches!(root.kind, IncRootKind::Perl5LibEnv) {
        std::env::current_dir()
            .map(|cwd| cwd.join(&root.path))
            .unwrap_or_else(|_| root.path.clone())
    } else {
        workspace_root.join(&root.path)
    }
}

fn workspace_bound_root_directory(
    root: &IncRoot,
    workspace_root: &std::path::Path,
) -> std::path::PathBuf {
    if root.path == std::path::Path::new(".") {
        workspace_root.to_path_buf()
    } else if root.path.is_absolute() {
        root.path.clone()
    } else {
        workspace_root.join(&root.path)
    }
}

fn annotate_root_label(base: &str, root: &IncRoot, workspace_root: &std::path::Path) -> String {
    let absolute = root_directory_absolute(root, workspace_root);

    if inc_root_is_probed_directly(root.kind) {
        return if absolute.exists() {
            base.to_string()
        } else {
            format!("{base}, does not exist")
        };
    }

    // Mirror `full_path_for_root` in `perl_module::resolution::uri`: absolute
    // workspace/lexical roots are joined directly and still participate in search.
    if root.path.is_absolute() {
        return if absolute.exists() {
            base.to_string()
        } else {
            format!("{base}, does not exist")
        };
    }

    let root_dir = workspace_bound_root_directory(root, workspace_root);
    match validate_workspace_path(&root_dir, workspace_root) {
        Ok(_) => {
            if root_dir.exists() {
                base.to_string()
            } else {
                format!("{base}, does not exist")
            }
        }
        Err(_) => format!("{base}, outside workspace: not searched"),
    }
}

fn root_source_label(source: &str) -> &'static str {
    match source {
        "use-lib-lexical" => "use lib",
        "workspace-include-paths" => "workspace includePaths",
        "perl5lib-env" => "PERL5LIB",
        "interpreter-startup-inc" => "interpreter startup @INC",
        _ => "unknown @INC source",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn perl5lib_root(path: &str) -> IncRoot {
        IncRoot {
            path: PathBuf::from(path),
            source: "perl5lib-env".to_string(),
            kind: IncRootKind::Perl5LibEnv,
            precedence: 0,
        }
    }

    fn workspace_root(path: PathBuf) -> IncRoot {
        IncRoot {
            path,
            source: "workspace-include-paths".to_string(),
            kind: IncRootKind::WorkspaceRelative,
            precedence: 0,
        }
    }

    #[test]
    fn external_perl5lib_roots_stay_searched_even_when_missing() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("ws");
        let inside = workspace.join("lib5");
        let outside = temp.path().join("poslib");
        std::fs::create_dir_all(&inside)?;
        std::fs::create_dir_all(&outside)?;
        let missing = workspace.join("nope");

        let displays = search_display_paths(
            &[
                perl5lib_root(inside.to_str().ok_or("inside utf8")?),
                perl5lib_root(outside.to_str().ok_or("outside utf8")?),
                perl5lib_root(missing.to_str().ok_or("missing utf8")?),
            ],
            &workspace,
        );

        assert_eq!(displays[0].source, "PERL5LIB");
        assert_eq!(displays[1].source, "PERL5LIB");
        assert!(
            displays[2].source.contains("does not exist")
                && !displays[2].source.contains("not searched"),
            "missing external root is probed, not skipped: {}",
            displays[2].source
        );
        Ok(())
    }

    #[test]
    fn workspace_relative_missing_root_is_probed_not_skipped() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("ws");
        std::fs::create_dir_all(&workspace)?;

        let displays =
            search_display_paths(&[workspace_root(PathBuf::from("vendor/lib/perl5"))], &workspace);
        assert!(
            displays[0].source.contains("does not exist")
                && !displays[0].source.contains("not searched"),
            "missing but policy-admitted root must stay searched: {}",
            displays[0].source
        );
        assert_eq!(displays[0].path, "vendor/lib/perl5");
        Ok(())
    }

    #[test]
    fn workspace_traversal_root_is_policy_rejected() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("ws");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(outside.join("lib"))?;

        let displays =
            search_display_paths(&[workspace_root(PathBuf::from("../outside/lib"))], &workspace);
        assert!(
            displays[0].source.contains("outside workspace")
                && displays[0].source.contains("not searched"),
            "traversal root must match validate_workspace_path rejection: {}",
            displays[0].source
        );
        Ok(())
    }

    #[test]
    fn absolute_workspace_root_outside_stays_searched() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("ws");
        let outside = temp.path().join("abslib");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(&outside)?;

        let displays = search_display_paths(&[workspace_root(outside.clone())], &workspace);
        assert!(
            !displays[0].source.contains("not searched"),
            "absolute workspace roots follow resolver direct-join semantics: {}",
            displays[0].source
        );
        assert_eq!(displays[0].source, "workspace includePaths");
        Ok(())
    }

    #[test]
    fn perl5lib_relative_root_uses_process_directory() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("ws");
        std::fs::create_dir_all(&workspace)?;
        let from_cwd = temp.path().join("from_cwd");
        std::fs::create_dir_all(&from_cwd)?;

        let previous = std::env::current_dir().ok();
        std::env::set_current_dir(temp.path())?;
        let result = (|| -> TestResult {
            let displays = search_display_paths(&[perl5lib_root("from_cwd")], &workspace);
            assert_eq!(displays[0].source, "PERL5LIB");
            Ok(())
        })();
        if let Some(dir) = previous {
            let _ = std::env::set_current_dir(dir);
        }
        result
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_root_is_not_searched() -> TestResult {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("ws");
        let outside = temp.path().join("outside");
        let escape = workspace.join("escape");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(&outside)?;
        symlink(&outside, &escape)?;

        let displays = search_display_paths(&[workspace_root(PathBuf::from("escape"))], &workspace);
        assert!(
            displays[0].source.contains("outside workspace")
                && displays[0].source.contains("not searched"),
            "symlink escape must be rejected like the resolver: {}",
            displays[0].source
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn contained_symlink_root_stays_searched() -> TestResult {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("ws");
        let lib = workspace.join("lib");
        let link = workspace.join("linked-lib");
        std::fs::create_dir_all(&lib)?;
        symlink(&lib, &link)?;

        let displays =
            search_display_paths(&[workspace_root(PathBuf::from("linked-lib"))], &workspace);
        assert_eq!(
            displays[0].source, "workspace includePaths",
            "contained symlink root remains searchable: {}",
            displays[0].source
        );
        Ok(())
    }
}
