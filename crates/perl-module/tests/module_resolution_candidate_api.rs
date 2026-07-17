use perl_module::{
    IncRoot, IncRootKind, ModuleUriResolution, collect_module_uri_candidates_with_effective_inc,
    resolve_module_uri_with_effective_inc,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn workspace_uri(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    url::Url::from_directory_path(path)
        .map(|url| url.to_string())
        .map_err(|_| "failed to create workspace URI".into())
}

fn file_uri(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    url::Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| "failed to create file URI".into())
}

fn write_module(root: &Path, relative_path: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = root.join(relative_path);
    let parent = path.parent().ok_or("module path has no parent")?;
    std::fs::create_dir_all(parent)?;
    std::fs::write(&path, "package Foo::Bar; 1;")?;
    Ok(path)
}

#[test]
fn public_candidate_api_preserves_inc_precedence_workspace_expansion_and_provenance()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let workspace_a = temp.path().join("workspace-a");
    let workspace_b = temp.path().join("workspace-b");
    let external = temp.path().join("external");
    let workspace_a_module = write_module(&workspace_a, "lib/Foo/Bar.pm")?;
    let workspace_b_module = write_module(&workspace_b, "lib/Foo/Bar.pm")?;
    let external_module = write_module(&external, "Foo/Bar.pm")?;
    let workspace_uris = vec![workspace_uri(&workspace_a)?, workspace_uri(&workspace_b)?];
    let roots = vec![
        IncRoot {
            kind: IncRootKind::ExternalAbsolute,
            path: external.clone(),
            precedence: 20,
            source: "external-late".to_string(),
        },
        IncRoot {
            kind: IncRootKind::WorkspaceRelative,
            path: PathBuf::from("lib"),
            precedence: 10,
            source: "workspace-first".to_string(),
        },
    ];

    let report = collect_module_uri_candidates_with_effective_inc(
        "Foo::Bar",
        &[],
        &workspace_uris,
        &roots,
        Duration::from_secs(1),
    );

    assert!(!report.timed_out);
    assert_eq!(report.candidates.len(), 3);
    assert_eq!(report.candidates[0].uri, file_uri(&workspace_a_module)?);
    assert_eq!(report.candidates[1].uri, file_uri(&workspace_b_module)?);
    assert_eq!(report.candidates[2].uri, file_uri(&external_module)?);
    assert_eq!(report.candidates[0].source, "workspace-first");
    assert_eq!(report.candidates[2].source, "external-late");
    assert_eq!(report.candidates[0].inc_root.as_ref(), Some(&roots[1]));
    assert_eq!(report.candidates[1].inc_root.as_ref(), Some(&roots[1]));
    assert_eq!(report.candidates[2].inc_root.as_ref(), Some(&roots[0]));
    assert_eq!(
        report.candidates.iter().map(|candidate| candidate.search_order).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    Ok(())
}

#[test]
fn public_candidate_api_labels_open_documents_and_deduplicates_uris()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let external = temp.path().join("external");
    let module = write_module(&external, "Foo/Bar.pm")?;
    let open_document_uri = file_uri(&module)?;
    let roots = [IncRoot {
        kind: IncRootKind::ExternalAbsolute,
        path: external,
        precedence: 0,
        source: "external".to_string(),
    }];

    let report = collect_module_uri_candidates_with_effective_inc(
        "Foo::Bar",
        &[open_document_uri.clone(), open_document_uri.clone()],
        &[],
        &roots,
        Duration::from_secs(1),
    );

    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].uri, open_document_uri);
    assert_eq!(report.candidates[0].source, "open-document");
    assert_eq!(report.candidates[0].inc_root, None);
    assert_eq!(report.candidates[0].search_order, 0);
    Ok(())
}

#[test]
fn public_candidate_api_preserves_partial_timeout_and_resolution_compatibility()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let external = temp.path().join("external");
    let module = write_module(&external, "Foo/Bar.pm")?;
    let open_document_uri = file_uri(&module)?;
    let roots = [IncRoot {
        kind: IncRootKind::ExternalAbsolute,
        path: external,
        precedence: 0,
        source: "external".to_string(),
    }];

    let partial = collect_module_uri_candidates_with_effective_inc(
        "Foo'Bar",
        std::slice::from_ref(&open_document_uri),
        &[],
        &roots,
        Duration::ZERO,
    );

    assert_eq!(partial.module_name, "Foo::Bar");
    assert_eq!(partial.relative_path, "Foo/Bar.pm");
    assert!(partial.timed_out);
    assert_eq!(partial.candidates.len(), 1);
    assert_eq!(partial.candidates[0].uri, open_document_uri);

    let resolved_open_document = resolve_module_uri_with_effective_inc(
        "Foo'Bar",
        std::slice::from_ref(&open_document_uri),
        &[],
        &roots,
        Duration::ZERO,
    );
    assert_eq!(resolved_open_document, ModuleUriResolution::Resolved(open_document_uri.clone()));

    let resolved =
        resolve_module_uri_with_effective_inc("Foo'Bar", &[], &[], &roots, Duration::from_secs(1));
    assert_eq!(resolved, ModuleUriResolution::Resolved(file_uri(&module)?));
    Ok(())
}
