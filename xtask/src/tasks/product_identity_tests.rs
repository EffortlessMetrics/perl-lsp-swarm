use super::{
    RepositoryContext, check_with_repository_context, check_with_resolved_repository_context,
    parse_github_repository,
};
use color_eyre::eyre::{Result, bail};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONTRACT: &str = r#"
schema_version = 1

[product]
name = "perl-lsp"
public_repository = "EffortlessMetrics/perl-lsp"
development_repository = "EffortlessMetrics/perl-lsp-swarm"

[server]
primary_executable = "perllsp"
cargo_package = "perllsp"
package_manifest = "crates/perllsp/Cargo.toml"
implementation_crate = "perl-lsp-rs"
implementation_manifest = "crates/perl-lsp-rs/Cargo.toml"
compatibility_executable = "perl-lsp"

[extension]
publisher = "EffortlessMetrics"
package_name = "perl-lsp-rs"
id = "EffortlessMetrics.perl-lsp-rs"
display_name = "Perl Language Server (perl-lsp)"
package_manifest = "vscode-extension/package.json"

[debug_adapter]
executable = "perl-dap"
cargo_package = "perl-dap"
package_manifest = "crates/perl-dap/Cargo.toml"
maturity = "preview"

[[conflicts]]
identity = "crates.io/perl-lsp"
relation = "different_project"
remediation = "Install perllsp."
"#;

#[test]
fn coherent_identity_contract_passes_in_both_declared_repositories() -> Result<()> {
    let repo = fixture_repo()?;
    check_with_repository_context(repo.path(), Some("EffortlessMetrics/perl-lsp-swarm"))?;
    check_with_repository_context(repo.path(), Some("EffortlessMetrics/perl-lsp"))
}

#[test]
fn inferred_undeclared_origin_is_an_unbound_local_checkout() -> Result<()> {
    let repo = fixture_repo()?;
    let context =
        RepositoryContext { repository: "fork-owner/perl-lsp".to_string(), authoritative: false };

    check_with_resolved_repository_context(repo.path(), Some(&context))
}

#[test]
fn unsupported_schema_version_fails() -> Result<()> {
    let repo = fixture_repo()?;
    let contract = CONTRACT.replace("schema_version = 1", "schema_version = 2");
    write(repo.path(), "policy/product-identity.toml", &contract)?;

    expect_failure(repo.path(), "unsupported product identity schema version")
}

#[test]
fn unknown_contract_field_fails() -> Result<()> {
    let repo = fixture_repo()?;
    let contract =
        CONTRACT.replace("schema_version = 1", "schema_version = 1\nunknown_contract_field = true");
    write(repo.path(), "policy/product-identity.toml", &contract)?;

    expect_failure(repo.path(), "unknown field")
}

#[test]
fn escaping_manifest_path_fails() -> Result<()> {
    let repo = fixture_repo()?;
    let contract = CONTRACT.replace(
        "package_manifest = \"crates/perl-dap/Cargo.toml\"",
        "package_manifest = \"../outside/Cargo.toml\"",
    );
    write(repo.path(), "policy/product-identity.toml", &contract)?;

    expect_failure(repo.path(), "canonical repository-relative syntax")
}

#[test]
fn absolute_manifest_path_fails() -> Result<()> {
    let repo = fixture_repo()?;
    let contract = CONTRACT.replace(
        "package_manifest = \"crates/perl-dap/Cargo.toml\"",
        "package_manifest = \"/outside/Cargo.toml\"",
    );
    write(repo.path(), "policy/product-identity.toml", &contract)?;

    expect_failure(repo.path(), "canonical repository-relative syntax")
}

#[test]
fn missing_product_conflict_fails() -> Result<()> {
    let repo = fixture_repo()?;
    let conflict = r#"
[[conflicts]]
identity = "crates.io/perl-lsp"
relation = "different_project"
remediation = "Install perllsp."
"#;
    let contract = CONTRACT.replace(conflict, "\n");
    write(repo.path(), "policy/product-identity.toml", &contract)?;

    expect_failure(repo.path(), "must classify \"crates.io/perl-lsp\"")
}

#[test]
fn duplicate_conflict_identity_fails() -> Result<()> {
    let repo = fixture_repo()?;
    let contract = format!(
        "{CONTRACT}\n[[conflicts]]\nidentity = \"crates.io/perl-lsp\"\n\
relation = \"different_project\"\nremediation = \"Do not install it.\"\n"
    );
    write(repo.path(), "policy/product-identity.toml", &contract)?;

    expect_failure(repo.path(), "duplicate product identity conflict")
}

#[test]
fn primary_package_cannot_be_a_conflict() -> Result<()> {
    let repo = fixture_repo()?;
    let contract = format!(
        "{CONTRACT}\n[[conflicts]]\nidentity = \"crates.io/perllsp\"\n\
relation = \"different_project\"\nremediation = \"Invalid conflict.\"\n"
    );
    write(repo.path(), "policy/product-identity.toml", &contract)?;

    expect_failure(repo.path(), "cannot also be classified as an identity conflict")
}

#[test]
fn workspace_repository_drift_fails() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "Cargo.toml",
        r#"[workspace.package]
repository = "https://github.com/other/project"

[workspace.dependencies]
perl-lsp-rs = { path = "crates/perl-lsp-rs" }
"#,
    )?;

    expect_failure(repo.path(), "workspace repository drifted")
}

#[test]
fn implementation_package_drift_fails() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perl-lsp-rs/Cargo.toml",
        r#"[package]
name = "different-implementation"
repository.workspace = true

[[bin]]
name = "perl-lsp"
"#,
    )?;

    expect_failure(repo.path(), "server implementation Cargo package drifted")
}

#[test]
fn debug_adapter_binary_drift_fails() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perl-dap/Cargo.toml",
        r#"[package]
name = "perl-dap"
repository.workspace = true
autobins = false

[[bin]]
name = "wrong-dap"
"#,
    )?;

    expect_failure(repo.path(), "does not expose binary \"perl-dap\"")
}

#[test]
fn extension_identity_drift_fails() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "vscode-extension/package.json",
        r#"{
  "name": "wrong-extension",
  "displayName": "Perl Language Server (perl-lsp)",
  "publisher": "EffortlessMetrics",
  "repository": {"url": "https://github.com/EffortlessMetrics/perl-lsp"}
}"#,
    )?;

    expect_failure(repo.path(), "extension package name drifted")
}

#[test]
fn package_repository_override_fails() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository = "https://github.com/other/project"

[[bin]]
name = "perllsp"

[dependencies]
perl-lsp-rs = { workspace = true }
"#,
    )?;

    expect_failure(repo.path(), "primary server package repository drifted")
}

#[test]
fn undeclared_repository_context_fails() -> Result<()> {
    let repo = fixture_repo()?;
    let error = match check_with_repository_context(repo.path(), Some("other/project")) {
        Ok(()) => bail!("undeclared repository context should fail"),
        Err(error) => error,
    };
    if !format!("{error:#}").contains("is not declared as either public") {
        bail!("unexpected error: {error:#}");
    }
    Ok(())
}

#[test]
fn identical_public_and_development_repositories_fail() -> Result<()> {
    let repo = fixture_repo()?;
    let contract = CONTRACT.replace(
        "development_repository = \"EffortlessMetrics/perl-lsp-swarm\"",
        "development_repository = \"EffortlessMetrics/perl-lsp\"",
    );
    write(repo.path(), "policy/product-identity.toml", &contract)?;

    expect_failure(repo.path(), "public and development repositories must remain distinct")
}

#[test]
fn missing_facade_dependency_fails() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
serde = "1"
"#,
    )?;

    expect_failure(repo.path(), "does not depend on declared implementation crate")
}

#[test]
fn same_named_dependency_alias_cannot_hide_another_package() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
perl-lsp-rs = { package = "different-project", version = "1" }
"#,
    )?;

    expect_failure(repo.path(), "does not depend on declared implementation crate")
}

#[test]
fn same_named_registry_dependency_is_not_the_governed_implementation() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
perl-lsp-rs = "999"
"#,
    )?;

    expect_failure(repo.path(), "governed in-tree workspace/path source")
}

#[test]
fn workspace_dependency_retarget_is_not_the_governed_implementation() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "Cargo.toml",
        r#"[workspace.package]
repository = "https://github.com/EffortlessMetrics/perl-lsp"

[workspace.dependencies]
perl-lsp-rs = "999"
"#,
    )?;

    expect_failure(repo.path(), "governed in-tree workspace/path source")
}

#[test]
fn workspace_alias_to_in_tree_implementation_is_accepted() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "Cargo.toml",
        r#"[workspace.package]
repository = "https://github.com/EffortlessMetrics/perl-lsp"

[workspace.dependencies]
server_impl = { package = "perl-lsp-rs", path = "crates/perl-lsp-rs" }
"#,
    )?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
server_impl = { workspace = true }
"#,
    )?;

    check_with_repository_context(repo.path(), Some("EffortlessMetrics/perl-lsp-swarm"))
}

#[test]
fn direct_in_tree_path_dependency_is_accepted() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
perl-lsp-rs = { path = "../perl-lsp-rs" }
"#,
    )?;

    check_with_repository_context(repo.path(), Some("EffortlessMetrics/perl-lsp-swarm"))
}

#[test]
fn implicit_primary_binary_is_accepted() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository.workspace = true

[dependencies]
perl-lsp-rs = { workspace = true }
"#,
    )?;

    check_with_repository_context(repo.path(), Some("EffortlessMetrics/perl-lsp-swarm"))
}

#[test]
fn missing_primary_binary_fails() -> Result<()> {
    let repo = fixture_repo()?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository.workspace = true
autobins = false

[[bin]]
name = "wrong"

[dependencies]
perl-lsp-rs = { workspace = true }
"#,
    )?;

    expect_failure(repo.path(), "does not expose binary \"perllsp\"")
}

#[test]
fn github_remote_forms_resolve_to_repository_slug() {
    for remote in [
        "https://github.com/EffortlessMetrics/perl-lsp-swarm.git",
        "ssh://git@github.com/EffortlessMetrics/perl-lsp-swarm.git",
        "git@github.com:EffortlessMetrics/perl-lsp-swarm.git",
    ] {
        assert_eq!(
            parse_github_repository(remote).as_deref(),
            Some("EffortlessMetrics/perl-lsp-swarm")
        );
    }
}

fn expect_failure(repo: &Path, expected: &str) -> Result<()> {
    let error = match check_with_repository_context(repo, Some("EffortlessMetrics/perl-lsp-swarm"))
    {
        Ok(()) => bail!("identity drift should fail"),
        Err(error) => error,
    };
    if !format!("{error:#}").contains(expected) {
        bail!("unexpected error: {error:#}");
    }
    Ok(())
}

fn fixture_repo() -> Result<TempDir> {
    let repo = TempDir::new()?;
    write(repo.path(), "policy/product-identity.toml", CONTRACT)?;
    write(
        repo.path(),
        "Cargo.toml",
        r#"[workspace.package]
repository = "https://github.com/EffortlessMetrics/perl-lsp"

[workspace.dependencies]
perl-lsp-rs = { path = "crates/perl-lsp-rs" }
"#,
    )?;
    write(
        repo.path(),
        "crates/perllsp/Cargo.toml",
        r#"[package]
name = "perllsp"
repository.workspace = true

[[bin]]
name = "perllsp"

[dependencies]
perl-lsp-rs = { workspace = true }
"#,
    )?;
    write(repo.path(), "crates/perllsp/src/main.rs", "fn main() {}\n")?;
    write(
        repo.path(),
        "crates/perl-lsp-rs/Cargo.toml",
        r#"[package]
name = "perl-lsp-rs"
repository.workspace = true

[[bin]]
name = "perl-lsp"
"#,
    )?;
    write(repo.path(), "crates/perl-lsp-rs/src/main.rs", "fn main() {}\n")?;
    write(
        repo.path(),
        "crates/perl-dap/Cargo.toml",
        r#"[package]
name = "perl-dap"
repository.workspace = true

[[bin]]
name = "perl-dap"
"#,
    )?;
    write(repo.path(), "crates/perl-dap/src/main.rs", "fn main() {}\n")?;
    write(
        repo.path(),
        "vscode-extension/package.json",
        r#"{
  "name": "perl-lsp-rs",
  "displayName": "Perl Language Server (perl-lsp)",
  "publisher": "EffortlessMetrics",
  "repository": {"url": "https://github.com/EffortlessMetrics/perl-lsp"}
}"#,
    )?;
    Ok(repo)
}

fn write(root: &Path, relative: &str, content: &str) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}
