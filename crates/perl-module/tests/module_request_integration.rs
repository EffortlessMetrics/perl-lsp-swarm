//! Integration proof for the M01 compatibility adapters (#8497).
//!
//! These tests drive the *real* resolution entrypoint against a real filesystem
//! and assert that the adapters preserve behaviour exactly. M01 introduces a
//! domain boundary; it must not move a single resolution decision.

use std::time::Duration;

use perl_module::{
    IncRoot, IncRootKind, ModuleName, ModuleRequest, ModuleRequestError, ModuleResolutionOutcome,
    ModuleUriResolution, module_name_to_path, outcome_from_uri_resolution,
    resolve_module_uri_with_effective_inc, uri_resolution_from_outcome,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn workspace_with_module(
    temp: &tempfile::TempDir,
    relative_path: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let workspace = temp.path().join("workspace");
    let module_file = workspace.join("lib").join(relative_path);
    std::fs::create_dir_all(module_file.parent().ok_or("missing module parent")?)?;
    std::fs::write(&module_file, "package Foo::Bar; 1;")?;

    Ok(url::Url::from_directory_path(&workspace)
        .map_err(|()| "failed to build workspace URI")?
        .to_string())
}

fn lib_root() -> IncRoot {
    IncRoot {
        kind: IncRootKind::WorkspaceRelative,
        path: std::path::PathBuf::from("lib"),
        precedence: 0,
        source: "workspace-include-paths".to_string(),
    }
}

#[test]
fn a_validated_request_resolves_exactly_as_the_legacy_string_did() -> TestResult {
    let temp = tempfile::tempdir()?;
    let workspace_uri = workspace_with_module(&temp, "Foo/Bar.pm")?;
    let roots = [lib_root()];

    let request = ModuleRequest::bareword("Foo::Bar")?;
    let module_name = request.module_name().ok_or("bareword request must carry a module name")?;

    let legacy = resolve_module_uri_with_effective_inc(
        "Foo::Bar",
        &[],
        std::slice::from_ref(&workspace_uri),
        &roots,
        Duration::from_secs(1),
    );
    let through_validated_name = resolve_module_uri_with_effective_inc(
        module_name.canonical(),
        &[],
        std::slice::from_ref(&workspace_uri),
        &roots,
        Duration::from_secs(1),
    );

    assert_eq!(
        legacy, through_validated_name,
        "routing through a validated name must not change the resolution result"
    );

    let outcome = outcome_from_uri_resolution(&legacy);
    assert!(outcome.is_resolved());
    assert!(outcome.has_complete_denominator());
    assert_eq!(
        uri_resolution_from_outcome(&outcome),
        Some(legacy),
        "the adapter round trip is lossless for a resolved search"
    );
    Ok(())
}

#[test]
fn a_legacy_spelling_resolves_to_the_same_module() -> TestResult {
    let temp = tempfile::tempdir()?;
    let workspace_uri = workspace_with_module(&temp, "Foo/Bar.pm")?;
    let roots = [lib_root()];

    let legacy_name = ModuleName::parse("Foo'Bar")?;
    let resolution = resolve_module_uri_with_effective_inc(
        legacy_name.canonical(),
        &[],
        std::slice::from_ref(&workspace_uri),
        &roots,
        Duration::from_secs(1),
    );

    assert!(
        matches!(&resolution, ModuleUriResolution::Resolved(uri) if uri.ends_with("lib/Foo/Bar.pm")),
        "the legacy spelling resolves to the same module, got {resolution:?}"
    );
    Ok(())
}

#[test]
fn a_complete_miss_is_reported_as_an_exact_absence() -> TestResult {
    let temp = tempfile::tempdir()?;
    let workspace_uri = workspace_with_module(&temp, "Foo/Bar.pm")?;
    let roots = [lib_root()];

    let resolution = resolve_module_uri_with_effective_inc(
        "Absent::Module",
        &[],
        std::slice::from_ref(&workspace_uri),
        &roots,
        Duration::from_secs(1),
    );

    assert_eq!(resolution, ModuleUriResolution::NotFound);
    let outcome = outcome_from_uri_resolution(&resolution);
    assert_eq!(outcome, ModuleResolutionOutcome::NotFound);
    assert!(
        outcome.has_complete_denominator(),
        "every authorized root was inspected, so the absence is exact"
    );
    Ok(())
}

#[test]
fn an_invalid_request_never_reaches_the_resolver() {
    // The legacy contract accepted this string and reported `NotFound`, which
    // reads as "this module does not exist" rather than "this was never a
    // module request".
    let legacy_accepts_the_string = ModuleUriResolution::NotFound;
    assert_eq!(
        outcome_from_uri_resolution(&legacy_accepts_the_string),
        ModuleResolutionOutcome::NotFound
    );

    let classified = ModuleRequest::bareword("../../etc/passwd");
    assert!(classified.is_err(), "a traversing string must never become a lookup subject");

    let outcome = classified
        .err()
        .map(ModuleResolutionOutcome::InvalidRequest)
        .unwrap_or(ModuleResolutionOutcome::NotFound);
    assert!(
        !outcome.has_complete_denominator(),
        "an invalid request has no denominator to be complete about"
    );
    assert_eq!(
        uri_resolution_from_outcome(&outcome),
        None,
        "the invalid classification must not narrow back into `NotFound`"
    );
}

#[test]
fn a_validated_name_never_derives_an_escaping_relative_path() -> Result<(), ModuleRequestError> {
    for text in ["Foo::Bar", "Foo'Bar", "strict", "C::Foo", "_Private::Util"] {
        let request = ModuleRequest::bareword(text)?;
        let Some(name) = request.module_name() else {
            continue;
        };
        let relative = module_name_to_path(name.canonical());

        assert!(relative.ends_with(".pm"));
        assert!(!relative.starts_with('/'), "{relative} must stay relative");
        assert!(
            !relative.split('/').any(|component| component == ".."),
            "{relative} must never traverse"
        );
    }
    Ok(())
}
