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
    assert!(
        !outcome.has_complete_denominator(),
        "the legacy enum carries no completeness signal, so even a successful \
         search cannot prove no higher-precedence root was skipped"
    );
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
fn a_legacy_miss_is_not_widened_into_a_proven_absence() -> TestResult {
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
    assert_eq!(outcome, ModuleResolutionOutcome::NotProvenAbsent);
    assert!(
        !outcome.has_complete_denominator(),
        "the three-state resolver cannot prove it inspected every authorized root"
    );
    Ok(())
}

/// Regression: the resolver reports a clean `NotFound` for a module that exists,
/// because the root holding it was skipped without any completeness signal.
///
/// `full_path_for_root` returns `None` when `validate_workspace_path` rejects the
/// joined path, and `collect_module_uri_candidates` simply `continue`s. Nothing
/// records that a configured root went uninspected, so `ModuleUriResolution::NotFound`
/// cannot mean "proven absent" — which is why `outcome_from_uri_resolution` widens
/// it to `NotProvenAbsent`.
#[test]
fn a_root_skipped_by_boundary_validation_still_reports_not_found() -> TestResult {
    let temp = tempfile::tempdir()?;
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;

    // The module really is there — under a root that escapes the workspace.
    let escaped = temp.path().join("escape");
    std::fs::create_dir_all(escaped.join("Foo"))?;
    std::fs::write(escaped.join("Foo/Bar.pm"), "package Foo::Bar; 1;")?;

    let workspace_uri = url::Url::from_directory_path(&workspace)
        .map_err(|()| "failed to build workspace URI")?
        .to_string();
    let roots = [IncRoot {
        kind: IncRootKind::WorkspaceRelative,
        path: std::path::PathBuf::from("../escape"),
        precedence: 0,
        source: "workspace-include-paths".to_string(),
    }];

    let resolution = resolve_module_uri_with_effective_inc(
        "Foo::Bar",
        &[],
        std::slice::from_ref(&workspace_uri),
        &roots,
        Duration::from_secs(5),
    );

    assert_eq!(
        resolution,
        ModuleUriResolution::NotFound,
        "the skipped root is indistinguishable from a clean miss in the legacy enum"
    );
    assert!(
        !outcome_from_uri_resolution(&resolution).has_complete_denominator(),
        "so the widened outcome must not claim a complete denominator"
    );
    Ok(())
}

/// Regression: a match returned from a *lower*-precedence root is not provably
/// the precedence winner, because a higher-precedence root can be skipped
/// without any completeness signal.
///
/// `collect_module_uri_candidates` sorts roots with `sort_by_key(|r| r.precedence)`
/// and then `continue`s past any root whose joined path fails workspace-boundary
/// validation. So precedence 0 here never gets inspected, precedence 1 supplies
/// the answer, and the legacy enum reports a bare `Resolved` that looks exact.
/// That is why `outcome_from_uri_resolution` widens it to `NotProvenPrecedence`.
#[test]
fn a_win_behind_a_skipped_higher_precedence_root_is_not_proven_exact() -> TestResult {
    let temp = tempfile::tempdir()?;
    let workspace = temp.path().join("workspace");

    // The higher-precedence root escapes the workspace, so it is skipped.
    let escaped = temp.path().join("escape");
    std::fs::create_dir_all(escaped.join("Foo"))?;
    std::fs::write(escaped.join("Foo/Bar.pm"), "package Foo::Bar; 1; # higher precedence")?;

    // The lower-precedence root is legitimate and holds the same module.
    let lib = workspace.join("lib").join("Foo");
    std::fs::create_dir_all(&lib)?;
    std::fs::write(lib.join("Bar.pm"), "package Foo::Bar; 1; # lower precedence")?;

    let workspace_uri = url::Url::from_directory_path(&workspace)
        .map_err(|()| "failed to build workspace URI")?
        .to_string();
    let roots = [
        IncRoot {
            kind: IncRootKind::WorkspaceRelative,
            path: std::path::PathBuf::from("../escape"),
            precedence: 0,
            source: "workspace-include-paths".to_string(),
        },
        IncRoot {
            kind: IncRootKind::WorkspaceRelative,
            path: std::path::PathBuf::from("lib"),
            precedence: 1,
            source: "workspace-include-paths".to_string(),
        },
    ];

    let resolution = resolve_module_uri_with_effective_inc(
        "Foo::Bar",
        &[],
        std::slice::from_ref(&workspace_uri),
        &roots,
        Duration::from_secs(5),
    );

    // The legacy enum cannot express that the winner is unproven.
    assert!(
        matches!(&resolution, ModuleUriResolution::Resolved(uri) if uri.ends_with("lib/Foo/Bar.pm")),
        "the lower-precedence root supplies the answer, got {resolution:?}"
    );

    let outcome = outcome_from_uri_resolution(&resolution);
    assert!(outcome.is_resolved(), "navigation must still work — a real file was found");
    assert_eq!(
        outcome.resolved_uri(),
        Some(&resolution).and_then(|r| match r {
            ModuleUriResolution::Resolved(uri) => Some(uri.as_str()),
            _ => None,
        }),
        "the URI must pass through unchanged; this adapter moves no resolution decision"
    );
    assert!(
        !outcome.has_complete_denominator(),
        "but precedence 0 was skipped, not searched, so the win is not proven exact"
    );
    assert_eq!(outcome.boundary_id(), "module_resolution.not_proven_precedence");
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
        ModuleResolutionOutcome::NotProvenAbsent
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
