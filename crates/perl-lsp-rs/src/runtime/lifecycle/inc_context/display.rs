use perl_lsp_rs_core::providers::missing_module::ModuleSearchPathDisplay;
use perl_module::IncRoot;

pub(super) fn search_display_paths(
    roots: &[IncRoot],
    workspace_root: &std::path::Path,
) -> Vec<ModuleSearchPathDisplay> {
    roots
        .iter()
        .map(|root| {
            let label = root_source_label(&root.source).to_string();
            // The resolver deliberately drops absolute roots outside the
            // workspace, and a missing directory cannot be searched. Label
            // both honestly instead of claiming they were searched (#17362).
            let absolute = if root.path.is_absolute() {
                root.path.clone()
            } else {
                workspace_root.join(&root.path)
            };
            let label = if !absolute.starts_with(workspace_root) {
                format!("{label}, outside workspace: not searched")
            } else if !absolute.exists() {
                format!("{label}, does not exist: not searched")
            } else {
                label
            };
            ModuleSearchPathDisplay::new(root.path.to_string_lossy().into_owned(), label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn root(path: &str, source: &str) -> IncRoot {
        IncRoot {
            path: PathBuf::from(path),
            source: source.to_string(),
            kind: perl_module::IncRootKind::Perl5LibEnv,
            precedence: 0,
        }
    }

    #[test]
    fn outside_and_missing_roots_are_labeled_not_searched() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("ws");
        let inside = workspace.join("lib5");
        let outside = temp.path().join("poslib");
        std::fs::create_dir_all(&inside)?;
        std::fs::create_dir_all(&outside)?;
        let missing = workspace.join("nope");

        let displays = search_display_paths(
            &[
                root(inside.to_str().ok_or("inside utf8")?, "perl5lib-env"),
                root(outside.to_str().ok_or("outside utf8")?, "perl5lib-env"),
                root(missing.to_str().ok_or("missing utf8")?, "perl5lib-env"),
            ],
            &workspace,
        );

        assert_eq!(displays[0].source, "PERL5LIB", "searched root keeps plain label");
        assert!(
            displays[1].source.contains("outside workspace")
                && displays[1].source.contains("not searched"),
            "outside root must be labeled not searched, got: {}",
            displays[1].source
        );
        assert!(
            displays[2].source.contains("does not exist")
                && displays[2].source.contains("not searched"),
            "missing root must be labeled not searched, got: {}",
            displays[2].source
        );
        Ok(())
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
