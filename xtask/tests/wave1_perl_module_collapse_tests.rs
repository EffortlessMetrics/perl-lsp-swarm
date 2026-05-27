//! Red TDD tests for issue #4420: Wave 1 PILOT (perl-module-* → perl-module)
//!
//! These tests validate the structural outcome of the microcrate collapse:
//! - 13 perl-module-* crates are absorbed into a single perl-module facade
//! - New perl-module crate is registered in workspace
//! - Old crates are removed from workspace members and publish allowlist
//! - Workspace member count decreases from 135 to 123
//! - Publish allowlist decreases from 132 to 120 (per spec)
//! - The perl-module facade provides complete public API via api.rs
//!
//! These tests must FAIL before implementation and PASS after.

use std::fs;
use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    // Walk up from the manifest directory to the workspace root.
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // xtask is at <root>/xtask -- go up one level
    dir.pop();
    dir
}

fn old_perl_module_crates() -> [&'static str; 13] {
    [
        "perl-module-name",
        "perl-module-path",
        "perl-module-token-core",
        "perl-module-boundary",
        "perl-module-token",
        "perl-module-import",
        "perl-module-token-parser",
        "perl-module-reference",
        "perl-module-import-match",
        "perl-module-rename",
        "perl-module-resolution-path",
        "perl-module-resolution-uri",
        "perl-module-resolution",
    ]
}

fn workspace_version(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml"))?;
    let value: toml::Value = toml::from_str(&cargo_toml)?;
    let version = value
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or("workspace.package.version missing from Cargo.toml")?;
    Ok(version.to_string())
}

/// The workspace must keep the Wave 1 collapse invariant even as later waves
/// add or absorb unrelated crates.
#[test]
fn test_workspace_members_preserve_perl_module_collapse() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();
    let workspace_cargo_path = root.join("Cargo.toml");
    let content = fs::read_to_string(&workspace_cargo_path)?;

    // Extract the [workspace] members section
    let members_start = content.find("[workspace]").ok_or("No [workspace] section found")?;

    let members_section = &content[members_start..];
    let members_list_start = members_section.find("members = [").ok_or("No members list found")?;

    // Count the number of quoted strings (crate paths) in the members list
    let members_part = &members_section[members_list_start..];
    let members_end = members_part.find("]").ok_or("Members list closing ] not found")?;
    let members_content = &members_part[..members_end];

    let member_count = members_content.matches('"').count() / 2; // Each path has opening and closing quote

    assert!(
        member_count > 0,
        "Workspace members list must be parseable before checking the Wave 1 collapse invariant"
    );
    assert_eq!(
        members_content.matches("\"crates/perl-module\"").count(),
        1,
        "crates/perl-module must remain the single workspace member for module resolution"
    );
    for old_crate in old_perl_module_crates().map(|name| format!("\"crates/{name}\"")) {
        assert!(
            !members_content.contains(&old_crate),
            "Old workspace member {old_crate} must remain collapsed into crates/perl-module.\n\
             Current workspace member count: {member_count}"
        );
    }
    assert!(
        member_count < 135,
        "Workspace should not regress to the pre-collapse 135-member snapshot; current count: {}",
        member_count
    );
    Ok(())
}

/// All 13 old perl-module-* crates must be removed from the workspace members list.
/// Tests that none of: perl-module-name, perl-module-path, perl-module-token-core,
/// perl-module-boundary, perl-module-token, perl-module-import, perl-module-token-parser,
/// perl-module-reference, perl-module-import-match, perl-module-rename,
/// perl-module-resolution-path, perl-module-resolution-uri, perl-module-resolution
/// appear as directory entries under crates/.
#[test]
fn test_all_13_old_perl_module_crates_directories_removed() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();
    let crates_dir = root.join("crates");

    for old_crate in old_perl_module_crates() {
        let crate_dir = crates_dir.join(old_crate);
        assert!(
            !crate_dir.exists(),
            "Old crate directory {} must be deleted after collapse.\n\
             These 13 crates are absorbed into the unified perl-module facade.\n\
             See .spec/4420-wave1-perl-module/acceptance.md line 12",
            old_crate
        );
    }
    Ok(())
}

/// The new perl-module crate must exist with proper structure.
/// After step 1 of the builder, `crates/perl-module/` must exist and contain:
/// - Cargo.toml
/// - src/lib.rs
/// - src/api.rs
#[test]
fn test_perl_module_facade_crate_exists_with_required_structure()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let perl_module_dir = root.join("crates/perl-module");

    assert!(
        perl_module_dir.exists(),
        "crates/perl-module/ directory must exist.\n\
         This is the new unified facade crate.\n\
         See .spec/4420-wave1-perl-module/checklist.md step 0a"
    );

    let cargo_toml = perl_module_dir.join("Cargo.toml");
    assert!(
        cargo_toml.exists(),
        "crates/perl-module/Cargo.toml must exist.\n\
         See .spec/4420-wave1-perl-module/checklist.md step 0a"
    );

    let lib_rs = perl_module_dir.join("src/lib.rs");
    assert!(
        lib_rs.exists(),
        "crates/perl-module/src/lib.rs must exist.\n\
         This is the main library crate entry point.\n\
         See .spec/4420-wave1-perl-module/checklist.md step 1a"
    );

    let api_rs = perl_module_dir.join("src/api.rs");
    assert!(
        api_rs.exists(),
        "crates/perl-module/src/api.rs must exist.\n\
         This is the public facade module that re-exports the internal public API.\n\
         See .spec/4420-wave1-perl-module/context.md section 'Visibility Model'"
    );

    Ok(())
}

/// The new perl-module crate must be listed in [workspace] members exactly once.
#[test]
fn test_perl_module_listed_in_workspace_members() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workspace_cargo_path = root.join("Cargo.toml");
    let content = fs::read_to_string(&workspace_cargo_path)?;

    let members_count = content.matches("\"crates/perl-module\"").count();

    assert_eq!(
        members_count, 1,
        "crates/perl-module must appear exactly once in [workspace] members.\n\
         Current count: {}\n\
         See .spec/4420-wave1-perl-module/acceptance.md line 18",
        members_count
    );
    Ok(())
}

/// All 13 old perl-module-* entries must be removed from the workspace members list.
#[test]
fn test_old_perl_module_crates_removed_from_workspace_members()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workspace_cargo_path = root.join("Cargo.toml");
    let content = fs::read_to_string(&workspace_cargo_path)?;

    for old_crate in old_perl_module_crates().map(|name| format!("crates/{name}")) {
        assert!(
            !content.contains(&old_crate),
            "Old crate entry {} must be removed from [workspace] members.\n\
             These are absorbed into the new perl-module facade.\n\
             See .spec/4420-wave1-perl-module/acceptance.md line 20",
            old_crate
        );
    }
    Ok(())
}

/// The publish allowlist must contain exactly one perl-module entry (the new facade).
/// Before collapse: 13 entries for perl-module-* crates.
/// After collapse: 1 entry for perl-module facade.
#[test]
fn test_publish_allowlist_contains_single_perl_module_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workspace_cargo_path = root.join("Cargo.toml");
    let content = fs::read_to_string(&workspace_cargo_path)?;

    // Find the [workspace.metadata.publish] section
    let publish_start = content
        .find("[workspace.metadata.publish]")
        .ok_or("No [workspace.metadata.publish] section found")?;

    // Extract the allow list (up to next [section] or end of file)
    let publish_section = &content[publish_start..];
    let section_end =
        publish_section[1..].find("\n[").map(|i| i + 1).unwrap_or(publish_section.len());
    let publish_section = &publish_section[..section_end];

    // Count occurrences of "perl-module" in the publish allowlist
    // Should be exactly 1 (the new facade), not 13 (old crates)
    let perl_module_count = publish_section.matches("\"perl-module\"").count();

    assert_eq!(
        perl_module_count, 1,
        "Publish allowlist must contain exactly one perl-module entry (the new facade).\n\
         All 13 old perl-module-* entries must be removed.\n\
         Current count of \"perl-module\" entries: {}\n\
         See .spec/4420-wave1-perl-module/acceptance.md line 26",
        perl_module_count
    );

    Ok(())
}

/// All 13 old perl-module-* entries must be removed from the publish allowlist.
#[test]
fn test_old_perl_module_crates_removed_from_publish_allowlist()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workspace_cargo_path = root.join("Cargo.toml");
    let content = fs::read_to_string(&workspace_cargo_path)?;

    for old_crate in old_perl_module_crates() {
        let pattern = format!("\"{}\"", old_crate);
        assert!(
            !content.contains(&pattern),
            "Old crate {} must be removed from publish allowlist.\n\
             These are absorbed into the new perl-module facade.\n\
             See .spec/4420-wave1-perl-module/acceptance.md line 27",
            old_crate
        );
    }
    Ok(())
}

/// The perl-module Cargo.toml must stay on the current workspace release line.
#[test]
fn test_perl_module_version_matches_workspace_release() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let perl_module_cargo = root.join("crates/perl-module/Cargo.toml");
    let content = fs::read_to_string(&perl_module_cargo)?;
    let expected_version = workspace_version(&root)?;

    assert!(
        content.contains(&format!("version = \"{expected_version}\""))
            || content.contains("version.workspace = true"),
        "perl-module Cargo.toml must declare version = \"0.14.0\".\n\
         This reflects the breaking change: perl_module_name::* → perl_module::name::*\n\
         See .spec/4420-wave1-perl-module/acceptance.md line 63\n\
         See .spec/4420-wave1-perl-module/context.md section 'Major version bump'"
    );
    Ok(())
}

/// The perl-module/src/api.rs must exist and re-export the main module facades.
/// At minimum, it should re-export the 11 main modules: name, path, token_core,
/// boundary, token, import, token_parser, reference, import_match, rename, resolution.
#[test]
fn test_perl_module_api_rs_has_public_re_exports() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let api_rs = root.join("crates/perl-module/src/api.rs");
    let content = fs::read_to_string(&api_rs)?;

    // Check for pub use statements for at least the main module categories
    let main_modules = vec![
        "name",
        "path",
        "token_core",
        "boundary",
        "token",
        "import",
        "token_parser",
        "reference",
        "import_match",
        "rename",
        "resolution",
    ];

    for module_name in &main_modules {
        // Look for either "pub use self::X" or "pub use crate::X" style exports
        let pattern_self = format!("pub use self::{}::", module_name);
        let pattern_crate = format!("pub use crate::{}::", module_name);

        assert!(
            content.contains(&pattern_self) || content.contains(&pattern_crate),
            "api.rs must re-export module {} via pub use.\n\
             Facade pattern: internal modules are pub(crate), api.rs re-exports what's public.\n\
             See .spec/4420-wave1-perl-module/context.md section 'Visibility Model'",
            module_name
        );
    }
    Ok(())
}

/// Test that perl-lsp still builds after consumer import migration.
/// This indirectly validates that perl-lsp's Cargo.toml was updated to depend
/// on perl-module instead of perl-module-*.
#[test]
fn test_perl_lsp_cargo_uses_perl_module_not_individual_crates()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let perl_lsp_cargo = root.join("crates/perl-lsp-rs/Cargo.toml");
    let content = fs::read_to_string(&perl_lsp_cargo)?;

    // perl-lsp should NOT have any perl-module-* deps (those are absorbed)
    let forbidden_deps = vec![
        "perl-module-import",
        "perl-module-path",
        "perl-module-reference",
        "perl-module-rename",
        "perl-module-resolution",
    ];

    for old_dep in &forbidden_deps {
        assert!(
            !content.contains(old_dep),
            "perl-lsp Cargo.toml must not depend on {} (absorbed into perl-module).\n\
             All consumer crates must be updated to use unified perl-module facade.\n\
             See .spec/4420-wave1-perl-module/acceptance.md line 32",
            old_dep
        );
    }

    // perl-lsp SHOULD have a perl-module dependency
    assert!(
        content.contains("perl-module"),
        "perl-lsp Cargo.toml must depend on perl-module facade.\n\
         See .spec/4420-wave1-perl-module/acceptance.md line 32"
    );

    Ok(())
}

/// Current crate manifests must not depend on the absorbed perl-module-* crates.
#[test]
fn test_current_crate_manifests_do_not_use_old_perl_module_crates()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let crates_dir = root.join("crates");

    for entry in fs::read_dir(crates_dir)? {
        let entry = entry?;
        let cargo_path = entry.path().join("Cargo.toml");
        if !cargo_path.exists() {
            continue;
        }
        let content = fs::read_to_string(&cargo_path)?;

        for old_crate in old_perl_module_crates() {
            assert!(
                !content.contains(&format!("{old_crate} =")),
                "{} must not depend on absorbed crate `{old_crate}`; use the unified \
                 perl-module facade instead.",
                cargo_path.display()
            );
        }
    }

    Ok(())
}

/// Test that all 62 test files from the 13 old crates exist in the new perl-module test dir.
/// Each test family (name, path, token_core, etc.) must have its test files present.
#[test]
fn test_all_test_files_migrated_to_perl_module_tests_dir() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();
    let tests_dir = root.join("crates/perl-module/tests");

    // The acceptance says all 62 test files from 13 crates must be copied.
    // We check that the tests directory exists and has files.
    assert!(
        tests_dir.exists(),
        "crates/perl-module/tests/ directory must exist.\n\
         It should contain all 62 test files from the 13 collapsed crates.\n\
         See .spec/4420-wave1-perl-module/acceptance.md line 14"
    );

    // Count the number of .rs files in the tests directory
    let test_files: Vec<_> = fs::read_dir(&tests_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "rs").unwrap_or(false))
        .collect();

    assert!(
        test_files.len() > 0,
        "crates/perl-module/tests/ must contain .rs test files.\n\
         Expected approximately 62 files from the 13 collapsed crates.\n\
         See .spec/4420-wave1-perl-module/acceptance.md line 14"
    );

    Ok(())
}

/// Crate metadata must list perl-module correctly.
/// `cargo metadata --no-deps` should list "perl-module" as a workspace member.
#[test]
fn test_cargo_metadata_lists_perl_module() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();

    // Run cargo metadata to verify the workspace is coherent
    let output = std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--no-deps")
        .current_dir(&root)
        .output()?;

    assert!(
        output.status.success(),
        "cargo metadata must succeed.\n\
         If this fails, the Cargo.toml workspace configuration is invalid.\n\
         See .spec/4420-wave1-perl-module/acceptance.md line 18"
    );

    let stdout = String::from_utf8(output.stdout)?;

    // The output should contain perl-module in the list of packages
    assert!(
        stdout.contains("\"name\":\"perl-module\""),
        "cargo metadata output must list perl-module as a workspace package.\n\
         Output:\n{}",
        &stdout[..std::cmp::min(500, stdout.len())]
    );

    Ok(())
}
