//! Deterministic Perl module URI resolution helpers.
//!
//! Extracts the URI-first, timeout-bounded resolution policy.

use crate::path::{module_name_to_path, module_path_to_name};
use perl_parser_core::path_security::validate_workspace_path;
use perl_workspace::folder::workspace_folder_to_path;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use url::Url;

/// Source/category of an effective include root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncRootKind {
    /// File-local lexical include roots (for example `use lib` overlays).
    FileLocalLexical,
    /// Workspace-relative include roots, resolved against each owning workspace.
    WorkspaceRelative,
    /// External absolute include roots.
    ExternalAbsolute,
    /// Paths sourced from the `PERL5LIB` environment variable.
    ///
    /// Treated like `ExternalAbsolute` for resolution (no workspace-boundary
    /// validation) but carries a distinct source label so diagnostics and
    /// tooling can tell environment-supplied roots apart from project-configured ones.
    Perl5LibEnv,
    /// Startup `@INC` entries from the selected Perl interpreter.
    InterpreterStartup,
    /// Runtime-derived include roots (reserved for future trusted runtime mode).
    RuntimeDerived,
}

/// A single ordered include root entry used to resolve modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncRoot {
    /// Root kind/category.
    pub kind: IncRootKind,
    /// Path value for this root.
    pub path: PathBuf,
    /// Search precedence: lower values are searched first.
    pub precedence: usize,
    /// Human-readable source label.
    pub source: String,
}

/// One existing module candidate discovered in effective `@INC` order.
///
/// The report intentionally carries only resolver-owned facts. Semantic
/// confidence, source generation, and dynamic-boundary explanations belong to
/// the semantic-facts layer that consumes this substrate.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleUriCandidate {
    /// File URI for the candidate. Filesystem candidates are canonicalized;
    /// open-document candidates preserve the supplied URI spelling.
    pub uri: String,
    /// Source label inherited from the effective include root, or
    /// `open-document` for an in-memory document.
    pub source: String,
    /// The effective include root that produced this candidate, when one
    /// exists. Open-document candidates have no filesystem root.
    pub inc_root: Option<IncRoot>,
    /// Stable order among the returned candidates; lower values win.
    pub search_order: usize,
}

/// Ordered existing candidates for a module-resolution request.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleUriCandidateReport {
    /// Canonicalized module name derived from the request.
    pub module_name: String,
    /// Relative filesystem path derived from `module_name`.
    pub relative_path: String,
    /// Existing candidates in deterministic search order.
    pub candidates: Vec<ModuleUriCandidate>,
    /// Whether the search budget expired before all roots were inspected.
    pub timed_out: bool,
}

fn candidate_report(
    module_name: &str,
    relative_path: &str,
    candidates: Vec<ModuleUriCandidate>,
    timed_out: bool,
) -> ModuleUriCandidateReport {
    ModuleUriCandidateReport {
        module_name: module_name.to_string(),
        relative_path: relative_path.to_string(),
        candidates,
        timed_out,
    }
}

/// Build ordered effective include roots from lexical, configured, environment,
/// and interpreter startup sources.
///
/// This centralizes the source-labeling and precedence model used by URI
/// resolution. Callers are still responsible for computing configured include
/// paths and deciding whether `PERL5LIB` / system `@INC` should participate.
///
/// # Security
///
/// `include_paths` entries are classified as `ExternalAbsolute` purely by
/// `Path::is_absolute()` — there is no provenance signal here at all. This
/// cannot distinguish an absolute root configured by the user from one
/// supplied by a workspace, whether that workspace supplied it via
/// `.perl-lsp.toml` or via resource-scoped LSP client settings (issue #4998).
/// Callers MUST NOT pass
/// untrusted absolute entries into `include_paths`; validate/reject those
/// before merging into the caller's `include_paths`. See
/// `perl_lsp_rs_core::config::ProjectConfig::apply_to_workspace_config`
/// (issue #4957, precedent: issue #3729) for where that untrusted-channel
/// sanitization happens.
#[must_use]
pub fn build_effective_inc_roots(
    include_paths: &[String],
    perl5lib_paths: &[String],
    use_perl5lib: bool,
    lexical_paths: &[String],
    system_paths: &[PathBuf],
) -> Vec<IncRoot> {
    let perl5lib_set: HashSet<String> =
        if use_perl5lib { perl5lib_paths.iter().cloned().collect() } else { HashSet::new() };

    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut precedence = 0usize;

    for path in lexical_paths {
        let path_buf = PathBuf::from(path);
        let kind = if path_buf.is_absolute() {
            IncRootKind::ExternalAbsolute
        } else {
            IncRootKind::FileLocalLexical
        };
        if !seen.insert(normalized_inc_key(&path_buf)) {
            continue;
        }
        roots.push(IncRoot {
            kind,
            path: path_buf,
            precedence,
            source: "use-lib-lexical".to_string(),
        });
        precedence += 1;
    }

    for path in include_paths {
        let path_buf = PathBuf::from(path);
        if !seen.insert(normalized_inc_key(&path_buf)) {
            continue;
        }
        let (kind, source) = if perl5lib_set.contains(path) {
            (IncRootKind::Perl5LibEnv, "perl5lib-env")
        } else if path_buf.is_absolute() {
            (IncRootKind::ExternalAbsolute, "workspace-include-paths")
        } else {
            (IncRootKind::WorkspaceRelative, "workspace-include-paths")
        };
        roots.push(IncRoot { kind, path: path_buf, precedence, source: source.to_string() });
        precedence += 1;
    }

    for path in system_paths {
        if !seen.insert(normalized_inc_key(path)) {
            continue;
        }
        roots.push(IncRoot {
            kind: IncRootKind::InterpreterStartup,
            path: path.clone(),
            precedence,
            source: "interpreter-startup-inc".to_string(),
        });
        precedence += 1;
    }

    roots
}

/// Outcome of a module name to URI resolution attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleUriResolution {
    /// A matching module URI was found.
    Resolved(String),
    /// No matching module was found.
    NotFound,
    /// Resolution stopped because the timeout budget was exhausted.
    TimedOut,
}

/// Resolve a module name to a `file://` URI using deterministic precedence.
///
/// Search order:
/// 1. Open document URIs (path-boundary match on relative module path)
/// 2. Workspace folders + `include_paths` (path-safe filesystem checks)
/// 3. System `@INC` paths (when `use_system_inc` is true)
///
/// # Security
///
/// Absolute `include_paths` entries are followed as literal external roots
/// with no workspace-boundary check (see [`build_effective_inc_roots`]).
/// Callers MUST NOT pass untrusted (e.g. workspace-file-sourced,
/// `.perl-lsp.toml`) absolute entries into `include_paths` — validate/reject
/// those upstream. See
/// `perl_lsp_rs_core::config::ProjectConfig::apply_to_workspace_config`
/// (issue #4957, precedent: issue #3729).
#[must_use]
pub fn resolve_module_uri(
    module_name: &str,
    open_document_uris: &[String],
    workspace_folders: &[String],
    include_paths: &[String],
    use_system_inc: bool,
    system_inc: &[PathBuf],
    timeout: Duration,
) -> ModuleUriResolution {
    let mut effective_inc_roots = Vec::new();
    let mut seen_include_paths = HashSet::new();

    for include_path in include_paths {
        let Some(path) = normalize_inc_path_string(include_path) else {
            continue;
        };
        if !seen_include_paths.insert(path.clone()) {
            continue;
        }

        let kind = if path.is_absolute() {
            IncRootKind::ExternalAbsolute
        } else {
            IncRootKind::WorkspaceRelative
        };
        effective_inc_roots.push(IncRoot {
            kind,
            path,
            precedence: effective_inc_roots.len(),
            source: "includePaths".to_string(),
        });
    }

    if use_system_inc {
        let mut seen_system_paths = HashSet::new();

        for path in system_inc {
            let Some(path) = normalize_system_inc_path(path) else {
                continue;
            };
            if !seen_system_paths.insert(path.clone()) {
                continue;
            }

            effective_inc_roots.push(IncRoot {
                kind: IncRootKind::InterpreterStartup,
                path,
                precedence: effective_inc_roots.len(),
                source: "interpreter-startup-inc".to_string(),
            });
        }
    }

    resolve_module_uri_with_effective_inc(
        module_name,
        open_document_uris,
        workspace_folders,
        &effective_inc_roots,
        timeout,
    )
}

/// Resolve a module name to a `file://` URI using an ordered effective `@INC` model.
#[must_use]
pub fn resolve_module_uri_with_effective_inc(
    module_name: &str,
    open_document_uris: &[String],
    workspace_folders: &[String],
    effective_inc_roots: &[IncRoot],
    timeout: Duration,
) -> ModuleUriResolution {
    let report = collect_module_uri_candidates(
        module_name,
        open_document_uris,
        workspace_folders,
        effective_inc_roots,
        timeout,
        Some(1),
    );

    if let Some(candidate) = report.candidates.first() {
        return ModuleUriResolution::Resolved(candidate.uri.clone());
    }
    if report.timed_out { ModuleUriResolution::TimedOut } else { ModuleUriResolution::NotFound }
}

/// Collect existing module candidates in the same order used by URI resolution.
///
/// Unlike [`resolve_module_uri_with_effective_inc`], this preserves losing
/// existing candidates so consumers can explain precedence and inspect
/// ambiguity without rebuilding the effective-root traversal themselves.
#[must_use]
pub fn collect_module_uri_candidates_with_effective_inc(
    module_name: &str,
    open_document_uris: &[String],
    workspace_folders: &[String],
    effective_inc_roots: &[IncRoot],
    timeout: Duration,
) -> ModuleUriCandidateReport {
    collect_module_uri_candidates(
        module_name,
        open_document_uris,
        workspace_folders,
        effective_inc_roots,
        timeout,
        None,
    )
}

fn collect_module_uri_candidates(
    module_name: &str,
    open_document_uris: &[String],
    workspace_folders: &[String],
    effective_inc_roots: &[IncRoot],
    timeout: Duration,
    candidate_limit: Option<usize>,
) -> ModuleUriCandidateReport {
    let start_time = Instant::now();
    let relative_path = module_name_to_path(module_name);
    let canonical_module_name = module_path_to_name(&relative_path);
    let mut candidates = Vec::new();
    let mut seen_uris = HashSet::new();
    let mut search_order = 0usize;

    for uri in open_document_uris {
        if open_document_uri_matches_relative_path(uri, &relative_path)
            && insert_seen_uri(&mut seen_uris, uri)
        {
            candidates.push(ModuleUriCandidate {
                uri: uri.clone(),
                source: "open-document".to_string(),
                inc_root: None,
                search_order,
            });
            search_order += 1;
            if candidate_limit == Some(candidates.len()) {
                return candidate_report(&canonical_module_name, &relative_path, candidates, false);
            }
        }
    }

    let mut ordered_roots = effective_inc_roots.to_vec();
    ordered_roots.sort_by_key(|r| r.precedence);

    for inc_root in &ordered_roots {
        if start_time.elapsed() >= timeout {
            return candidate_report(&canonical_module_name, &relative_path, candidates, true);
        }

        match inc_root.kind {
            IncRootKind::FileLocalLexical | IncRootKind::WorkspaceRelative => {
                for workspace_folder in workspace_folders {
                    if start_time.elapsed() >= timeout {
                        return candidate_report(
                            &canonical_module_name,
                            &relative_path,
                            candidates,
                            true,
                        );
                    }

                    let workspace_path = workspace_folder_to_path(workspace_folder);
                    let full_path = full_path_for_root(inc_root, &workspace_path, &relative_path);
                    let Some(full_path) = full_path else { continue };

                    if full_path.is_file()
                        && let Ok(url) = Url::from_file_path(&full_path)
                    {
                        let uri = url.to_string();
                        if insert_seen_uri(&mut seen_uris, &uri) {
                            candidates.push(ModuleUriCandidate {
                                uri,
                                source: inc_root.source.clone(),
                                inc_root: Some(inc_root.clone()),
                                search_order,
                            });
                            search_order += 1;
                            if candidate_limit == Some(candidates.len()) {
                                return candidate_report(
                                    &canonical_module_name,
                                    &relative_path,
                                    candidates,
                                    false,
                                );
                            }
                        }
                    }
                }
            }
            IncRootKind::ExternalAbsolute
            | IncRootKind::Perl5LibEnv
            | IncRootKind::InterpreterStartup
            | IncRootKind::RuntimeDerived => {
                let full_path = inc_root.path.join(&relative_path);
                if full_path.is_file()
                    && let Ok(url) = Url::from_file_path(&full_path)
                {
                    let uri = url.to_string();
                    if insert_seen_uri(&mut seen_uris, &uri) {
                        candidates.push(ModuleUriCandidate {
                            uri,
                            source: inc_root.source.clone(),
                            inc_root: Some(inc_root.clone()),
                            search_order,
                        });
                        search_order += 1;
                        if candidate_limit == Some(candidates.len()) {
                            return candidate_report(
                                &canonical_module_name,
                                &relative_path,
                                candidates,
                                false,
                            );
                        }
                    }
                }
            }
        }
    }

    candidate_report(&canonical_module_name, &relative_path, candidates, false)
}

fn insert_seen_uri(seen_uris: &mut HashSet<String>, uri: &str) -> bool {
    let identity = Url::parse(uri)
        .ok()
        .filter(|url| url.scheme() == "file")
        .and_then(|url| url.to_file_path().ok())
        .and_then(|path| Url::from_file_path(path).ok())
        .map_or_else(|| uri.to_string(), |url| url.to_string());
    seen_uris.insert(identity)
}

fn open_document_uri_matches_relative_path(uri: &str, relative_path: &str) -> bool {
    if relative_path.is_empty() {
        return false;
    }

    let normalized_uri = uri.replace('\\', "/");
    let normalized_relative_path = relative_path.replace('\\', "/");
    normalized_uri
        .strip_suffix(&normalized_relative_path)
        .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
}

fn normalize_inc_path_string(input: &str) -> Option<PathBuf> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(normalize_path_for_dedupe(Path::new(trimmed)))
}

fn normalize_system_inc_path(input: &Path) -> Option<PathBuf> {
    let trimmed = input.to_string_lossy().trim().to_string();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = normalize_path_for_dedupe(Path::new(&trimmed));
    if normalized == Path::new(".") {
        return None;
    }

    Some(normalized)
}

fn normalize_path_for_dedupe(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        if component == Component::CurDir {
            continue;
        }
        normalized.push(component.as_os_str());
    }

    if normalized.as_os_str().is_empty() { PathBuf::from(".") } else { normalized }
}

fn normalized_inc_key(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized == "/" { normalized } else { normalized.trim_end_matches('/').to_string() }
}

fn full_path_for_root(
    inc_root: &IncRoot,
    workspace_path: &Path,
    relative_path: &str,
) -> Option<PathBuf> {
    match inc_root.kind {
        IncRootKind::FileLocalLexical | IncRootKind::WorkspaceRelative => {
            if inc_root.path == Path::new(".") {
                let full_path = workspace_path.join(relative_path);
                validate_workspace_path(&full_path, workspace_path).ok()
            } else if inc_root.path.is_absolute() {
                Some(inc_root.path.join(relative_path))
            } else {
                let full_path = workspace_path.join(&inc_root.path).join(relative_path);
                validate_workspace_path(&full_path, workspace_path).ok()
            }
        }
        IncRootKind::ExternalAbsolute
        | IncRootKind::Perl5LibEnv
        | IncRootKind::InterpreterStartup
        | IncRootKind::RuntimeDerived => Some(inc_root.path.join(relative_path)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IncRoot, IncRootKind, ModuleUriResolution, build_effective_inc_roots,
        collect_module_uri_candidates_with_effective_inc, open_document_uri_matches_relative_path,
        resolve_module_uri_with_effective_inc,
    };
    use std::path::PathBuf;

    #[test]
    fn effective_inc_roots_dedupes_normalized_sources() {
        let include_paths = vec!["lib".to_string(), "lib/".to_string(), "other".to_string()];
        let lexical_paths = vec!["lib\\".to_string()];
        let system_paths = vec![PathBuf::from("other/"), PathBuf::from("syslib")];

        let roots =
            build_effective_inc_roots(&include_paths, &[], false, &lexical_paths, &system_paths);
        let root_paths: Vec<String> =
            roots.iter().map(|root| root.path.to_string_lossy().replace('\\', "/")).collect();

        assert_eq!(root_paths, vec!["lib/".to_string(), "other".to_string(), "syslib".to_string()]);
        assert_eq!(roots[0].source, "use-lib-lexical");
        assert_eq!(roots[1].source, "workspace-include-paths");
        assert_eq!(roots[2].source, "interpreter-startup-inc");
    }

    #[test]
    fn effective_inc_roots_preserves_first_source_precedence() {
        let include_paths = vec!["dup".to_string(), "late".to_string()];
        let lexical_paths = vec!["dup".to_string()];
        let system_paths = vec![PathBuf::from("late"), PathBuf::from("sys")];

        let roots =
            build_effective_inc_roots(&include_paths, &[], false, &lexical_paths, &system_paths);

        assert_eq!(roots.len(), 3);
        assert_eq!(roots[0].path, PathBuf::from("dup"));
        assert_eq!(roots[0].kind, IncRootKind::FileLocalLexical);
        assert_eq!(roots[1].path, PathBuf::from("late"));
        assert_eq!(roots[1].kind, IncRootKind::WorkspaceRelative);
        assert_eq!(roots[2].path, PathBuf::from("sys"));
        assert_eq!(roots[2].kind, IncRootKind::InterpreterStartup);
        assert_eq!(roots[0].precedence, 0);
        assert_eq!(roots[1].precedence, 1);
        assert_eq!(roots[2].precedence, 2);
    }

    #[test]
    fn effective_inc_roots_labels_perl5lib_only_when_enabled() {
        let perl5lib_path = "perl5lib".to_string();
        let include_paths = vec![perl5lib_path.clone(), "lib".to_string()];

        let enabled = build_effective_inc_roots(
            &include_paths,
            std::slice::from_ref(&perl5lib_path),
            true,
            &[],
            &[],
        );
        assert_eq!(enabled[0].kind, IncRootKind::Perl5LibEnv);
        assert_eq!(enabled[0].source, "perl5lib-env");
        assert_eq!(enabled[1].kind, IncRootKind::WorkspaceRelative);

        let disabled = build_effective_inc_roots(&include_paths, &[perl5lib_path], false, &[], &[]);
        assert_eq!(disabled[0].kind, IncRootKind::WorkspaceRelative);
        assert_eq!(disabled[0].source, "workspace-include-paths");
        assert_eq!(disabled[1].kind, IncRootKind::WorkspaceRelative);
    }

    #[test]
    fn candidate_report_preserves_losing_roots_in_search_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let first = workspace.join("first").join("Foo").join("Bar.pm");
        let second = workspace.join("second").join("Foo").join("Bar.pm");
        std::fs::create_dir_all(first.parent().ok_or("missing first parent")?)?;
        std::fs::create_dir_all(second.parent().ok_or("missing second parent")?)?;
        std::fs::write(&first, "package Foo::Bar; 1;")?;
        std::fs::write(&second, "package Foo::Bar; 1;")?;

        let workspace_uri = url::Url::from_directory_path(&workspace)
            .map_err(|_| "failed to create workspace URI")?
            .to_string();
        let roots = build_effective_inc_roots(
            &["first".to_string(), "second".to_string()],
            &[],
            false,
            &[],
            &[],
        );
        let report = collect_module_uri_candidates_with_effective_inc(
            "Foo::Bar",
            &[],
            std::slice::from_ref(&workspace_uri),
            &roots,
            std::time::Duration::from_secs(1),
        );

        assert!(!report.timed_out);
        assert_eq!(report.module_name, "Foo::Bar");
        assert_eq!(report.relative_path, "Foo/Bar.pm");
        assert_eq!(report.candidates.len(), 2);
        assert_eq!(report.candidates[0].source, "workspace-include-paths");
        assert_eq!(
            report.candidates[0].inc_root.as_ref().map(|root| &root.path),
            Some(&PathBuf::from("first"))
        );
        assert_eq!(report.candidates[0].inc_root.as_ref(), Some(&roots[0]));
        assert_eq!(report.candidates[0].search_order, 0);
        assert_eq!(report.candidates[1].inc_root.as_ref(), Some(&roots[1]));
        assert_eq!(report.candidates[1].search_order, 1);
        assert!(report.candidates[0].uri.ends_with("first/Foo/Bar.pm"));
        assert!(report.candidates[1].uri.ends_with("second/Foo/Bar.pm"));

        assert_eq!(
            resolve_module_uri_with_effective_inc(
                "Foo::Bar",
                &[],
                std::slice::from_ref(&workspace_uri),
                &roots,
                std::time::Duration::from_secs(1),
            ),
            ModuleUriResolution::Resolved(report.candidates[0].uri.clone())
        );
        Ok(())
    }

    #[test]
    fn candidate_report_visits_mixed_root_kinds_in_precedence_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let workspace_a = temp.path().join("workspace-a");
        let workspace_b = temp.path().join("workspace-b");
        let external = temp.path().join("external");
        std::fs::create_dir_all(workspace_a.join("lib/Foo"))?;
        std::fs::create_dir_all(workspace_b.join("lib/Foo"))?;
        std::fs::create_dir_all(external.join("Foo"))?;
        std::fs::write(workspace_a.join("lib/Foo/Bar.pm"), "package Foo::Bar; 1;")?;
        std::fs::write(workspace_b.join("lib/Foo/Bar.pm"), "package Foo::Bar; 1;")?;
        std::fs::write(external.join("Foo/Bar.pm"), "package Foo::Bar; 1;")?;

        let workspace_uris = [
            url::Url::from_directory_path(&workspace_a)
                .map_err(|_| "failed to create first workspace URI")?
                .to_string(),
            url::Url::from_directory_path(&workspace_b)
                .map_err(|_| "failed to create second workspace URI")?
                .to_string(),
        ];
        let roots = [
            IncRoot {
                kind: IncRootKind::ExternalAbsolute,
                path: external,
                precedence: 0,
                source: "external-first".to_string(),
            },
            IncRoot {
                kind: IncRootKind::WorkspaceRelative,
                path: PathBuf::from("lib"),
                precedence: 1,
                source: "workspace-second".to_string(),
            },
        ];

        let report = collect_module_uri_candidates_with_effective_inc(
            "Foo::Bar",
            &[],
            &workspace_uris,
            &roots,
            std::time::Duration::from_secs(1),
        );

        assert!(!report.timed_out);
        assert_eq!(report.candidates.len(), 3);
        assert_eq!(report.candidates[0].source, "external-first");
        assert_eq!(report.candidates[0].search_order, 0);
        assert_eq!(report.candidates[0].inc_root.as_ref(), Some(&roots[0]));
        assert!(report.candidates[0].uri.ends_with("external/Foo/Bar.pm"));
        assert_eq!(report.candidates[1].source, "workspace-second");
        assert_eq!(report.candidates[1].search_order, 1);
        assert!(report.candidates[1].uri.ends_with("workspace-a/lib/Foo/Bar.pm"));
        assert_eq!(report.candidates[2].search_order, 2);
        assert!(report.candidates[2].uri.ends_with("workspace-b/lib/Foo/Bar.pm"));
        assert_eq!(
            resolve_module_uri_with_effective_inc(
                "Foo::Bar",
                &[],
                &workspace_uris,
                &roots,
                std::time::Duration::from_secs(1),
            ),
            ModuleUriResolution::Resolved(report.candidates[0].uri.clone())
        );
        Ok(())
    }

    #[test]
    fn candidate_report_labels_open_documents_without_a_filesystem_root() {
        let open_document = "file:///workspace/lib/Foo/Bar.pm".to_string();
        let report = collect_module_uri_candidates_with_effective_inc(
            "Foo'Bar",
            std::slice::from_ref(&open_document),
            &[],
            &[],
            std::time::Duration::from_secs(1),
        );

        assert!(!report.timed_out);
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.candidates[0].uri, open_document);
        assert_eq!(report.candidates[0].source, "open-document");
        assert!(report.candidates[0].inc_root.is_none());
        assert_eq!(report.candidates[0].search_order, 0);
    }

    #[test]
    fn open_document_uri_match_rejects_empty_relative_path() {
        assert!(
            !open_document_uri_matches_relative_path("file:///workspace/lib/Foo.pm", ""),
            "empty relative paths must never match an open document"
        );
    }

    #[test]
    fn open_document_uri_match_accepts_exact_relative_path() {
        let cases = [("Foo/Bar.pm", "Foo/Bar.pm", true), ("Other/Bar.pm", "Foo/Bar.pm", false)];

        for (normalized_uri, normalized_relative_path, expected) in cases {
            assert_eq!(
                open_document_uri_matches_relative_path(normalized_uri, normalized_relative_path),
                expected,
                "exact relative path equality should decide raw relative inputs"
            );
        }
    }

    #[test]
    fn open_document_uri_match_accepts_path_bounded_suffix() {
        assert!(
            open_document_uri_matches_relative_path(
                "file:///workspace/local/lib/Foo/Bar.pm",
                "Foo/Bar.pm"
            ),
            "open document URIs may contain editor or workspace prefixes before the module path"
        );
    }

    #[test]
    fn open_document_uri_match_rejects_unbounded_suffix() {
        assert!(
            !open_document_uri_matches_relative_path(
                "file:///workspace/local/lib/MyFoo/Bar.pm",
                "Foo/Bar.pm"
            ),
            "the preceding URI segment must end before the module path starts"
        );
    }

    #[test]
    fn open_document_uri_match_normalizes_windows_separators() {
        assert!(
            open_document_uri_matches_relative_path(
                "file:///workspace\\local\\lib\\Foo\\Bar.pm",
                "Foo\\Bar.pm"
            ),
            "path-boundary matching should not depend on slash direction"
        );
    }
}
