use perl_module::{
    IncRoot, IncRootKind, ModuleUriResolution, collect_module_uri_candidates_with_effective_inc,
    resolve_module_uri, resolve_module_uri_with_effective_inc,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn file_uri(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    url::Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| "failed to create file URI".into())
}

fn directory_uri(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    url::Url::from_directory_path(path)
        .map(|url| url.to_string())
        .map_err(|_| "failed to create directory URI".into())
}

#[test]
fn given_open_document_when_resolving_then_open_document_takes_precedence() {
    let open_doc = "file:///workspace/lib/Foo/Bar.pm".to_string();

    let result = resolve_module_uri(
        "Foo::Bar",
        std::slice::from_ref(&open_doc),
        &["file:///workspace".to_string()],
        &["lib".to_string()],
        false,
        &[],
        Duration::from_millis(50),
    );

    assert_eq!(result, ModuleUriResolution::Resolved(open_doc));
}

#[test]
fn given_workspace_folder_when_resolving_then_workspace_file_uri_is_returned()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let workspace = temp.path().join("workspace");
    let module_file = workspace.join("lib").join("Demo").join("Worker.pm");

    std::fs::create_dir_all(module_file.parent().unwrap_or(&workspace))?;
    std::fs::write(&module_file, "package Demo::Worker; 1;")?;

    let workspace_uri =
        url::Url::from_file_path(&workspace).map_err(|()| "failed to build workspace URI")?;
    let workspace_uri = workspace_uri.to_string();

    let result = resolve_module_uri(
        "Demo::Worker",
        &[],
        &[workspace_uri],
        &["lib".to_string()],
        false,
        &[],
        Duration::from_millis(50),
    );

    match result {
        ModuleUriResolution::Resolved(uri) => {
            assert!(uri.starts_with("file://"));
            assert!(uri.ends_with("Demo/Worker.pm") || uri.ends_with("Demo\\Worker.pm"));
        }
        other => return Err(format!("expected resolved URI, got {other:?}").into()),
    }

    Ok(())
}

#[test]
fn given_system_inc_disabled_when_resolving_then_system_paths_are_ignored()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let system_inc = temp.path().join("perl5");
    let module_file = system_inc.join("Only").join("InInc.pm");

    std::fs::create_dir_all(module_file.parent().unwrap_or(&system_inc))?;
    std::fs::write(&module_file, "package Only::InInc; 1;")?;

    let result = resolve_module_uri(
        "Only::InInc",
        &[],
        &[],
        &["lib".to_string()],
        false,
        &[PathBuf::from(&system_inc)],
        Duration::from_millis(50),
    );

    assert_eq!(result, ModuleUriResolution::NotFound);
    Ok(())
}

#[test]
fn given_workspace_roots_when_collecting_then_root_precedence_is_workspace_expanded()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let workspace_a = temp.path().join("workspace-a");
    let workspace_b = temp.path().join("workspace-b");
    let root_one = PathBuf::from("root-one");
    let root_two = PathBuf::from("root-two");
    let mut expected_uris = Vec::new();

    for root in [&root_one, &root_two] {
        for workspace in [&workspace_a, &workspace_b] {
            let module = workspace.join(root).join("Foo").join("Bar.pm");
            std::fs::create_dir_all(module.parent().ok_or("workspace module has no parent")?)?;
            std::fs::write(&module, "package Foo::Bar; 1;")?;
            expected_uris.push(file_uri(&module)?);
        }
    }

    let workspace_folders = vec![directory_uri(&workspace_a)?, directory_uri(&workspace_b)?];
    let roots = vec![
        IncRoot {
            kind: IncRootKind::WorkspaceRelative,
            path: root_one,
            precedence: 0,
            source: "root-one".to_string(),
        },
        IncRoot {
            kind: IncRootKind::WorkspaceRelative,
            path: root_two,
            precedence: 1,
            source: "root-two".to_string(),
        },
    ];

    let report = collect_module_uri_candidates_with_effective_inc(
        "Foo::Bar",
        &[],
        &workspace_folders,
        &roots,
        Duration::from_secs(1),
    );
    let first_uri = expected_uris.first().cloned().ok_or("expected candidates are empty")?;
    assert_eq!(
        resolve_module_uri_with_effective_inc(
            "Foo::Bar",
            &[],
            &workspace_folders,
            &roots,
            Duration::from_secs(1),
        ),
        ModuleUriResolution::Resolved(first_uri)
    );
    assert_eq!(
        report.candidates.iter().map(|candidate| candidate.uri.as_str()).collect::<Vec<_>>(),
        expected_uris.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(
        report.candidates.iter().map(|candidate| candidate.search_order).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    Ok(())
}

// ── URI spelling preservation ────────────────────────────────────────────────

/// GIVEN an open document with a URI that contains raw characters needing
/// URL-encoding (spaces, percent signs, angle brackets) WHEN the module
/// candidate report is collected THEN the candidate URI equals the supplied
/// URI verbatim, with no re-encoding applied.
///
/// `ModuleUriCandidate.uri` contract: "open-document candidates preserve the
/// supplied URI spelling."  Asserting the raw form avoids the platform-
/// dependent Url::parse → to_file_path → from_file_path round-trip that
/// URL-encodes on Linux but silently falls back to the original string on
/// Windows, causing intermittent cross-platform test failures (issue #6234).
#[test]
fn given_open_document_with_unencoded_uri_when_resolving_then_uri_spelling_is_preserved() {
    // Build a URI with characters that would be percent-encoded by a
    // URL round-trip: space (%20), percent-sign (%25), angle bracket (%3C).
    let open_doc = "file:///workspace/lib/My Module%<special>/Foo.pm".to_string();

    let report = collect_module_uri_candidates_with_effective_inc(
        "My Module%<special>::Foo",
        std::slice::from_ref(&open_doc),
        &[],
        &[],
        Duration::from_secs(1),
    );

    assert_eq!(report.candidates.len(), 1, "expected exactly one candidate");
    let candidate = &report.candidates[0];
    assert_eq!(
        candidate.uri, open_doc,
        "open-document candidate must preserve the raw supplied URI, not re-encode it"
    );
    assert_eq!(candidate.source, "open-document");
    assert!(candidate.inc_root.is_none());
    assert_eq!(candidate.search_order, 0);
}
