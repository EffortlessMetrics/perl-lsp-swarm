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
            // Label roots the resolver cannot search (#17362). The URI
            // resolver probes external kinds (PERL5LIB, interpreter startup,
            // absolute) directly wherever they live; workspace kinds are
            // boundary-validated and dropped outside the workspace. Relative
            // PERL5LIB roots resolve against the process directory, matching
            // the resolver; other relative roots resolve against the
            // workspace. A missing directory cannot be searched either way.
            use perl_module::IncRootKind;
            let external = matches!(
                root.kind,
                IncRootKind::ExternalAbsolute
                    | IncRootKind::Perl5LibEnv
                    | IncRootKind::InterpreterStartup
                    | IncRootKind::RuntimeDerived
            );
            let absolute = if root.path.is_absolute() {
                root.path.clone()
            } else if matches!(root.kind, IncRootKind::Perl5LibEnv) {
                std::env::current_dir()
                    .map(|cwd| cwd.join(&root.path))
                    .unwrap_or_else(|_| root.path.clone())
            } else {
                workspace_root.join(&root.path)
            };
            let label = if !external && !absolute.starts_with(workspace_root) {
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

        // External PERL5LIB roots are probed directly by the URI resolver,
        // wherever they live: both keep the plain searched label.
        assert_eq!(displays[0].source, "PERL5LIB", "inside root keeps plain label");
        assert_eq!(displays[1].source, "PERL5LIB", "external root is searched directly");
        assert!(
            displays[2].source.contains("does not exist")
                && displays[2].source.contains("not searched"),
            "missing root must be labeled not searched, got: {}",
            displays[2].source
        );

        // A workspace-relative root outside the workspace is dropped by
        // boundary validation and must be labeled not searched.
        let workspace_kind = IncRoot {
            path: outside.clone(),
            source: "workspace-include-paths".to_string(),
            kind: perl_module::IncRootKind::WorkspaceRelative,
            precedence: 0,
        };
        let displays = search_display_paths(&[workspace_kind], &workspace);
        assert!(
            displays[0].source.contains("outside workspace")
                && displays[0].source.contains("not searched"),
            "outside workspace root must be labeled not searched, got: {}",
            displays[0].source
        );
        Ok(())
    }

    /// Call-observation proof for the `root_source_label` seam: every @INC
    /// source kind maps to its client-facing label and unknown sources fall
    /// back without panicking. The helper moved into production scope with
    /// the workspace-rename benchmark work (#18031).
    #[test]
    fn root_source_label_maps_every_inc_source_kind() {
        assert_eq!(root_source_label("use-lib-lexical"), "use lib");
        assert_eq!(root_source_label("workspace-include-paths"), "workspace includePaths");
        assert_eq!(root_source_label("perl5lib-env"), "PERL5LIB");
        assert_eq!(root_source_label("interpreter-startup-inc"), "interpreter startup @INC");
        assert_eq!(root_source_label("some-future-source"), "unknown @INC source");
        assert_eq!(root_source_label(""), "unknown @INC source");
    }
}
