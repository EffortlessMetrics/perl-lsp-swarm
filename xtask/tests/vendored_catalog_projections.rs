//! Discriminating edge-case tests for vendored feature-catalog projections (#9199).
//!
//! These fixtures exercise the landed #9198 byte-projection generator and the
//! checker without touching the live workspace tree. A passing live-tree check
//! alone cannot distinguish "inventory+checker work" from "the four copies
//! happen to match today".

use std::error::Error;
use std::fs;
use std::path::Path;

use perl_lsp_rs_core::feature_catalog::{Catalog, Maturity};
use tempfile::TempDir;
use xtask::utils::project_root;
use xtask::vendored_catalog::{
    AUTHORITY_RELATIVE, DECLARED_PROJECTIONS, GENERATED_MARKER, ProjectionClass, ViolationCode,
    check, regenerate,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const MINIMAL_AUTHORITY: &str = r#"# LSP/DAP Feature & Evidence Catalog - AUTHORITY
# Crate-local `features_sot.toml` files are DETERMINISTIC GENERATED PROJECTIONS of this file.

[meta]
version = "0.0.0-test"
lsp_version = "3.18"

[[feature]]
id = "lsp.completion"
maturity = "proven"
advertised = true
area = "text_document"
policy_class = "request_response"
direction = "client_to_server"
capability_gate = "completionProvider"
registration = "static_capabilities"
implementation_owner = "crates/perl-lsp-rs/src/completion.rs"
state_owner = "workspace_index"
claim_boundary = "fixture"
tests = ["crates/perl-lsp-rs/tests/lsp_completion_tests.rs"]
evidence = [{ class = "integration_test", id = "crates/perl-lsp-rs/tests/lsp_completion_tests.rs" }]
description = "completion"

[[feature]]
id = "dap.core"
maturity = "preview"
advertised = true
area = "debug"
description = "preview row"

[[feature]]
id = "lsp.planned"
maturity = "planned"
advertised = false
area = "text_document"
description = "planned row"

[[feature]]
id = "lsp.unsupported"
maturity = "unsupported"
advertised = false
area = "text_document"
description = "unsupported row"

[[feature]]
id = "lsp.not_proven"
maturity = "not_proven"
advertised = true
counts_in_coverage = false
area = "text_document"
description = "not proven row"
"#;

fn fixture_root() -> TestResult<TempDir> {
    Ok(TempDir::new()?)
}

fn write_authority(root: &Path, body: &str) -> TestResult {
    fs::write(root.join(AUTHORITY_RELATIVE), body)?;
    Ok(())
}

fn write_projection(root: &Path, relative: &str, body: &str) -> TestResult {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, body)?;
    Ok(())
}

fn write_all_declared(root: &Path, body: &str) -> TestResult {
    for projection in DECLARED_PROJECTIONS {
        write_projection(root, projection.relative_path, body)?;
    }
    Ok(())
}

fn codes_for(root: &Path) -> Vec<ViolationCode> {
    check(root).into_iter().map(|violation| violation.code).collect()
}

fn has_code_on(root: &Path, code: ViolationCode, relative: &str) -> bool {
    check(root).into_iter().any(|violation| violation.code == code && violation.path == relative)
}

fn parse_catalog(bytes: &[u8]) -> TestResult<Catalog> {
    let text = std::str::from_utf8(bytes)?;
    Ok(toml::from_str(text)?)
}

fn maturity_of(catalog: &Catalog, id: &str) -> TestResult<Maturity> {
    catalog
        .feature
        .iter()
        .find(|feature| feature.id == id)
        .map(|feature| feature.maturity)
        .ok_or_else(|| format!("missing feature {id}").into())
}

#[test]
fn inventory_declares_exactly_the_four_full_catalog_packages() {
    assert_eq!(DECLARED_PROJECTIONS.len(), 4);
    let mut packages = Vec::new();
    let mut paths = Vec::new();
    for projection in DECLARED_PROJECTIONS {
        assert_eq!(projection.class, ProjectionClass::FullCatalog);
        packages.push(projection.package);
        paths.push(projection.relative_path);
    }
    packages.sort_unstable();
    packages.dedup();
    paths.sort_unstable();
    paths.dedup();
    assert_eq!(packages.len(), 4, "package names must be unique");
    assert_eq!(paths.len(), 4, "projection paths must be unique");
    assert!(packages.contains(&"perl-dap"));
    assert!(packages.contains(&"perl-parser"));
    assert!(packages.contains(&"perl-lsp-rs"));
    assert!(packages.contains(&"perl-lsp-rs-core"));
}

#[test]
fn regenerate_writes_byte_identical_full_catalog_projections() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;

    let report = regenerate(root)?;
    assert_eq!(report.files.len(), 4);
    assert!(report.any_updated(), "missing projections must be written");

    let authority = fs::read(root.join(AUTHORITY_RELATIVE))?;
    for file in &report.files {
        let bytes = fs::read(root.join(&file.relative_path))?;
        assert_eq!(bytes, authority, "{}", file.relative_path);
        assert!(file.updated);
    }
    Ok(())
}

#[test]
fn second_regenerate_is_clean() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;

    let before: Vec<(String, Vec<u8>)> = DECLARED_PROJECTIONS
        .iter()
        .map(|projection| {
            let bytes = fs::read(root.join(projection.relative_path))?;
            Ok((projection.relative_path.to_string(), bytes))
        })
        .collect::<TestResult<Vec<_>>>()?;

    let report = regenerate(root)?;
    assert!(!report.any_updated(), "second run must not rewrite matching projections");
    for (relative, bytes) in before {
        assert_eq!(fs::read(root.join(&relative))?, bytes, "{relative} changed on second run");
    }
    assert!(
        codes_for(root).is_empty(),
        "second-run-clean tree must check clean: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn regenerate_preserves_preview_not_proven_and_unsupported_maturity() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;

    let authority = parse_catalog(&fs::read(root.join(AUTHORITY_RELATIVE))?)?;
    for projection in DECLARED_PROJECTIONS {
        let catalog = parse_catalog(&fs::read(root.join(projection.relative_path))?)?;
        assert_eq!(maturity_of(&catalog, "dap.core")?, Maturity::Preview);
        assert_eq!(maturity_of(&catalog, "lsp.not_proven")?, Maturity::NotProven);
        assert_eq!(maturity_of(&catalog, "lsp.unsupported")?, Maturity::Unsupported);
        assert_eq!(maturity_of(&catalog, "lsp.planned")?, Maturity::Planned);
        assert_eq!(maturity_of(&catalog, "dap.core")?, maturity_of(&authority, "dap.core")?);
    }
    Ok(())
}

#[test]
fn manual_maturity_edit_fails_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;

    let target = DECLARED_PROJECTIONS[0].relative_path;
    let mutated = MINIMAL_AUTHORITY.replace("maturity = \"preview\"", "maturity = \"proven\"");
    write_projection(root, target, &mutated)?;

    assert!(
        has_code_on(root, ViolationCode::VendoredDrift, target),
        "hand-edited maturity must not pass: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn stale_authority_bytes_fail_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;

    let target = DECLARED_PROJECTIONS[1].relative_path;
    let stale = MINIMAL_AUTHORITY.replace("version = \"0.0.0-test\"", "version = \"0.0.0-old\"");
    write_projection(root, target, &stale)?;

    assert!(
        has_code_on(root, ViolationCode::VendoredDrift, target),
        "old-root bytes must not pass: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn undeclared_feature_subset_fails_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;

    let target = DECLARED_PROJECTIONS[2].relative_path;
    let subset = MINIMAL_AUTHORITY.replace(
        r#"
[[feature]]
id = "dap.core"
maturity = "preview"
advertised = true
area = "debug"
description = "preview row"
"#,
        "",
    );
    write_projection(root, target, &subset)?;

    assert!(
        has_code_on(root, ViolationCode::UndeclaredSubset, target),
        "undeclared subset must not pass as FullCatalog: {:?}",
        check(root)
    );
    assert!(
        !has_code_on(root, ViolationCode::VendoredDrift, target),
        "subset should be classified, not generic drift: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn missing_declared_projection_fails_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;

    let target = DECLARED_PROJECTIONS[3].relative_path;
    fs::remove_file(root.join(target))?;

    assert!(
        has_code_on(root, ViolationCode::MissingProjection, target),
        "missing declared fallback must not pass: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn undeclared_extra_fallback_fails_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;
    write_projection(root, "crates/perl-lexer/features_sot.toml", MINIMAL_AUTHORITY)?;

    assert!(
        has_code_on(
            root,
            ViolationCode::UndeclaredProjection,
            "crates/perl-lexer/features_sot.toml"
        ),
        "undeclared extra fallback must not pass: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn absorbed_proof_path_in_a_fallback_fails_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;

    let target = DECLARED_PROJECTIONS[0].relative_path;
    let mutated = MINIMAL_AUTHORITY.replace(
        "crates/perl-lsp-rs/tests/lsp_completion_tests.rs",
        "crates/perl-feature-catalog/src/lib.rs",
    );
    write_projection(root, target, &mutated)?;

    assert!(
        has_code_on(root, ViolationCode::StaleProofPath, target),
        "absorbed crate proof path must fail: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn bridge_era_feature_id_in_a_fallback_fails_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_authority(root, MINIMAL_AUTHORITY)?;
    regenerate(root)?;

    let target = "crates/perl-dap/features_sot.toml";
    let mutated = format!("{MINIMAL_AUTHORITY}\nid = \"legacy-pls-bridge\"\n");
    write_projection(root, target, &mutated)?;

    assert!(
        has_code_on(root, ViolationCode::StaleProofPath, target),
        "bridge-era id must fail: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn authority_without_generated_marker_fails_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    let unmarked = MINIMAL_AUTHORITY.replace("GENERATED PROJECTIONS", "hand-edited copies");
    write_authority(root, &unmarked)?;
    write_all_declared(root, &unmarked)?;

    assert!(
        has_code_on(root, ViolationCode::MissingGeneratedMarker, AUTHORITY_RELATIVE),
        "missing generated marker must fail: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn missing_authority_fails_closed() -> TestResult {
    let temp = fixture_root()?;
    let root = temp.path();
    write_all_declared(root, MINIMAL_AUTHORITY)?;

    assert!(
        codes_for(root).contains(&ViolationCode::MissingAuthority),
        "missing root authority must fail: {:?}",
        check(root)
    );
    Ok(())
}

#[test]
fn live_workspace_matches_declared_inventory_after_generation_contract() -> TestResult {
    // Production-path trace: the real tree must satisfy the same checker the
    // fixtures use. Drift on origin/main is a candidate defect, not a skipped case.
    let root = project_root()?;
    let violations = check(&root);
    assert!(
        violations.is_empty(),
        "live workspace vendored projections are not clean (#9199): {violations:?}"
    );

    let mut found = Vec::new();
    for entry in fs::read_dir(root.join("crates"))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            format!("crate directory name is not UTF-8: {}", entry.file_name().to_string_lossy())
        })?;
        let relative = format!("crates/{name}/features_sot.toml");
        if root.join(&relative).is_file() {
            found.push(relative);
        }
    }
    found.sort();
    let mut declared: Vec<String> = DECLARED_PROJECTIONS
        .iter()
        .map(|projection| projection.relative_path.to_string())
        .collect();
    declared.sort();
    assert_eq!(found, declared, "live crate-local fallbacks must equal the declared inventory");

    let authority = fs::read(root.join(AUTHORITY_RELATIVE))?;
    assert!(
        std::str::from_utf8(&authority)?.contains(GENERATED_MARKER),
        "live authority must declare generated projections"
    );
    Ok(())
}

#[test]
fn display_line_keeps_historical_vendored_drift_wording() {
    let line = xtask::vendored_catalog::ProjectionViolation {
        code: ViolationCode::VendoredDrift,
        path: "crates/perl-lsp-rs-core/features_sot.toml".to_string(),
        detail: String::new(),
    }
    .display_line();
    assert!(line.starts_with("VENDORED_DRIFT:"));
    assert!(line.contains("cargo xtask features regen-vendored"));
}
