#!/usr/bin/env python3
from pathlib import Path
from textwrap import dedent, indent

SOURCE = Path("crates/perl-lsp-rs/src/runtime/lifecycle/module_resolution.rs")
text = SOURCE.read_text(encoding="utf-8")


def block(raw: str, spaces: int = 0) -> str:
    value = dedent(raw)
    if value.startswith("\n"):
        value = value[1:]
    if not value.endswith("\n"):
        value += "\n"
    return indent(value, " " * spaces)


def replace_once(label: str, old: str, new: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    text = text.replace(old, new, 1)


def replace_between(label: str, start_marker: str, end_marker: str, replacement: str) -> None:
    global text
    start_count = text.count(start_marker)
    end_count = text.count(end_marker)
    if start_count != 1 or end_count < 1:
        raise SystemExit(
            f"{label}: expected one start and at least one end marker, "
            f"found start={start_count}, end={end_count}"
        )
    start = text.index(start_marker)
    end = text.index(end_marker, start)
    text = text[:start] + replacement + text[end:]


replace_once(
    "module-resolution imports",
    block(
        '''
        use perl_module::{
            ModuleUriResolution, build_effective_inc_roots,
            resolve_module_path as resolve_workspace_module_path, resolve_module_uri_with_effective_inc,
        };
        use perl_module::{UseLibPath, resolve_use_lib_paths_from_source};
        ''',
    ),
    block(
        '''
        #[cfg(test)]
        use perl_module::build_effective_inc_roots;
        use perl_module::{
            ModuleUriResolution, resolve_module_path as resolve_workspace_module_path,
            resolve_module_uri_with_effective_inc,
        };
        use perl_module::UseLibPath;
        ''',
    ),
)

replace_between(
    "remove legacy lexical root assembler",
    "/// Prepend `use lib` paths extracted from `doc_text` to `include_paths`.\n",
    "impl LspServer {\n",
    "",
)

replace_once(
    "test-only workspace-root helper",
    "fn workspace_root_for_doc(workspace_folders: &[String], doc_uri: Option<&str>) -> Option<PathBuf> {\n",
    "#[cfg(test)]\nfn workspace_root_for_doc(workspace_folders: &[String], doc_uri: Option<&str>) -> Option<PathBuf> {\n",
)

replace_between(
    "remove cloned workspace config and root adapters",
    "fn workspace_config_for_doc(\n",
    "fn append_system_inc_paths(\n",
    "",
)

for label, marker in [
    ("test-only system-inc append adapter", "fn append_system_inc_paths(\n"),
    ("test-only system-inc append helper", "fn append_system_inc_paths_from(\n"),
    ("test-only normalized inc key", "fn normalized_inc_key(path: &std::path::Path) -> String {\n"),
]:
    replace_once(label, marker, "#[cfg(test)]\n" + marker)

replace_between(
    "context-free resolver delegation",
    "    #[allow(dead_code)] // Used by tests and available for callers without a document URI\n",
    "    /// Resolve module path with document URI for FindBin support.\n",
    block(
        '''
        #[allow(dead_code)] // Used by tests and available for callers without a document URI
        pub(crate) fn resolve_module_path(
            &self,
            module: &str,
            doc_text: Option<&str>,
        ) -> Option<PathBuf> {
            self.resolve_module_path_with_uri(module, doc_text, None)
        }

        ''',
        4,
    ),
)

replace_between(
    "URI-aware resolver cutover",
    "    /// Resolve module path with document URI for FindBin support.\n",
    "    /// Resolve an XS bootstrap target to the most likely `.xs` source path.\n",
    block(
        '''
        /// Resolve module path with document URI for FindBin support.
        ///
        /// This compatibility entry point consumes the same labeled effective
        /// `@INC` context as the canonical URI resolver. In particular, startup
        /// `@INC` is acquired through the stored folder/global configuration, so
        /// its bounded retry and settled-cache state survives across requests.
        pub(crate) fn resolve_module_path_with_uri(
            &self,
            module: &str,
            doc_text: Option<&str>,
            doc_uri: Option<&str>,
        ) -> Option<PathBuf> {
            let context = match self.effective_inc_context_for_doc(doc_uri, doc_text, None) {
                Some(context) => context,
                None => {
                    if !self.root_undetected_shown.fetch_or(true, Ordering::SeqCst) {
                        self.show_message_or_log(
                            MessageType::Warning,
                            "Perl LSP: workspace root not detected — module resolution disabled. \\
                             To enable: open the project folder in your editor (File > Open Folder) \\
                             rather than individual files. This warning appears once per server session.",
                        );
                    }
                    return None;
                }
            };

            let include_paths = context
                .effective_roots
                .iter()
                .map(|root| root.path.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            resolve_workspace_module_path(&context.root, module, &include_paths)
        }

        ''',
        4,
    ),
)

system_test_marker = block(
    '''
    #[test]
    fn test_resolve_module_path_with_uri_honors_system_inc_opt_in() -> TestResult {
    ''',
    4,
)

new_tests = block(
    r'''
    #[test]
    fn resolve_module_path_with_uri_consumes_the_canonical_inc_context() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let module_file = workspace.join("lib").join("Canonical").join("Route.pm");
        fs::create_dir_all(module_file.parent().ok_or("missing module parent")?)?;
        fs::write(&module_file, "package Canonical::Route; 1;")?;

        let doc_uri = Url::from_file_path(workspace.join("main.pl"))
            .map_err(|_| "failed to create doc uri")?
            .to_string();
        let server = LspServer::new();
        *server.root_path.lock() = Some(workspace.clone());
        {
            let mut config = server.workspace_config.lock();
            config.include_paths = vec!["lib".to_string()];
            config.use_system_inc = false;
            config.use_perl5lib = false;
        }

        let builds = crate::runtime::lifecycle::inc_context::inc_context_build_probe();
        let resolved = server.resolve_module_path_with_uri(
            "Canonical::Route",
            Some("use Canonical::Route;\n"),
            Some(&doc_uri),
        );

        assert_eq!(resolved, Some(module_file));
        assert_eq!(
            builds.count(),
            1,
            "the compatibility entry point must assemble the canonical EffectiveIncContext"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn resolve_module_path_with_uri_reuses_stored_system_inc_after_probe_binary_disappears()
    -> TestResult {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let external_inc = temp.path().join("external-inc");
        let module_file = external_inc.join("Stored").join("SystemInc.pm");
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(module_file.parent().ok_or("missing module parent")?)?;
        fs::write(&module_file, "package Stored::SystemInc; 1;")?;

        let fake_perl = temp.path().join("fake-perl");
        let invocation_log = temp.path().join("probe-count");
        let log_display = invocation_log.to_string_lossy();
        let inc_display = external_inc.to_string_lossy();
        for (label, value) in [("log", log_display.as_ref()), ("inc", inc_display.as_ref())] {
            assert!(
                !value
                    .chars()
                    .any(|character| matches!(character, '"' | '$' | '`' | '\\' | '\n')),
                "temporary {label} path is not safe to embed in the fake interpreter: {value:?}"
            );
        }
        fs::write(
            &fake_perl,
            format!(
                "#!/bin/sh\nprintf 'probe\\n' >> \"{log_display}\"\nprintf '%s\\n' \"{inc_display}\"\n"
            ),
        )?;
        let mut permissions = fs::metadata(&fake_perl)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_perl, permissions)?;

        let doc_uri = Url::from_file_path(workspace.join("main.pl"))
            .map_err(|_| "failed to create doc uri")?
            .to_string();
        let folder_uri = Url::from_directory_path(&workspace)
            .map_err(|_| "failed to create folder uri")?
            .to_string();
        let mut config = perl_lsp_rs_core::config::WorkspaceConfig::default();
        config.include_paths = Vec::new();
        config.use_system_inc = true;
        config.use_perl5lib = false;
        config.perl_path = Some(fake_perl.to_string_lossy().into_owned());

        let server = LspServer::new();
        *server.root_path.lock() = Some(workspace.clone());
        *server.workspace_folders.lock() = vec![
            WorkspaceFolderState::new(folder_uri)
                .with_path(workspace)
                .with_effective_workspace_config(config),
        ];

        let builds = crate::runtime::lifecycle::inc_context::inc_context_build_probe();
        let first = server.resolve_module_path_with_uri(
            "Stored::SystemInc",
            Some("use Stored::SystemInc;\n"),
            Some(&doc_uri),
        );
        assert_eq!(first, Some(module_file.clone()));

        // Removing the admitted binary makes any second probe fail. Resolution
        // can still succeed only when the first successful outcome was retained
        // in the stored folder configuration rather than an ephemeral clone.
        fs::remove_file(&fake_perl)?;
        let second = server.resolve_module_path_with_uri(
            "Stored::SystemInc",
            Some("use Stored::SystemInc;\n"),
            Some(&doc_uri),
        );
        assert_eq!(second, Some(module_file));
        assert_eq!(builds.count(), 2, "each request should assemble one canonical context");

        let invocations = fs::read_to_string(&invocation_log)?;
        assert_eq!(
            invocations.lines().count(),
            1,
            "a settled startup @INC success must be reused across compatibility requests"
        );
        Ok(())
    }

    ''',
    4,
)
replace_once(
    "canonical-route regression tests",
    system_test_marker,
    new_tests + system_test_marker,
)

SOURCE.write_text(text, encoding="utf-8")
